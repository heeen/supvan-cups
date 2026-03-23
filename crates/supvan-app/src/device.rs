//! PAPPL device scheme callbacks for btrfcomm:// and usbhid:// schemes.
//!
//! These are `unsafe extern "C"` functions registered via `papplDeviceAddScheme`.
//! They bridge between PAPPL's device model and our KsDevice type.

use std::ffi::{c_char, c_void, CStr, CString};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pappl_sys::*;

use crate::battery_provider;
use crate::discover;
use crate::printer_device::{KsDevice, PAPPL_PREASON_OTHER};
use crate::usb_discover;

// --- BT connection cache ---
//
// PAPPL opens/closes the device for every status poll and print job.
// For BT, each cycle is a full RFCOMM connect/disconnect which destabilizes
// the link. We cache the last connection and reuse it if the address matches.
//
// To avoid draining the printer battery, the cache has an idle timeout:
// connections are only cached while a print job is recent. After the timeout,
// connections are dropped so the printer can sleep. Configurable via
// SUPVAN_BT_IDLE_TIMEOUT (seconds, default 120).

static BT_CONN_CACHE: Mutex<Option<Box<KsDevice>>> = Mutex::new(None);

/// Epoch seconds of the last print job start. 0 = no job yet.
static LAST_PRINT_TIME: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn bt_idle_timeout() -> u64 {
    static CACHED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("SUPVAN_BT_IDLE_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120)
    })
}

/// Record that a print job is active (call from rstartjob).
pub fn bt_touch_print_time() {
    LAST_PRINT_TIME.store(now_secs(), Ordering::Relaxed);
}

/// Take a cached BT connection for `addr`, or open a new one.
fn bt_conn_open(addr: &str) -> Option<Box<KsDevice>> {
    if let Ok(mut cache) = BT_CONN_CACHE.lock() {
        if let Some(dev) = cache.take() {
            if dev.addr.as_deref() == Some(addr) {
                if dev.is_alive() {
                    log::info!("bt_conn_open: reusing cached connection to {addr}");
                    return Some(dev);
                }
                log::info!("bt_conn_open: cached connection to {addr} is dead, reconnecting");
                drop(dev);
            } else {
                log::info!("bt_conn_open: dropping cached connection (different addr)");
                drop(dev);
            }
        }
    }
    KsDevice::open(addr)
}

/// Return a BT connection to the cache instead of dropping it.
/// Drops the connection if the socket is dead or idle too long (so the
/// printer can sleep and save battery).
fn bt_conn_close(dev: Box<KsDevice>) {
    let addr = dev.addr.as_deref().unwrap_or("?");
    if !dev.is_alive() {
        log::info!("bt_conn_close: connection to {addr} is dead, dropping");
        return;
    }
    let last = LAST_PRINT_TIME.load(Ordering::Relaxed);
    let idle = now_secs().saturating_sub(last);
    if last == 0 || idle > bt_idle_timeout() {
        log::info!(
            "bt_conn_close: idle {idle}s > timeout {}s, dropping connection to {addr}",
            bt_idle_timeout()
        );
        return;
    }
    if let Ok(mut cache) = BT_CONN_CACHE.lock() {
        log::info!("bt_conn_close: caching connection to {addr} (idle {idle}s)");
        *cache = Some(dev);
    }
}

// --- Bluetooth RFCOMM callbacks ---

/// List callback: discover Bluetooth printers via BlueZ D-Bus.
///
/// Skips reporting BT devices that are also available over USB HID,
/// since USB is more reliable and doesn't require pairing.
///
/// Always returns `false` so PAPPL continues to enumerate other schemes
/// (the return value means "stop iterating", not "found devices").
pub unsafe extern "C" fn bt_list_cb(
    cb: pappl_device_cb_t,
    data: *mut c_void,
    _err_cb: pappl_deverror_cb_t,
    _err_data: *mut c_void,
) -> bool {
    let pappl_cb = match cb {
        Some(f) => f,
        None => return false,
    };

    // Check if the printer is reachable over USB — if so, skip BT to avoid duplicates.
    let usb_available = usb_discover::has_device();

    discover::discover(|device_info, device_uri, device_id| {
        if usb_available {
            log::info!("bt_list_cb: skipping BT device (USB HID available): {device_uri}");
            return true; // continue enumeration but don't report
        }
        let c_info = CString::new(device_info).unwrap_or_default();
        let c_uri = CString::new(device_uri).unwrap_or_default();
        let c_id = CString::new(device_id).unwrap_or_default();
        unsafe { pappl_cb(c_info.as_ptr(), c_uri.as_ptr(), c_id.as_ptr(), data) }
    });
    false
}

/// Open callback: connect to the printer at the btrfcomm:// URI.
pub unsafe extern "C" fn bt_open_cb(
    device: *mut pappl_device_t,
    device_uri: *const c_char,
    _name: *const c_char,
) -> bool {
    if device_uri.is_null() {
        return false;
    }

    let uri_str = match CStr::from_ptr(device_uri).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    // Extract address from btrfcomm://bt/XX:XX:XX:XX:XX:XX
    let addr = match uri_str.strip_prefix("btrfcomm://") {
        Some(rest) => {
            // Skip the dummy host to get the path (BT address)
            match rest.find('/') {
                Some(pos) => &rest[pos + 1..],
                None => return false,
            }
        }
        None => return false,
    };

    let dev = match bt_conn_open(addr) {
        Some(d) => d,
        None => return false,
    };

    if let Some(h) = battery_provider::handle() {
        h.add_device(addr, 100);
    }

    papplDeviceSetData(device, Box::into_raw(dev) as *mut c_void);
    true
}

/// Close callback: cache the BT connection for reuse instead of disconnecting.
pub unsafe extern "C" fn bt_close_cb(device: *mut pappl_device_t) {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if !ptr.is_null() {
        let dev = Box::from_raw(ptr);
        bt_conn_close(dev);
    }
}

/// Read callback: read raw bytes from the RFCOMM socket.
pub unsafe extern "C" fn bt_read_cb(
    device: *mut pappl_device_t,
    buffer: *mut c_void,
    bytes: usize,
) -> isize {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if ptr.is_null() {
        return -1;
    }
    (*ptr).read(buffer as *mut u8, bytes)
}

/// Write callback: write raw bytes to the RFCOMM socket.
pub unsafe extern "C" fn bt_write_cb(
    device: *mut pappl_device_t,
    buffer: *const c_void,
    bytes: usize,
) -> isize {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if ptr.is_null() {
        return -1;
    }
    (*ptr).write(buffer as *const u8, bytes)
}

/// Status callback: query printer status flags.
pub unsafe extern "C" fn bt_status_cb(device: *mut pappl_device_t) -> pappl_preason_t {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if ptr.is_null() {
        return PAPPL_PREASON_OTHER;
    }
    (*ptr).status()
}

// --- USB HID callbacks ---

/// List callback: discover USB HID printers via sysfs.
///
/// Always returns `false` so PAPPL continues to enumerate other schemes.
pub unsafe extern "C" fn usb_list_cb(
    cb: pappl_device_cb_t,
    data: *mut c_void,
    _err_cb: pappl_deverror_cb_t,
    _err_data: *mut c_void,
) -> bool {
    let pappl_cb = match cb {
        Some(f) => f,
        None => return false,
    };

    usb_discover::discover(|device_info, device_uri, device_id| {
        let c_info = CString::new(device_info).unwrap_or_default();
        let c_uri = CString::new(device_uri).unwrap_or_default();
        let c_id = CString::new(device_id).unwrap_or_default();
        unsafe { pappl_cb(c_info.as_ptr(), c_uri.as_ptr(), c_id.as_ptr(), data) }
    });
    false
}

/// Open callback: open the USB HID device for usbhid://SERIAL.
pub unsafe extern "C" fn usb_open_cb(
    device: *mut pappl_device_t,
    device_uri: *const c_char,
    _name: *const c_char,
) -> bool {
    if device_uri.is_null() {
        return false;
    }

    let uri_str = match CStr::from_ptr(device_uri).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };

    let id = match uri_str.strip_prefix("usbhid://") {
        Some(s) => s,
        None => return false,
    };

    log::info!("usb_open_cb: resolving id '{id}' to hidraw path");
    let hidraw_path = match usb_discover::find_device_by_id(id) {
        Some(p) => p,
        None => {
            log::warn!("usb_open_cb: device id '{id}' not found");
            return false;
        }
    };

    log::info!("usb_open_cb: resolved to {hidraw_path}");

    let dev = match KsDevice::open_usb(&hidraw_path) {
        Some(d) => d,
        None => return false,
    };

    papplDeviceSetData(device, Box::into_raw(dev) as *mut c_void);
    true
}

/// Close callback: close and free the USB HID KsDevice.
pub unsafe extern "C" fn usb_close_cb(device: *mut pappl_device_t) {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
    }
}

/// Read callback: read raw bytes from the hidraw device.
pub unsafe extern "C" fn usb_read_cb(
    device: *mut pappl_device_t,
    buffer: *mut c_void,
    bytes: usize,
) -> isize {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if ptr.is_null() {
        return -1;
    }
    (*ptr).read(buffer as *mut u8, bytes)
}

/// Write callback: write raw bytes to the hidraw device.
pub unsafe extern "C" fn usb_write_cb(
    device: *mut pappl_device_t,
    buffer: *const c_void,
    bytes: usize,
) -> isize {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if ptr.is_null() {
        return -1;
    }
    (*ptr).write(buffer as *const u8, bytes)
}

/// Status callback: query USB HID printer status flags.
pub unsafe extern "C" fn usb_status_cb(device: *mut pappl_device_t) -> pappl_preason_t {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if ptr.is_null() {
        return PAPPL_PREASON_OTHER;
    }
    (*ptr).status()
}
