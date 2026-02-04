//! PAPPL system callback and save callback.

use std::ffi::{c_int, c_void, CString};
use std::sync::OnceLock;

use pappl_sys::*;

use crate::device;
use crate::driver;

/// State file path: $XDG_STATE_HOME/supvan-printer-app.state or
/// ~/.local/state/supvan-printer-app.state.
static STATE_PATH: OnceLock<CString> = OnceLock::new();

fn build_state_path() -> CString {
    let dir = std::env::var("XDG_STATE_HOME")
        .ok()
        .filter(|s| !s.is_empty());
    let path = if let Some(dir) = dir {
        format!("{dir}/supvan-printer-app.state")
    } else {
        let home = std::env::var("HOME").ok().unwrap_or_else(|| {
            // Fallback to getpwuid
            unsafe {
                let pw = libc::getpwuid(libc::getuid());
                if pw.is_null() || (*pw).pw_dir.is_null() {
                    "/tmp".to_string()
                } else {
                    std::ffi::CStr::from_ptr((*pw).pw_dir)
                        .to_str()
                        .unwrap_or("/tmp")
                        .to_string()
                }
            }
        });
        format!("{home}/.local/state/supvan-printer-app.state")
    };
    CString::new(path).expect("state path contains NUL")
}

fn state_path() -> &'static CString {
    STATE_PATH.get_or_init(build_state_path)
}

/// Save callback: persist system state to disk.
unsafe extern "C" fn ks_save_cb(system: *mut pappl_system_t, _data: *mut c_void) -> bool {
    papplSystemSaveState(system, state_path().as_ptr());
    true
}

/// System callback: create and configure the PAPPL system.
pub unsafe extern "C" fn ks_system_cb(
    _num_options: c_int,
    _options: *mut cups_option_t,
    _data: *mut c_void,
) -> *mut pappl_system_t {
    let system = papplSystemCreate(
        pappl_soptions_e_PAPPL_SOPTIONS_MULTI_QUEUE | pappl_soptions_e_PAPPL_SOPTIONS_WEB_INTERFACE,
        c"supvan-printer-app".as_ptr(),
        8631,
        std::ptr::null(), // subtypes
        std::ptr::null(), // spooldir
        std::ptr::null(), // logfile
        pappl_loglevel_e_PAPPL_LOGLEVEL_DEBUG,
        std::ptr::null(), // auth_service
        false,            // tls_only
    );

    if system.is_null() {
        return std::ptr::null_mut();
    }

    // Bind TCP listener
    papplSystemAddListeners(system, std::ptr::null());

    papplSystemSetFooterHTML(system, c"Supvan T50 Pro Printer Application".as_ptr());

    let mut version: pappl_version_t = Default::default();
    crate::util::copy_to_c_buf(&mut version.name, b"supvan-printer-app");
    crate::util::copy_to_c_buf(&mut version.sversion, b"1.0.0");
    version.version = [1, 0, 0, 0];
    papplSystemSetVersions(system, 1, &mut version);

    // Register btrfcomm:// device scheme
    papplDeviceAddScheme(
        c"btrfcomm".as_ptr(),
        pappl_devtype_e_PAPPL_DEVTYPE_CUSTOM_LOCAL,
        Some(device::bt_list_cb),
        Some(device::bt_open_cb),
        Some(device::bt_close_cb),
        Some(device::bt_read_cb),
        Some(device::bt_write_cb),
        Some(device::bt_status_cb),
        None, // id_cb
    );

    // Register usbhid:// device scheme
    papplDeviceAddScheme(
        c"usbhid".as_ptr(),
        pappl_devtype_e_PAPPL_DEVTYPE_CUSTOM_LOCAL,
        Some(device::usb_list_cb),
        Some(device::usb_open_cb),
        Some(device::usb_close_cb),
        Some(device::usb_read_cb),
        Some(device::usb_write_cb),
        Some(device::usb_status_cb),
        None, // id_cb
    );

    // Register printer driver
    let mut drv = pappl_pr_driver_t {
        name: driver::DRIVER_NAME.as_ptr(),
        description: c"Supvan T50 Pro".as_ptr(),
        device_id: c"MFG:Supvan;MDL:T50 Pro;CMD:SUPVAN;".as_ptr(),
        extension: std::ptr::null_mut(),
    };
    papplSystemSetPrinterDrivers(
        system,
        1,
        &mut drv,
        Some(driver::ks_autoadd_cb),
        None, // create_cb
        Some(driver::ks_driver_cb),
        std::ptr::null_mut(),
    );

    // Persist configuration
    papplSystemSetSaveCallback(system, Some(ks_save_cb), std::ptr::null_mut());
    papplSystemLoadState(system, state_path().as_ptr());

    // Auto-discover and add Bluetooth printers
    papplSystemCreatePrinters(
        system,
        pappl_devtype_e_PAPPL_DEVTYPE_CUSTOM_LOCAL,
        None, // create_cb
        std::ptr::null_mut(),
    );

    system
}
