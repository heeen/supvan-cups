use std::sync::atomic::Ordering;
use std::time::Instant;

use ipp_printer_app::{JobFailure, JobOptions, PrinterHandle, PrinterReason, RasterDriver};
use supvan_proto::bitmap::{DEFAULT_MARGIN_DOTS, center_in_printhead, raster_to_column_major};
use supvan_proto::buffer::split_into_buffers;
use supvan_proto::compress::compress_buffers;
use supvan_proto::error::Error as ProtoError;
use supvan_proto::speed::calc_speed;
use supvan_proto::status::PrinterStatus;

use crate::dither::dither_line;
use crate::dump::{JobDump, JobManifest, PgmAccumulator, dumps_enabled};
use crate::mock;
use crate::printer_device::KsDevice;

/// Maximum device print density; darkness (0-100%) scales onto 0..=MAX_DENSITY.
const MAX_DENSITY: i32 = 15;

/// Poll cadence and budget while waiting for print completion
/// (COMPLETION_POLLS × COMPLETION_POLL_INTERVAL = 30s).
const COMPLETION_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
const COMPLETION_POLLS: u32 = 300;

/// Minimal RFC-3339-ish timestamp without pulling chrono.
fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("unix:{secs}")
}

/// Map raw printer status flags to IPP `printer-state-reasons`.
///
/// Single source of truth, shared by the terminal job-failure path
/// ([`failure_from_status`]), live status polling ([`KsDevice::status`]), and
/// the mock simulator. Returns the raw reason bits with no fallback — callers
/// decide how an empty set is treated (failure path forces `OTHER`, live
/// polling leaves it empty = nothing wrong).
pub(crate) fn reasons_from_status(s: &PrinterStatus) -> PrinterReason {
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
    if s.ribbon_end {
        reasons |= PrinterReason::MEDIA_NEEDED;
    }
    if s.head_temp_high {
        reasons |= PrinterReason::OTHER;
    }
    reasons
}

pub fn failure_from_status(s: &PrinterStatus, context: &str) -> JobFailure {
    let mut reasons = reasons_from_status(s);
    if reasons.is_empty() {
        reasons = PrinterReason::OTHER;
    }
    let desc = s.error_description().unwrap_or_else(|| "unknown".into());
    JobFailure::new(reasons, format!("{context}: {desc}"))
}

fn failure_from_proto(e: ProtoError, context: &str) -> JobFailure {
    let reasons = match &e {
        ProtoError::Io(_) => PrinterReason::OFFLINE,
        _ => PrinterReason::OTHER,
    };
    JobFailure::new(reasons, format!("{context}: {e}"))
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
        log::info!(
            "KsJob::start: {w}x{h}, bpl={bpl}, density={density}, printhead={printhead_width_dots}"
        );
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

    pub async fn transfer_page(&mut self, dev: &KsDevice) -> Result<(), JobFailure> {
        let is_mock = dev.is_mock();
        let started = Instant::now();
        log::info!(
            "KsJob::transfer_page: {}x{}, {} lines, mock={}",
            self.width,
            self.height,
            self.lines_received,
            is_mock,
        );

        // Allocate one dump seq per page so all per-page artefacts share NNNN.
        let dump = JobDump::allocate();

        if let Some(acc) = self.pgm_acc.take() {
            dump.pgm(&acc);
        }
        dump.pbm(
            &self.raster_data,
            self.width,
            self.height,
            self.bytes_per_line,
        );

        let (col_data, num_cols, _) =
            raster_to_column_major(&self.raster_data, self.width, self.height);
        let (canvas, canvas_bpl) =
            center_in_printhead(&col_data, num_cols, self.width, self.printhead_width_dots);
        dump.printhead_pbm(&canvas, num_cols, canvas_bpl, self.printhead_width_dots);

        let buffers = split_into_buffers(
            &canvas,
            canvas_bpl as u8,
            num_cols as u16,
            DEFAULT_MARGIN_DOTS,
            DEFAULT_MARGIN_DOTS,
            self.density,
        );

        let (compressed, avg) = compress_buffers(&buffers)
            .map_err(|e| JobFailure::other(format!("compression: {e}")))?;
        let speed = calc_speed(avg);

        let outcome: Result<(), JobFailure> = if let Some(ref printer) = dev.printer {
            dev.printing.store(true, Ordering::Release);
            let result = printer.print_compressed(&compressed, speed).await;
            dev.printing.store(false, Ordering::Release);
            match result {
                Ok(()) => Ok(()),
                Err(ProtoError::InvalidResponse(msg)) => {
                    if let Ok(Some(s)) = printer.query_status().await {
                        if s.has_error() {
                            Err(failure_from_status(&s, "print_compressed"))
                        } else {
                            Err(JobFailure::other(msg))
                        }
                    } else {
                        Err(JobFailure::other(msg))
                    }
                }
                Err(e) => Err(failure_from_proto(e, "print_compressed")),
            }
        } else {
            // Mock device: simulate the print delay, then check the simulator
            // for a queued failure. Dumps already happened above so the operator
            // can still inspect the output even on a simulated abort.
            tokio::time::sleep(mock::controller().delay()).await;
            match mock::controller().take_print_failure() {
                Some(f) => Err(f),
                None => {
                    log::info!("KsJob::transfer_page: mock — dumped, no transfer");
                    Ok(())
                }
            }
        };

        // Manifest reflects what really happened (real or simulated).
        let (sim_outcome, _len) = match &outcome {
            Ok(()) => ("completed".to_string(), 0usize),
            Err(f) => (format!("aborted: {}", f.message), 0),
        };
        dump.manifest(&JobManifest {
            timestamp: now_iso(),
            width: self.width,
            height: self.height,
            bytes_per_line: self.bytes_per_line,
            density: self.density,
            printhead_width_dots: self.printhead_width_dots,
            copies: 1,
            mock: is_mock,
            simulated_outcome: sim_outcome,
            elapsed_ms: started.elapsed().as_millis(),
        });

        outcome
    }

    pub fn clear_page(&mut self) {
        self.raster_data.fill(0);
        self.lines_received = 0;
    }

    pub async fn end(self, dev: &KsDevice) {
        if let Some(ref printer) = dev.printer {
            let mut settled = false;
            for i in 0..COMPLETION_POLLS {
                tokio::time::sleep(COMPLETION_POLL_INTERVAL).await;
                match printer.query_status().await {
                    Ok(Some(s)) if !s.printing && !s.device_busy => {
                        log::info!("KsJob::end: complete after {i} polls");
                        settled = true;
                        break;
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("KsJob::end: status error: {e}");
                        settled = true;
                        break;
                    }
                }
            }
            if !settled {
                log::warn!("KsJob::end: timeout waiting for completion");
            }
            dev.printing.store(false, Ordering::Release);
        }
    }
}

#[async_trait::async_trait]
impl RasterDriver for KsJob {
    type Device = KsDevice;

    fn start_job(
        printer: &PrinterHandle<'_>,
        options: &JobOptions,
        dev: &Self::Device,
    ) -> Result<Self, JobFailure> {
        let w = options.width;
        let h = options.height;
        let bpl = if options.bits_per_pixel == 8 {
            w.div_ceil(8)
        } else {
            options.bytes_per_line
        };

        let darkness = printer.darkness();
        // darkness is 0-100%; scale to 0-MAX_DENSITY, rounding to nearest.
        let density = ((darkness * MAX_DENSITY + 50) / 100) as u8;
        let printhead_width_dots = printer.printhead_width_dots();

        let mut ks = KsJob::start(dev, w, h, bpl, density, printhead_width_dots)?;
        if options.bits_per_pixel == 8 && dumps_enabled() {
            ks.pgm_acc = Some(PgmAccumulator::new(w, h));
        }
        Ok(ks)
    }

    fn write_line(&mut self, options: &JobOptions, y: u32, line: &[u8]) -> Result<(), JobFailure> {
        if options.bits_per_pixel == 8 {
            let width = options.width;
            let input = &line[..(width as usize).min(line.len())];
            if let Some(ref mut acc) = self.pgm_acc {
                acc.push_line(y, input);
            }
            let bpl_1bpp = width.div_ceil(8) as usize;
            let mut mono = vec![0u8; bpl_1bpp];
            dither_line(input, width, y, &mut mono);
            if !self.append_line(y, &mono) {
                return Err(JobFailure::other(format!(
                    "write_line: y={y} out of bounds"
                )));
            }
            return Ok(());
        }
        if !self.append_line(y, line) {
            return Err(JobFailure::other(format!(
                "write_line: y={y} out of bounds"
            )));
        }
        Ok(())
    }

    async fn end_page(
        &mut self,
        options: &JobOptions,
        _page: u32,
        dev: &Self::Device,
    ) -> Result<(), JobFailure> {
        let copies = options.copies;
        for copy in 0..copies {
            if copies > 1 {
                log::info!("end_page: copy {}/{copies}", copy + 1);
            }
            self.transfer_page(dev).await?;
        }
        self.clear_page();
        Ok(())
    }

    async fn end_job(self, dev: &Self::Device) {
        self.end(dev).await;
    }
}
