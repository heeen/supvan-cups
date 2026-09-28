use std::sync::atomic::Ordering;
use std::time::Instant;

use ipp_printer_app::{JobFailure, JobOptions, PrinterHandle, PrinterReason, RasterDriver};
use supvan_proto::bitmap::{center_in_printhead, raster_to_column_major};
use supvan_proto::buffer::{
    Density, PageOptions, split_into_buffers, split_into_buffers_profiled,
};
use supvan_proto::compress::{compress_buffers, compress_buffers_individually};
use supvan_proto::profile::PrintProfile;
use supvan_proto::dither::{DitherMode, Ditherer, to_gray_line};
use supvan_proto::error::Error as ProtoError;
use supvan_proto::speed::calc_speed;
use supvan_proto::status::PrinterStatus;

use crate::dump::{JobDump, JobManifest, PgmAccumulator, dumps_enabled};
use crate::mock;
use crate::printer_device::KsDevice;

/// Maximum device print density; darkness (0-100%) scales onto 0..=MAX_DENSITY.
const MAX_DENSITY: i32 = 15;

/// Halftone kernel for 8bpp input, from `SUPVAN_DITHER` — the app takes all its
/// other configuration from the environment too. Unrecognised values warn and
/// fall back rather than failing a print.
fn configured_dither() -> DitherMode {
    let Ok(raw) = std::env::var("SUPVAN_DITHER") else {
        return DitherMode::default();
    };
    raw.parse().unwrap_or_else(|e| {
        log::warn!("SUPVAN_DITHER: {e}; using {:?}", DitherMode::default());
        DitherMode::default()
    })
}

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
pub(crate) fn reasons_from_status(
    s: &PrinterStatus,
    profile: PrintProfile,
) -> PrinterReason {
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
    if s.ribbon_end && profile.params().ribbon_end_is_fatal {
        reasons |= PrinterReason::MEDIA_NEEDED;
    }
    if s.head_temp_high {
        reasons |= PrinterReason::OTHER;
    }
    reasons
}

pub fn failure_from_status(
    s: &PrinterStatus,
    profile: PrintProfile,
    context: &str,
) -> JobFailure {
    let mut reasons = reasons_from_status(s, profile);
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
    ditherer: Ditherer,
}

impl KsJob {
    /// Allocate the page buffer for an incoming raster.
    ///
    /// The buffer is always tightly packed 1 bpp at `ceil(w/8)`, whatever the
    /// source says: contone lines are dithered before `append_line`, and
    /// [`raster_to_column_major`] re-reads the buffer at that same hard-coded
    /// stride. Sizing from `src_bytes_per_line` instead copies raw RGB into
    /// the bitmap (white `0xFF` becomes eight set dots — a black label), or,
    /// for a padded 1 bpp source, shears every row. `append_line` truncates
    /// the padding away as each line arrives.
    pub fn start(
        _dev: &KsDevice,
        w: u32,
        h: u32,
        src_bits_per_pixel: u32,
        src_bytes_per_line: u32,
        density: u8,
        printhead_width_dots: u32,
    ) -> Result<Self, JobFailure> {
        let bpl = w.div_ceil(8);
        if src_bits_per_pixel == 1 && src_bytes_per_line > bpl {
            log::debug!(
                "KsJob::start: source rows are padded to {src_bytes_per_line} bytes, \
                 packing to {bpl}"
            );
        }
        let dither = configured_dither();
        log::info!(
            "KsJob::start: {w}x{h}, {src_bits_per_pixel}bpp in, bpl={bpl}, \
             density={density}, printhead={printhead_width_dots}, dither={dither:?}"
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
            ditherer: Ditherer::new(dither, w),
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

        // Zero margins: the IPP layer declares zero hard margins, so CUPS
        // hands us exactly the printable area.
        let profile = match dev.printer.as_ref() {
            Some(printer) => printer.profile().await,
            None => PrintProfile::TSeries,
        };

        let (compressed, avg) = match profile {
            PrintProfile::TSeries => {
                let buffers = split_into_buffers(
                    &canvas,
                    canvas_bpl as u8,
                    num_cols as u16,
                    0,
                    0,
                    Density::uniform(self.density),
                    PageOptions::default(),
                );

                compress_buffers(&buffers)
                    .map_err(|e| JobFailure::other(format!("compression: {e}")))?
            }
            PrintProfile::ESeries => {
                let buffers = split_into_buffers_profiled(
                    &canvas,
                    canvas_bpl as u8,
                    num_cols as u16,
                    0,
                    0,
                    Density::uniform(self.density),
                    PageOptions::default(),
                    profile,
                );

                compress_buffers_individually(&buffers)
                    .map_err(|e| JobFailure::other(format!("compression: {e}")))?
            }
        };

        let speed = calc_speed(avg);

        let outcome: Result<(), JobFailure> = if let Some(ref printer) = dev.printer {
            dev.printing.store(true, Ordering::Release);
            let result = printer.print_compressed(&compressed, speed).await;
            dev.printing.store(false, Ordering::Release);
            match result {
                Ok(()) => Ok(()),
                Err(ProtoError::InvalidResponse(msg)) => {
                    if let Ok(Some(s)) = printer.query_status().await {
                        let profile = printer.profile().await;
                        let fatal = s.label_rw_error
                            || s.label_end
                            || s.label_mode_error
                            || s.ribbon_rw_error
                            || (s.ribbon_end && profile.params().ribbon_end_is_fatal)
                            || s.cover_open
                            || s.head_temp_high
                            || s.label_not_installed;

                        if fatal {
                            Err(failure_from_status(&s, profile, "print_compressed"))
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

        let darkness = printer.darkness();
        // darkness is 0-100%; scale to 0-MAX_DENSITY, rounding to nearest.
        let density = ((darkness * MAX_DENSITY + 50) / 100) as u8;
        let printhead_width_dots = printer.printhead_width_dots();

        let mut ks = KsJob::start(
            dev,
            w,
            h,
            options.bits_per_pixel,
            options.bytes_per_line,
            density,
            printhead_width_dots,
        )?;
        if options.bits_per_pixel > 1 && dumps_enabled() {
            ks.pgm_acc = Some(PgmAccumulator::new(w, h));
        }
        Ok(ks)
    }

    fn write_line(&mut self, options: &JobOptions, y: u32, line: &[u8]) -> Result<(), JobFailure> {
        // Anything deeper than 1 bpp is a contone raster we have to dither
        // down to the printhead's 1-bit dots.
        if let Some(input) = to_gray_line(options.bits_per_pixel, line, options.width) {
            let width = options.width;
            if let Some(ref mut acc) = self.pgm_acc {
                acc.push_line(y, &input);
            }
            let bpl_1bpp = width.div_ceil(8) as usize;
            let mut mono = vec![0u8; bpl_1bpp];
            self.ditherer.line(&input, y, &mut mono);
            if !self.append_line(y, &mono) {
                return Err(JobFailure::other(format!(
                    "write_line: y={y} out of bounds"
                )));
            }
            return Ok(());
        }
        if options.bits_per_pixel != 1 {
            return Err(JobFailure::other(format!(
                "write_line: unsupported raster depth {} bpp",
                options.bits_per_pixel
            )));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// CUPS may pad a 1 bpp scanline wider than the page needs; keeping that
    /// padding shears every row, since `raster_to_column_major` re-reads the
    /// buffer at `ceil(width/8)`.
    #[test]
    fn start_packs_padded_1bpp_rows_to_the_page_stride() {
        let dev = KsDevice::open_mock();
        let (w, h) = (100u32, 4u32);
        let packed = w.div_ceil(8); // 13
        let padded = 16; // what CUPS handed us

        let mut ks = KsJob::start(&dev, w, h, 1, padded, 0, 128).unwrap();
        assert_eq!(ks.bytes_per_line, packed);
        assert_eq!(ks.raster_data.len(), (h * packed) as usize);

        // Every row arrives at the source stride, with the padding set.
        for y in 0..h {
            let mut line = vec![0x00u8; padded as usize];
            line[0] = 0xA5;
            for b in line.iter_mut().skip(packed as usize) {
                *b = 0xFF;
            }
            assert!(ks.append_line(y, &line));
        }

        // Each row starts where the packed stride says it does, and none of
        // the source padding leaked in.
        for y in 0..h {
            let row = &ks.raster_data[(y * packed) as usize..((y + 1) * packed) as usize];
            assert_eq!(row[0], 0xA5, "row {y} misaligned");
            assert!(row[1..].iter().all(|&b| b == 0x00), "row {y} kept padding");
        }
    }

    #[test]
    fn start_sizes_contone_pages_from_the_dithered_width() {
        let dev = KsDevice::open_mock();
        let ks = KsJob::start(&dev, 100, 4, 24, 300, 0, 128).unwrap();
        assert_eq!(ks.bytes_per_line, 13);
    }
}
