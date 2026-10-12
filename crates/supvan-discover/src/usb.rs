//! USB HID device discovery via sysfs.
//!
//! Scans `/sys/class/hidraw/` for devices matching VID 0x1820 and a known
//! Supvan USB PID (see [`models::model_by_pid`]).

use std::fs;
use std::path::Path;

use supvan_proto::printer::Printer;

use crate::models;
use crate::is_mock_mode;

const SUPVAN_USB_VID: &str = "1820";

/// One USB-attached Supvan candidate ready for cross-correlation with BT.
pub struct UsbCandidate {
    pub hidraw_path: String,
    /// The USB serial, else the bus topology (`bus-1-2.3`) — stable across
    /// hotplug on the same port. A grouping key when `printer_name` is missing.
    pub port_key: String,
    pub model_name: String,
    /// The printer's serial, read over RETURN_MAT — the same string it
    /// advertises over Bluetooth, so the join key for "same physical printer".
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
    let mut out = Vec::new();
    for (hidraw_path, model, ids) in supvan_hidraw_devices() {
        let Some(port_key) = ids.serial.or(ids.bus_path.map(|p| format!("bus-{p}"))) else {
            continue;
        };
        let printer_name = probe_printer_name(&hidraw_path).await;
        out.push(UsbCandidate {
            hidraw_path,
            port_key,
            model_name: model.name.clone(),
            printer_name,
        });
    }
    out
}

/// Every Supvan hidraw node: `(/dev path, model, USB identity)`.
fn supvan_hidraw_devices() -> Vec<(String, &'static models::UsbModel, UsbIds)> {
    let hidraw_dir = Path::new("/sys/class/hidraw");
    let Ok(entries) = fs::read_dir(hidraw_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            if !name.starts_with("hidraw") {
                return None;
            }
            let ids = read_usb_ids(&hidraw_dir.join(&name).join("device"))?;
            if ids.vid != SUPVAN_USB_VID {
                return None;
            }
            let model = models::model_by_pid(&ids.pid)?;
            Some((format!("/dev/{name}"), model, ids))
        })
        .collect()
}

/// USB device identity read from sysfs.
struct UsbIds {
    vid: String,
    pid: String,
    serial: Option<String>,
    /// Bus topology path (e.g. "1-2.3"), stable across reboots for a given port.
    bus_path: Option<String>,
}

/// Read a sysfs attribute file, trimmed, returning `None` if absent or empty.
fn read_trimmed(path: &Path) -> Option<String> {
    fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Max levels to walk up from the hidraw node looking for the USB device dir.
const MAX_SYSFS_WALK_DEPTH: usize = 6;

/// Walk up the sysfs device tree to find idVendor, idProduct, serial, and bus path.
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
