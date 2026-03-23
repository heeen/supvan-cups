//! USB HID device discovery via sysfs.
//!
//! Scans `/sys/class/hidraw/` for devices matching VID=0x1820 and a known
//! Supvan USB PID (see `models::USB_MODELS`).
//!
//! URIs use the USB serial number for stability across hotplug:
//! `usbhid://SERIAL` (e.g. `usbhid://7E1222120101`).
//! Devices without a serial number use a bus-topology URI instead:
//! `usbhid://bus:BUSNUM-DEVPATH` (e.g. `usbhid://bus:1-2.3`).

use std::fs;
use std::path::Path;

use crate::models;

const SUPVAN_USB_VID: &str = "1820";

/// Discover USB HID devices matching any known Supvan model.
///
/// Calls `cb(device_info, device_uri, device_id)` for each match.
/// Returns `true` if at least one device was found.
pub fn discover<F>(mut cb: F) -> bool
where
    F: FnMut(&str, &str, &str) -> bool,
{
    log::info!("usb_discover: scanning /sys/class/hidraw");
    let hidraw_dir = Path::new("/sys/class/hidraw");
    let entries = match fs::read_dir(hidraw_dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("usb_discover: cannot read /sys/class/hidraw: {e}");
            return false;
        }
    };

    let mut found = false;

    for entry in entries.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.starts_with("hidraw") {
            continue;
        }

        let device_path = hidraw_dir.join(&*name_str).join("device");

        let ids = match read_usb_ids(&device_path) {
            Some(ids) => ids,
            None => {
                log::debug!("usb_discover: {name_str}: no USB IDs found");
                continue;
            }
        };

        log::debug!(
            "usb_discover: {name_str}: VID={} PID={} serial={:?} bus_path={:?}",
            ids.vid, ids.pid, ids.serial, ids.bus_path
        );

        if ids.vid != SUPVAN_USB_VID {
            continue;
        }

        let model = match models::model_by_pid(&ids.pid) {
            Some(m) => m,
            None => {
                log::debug!("usb_discover: {name_str}: unknown PID {}", ids.pid);
                continue;
            }
        };

        let dev_path = format!("/dev/{name_str}");
        let uri = if let Some(ref serial) = ids.serial {
            format!("usbhid://{serial}")
        } else if let Some(ref bus_path) = ids.bus_path {
            log::info!("usb_discover: {name_str}: no serial, using bus path {bus_path}");
            format!("usbhid://bus:{bus_path}")
        } else {
            log::warn!("usb_discover: {name_str}: no serial and no bus path, skipping");
            continue;
        };
        let info = format!("Supvan {} (USB)", model.name);
        let device_id = format!("MFG:Supvan;MDL:{};CMD:SUPVAN;", model.name);

        log::info!(
            "usb_discover: found {dev_path} (VID={}, PID={}, serial={:?}, bus_path={:?}) -> {uri}",
            ids.vid, ids.pid, ids.serial, ids.bus_path
        );
        found = true;

        if !cb(&info, &uri, &device_id) {
            break;
        }
    }

    found
}

/// Quick check: is any Supvan USB HID device present?
pub fn has_device() -> bool {
    discover(|_, _, _| true)
}

/// Find the current `/dev/hidrawN` path for a device with the given ID.
///
/// The `id` is the part after `usbhid://` in the URI:
/// - A serial number (e.g. "7E1222120101") — matched against USB serial
/// - A bus path prefixed with "bus:" (e.g. "bus:1-2.3") — matched against sysfs topology
pub fn find_device_by_id(id: &str) -> Option<String> {
    let mut path = None;
    scan_hidraw_paths(|dev_path, ids| {
        let matches = if let Some(bus_path) = id.strip_prefix("bus:") {
            ids.bus_path.as_deref() == Some(bus_path)
        } else {
            ids.serial.as_deref() == Some(id)
        };
        if matches {
            path = Some(dev_path.to_string());
            return false; // stop
        }
        true // continue
    });
    path
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
fn read_usb_ids(device_path: &Path) -> Option<UsbIds> {
    // Resolve the "device" symlink to get the real path
    let real_path = fs::canonicalize(device_path).ok()?;

    // Walk up the directory tree looking for idVendor/idProduct
    let mut current = real_path.as_path();
    for _ in 0..6 {
        let vid_path = current.join("idVendor");
        let pid_path = current.join("idProduct");
        if vid_path.exists() && pid_path.exists() {
            let vid = fs::read_to_string(&vid_path).ok()?.trim().to_string();
            let pid = fs::read_to_string(&pid_path).ok()?.trim().to_string();
            let serial = fs::read_to_string(current.join("serial"))
                .ok()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty());
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
    let busnum = fs::read_to_string(usb_dev_dir.join("busnum"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    let devpath = fs::read_to_string(usb_dev_dir.join("devpath"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    Some(format!("{busnum}-{devpath}"))
}
