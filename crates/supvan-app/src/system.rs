//! PAPPL system callback and save callback.

use std::ffi::{c_int, c_void, CStr, CString};
use std::sync::OnceLock;

use pappl_sys::*;

use crate::device;
use crate::driver;
use crate::models;
use crate::usb_discover;

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
        c"-".as_ptr(),    // logfile: "-" = stderr
        pappl_loglevel_e_PAPPL_LOGLEVEL_DEBUG,
        std::ptr::null(), // auth_service
        false,            // tls_only
    );

    if system.is_null() {
        return std::ptr::null_mut();
    }

    // Bind TCP listener
    papplSystemAddListeners(system, std::ptr::null());

    papplSystemSetFooterHTML(system, c"Supvan Printer Application".as_ptr());

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

    // Register printer drivers (all families)
    let mut drivers: Vec<pappl_pr_driver_t> = models::families()
        .iter()
        .map(|f| pappl_pr_driver_t {
            name: f.driver_name.as_ptr(),
            description: f.description.as_ptr(),
            device_id: f.device_id.as_ptr(),
            extension: std::ptr::null_mut(),
        })
        .collect();
    papplSystemSetPrinterDrivers(
        system,
        drivers.len() as i32,
        drivers.as_mut_ptr(),
        Some(driver::ks_autoadd_cb),
        None, // create_cb
        Some(driver::ks_driver_cb),
        std::ptr::null_mut(),
    );

    // Persist configuration
    papplSystemSetSaveCallback(system, Some(ks_save_cb), std::ptr::null_mut());
    papplSystemLoadState(system, state_path().as_ptr());

    prune_stale_usb_printers(system);

    // Discover and auto-add printers. We must call this ourselves because
    // PAPPL's _papplMainloopRunServer only auto-adds when it handles
    // LoadState itself (which we do here instead).
    // papplSystemCreatePrinters was added in PAPPL 1.4.
    #[cfg(pappl_1_4)]
    papplSystemCreatePrinters(
        system,
        pappl_devtype_e_PAPPL_DEVTYPE_LOCAL,
        None,
        std::ptr::null_mut(),
    );

    system
}

/// Delete USB printers whose device is no longer physically present.
///
/// After loading persisted state, any `usbhid://` printer whose serial (or
/// path) no longer resolves to a real hidraw device is stale. We delete it
/// so users don't see ghost printers that silently eat jobs. The printer
/// will be re-created via PAPPL auto-add on next plug.
unsafe fn prune_stale_usb_printers(system: *mut pappl_system_t) {
    // Collect printer pointers first — can't delete during iteration.
    let mut stale: Vec<*mut pappl_printer_t> = Vec::new();

    unsafe extern "C" fn collect_cb(printer: *mut pappl_printer_t, data: *mut c_void) {
        let stale = &mut *(data as *mut Vec<*mut pappl_printer_t>);
        let uri_ptr = papplPrinterGetDeviceURI(printer);
        if uri_ptr.is_null() {
            return;
        }
        let uri = CStr::from_ptr(uri_ptr);
        let uri_str = match uri.to_str() {
            Ok(s) => s,
            Err(_) => return,
        };

        let serial = match uri_str.strip_prefix("usbhid://") {
            Some(s) => s,
            None => return, // not a USB printer
        };

        let device_present = usb_discover::find_device_by_serial(serial).is_some();

        if !device_present {
            log::info!("Pruning stale USB printer: {uri_str}");
            stale.push(printer);
        }
    }

    papplSystemIteratePrinters(
        system,
        Some(collect_cb),
        &mut stale as *mut Vec<_> as *mut c_void,
    );

    if !stale.is_empty() {
        for printer in &stale {
            papplPrinterDelete(*printer);
        }
        // Persist the deletions so stale printers don't reappear on next restart.
        papplSystemSaveState(system, state_path().as_ptr());
    }
}
