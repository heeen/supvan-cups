use std::sync::LazyLock;
use std::time::{Duration, Instant};

use ipp_printer_app::{JobFailure, JobOptions, PrinterHandle, PrinterReason, RasterDriver};
use supvan_proto::buffer::{Density, MAX_DENSITY};
use supvan_proto::dither::{DitherMode, Ditherer, to_gray_line};
use supvan_proto::error::Error as ProtoError;
use supvan_proto::page;
use supvan_proto::status::PrinterStatus;

use crate::dump::{JobDump, JobManifest, PgmAccumulator, dumps_enabled};
use crate::mock;
use crate::printer_device::KsDevice;

/// Halftone kernel for 8bpp input, from `SUPVAN_DITHER` — the app takes all its
/// other configuration from the environment too.
static DITHER: LazyLock<DitherMode> = LazyLock::new(|| {
    crate::env::parsed("SUPVAN_DITHER", &DitherMode::NAMES.join("|"), |s| {
        s.parse().ok()
    })
    .unwrap_or_default()
});

/// Poll cadence and budget while waiting for print completion
/// (COMPLETION_POLLS × COMPLETION_POLL_INTERVAL = 30s).
const COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const COMPLETION_POLLS: u32 = 300;

/// `unix:<seconds>` — enough to order dumps without pulling in chrono.
fn unix_timestamp() -> String {
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
    width: u32,
    height: u32,
    bytes_per_line: u32,
    raster_data: Vec<u8>,
    lines_received: u32,
    density: u8,
    printhead_width_dots: u32,
    pgm_acc: Option<PgmAccumulator>,
    ditherer: Ditherer,
    /// Scratch line for dithered output, reused across lines.
    mono: Vec<u8>,
}

impl KsJob {
    /// Allocate the page buffer for an incoming raster.
    ///
    /// The buffer is always tightly packed 1 bpp at `ceil(w/8)`, whatever the
    /// source says: contone lines are dithered before `append_line`, and
    /// [`page::printhead_canvas`] re-reads the buffer at that same stride.
    /// Sizing from `src_bytes_per_line` instead copies raw RGB into the bitmap
    /// (white `0xFF` becomes eight set dots — a black label), or, for a padded
    /// 1 bpp source, shears every row. `append_line` truncates the padding
    /// away as each line arrives.
    fn start(
        w: u32,
        h: u32,
        src_bits_per_pixel: u32,
        src_bytes_per_line: u32,
        density: u8,
        printhead_width_dots: u32,
    ) -> Self {
        let bpl = w.div_ceil(8);
        if src_bits_per_pixel == 1 && src_bytes_per_line > bpl {
            log::debug!(
                "KsJob::start: source rows are padded to {src_bytes_per_line} bytes, \
                 packing to {bpl}"
            );
        }
        let dither = *DITHER;
        log::info!(
            "KsJob::start: {w}x{h}, {src_bits_per_pixel}bpp in, bpl={bpl}, \
             density={density}, printhead={printhead_width_dots}, dither={dither:?}"
        );
        KsJob {
            width: w,
            height: h,
            bytes_per_line: bpl,
            raster_data: vec![0u8; (h * bpl) as usize],
            lines_received: 0,
            density,
            printhead_width_dots,
            pgm_acc: None,
            ditherer: Ditherer::new(dither, w),
            mono: vec![0u8; bpl as usize],
        }
    }

    fn append_line(&mut self, y: u32, line: &[u8]) -> Result<(), JobFailure> {
        if y >= self.height {
            return Err(JobFailure::other(format!(
                "write_line: y={y} out of bounds"
            )));
        }
        let copy_len = line.len().min(self.bytes_per_line as usize);
        let offset = (y * self.bytes_per_line) as usize;
        self.raster_data[offset..offset + copy_len].copy_from_slice(&line[..copy_len]);
        self.lines_received += 1;
        Ok(())
    }

    /// The page as print buffers, with its dumps written under one sequence
    /// number.
    fn page_buffers(&mut self, dump: &JobDump) -> Vec<page::PrintBuffer> {
        if let Some(acc) = self.pgm_acc.take() {
            dump.pgm(&acc);
        }
        dump.pbm(
            &self.raster_data,
            self.width,
            self.height,
            self.bytes_per_line,
        );
        let canvas = page::printhead_canvas(
            &self.raster_data,
            self.width,
            self.height,
            self.printhead_width_dots,
        );
        dump.printhead_pbm(&canvas, self.printhead_width_dots);
        // IPP carries one print-quality knob, so both trims move together —
        // the vendor's own behaviour when its packed density fits in a byte.
        page::full_bleed_buffers(&canvas, Density::uniform(self.density))
    }

    /// Print one copy of the page, or simulate it on the mock.
    async fn print(dev: &KsDevice, buffers: &[page::PrintBuffer]) -> Result<(), JobFailure> {
        let Some(printer) = dev.printer().await else {
            // Mock device: simulate the print delay, then check the simulator
            // for a queued failure. Dumps already happened, so the operator
            // can still inspect the output even on a simulated abort.
            tokio::time::sleep(mock::controller().delay()).await;
            if let Some(f) = mock::controller().take_print_failure() {
                return Err(f);
            }
            log::info!("KsJob: mock — dumped, no transfer");
            return Ok(());
        };
        match printer.print_buffers(buffers).await {
            Ok(()) => Ok(()),
            Err(ProtoError::InvalidResponse(msg)) => {
                if let Ok(Some(s)) = printer.query_status().await
                    && s.has_error()
                {
                    Err(failure_from_status(&s, "print_buffers"))
                } else {
                    Err(JobFailure::other(msg))
                }
            }
            Err(e) => Err(failure_from_proto(e, "print_buffers")),
        }
    }

    /// Print the accumulated page `copies` times, then clear it for the next.
    async fn print_page(&mut self, dev: &KsDevice, copies: u32) -> Result<(), JobFailure> {
        let started = Instant::now();
        log::info!(
            "KsJob::print_page: {}x{}, {} lines, {copies} copies, mock={}",
            self.width,
            self.height,
            self.lines_received,
            dev.is_mock(),
        );
        let dump = JobDump::allocate();
        let buffers = self.page_buffers(&dump);

        let mut outcome = Ok(());
        for copy in 1..=copies {
            if copies > 1 {
                log::info!("print_page: copy {copy}/{copies}");
            }
            outcome = Self::print(dev, &buffers).await;
            if outcome.is_err() {
                break;
            }
        }

        dump.manifest(&JobManifest {
            timestamp: unix_timestamp(),
            width: self.width,
            height: self.height,
            bytes_per_line: self.bytes_per_line,
            density: self.density,
            printhead_width_dots: self.printhead_width_dots,
            copies,
            mock: dev.is_mock(),
            simulated_outcome: match &outcome {
                Ok(()) => "completed".to_string(),
                Err(f) => format!("aborted: {}", f.message),
            },
            elapsed_ms: started.elapsed().as_millis(),
        });

        self.raster_data.fill(0);
        self.lines_received = 0;
        outcome
    }
}

#[async_trait::async_trait]
impl RasterDriver for KsJob {
    type Device = KsDevice;

    fn start_job(
        printer: &PrinterHandle<'_>,
        options: &JobOptions,
        _dev: &Self::Device,
    ) -> Result<Self, JobFailure> {
        let w = options.width;
        let h = options.height;

        // Darkness is 0-100%; scale to 0-MAX_DENSITY, rounding to nearest.
        let max = i32::from(MAX_DENSITY);
        let density = ((printer.darkness() * max + 50) / 100) as u8;

        let mut ks = KsJob::start(
            w,
            h,
            options.bits_per_pixel,
            options.bytes_per_line,
            density,
            printer.printhead_width_dots(),
        );
        if options.bits_per_pixel > 1 && dumps_enabled() {
            ks.pgm_acc = Some(PgmAccumulator::new(w, h));
        }
        Ok(ks)
    }

    fn write_line(&mut self, options: &JobOptions, y: u32, line: &[u8]) -> Result<(), JobFailure> {
        // Anything deeper than 1 bpp is a contone raster we have to dither
        // down to the printhead's 1-bit dots.
        let Some(gray) = to_gray_line(options.bits_per_pixel, line, options.width) else {
            if options.bits_per_pixel != 1 {
                return Err(JobFailure::other(format!(
                    "write_line: unsupported raster depth {} bpp",
                    options.bits_per_pixel
                )));
            }
            return self.append_line(y, line);
        };
        if let Some(acc) = &mut self.pgm_acc {
            acc.push_line(y, &gray);
        }
        let mut mono = std::mem::take(&mut self.mono);
        mono.fill(0);
        self.ditherer.line(&gray, y, &mut mono);
        let appended = self.append_line(y, &mono);
        self.mono = mono;
        appended
    }

    async fn end_page(
        &mut self,
        options: &JobOptions,
        _page: u32,
        dev: &Self::Device,
    ) -> Result<(), JobFailure> {
        self.print_page(dev, options.copies).await
    }

    async fn end_job(self, dev: &Self::Device) {
        for i in 0..COMPLETION_POLLS {
            tokio::time::sleep(COMPLETION_POLL_INTERVAL).await;
            let Some(printer) = dev.printer().await else {
                return;
            };
            match printer.query_status().await {
                Ok(Some(s)) if !s.printing && !s.device_busy => {
                    log::info!("KsJob::end_job: complete after {i} polls");
                    return;
                }
                Ok(_) => {}
                Err(e) => {
                    log::warn!("KsJob::end_job: status error: {e}");
                    return;
                }
            }
        }
        log::warn!("KsJob::end_job: timeout waiting for completion");
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
        let (w, h) = (100u32, 4u32);
        let packed = w.div_ceil(8); // 13
        let padded = 16; // what CUPS handed us

        let mut ks = KsJob::start(w, h, 1, padded, 0, 128);
        assert_eq!(ks.bytes_per_line, packed);
        assert_eq!(ks.raster_data.len(), (h * packed) as usize);

        // Every row arrives at the source stride, with the padding set.
        for y in 0..h {
            let mut line = vec![0x00u8; padded as usize];
            line[0] = 0xA5;
            for b in line.iter_mut().skip(packed as usize) {
                *b = 0xFF;
            }
            ks.append_line(y, &line).unwrap();
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
        let ks = KsJob::start(100, 4, 24, 300, 0, 128);
        assert_eq!(ks.bytes_per_line, 13);
    }
}
