//! PAPPL raster callbacks and printer status callback.

use std::ffi::c_void;

use pappl_sys::*;

use crate::dither::dither_line;
use crate::driver::{fill_media_col, find_best_media};
use crate::dump::PgmAccumulator;
use crate::job::KsJob;
use crate::printer_device::KsDevice;
use crate::util::copy_to_c_buf;

/// Helper: get printer darkness setting (0-100).
unsafe fn get_darkness(job: *mut pappl_job_t) -> i32 {
    let printer = papplJobGetPrinter(job);
    if printer.is_null() {
        return 50;
    }

    let mut data: pappl_pr_driver_data_t = Default::default();
    let result = papplPrinterGetDriverData(printer, &mut data);
    if result.is_null() {
        return 50;
    }

    data.darkness_configured
}

/// Start-job callback.
pub unsafe extern "C" fn ks_rstartjob_cb(
    job: *mut pappl_job_t,
    options: *mut pappl_pr_options_t,
    device: *mut pappl_device_t,
) -> bool {
    let dev_ptr = papplDeviceGetData(device) as *mut KsDevice;
    if dev_ptr.is_null() || options.is_null() {
        return false;
    }
    let dev = &*dev_ptr;
    let opts = &*options;

    let w = opts.header.cupsWidth;
    let h = opts.header.cupsHeight;
    let bpl = if opts.header.cupsBitsPerPixel == 8 {
        w.div_ceil(8)
    } else {
        opts.header.cupsBytesPerLine
    };

    // Map darkness (0-100%) to density (0-15)
    let darkness = get_darkness(job);
    let density = ((darkness * 15 + 50) / 100) as u8;

    let mut ks_job = match KsJob::start(dev, w, h, bpl, density) {
        Some(j) => j,
        None => return false,
    };

    // Create PGM accumulator if dump dir is set and input is 8bpp
    if opts.header.cupsBitsPerPixel == 8 && std::env::var("SUPVAN_DUMP_DIR").is_ok() {
        ks_job.pgm_acc = Some(PgmAccumulator::new(w, h));
    }

    papplJobSetData(job, Box::into_raw(ks_job) as *mut c_void);
    true
}

/// Start-page callback (no-op).
pub unsafe extern "C" fn ks_rstartpage_cb(
    _job: *mut pappl_job_t,
    _options: *mut pappl_pr_options_t,
    _device: *mut pappl_device_t,
    _page: u32,
) -> bool {
    true
}

/// Write-line callback: dither 8bpp to 1bpp if needed, then write to job.
pub unsafe extern "C" fn ks_rwriteline_cb(
    job: *mut pappl_job_t,
    options: *mut pappl_pr_options_t,
    _device: *mut pappl_device_t,
    y: u32,
    line: *const u8,
) -> bool {
    let job_ptr = papplJobGetData(job) as *mut KsJob;
    if job_ptr.is_null() || line.is_null() || options.is_null() {
        return false;
    }
    let ks_job = &mut *job_ptr;
    let opts = &*options;
    let width = opts.header.cupsWidth;

    if opts.header.cupsBitsPerPixel == 8 {
        let input = std::slice::from_raw_parts(line, width as usize);

        // Feed raw 8bpp to PGM accumulator if present
        if let Some(ref mut acc) = ks_job.pgm_acc {
            acc.push_line(y, input);
        }

        // Dither 8bpp -> 1bpp
        let bpl_1bpp = width.div_ceil(8) as usize;
        let mut mono = vec![0u8; bpl_1bpp];
        dither_line(input, width, y, &mut mono);

        return ks_job.write_line(y, &mono);
    }

    // Already 1bpp — pass through
    let bpl = opts.header.cupsBytesPerLine as usize;
    let input = std::slice::from_raw_parts(line, bpl);
    ks_job.write_line(y, input)
}

/// End-page callback.
pub unsafe extern "C" fn ks_rendpage_cb(
    job: *mut pappl_job_t,
    _options: *mut pappl_pr_options_t,
    device: *mut pappl_device_t,
    _page: u32,
) -> bool {
    let job_ptr = papplJobGetData(job) as *mut KsJob;
    let dev_ptr = papplDeviceGetData(device) as *mut KsDevice;
    if job_ptr.is_null() || dev_ptr.is_null() {
        return false;
    }

    let ks_job = &mut *job_ptr;
    let dev = &*dev_ptr;
    ks_job.end_page(dev)
}

/// End-job callback.
pub unsafe extern "C" fn ks_rendjob_cb(
    job: *mut pappl_job_t,
    _options: *mut pappl_pr_options_t,
    device: *mut pappl_device_t,
) -> bool {
    let job_ptr = papplJobGetData(job) as *mut KsJob;
    let dev_ptr = papplDeviceGetData(device) as *mut KsDevice;

    if !job_ptr.is_null() {
        let ks_job = Box::from_raw(job_ptr);
        if !dev_ptr.is_null() {
            let dev = &*dev_ptr;
            ks_job.end(dev);
        }
    }

    papplJobSetData(job, std::ptr::null_mut());
    true
}

/// Printer status callback — update media-ready from loaded roll.
pub unsafe extern "C" fn ks_status_cb(printer: *mut pappl_printer_t) -> bool {
    let device = papplPrinterOpenDevice(printer);
    if device.is_null() {
        return false;
    }

    let dev_ptr = papplDeviceGetData(device) as *mut KsDevice;
    if dev_ptr.is_null() {
        papplPrinterCloseDevice(printer);
        return false;
    }
    let dev = &*dev_ptr;

    let mat = match dev.material() {
        Some(m) => m,
        None => {
            papplPrinterCloseDevice(printer);
            return true; // non-fatal
        }
    };

    papplPrinterCloseDevice(printer);

    let w_hmm = mat.width_mm as i32 * 100;
    let h_hmm = mat.height_mm as i32 * 100;

    let best = match find_best_media(w_hmm, h_hmm) {
        Some(i) => i,
        None => return true,
    };

    let mut ready: pappl_media_col_t = Default::default();
    fill_media_col(&mut ready, best);

    papplPrinterSetReadyMedia(printer, 1, &mut ready);

    // Report labels remaining as a supply
    if mat.remaining >= 0 {
        let mut supply: pappl_supply_t = Default::default();
        let desc = format!("Labels ({} remaining)", mat.remaining);
        copy_to_c_buf(&mut supply.description, desc.as_bytes());
        supply.color = pappl_supply_color_e_PAPPL_SUPPLY_COLOR_NO_COLOR;
        supply.type_ = pappl_supply_type_e_PAPPL_SUPPLY_TYPE_OTHER;
        supply.is_consumed = true;
        supply.level = (mat.remaining as i32).min(100);
        papplPrinterSetSupplies(printer, 1, &mut supply);
    }

    true
}
