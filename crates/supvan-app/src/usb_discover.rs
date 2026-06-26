//! USB HID device discovery via sysfs.
//!
//! Scans `/sys/class/hidraw/` for devices matching VID=0x1820 and a known
//! Supvan USB PID (see `models::USB_MODELS`).
//!
//! URIs use the USB serial number for stability across hotplug:
//! `usbhid://SERIAL` (e.g. `usbhid://7E1222120101`).
//! Devices without a serial number use a bus-topology URI instead:
//! `usbhid://bus-BUSNUM-DEVPATH` (e.g. `usbhid://bus-1-2.3`).

use std::fs;
use std::path::Path;

use supvan_proto::printer::Printer;

use crate::models;
use crate::util::is_mock_mode;

const SUPVAN_USB_VID: &str = "1820";

/// One USB-attached Supvan candidate ready for cross-correlation with BT.
///
/// `printer_name` is the result of `RD_DEV_NAME` over HID — when present, this
/// is the unique self-identifying string the firmware reports on *every*
/// transport (USB and BT). It's the join key for "same physical printer".
pub struct UsbCandidate {
    pub hidraw_path: String,
    /// What goes after `usbhid://` in the legacy URI (serial or `bus-N-path`).
    pub uri_id: String,
    pub model_name: String,
    pub printer_name: Option<String>,
}

/// Open the HID just long enough to issue RETURN_MAT and parse the embedded
/// device serial from the 64-byte response. Silent over USB.
///
/// `RD_DEV_NAME` (0x16) doesn't work over USB — the 8-byte HID status frame
/// can't carry a string — but `RETURN_MAT` (0x30) returns a 64-byte report
/// whose offset-40 field holds the printer's serial as ASCII (matches the
/// `Name` BlueZ exposes over BT). That's the cross-transport join key.
async fn probe_printer_name(hidraw_path: &str) -> Option<String> {
    if is_mock_mode() {
        return None;
    }
    let printer = Printer::open_usb(hidraw_path).ok()?;
    let mat = match printer.query_material().await {
        Ok(Some(m)) => m,
        Ok(None) => {
            log::warn!("usb_discover: {hidraw_path}: RETURN_MAT returned None");
            return None;
        }
        Err(e) => {
            log::warn!("usb_discover: {hidraw_path}: RETURN_MAT errored: {e}");
            return None;
        }
    };
    log::info!(
        "usb_discover: probed {hidraw_path} -> device_sn={:?} remaining={:?}",
        mat.device_sn,
        mat.remaining,
    );
    mat.device_sn
}

/// Walk /sys/class/hidraw and return one [`UsbCandidate`] per Supvan device.
/// Each entry's HID is briefly opened to read the printer-reported name.
pub async fn list_candidates() -> Vec<UsbCandidate> {
    // First pass (sync sysfs walk): collect addressable Supvan hidraw devices.
    let mut found = Vec::new();
    scan_hidraw_paths(|dev_path, ids| {
        let Some(model) = models::model_by_pid(&ids.pid) else {
            return true;
        };
        let uri_id = if let Some(ref serial) = ids.serial {
            serial.clone()
        } else if let Some(ref bus_path) = ids.bus_path {
            format!("bus-{bus_path}")
        } else {
            return true;
        };
        found.push((dev_path.to_string(), model.name.clone(), uri_id));
        true
    });
    // Second pass (async): probe each device's firmware-reported name.
    let mut out = Vec::new();
    for (hidraw_path, model_name, uri_id) in found {
        let printer_name = probe_printer_name(&hidraw_path).await;
        out.push(UsbCandidate {
            hidraw_path,
            uri_id,
            model_name,
            printer_name,
        });
    }
    out
}

/// Low-level scan of /sys/class/hidraw: calls `cb(dev_path, usb_ids)` for each
/// Supvan device found. Return `false` from `cb` to stop early.
fn scan_hidraw_paths<F>(mut cb: F)
where
    F: FnMut(&str, &UsbIds) -> bool,
{
    let hidraw_dir = Path::new("/sys/class/hidraw");
    let entries = match fs::read_dir(hidraw_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("hidraw") {
            continue;
        }

        let device_path = hidraw_dir.join(&*name_str).join("device");
        let ids = match read_usb_ids(&device_path) {
            Some(ids) => ids,
            None => continue,
        };

        if ids.vid != SUPVAN_USB_VID || models::model_by_pid(&ids.pid).is_none() {
            continue;
        }

        let dev_path = format!("/dev/{name_str}");
        if !cb(&dev_path, &ids) {
            break;
        }
    }
}

/// USB device identity read from sysfs.
struct UsbIds {
    vid: String,
    pid: String,
    serial: Option<String>,
    /// Bus topology path (e.g. "1-2.3"), stable across reboots for a given port.
    bus_path: Option<String>,
}

/// Walk up the sysfs device tree to find idVendor, idProduct, serial, and bus path.
/// Read a sysfs attribute file, trimmed, returning `None` if absent or empty.
fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Max levels to walk up from the hidraw node looking for the USB device dir.
const MAX_SYSFS_WALK_DEPTH: usize = 6;

fn read_usb_ids(device_path: &Path) -> Option<UsbIds> {
    // Resolve the "device" symlink to get the real path
    let real_path = fs::canonicalize(device_path).ok()?;

    // Walk up the directory tree looking for idVendor/idProduct
    let mut current = real_path.as_path();
    for _ in 0..MAX_SYSFS_WALK_DEPTH {
        let vid_path = current.join("idVendor");
        let pid_path = current.join("idProduct");
        if vid_path.exists() && pid_path.exists() {
            let vid = fs::read_to_string(&vid_path).ok()?.trim().to_string();
            let pid = fs::read_to_string(&pid_path).ok()?.trim().to_string();
            let serial = read_trimmed(&current.join("serial"));
            let bus_path = read_bus_path(current);
            return Some(UsbIds {
                vid,
                pid,
                serial,
                bus_path,
            });
        }
        current = current.parent()?;
    }
    None
}

/// Read the USB bus topology path from sysfs (busnum + devpath).
///
/// Returns e.g. "1-2.3" which is stable across reboots as long as the
/// device stays on the same physical port.
fn read_bus_path(usb_dev_dir: &Path) -> Option<String> {
    let busnum = read_trimmed(&usb_dev_dir.join("busnum"))?;
    let devpath = read_trimmed(&usb_dev_dir.join("devpath"))?;
    Some(format!("{busnum}-{devpath}"))
}
