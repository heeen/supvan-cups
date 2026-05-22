use std::sync::atomic::Ordering;

use ipp_printer_app::{JobFailure, JobOptions, PrinterHandle, PrinterReason, RasterDriver};
use supvan_proto::bitmap::{center_in_printhead, raster_to_column_major, DEFAULT_MARGIN_DOTS};
use supvan_proto::buffer::split_into_buffers;
use supvan_proto::compress::compress_buffers;
use supvan_proto::error::Error as ProtoError;
use supvan_proto::speed::calc_speed;
use supvan_proto::status::PrinterStatus;

use crate::device;
use crate::dither::dither_line;
use crate::dump::{dump_pbm, dump_printhead_pbm, PgmAccumulator};
use crate::models;
use crate::printer_device::KsDevice;

pub fn failure_from_status(s: &PrinterStatus, context: &str) -> JobFailure {
    let mut reasons = PrinterReason::empty();
    if s.cover_open {
        reasons |= PrinterReason::COVER_OPEN;
    }
    if s.label_end || s.label_not_installed {
        reasons |= PrinterReason::MEDIA_EMPTY;
    }
    if s.label_rw_error || s.label_mode_error || s.ribbon_rw_error {
        reasons |= PrinterReason::MEDIA_JAM;
    }
    if reasons.is_empty() {
        reasons = PrinterReason::OTHER;
    }
    let desc = s.error_description().unwrap_or_else(|| "unknown".into());
    JobFailure::new(reasons, format!("{context}: {desc}"))
}

pub fn failure_from_proto(e: ProtoError, context: &str) -> JobFailure {
    let reasons = match &e {
        ProtoError::Io(_) => PrinterReason::OFFLINE,
        _ => PrinterReason::OTHER,
    };
    JobFailure::new(reasons, format!("{context}: {e}"))
}

fn get_family(driver_name: &str) -> Option<&'static models::DriverFamily> {
    use std::ffi::CString;
    let name = CString::new(driver_name).ok()?;
    models::family_by_driver_name(name.as_c_str())
}

pub struct KsJob {
    pub width: u32,
    pub height: u32,
    pub bytes_per_line: u32,
    pub raster_data: Vec<u8>,
    pub lines_received: u32,
    pub density: u8,
    pub printhead_width_dots: u32,
    pub pgm_acc: Option<PgmAccumulator>,
}

impl KsJob {
    pub fn start(
        _dev: &KsDevice,
        w: u32,
        h: u32,
        bpl: u32,
        density: u8,
        printhead_width_dots: u32,
    ) -> Result<Self, JobFailure> {
        log::info!("KsJob::start: {w}x{h}, bpl={bpl}, density={density}, printhead={printhead_width_dots}");
        Ok(KsJob {
            width: w,
            height: h,
            bytes_per_line: bpl,
            raster_data: vec![0u8; (h * bpl) as usize],
            lines_received: 0,
            density,
            printhead_width_dots,
            pgm_acc: None,
        })
    }

    pub fn append_line(&mut self, y: u32, line: &[u8]) -> bool {
        if y >= self.height {
            return false;
        }
        let copy_len = line.len().min(self.bytes_per_line as usize);
        let offset = (y * self.bytes_per_line) as usize;
        self.raster_data[offset..offset + copy_len].copy_from_slice(&line[..copy_len]);
        self.lines_received += 1;
        true
    }

    pub fn transfer_page(&mut self, dev: &KsDevice) -> Result<(), JobFailure> {
        let mock = dev.is_mock();
        log::info!(
            "KsJob::transfer_page: {}x{}, {} lines, mock={}",
            self.width,
            self.height,
            self.lines_received,
            mock
        );

        if let Some(ref acc) = self.pgm_acc {
            acc.flush();
        }
        self.pgm_acc = None;

        dump_pbm(
            &self.raster_data,
            self.width,
            self.height,
            self.bytes_per_line,
        );

        let (col_data, num_cols, _) =
            raster_to_column_major(&self.raster_data, self.width, self.height);
        let (canvas, canvas_bpl) =
            center_in_printhead(&col_data, num_cols, self.width, self.printhead_width_dots);
        dump_printhead_pbm(&canvas, num_cols, canvas_bpl, self.printhead_width_dots);

        let buffers = split_into_buffers(
            &canvas,
            canvas_bpl as u8,
            num_cols as u16,
            DEFAULT_MARGIN_DOTS,
            DEFAULT_MARGIN_DOTS,
            self.density,
        );

        let (compressed, avg) =
            compress_buffers(&buffers).map_err(|e| JobFailure::other(format!("compression: {e}")))?;
        let speed = calc_speed(avg);

        if let Some(ref printer) = dev.printer {
            dev.printing.store(true, Ordering::Release);
            let result = printer.print_compressed(&compressed, speed);
            dev.printing.store(false, Ordering::Release);
            if let Err(e) = result {
                if let ProtoError::InvalidResponse(msg) = &e {
                    if let Ok(Some(s)) = printer.query_status() {
                        if s.has_error() {
                            return Err(failure_from_status(&s, "print_compressed"));
                        }
                    }
                    return Err(JobFailure::other(msg.clone()));
                }
                return Err(failure_from_proto(e, "print_compressed"));
            }
        } else {
            log::info!("KsJob::transfer_page: mock — skipping transfer");
        }
        Ok(())
    }

    pub fn clear_page(&mut self) {
        self.raster_data.fill(0);
        self.lines_received = 0;
    }

    pub fn end(self, dev: &KsDevice) {
        if let Some(ref printer) = dev.printer {
            for i in 0..300 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                match printer.query_status() {
                    Ok(Some(s)) if !s.printing && !s.device_busy => {
                        log::info!("KsJob::end: complete after {i} polls");
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("KsJob::end: status error: {e}");
                        break;
                    }
                }
            }
            dev.printing.store(false, Ordering::Release);
        }
    }
}

impl RasterDriver for KsJob {
    type Device = KsDevice;

    fn start_job(
        printer: &PrinterHandle<'_>,
        options: &JobOptions,
        dev: &Self::Device,
    ) -> Result<Self, JobFailure> {
        device::bt_touch_print_time();

        let w = options.width();
        let h = options.height();
        let bpl = if options.bits_per_pixel() == 8 {
            w.div_ceil(8)
        } else {
            options.bytes_per_line()
        };

        let darkness = printer.darkness();
        let density = ((darkness * 15 + 50) / 100) as u8;
        let printhead_width_dots = printer.printhead_width_dots();

        let mut ks = KsJob::start(dev, w, h, bpl, density, printhead_width_dots)?;
        if options.bits_per_pixel() == 8 && std::env::var("SUPVAN_DUMP_DIR").is_ok() {
            ks.pgm_acc = Some(PgmAccumulator::new(w, h));
        }
        Ok(ks)
    }

    fn write_line(
        &mut self,
        options: &JobOptions,
        y: u32,
        line: &[u8],
    ) -> Result<(), JobFailure> {
        if options.bits_per_pixel() == 8 {
            let width = options.width();
            let input = &line[..(width as usize).min(line.len())];
            if let Some(ref mut acc) = self.pgm_acc {
                acc.push_line(y, input);
            }
            let bpl_1bpp = width.div_ceil(8) as usize;
            let mut mono = vec![0u8; bpl_1bpp];
            dither_line(input, width, y, &mut mono);
            if !self.append_line(y, &mono) {
                return Err(JobFailure::other(format!("write_line: y={y} out of bounds")));
            }
            return Ok(());
        }
        if !self.append_line(y, line) {
            return Err(JobFailure::other(format!("write_line: y={y} out of bounds")));
        }
        Ok(())
    }

    fn end_page(
        &mut self,
        options: &JobOptions,
        _page: u32,
        dev: &Self::Device,
    ) -> Result<(), JobFailure> {
        let copies = options.copies();
        for copy in 0..copies {
            if copies > 1 {
                log::info!("end_page: copy {}/{copies}", copy + 1);
            }
            self.transfer_page(dev)?;
        }
        self.clear_page();
        Ok(())
    }

    fn end_job(self, dev: &Self::Device) {
        self.end(dev);
    }

    fn printer_status(printer: &PrinterHandle<'_>) -> bool {
        let _family = get_family(printer.driver_name()).unwrap_or(models::default_family());
        // Dynamic media/supply IPP attributes can be added in a later pass.
        true
    }
}
