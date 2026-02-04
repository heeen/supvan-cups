//! USB HID device discovery via sysfs.
//!
//! Scans `/sys/class/hidraw/` for devices matching VID=0x1820, PID=0x2073
//! (Supvan T50 Pro USB HID interface).

use std::fs;
use std::path::Path;

const SUPVAN_USB_VID: &str = "1820";
const SUPVAN_USB_PID: &str = "2073";

/// Discover USB HID devices matching the Supvan T50 Pro.
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

        // The sysfs hierarchy for a USB HID device is:
        //   /sys/class/hidraw/hidrawN/device -> ../../../0003:1820:2073.XXXX
        // The USB device info is at:
        //   /sys/class/hidraw/hidrawN/device/../../../idVendor
        //   /sys/class/hidraw/hidrawN/device/../../../idProduct
        //
        // We walk up from the hidraw device symlink to find the USB device.
        let device_path = hidraw_dir.join(&*name_str).join("device");

        // Try to find idVendor/idProduct by walking up the device tree
        let (vid, pid) = match read_usb_ids(&device_path) {
            Some(ids) => ids,
            None => {
                log::debug!("usb_discover: {name_str}: no USB IDs found");
                continue;
            }
        };

        log::debug!("usb_discover: {name_str}: VID={vid} PID={pid}");

        if vid != SUPVAN_USB_VID || pid != SUPVAN_USB_PID {
            continue;
        }

        let dev_path = format!("/dev/{name_str}");
        let uri = format!("usbhid://{dev_path}");
        let info = format!("Supvan T50 Pro (USB)");
        let device_id = "MFG:Supvan;MDL:T50 Pro;CMD:SUPVAN;";

        log::info!("usb_discover: found {dev_path} (VID={vid}, PID={pid})");
        found = true;

        if !cb(&info, &uri, device_id) {
            break;
        }
    }

    found
}

/// Quick check: is any Supvan USB HID device present?
pub fn has_device() -> bool {
    discover(|_, _, _| true)
}

/// Walk up the sysfs device tree to find idVendor and idProduct.
fn read_usb_ids(device_path: &Path) -> Option<(String, String)> {
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
            return Some((vid, pid));
        }
        current = current.parent()?;
    }
    None
}
