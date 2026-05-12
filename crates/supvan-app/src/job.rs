use std::sync::atomic::Ordering;

use supvan_proto::bitmap::{center_in_printhead, raster_to_column_major, DEFAULT_MARGIN_DOTS};
use supvan_proto::buffer::split_into_buffers;
use supvan_proto::compress::compress_buffers;
use supvan_proto::error::Error as ProtoError;
use supvan_proto::speed::calc_speed;
use supvan_proto::status::PrinterStatus;

use crate::dump::{dump_pbm, dump_printhead_pbm, PgmAccumulator};
use crate::printer_device::{
    KsDevice, PAPPL_PREASON_COVER_OPEN, PAPPL_PREASON_MEDIA_EMPTY, PAPPL_PREASON_MEDIA_JAM,
    PAPPL_PREASON_NONE, PAPPL_PREASON_OFFLINE, PAPPL_PREASON_OTHER,
};

/// Failure of a print job, carrying the PAPPL reason flags and a
/// human-readable message so callers can surface it to CUPS / IPP clients.
pub struct JobFailure {
    /// Bits to OR into `papplPrinterSetReasons`.
    pub preason: u32,
    /// Short message, suitable for `papplJobSetMessage` and the job log.
    pub message: String,
}

impl JobFailure {
    pub fn other(msg: impl Into<String>) -> Self {
        Self {
            preason: PAPPL_PREASON_OTHER,
            message: msg.into(),
        }
    }

    /// Translate a printer status with at least one error flag set into
    /// PAPPL reasons + a message. Falls back to `OTHER` if no specific
    /// flag matches.
    pub fn from_status(s: &PrinterStatus, context: &str) -> Self {
        let mut preason = PAPPL_PREASON_NONE;
        if s.cover_open {
            preason |= PAPPL_PREASON_COVER_OPEN;
        }
        if s.label_end || s.label_not_installed {
            preason |= PAPPL_PREASON_MEDIA_EMPTY;
        }
        if s.label_rw_error || s.label_mode_error || s.ribbon_rw_error {
            preason |= PAPPL_PREASON_MEDIA_JAM;
        }
        if preason == PAPPL_PREASON_NONE {
            preason = PAPPL_PREASON_OTHER;
        }
        let desc = s.error_description().unwrap_or_else(|| "unknown".into());
        Self {
            preason,
            message: format!("{context}: {desc}"),
        }
    }

    /// Translate a transport-level error. ENOTCONN / IO failures map to
    /// OFFLINE; everything else to OTHER.
    pub fn from_proto(e: ProtoError, context: &str) -> Self {
        let preason = match &e {
            ProtoError::Io(io) if io.raw_os_error() == Some(libc::ENOTCONN) => {
                PAPPL_PREASON_OFFLINE
            }
            ProtoError::Io(_) => PAPPL_PREASON_OFFLINE,
            _ => PAPPL_PREASON_OTHER,
        };
        Self {
            preason,
            message: format!("{context}: {e}"),
        }
    }
}

/// Opaque job handle accumulating raster scanlines.
pub struct KsJob {
    pub width: u32,
    pub height: u32,
    pub bytes_per_line: u32,
    pub raster_data: Vec<u8>,
    pub lines_received: u32,
    pub density: u8,
    pub printhead_width_dots: u32,
    /// Pre-dither PGM accumulator (only when SUPVAN_DUMP_DIR is set and input is 8bpp).
    pub pgm_acc: Option<PgmAccumulator>,
}

impl KsJob {
    /// Allocate the raster buffer. No printer protocol runs here: the
    /// entire CHECK_DEVICE → wait_ready → START_PRINT → … sequence happens
    /// in `end_page` via `print_compressed`, so CUPS drives the printer
    /// with the same single-shot flow as the CLI's test-print. Doing the
    /// CHECK_DEVICE early adds idle time before START_PRINT and an extra
    /// command frame the CLI doesn't send.
    pub fn start(
        _dev: &KsDevice,
        w: u32,
        h: u32,
        bpl: u32,
        density: u8,
        printhead_width_dots: u32,
    ) -> Result<Box<Self>, JobFailure> {
        log::info!("KsJob::start: {w}x{h}, bpl={bpl}, density={density}, printhead={printhead_width_dots}");

        Ok(Box::new(KsJob {
            width: w,
            height: h,
            bytes_per_line: bpl,
            raster_data: vec![0u8; (h * bpl) as usize],
            lines_received: 0,
            density,
            printhead_width_dots,
            pgm_acc: None,
        }))
    }

    /// Append a single raster scanline at position `y`.
    pub fn write_line(&mut self, y: u32, line: &[u8]) -> bool {
        if y >= self.height {
            return false;
        }

        let copy_len = line.len().min(self.bytes_per_line as usize);
        let offset = (y * self.bytes_per_line) as usize;
        self.raster_data[offset..offset + copy_len].copy_from_slice(&line[..copy_len]);
        self.lines_received += 1;

        #[allow(clippy::manual_is_multiple_of)]
        if self.lines_received % 100 == 0 {
            log::debug!(
                "KsJob::write_line: received {} / {} lines",
                self.lines_received,
                self.height
            );
        }
        true
    }

    /// Process and transfer a completed page.
    pub fn end_page(&mut self, dev: &KsDevice) -> Result<(), JobFailure> {
        let mock = dev.is_mock();

        log::info!(
            "KsJob::end_page: {}x{}, {} lines received, mock={}",
            self.width,
            self.height,
            self.lines_received,
            mock,
        );

        // Flush PGM accumulator if present
        if let Some(ref acc) = self.pgm_acc {
            acc.flush();
        }
        self.pgm_acc = None;

        // PBM dump (before any transforms)
        dump_pbm(
            &self.raster_data,
            self.width,
            self.height,
            self.bytes_per_line,
        );

        // 1. Rotate to column-major LSB-first
        log::debug!("KsJob::end_page: raster_to_column_major");
        let (col_data, num_cols, _col_bpl) =
            raster_to_column_major(&self.raster_data, self.width, self.height);

        // 2. Center in printhead canvas
        log::debug!(
            "KsJob::end_page: center_in_printhead ({}dots)",
            self.printhead_width_dots
        );
        let (canvas, canvas_bpl) =
            center_in_printhead(&col_data, num_cols, self.width, self.printhead_width_dots);

        // Dump printhead-sized image (column-major → viewable PBM)
        dump_printhead_pbm(&canvas, num_cols, canvas_bpl, self.printhead_width_dots);

        // 3. Split into print buffers
        log::debug!("KsJob::end_page: split_into_buffers");
        let buffers = split_into_buffers(
            &canvas,
            canvas_bpl as u8,
            num_cols as u16,
            DEFAULT_MARGIN_DOTS,
            DEFAULT_MARGIN_DOTS,
            self.density,
        );
        log::info!("KsJob::end_page: {} print buffers", buffers.len());

        // 4. Compress
        log::debug!("KsJob::end_page: compress_buffers");
        let (compressed, avg) = match compress_buffers(&buffers) {
            Ok(r) => r,
            Err(e) => {
                return Err(JobFailure::other(format!("compression failed: {e}")));
            }
        };
        let speed = calc_speed(avg);
        log::info!(
            "KsJob::end_page: compressed {} bytes (from {} buffers), avg={}, speed={}",
            compressed.len(),
            buffers.len(),
            avg,
            speed,
        );

        // 5+6: Transfer (real mode only). Delegate the entire print flow
        // (CHECK_DEVICE → wait_ready → START_PRINT → wait_printing →
        //  wait_buffer_ready → transfer → poll completion) to the same
        // `print_compressed` function that the CLI's `test-print` uses, so
        // CUPS and CLI are guaranteed to drive the printer identically.
        //
        // We keep `dev.printing` as a sentinel only for the (concurrent)
        // status callback; `print_compressed` already handles the START
        // sequencing for us.
        if let Some(ref printer) = dev.printer {
            log::info!(
                "KsJob::end_page: print_compressed density={} compressed_bytes={}",
                self.density,
                compressed.len()
            );
            dev.printing.store(true, Ordering::Release);
            let result = printer.print_compressed(&compressed, speed);
            dev.printing.store(false, Ordering::Release);
            if let Err(e) = result {
                if let ProtoError::InvalidResponse(msg) = &e {
                    if let Ok(Some(s)) = printer.query_status() {
                        if s.has_error() {
                            return Err(JobFailure::from_status(&s, "print_compressed"));
                        }
                    }
                    return Err(JobFailure::other(msg.clone()));
                }
                return Err(JobFailure::from_proto(e, "print_compressed"));
            }
        } else {
            log::info!("KsJob::end_page: mock — skipping BT transfer");
        }

        Ok(())
    }

    /// Clear the raster buffer for the next page.
    pub fn clear_page(&mut self) {
        self.raster_data.fill(0);
        self.lines_received = 0;
    }

    /// End a print job: wait for completion.
    pub fn end(self, dev: &KsDevice) {
        if let Some(ref printer) = dev.printer {
            log::info!("KsJob::end: waiting for completion");

            for i in 0..300 {
                std::thread::sleep(std::time::Duration::from_millis(100));
                match printer.query_status() {
                    Ok(Some(s)) => {
                        if !s.printing && !s.device_busy {
                            log::info!("KsJob::end: print complete (after {i} polls)");
                            break;
                        }
                    }
                    Ok(None) => {}
                    Err(e) => {
                        log::warn!("KsJob::end: status query error: {e}");
                        break;
                    }
                }
            }

            dev.printing.store(false, Ordering::Release);
        } else {
            log::info!("KsJob::end: mock — done");
        }
    }
}
