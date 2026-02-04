mod battery_provider;
mod device;
mod discover;
mod dither;
mod driver;
mod dump;
mod job;
mod printer_device;
mod raster;
mod system;
mod usb_discover;
mod util;

use pappl_sys::*;

fn main() {
    let _ = env_logger::try_init();

    let mut drv = pappl_pr_driver_t {
        name: driver::DRIVER_NAME.as_ptr(),
        description: c"Supvan T50 Pro".as_ptr(),
        device_id: c"MFG:Supvan;MDL:T50 Pro;CMD:SUPVAN;".as_ptr(),
        extension: std::ptr::null_mut(),
    };

    let argc = std::env::args().count() as i32;
    let args: Vec<std::ffi::CString> = std::env::args()
        .map(|a| std::ffi::CString::new(a).expect("arg contains NUL"))
        .collect();
    let mut argv: Vec<*mut std::ffi::c_char> = args
        .iter()
        .map(|a| a.as_ptr() as *mut std::ffi::c_char)
        .collect();

    let ret = unsafe {
        papplMainloop(
            argc,
            argv.as_mut_ptr(),
            c"1.0.0".as_ptr(),
            c"Supvan T50 Pro Printer Application".as_ptr(),
            1, // num_drivers
            &mut drv,
            Some(driver::ks_autoadd_cb),
            Some(driver::ks_driver_cb),
            std::ptr::null(), // subcmd_name
            None,             // subcmd_cb
            Some(system::ks_system_cb),
            None, // usage_cb
            std::ptr::null_mut(),
        )
    };

    std::process::exit(ret);
}
