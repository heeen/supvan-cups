//! PAPPL raster callbacks and printer status callback.

use pappl_sys::*;

use crate::battery_provider;
use crate::device;
use crate::dither::dither_line;
use crate::driver::{fill_media_col, find_best_media};
use crate::dump::PgmAccumulator;
use crate::job::{JobFailure, KsJob};
use crate::models;
use crate::printer_device::KsDevice;
use crate::util::copy_to_c_buf;

/// Surface a `JobFailure` to PAPPL/CUPS via the safe pappl-rs API.
unsafe fn report_job_failure(job: *mut pappl_job_t, fail: &JobFailure) {
    let job = pappl_rs::Job::from_raw(job);
    job.fail(fail.printer_reasons, &fail.message);
}

/// Helper: get printer darkness setting (0-100).
unsafe fn get_darkness(job: &pappl_rs::Job<'_>) -> i32 {
    let printer = job.printer();
    if printer.is_null() {
        return 50;
    }
    let mut data: pappl_pr_driver_data_t = Default::default();
    let result = papplPrinterGetDriverData(printer.as_raw(), &mut data);
    if result.is_null() {
        return 50;
    }
    data.darkness_configured
}

/// Look up the driver family for a printer via its driver name.
fn get_family(printer: &pappl_rs::Printer<'_>) -> Option<&'static models::DriverFamily> {
    let name = printer.driver_name()?;
    models::family_by_driver_name(name)
}

/// Start-job callback.
pub unsafe extern "C" fn ks_rstartjob_cb(
    job: *mut pappl_job_t,
    options: *mut pappl_pr_options_t,
    device: *mut pappl_device_t,
) -> bool {
    let dev_handle = pappl_rs::DeviceHandle::from_raw(device);
    let Some(dev) = dev_handle.data::<KsDevice>() else {
        return false;
    };
    if options.is_null() {
        return false;
    }
    let opts = &*options;
    let job = pappl_rs::Job::from_raw(job);

    device::bt_touch_print_time();

    let w = opts.header.cupsWidth;
    let h = opts.header.cupsHeight;
    let bpl = if opts.header.cupsBitsPerPixel == 8 {
        w.div_ceil(8)
    } else {
        opts.header.cupsBytesPerLine
    };

    let darkness = get_darkness(&job);
    let density = ((darkness * 15 + 50) / 100) as u8;

    let printhead_width_dots = get_family(&job.printer())
        .map(|f| f.printhead_width_dots)
        .unwrap_or(384);

    let ks_job = match KsJob::start(dev, w, h, bpl, density, printhead_width_dots) {
        Ok(j) => j,
        Err(fail) => {
            report_job_failure(job.as_raw(), &fail);
            return false;
        }
    };

    // Create PGM accumulator if dump dir is set and input is 8bpp
    if opts.header.cupsBitsPerPixel == 8 && std::env::var("SUPVAN_DUMP_DIR").is_ok() {
        // Need mutable access — store first, then mutate
    }

    job.set_data(*ks_job);

    if opts.header.cupsBitsPerPixel == 8 && std::env::var("SUPVAN_DUMP_DIR").is_ok() {
        if let Some(ks) = job.data_mut::<KsJob>() {
            ks.pgm_acc = Some(PgmAccumulator::new(w, h));
        }
    }

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
    let job = pappl_rs::Job::from_raw(job);
    let Some(ks_job) = job.data_mut::<KsJob>() else {
        return false;
    };
    if line.is_null() || options.is_null() {
        return false;
    }
    let opts = &*options;
    let width = opts.header.cupsWidth;

    if opts.header.cupsBitsPerPixel == 8 {
        let input = std::slice::from_raw_parts(line, width as usize);

        if let Some(ref mut acc) = ks_job.pgm_acc {
            acc.push_line(y, input);
        }

        let bpl_1bpp = width.div_ceil(8) as usize;
        let mut mono = vec![0u8; bpl_1bpp];
        dither_line(input, width, y, &mut mono);

        return ks_job.write_line(y, &mono);
    }

    let bpl = opts.header.cupsBytesPerLine as usize;
    let input = std::slice::from_raw_parts(line, bpl);
    ks_job.write_line(y, input)
}

/// End-page callback.
///
/// Handles copies: PAPPL advertises `copies-supported 1-999` for non-raster
/// formats, so CUPS may send only one page and set `options->copies > 1`.
/// We repeat the page transfer for each copy.
pub unsafe extern "C" fn ks_rendpage_cb(
    job: *mut pappl_job_t,
    options: *mut pappl_pr_options_t,
    device: *mut pappl_device_t,
    _page: u32,
) -> bool {
    let dev_handle = pappl_rs::DeviceHandle::from_raw(device);
    let Some(dev) = dev_handle.data::<KsDevice>() else {
        return false;
    };
    let job = pappl_rs::Job::from_raw(job);
    let Some(ks_job) = job.data_mut::<KsJob>() else {
        return false;
    };
    if options.is_null() {
        return false;
    }

    let copies = ((*options).copies as u32).max(1);

    for copy in 0..copies {
        if copies > 1 {
            log::info!("ks_rendpage_cb: printing copy {}/{copies}", copy + 1);
        }
        if let Err(fail) = ks_job.end_page(dev) {
            report_job_failure(job.as_raw(), &fail);
            return false;
        }
    }
    ks_job.clear_page();
    true
}

/// End-job callback.
pub unsafe extern "C" fn ks_rendjob_cb(
    job: *mut pappl_job_t,
    _options: *mut pappl_pr_options_t,
    device: *mut pappl_device_t,
) -> bool {
    let job = pappl_rs::Job::from_raw(job);

    if let Some(ks_job) = job.take_data::<KsJob>() {
        let dev_handle = pappl_rs::DeviceHandle::from_raw(device);
        if let Some(dev) = dev_handle.data::<KsDevice>() {
            ks_job.end(dev);
        }
    }

    true
}

/// Printer status callback — update media-ready from loaded roll.
pub unsafe extern "C" fn ks_status_cb(printer: *mut pappl_printer_t) -> bool {
    let p = pappl_rs::Printer::from_raw(printer);
    let family = get_family(&p).unwrap_or(models::default_family());

    let Some(dev_handle) = p.open_device() else {
        p.set_reasons(pappl_rs::PrinterReason::OFFLINE, pappl_rs::PrinterReason::empty());
        return false;
    };

    p.set_reasons(pappl_rs::PrinterReason::empty(), pappl_rs::PrinterReason::OFFLINE);

    let Some(dev) = dev_handle.data::<KsDevice>() else {
        p.close_device();
        return false;
    };

    let mat = match dev.material() {
        Some(m) => m,
        None => {
            p.close_device();
            return true;
        }
    };
    let low_battery = dev.battery_low();
    let bt_addr = dev.addr.clone();

    p.close_device();

    if let (Some(h), Some(addr)) = (battery_provider::handle(), bt_addr.as_deref()) {
        let percentage = if low_battery { 10 } else { 100 };
        h.update_battery(addr, percentage);
    }

    let w_hmm = mat.width_mm as i32 * 100;
    let h_hmm = mat.height_mm as i32 * 100;

    let best = match find_best_media(family, w_hmm, h_hmm) {
        Some(i) => i,
        None => return true,
    };

    let mut ready: pappl_media_col_t = Default::default();
    fill_media_col(family, &mut ready, best);

    papplPrinterSetReadyMedia(printer, 1, &mut ready);

    let mut supplies: [pappl_supply_t; 2] = Default::default();
    let mut num_supplies = 0;

    if mat.remaining >= 0 {
        copy_to_c_buf(&mut supplies[num_supplies].description, b"Labels");
        supplies[num_supplies].color = pappl_supply_color_e_PAPPL_SUPPLY_COLOR_NO_COLOR;
        supplies[num_supplies].type_ = pappl_supply_type_e_PAPPL_SUPPLY_TYPE_OTHER;
        supplies[num_supplies].is_consumed = true;
        supplies[num_supplies].level = mat.remaining.min(100);
        num_supplies += 1;
    }

    copy_to_c_buf(&mut supplies[num_supplies].description, b"Battery");
    supplies[num_supplies].color = pappl_supply_color_e_PAPPL_SUPPLY_COLOR_NO_COLOR;
    supplies[num_supplies].type_ = pappl_supply_type_e_PAPPL_SUPPLY_TYPE_OTHER;
    supplies[num_supplies].is_consumed = false;
    supplies[num_supplies].level = if low_battery { 10 } else { 100 };
    num_supplies += 1;

    papplPrinterSetSupplies(printer, num_supplies as i32, supplies.as_mut_ptr());

    true
}
