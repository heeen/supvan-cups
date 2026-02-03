//! PAPPL device scheme callbacks for btrfcomm:// scheme.
//!
//! These are `unsafe extern "C"` functions registered via `papplDeviceAddScheme`.
//! They bridge between PAPPL's device model and our KsDevice type.

use std::ffi::{c_char, c_void, CStr, CString};

use pappl_sys::*;

use crate::discover;
use crate::printer_device::{KsDevice, PAPPL_PREASON_OTHER};

/// List callback: discover Bluetooth printers via BlueZ D-Bus.
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

    discover::discover(|device_info, device_uri, device_id| {
        let c_info = CString::new(device_info).unwrap_or_default();
        let c_uri = CString::new(device_uri).unwrap_or_default();
        let c_id = CString::new(device_id).unwrap_or_default();
        unsafe { pappl_cb(c_info.as_ptr(), c_uri.as_ptr(), c_id.as_ptr(), data) }
    })
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

    let dev = match KsDevice::open(addr) {
        Some(d) => d,
        None => return false,
    };

    papplDeviceSetData(device, Box::into_raw(dev) as *mut c_void);
    true
}

/// Close callback: disconnect and free the KsDevice.
pub unsafe extern "C" fn bt_close_cb(device: *mut pappl_device_t) {
    let ptr = papplDeviceGetData(device) as *mut KsDevice;
    if !ptr.is_null() {
        drop(Box::from_raw(ptr));
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
