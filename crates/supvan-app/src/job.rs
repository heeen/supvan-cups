use std::sync::atomic::Ordering;

use supvan_proto::bitmap::{
    center_in_printhead, raster_to_column_major, DEFAULT_MARGIN_DOTS, PRINTHEAD_WIDTH_DOTS,
};
use supvan_proto::buffer::split_into_buffers;
use supvan_proto::compress::compress_buffers;
use supvan_proto::speed::calc_speed;

use crate::dump::{dump_pbm, dump_printhead_pbm, PgmAccumulator};
use crate::printer_device::KsDevice;

/// Opaque job handle accumulating raster scanlines.
pub struct KsJob {
    pub width: u32,
    pub height: u32,
    pub bytes_per_line: u32,
    pub raster_data: Vec<u8>,
    pub lines_received: u32,
    pub density: u8,
    /// Pre-dither PGM accumulator (only when SUPVAN_DUMP_DIR is set and input is 8bpp).
    pub pgm_acc: Option<PgmAccumulator>,
}

impl KsJob {
    /// Start a print job: runs CHECK_DEVICE -> wait_ready -> START_PRINT -> wait_printing.
    ///
    /// In mock mode, skips all printer protocol and just allocates the raster buffer.
    pub fn start(dev: &KsDevice, w: u32, h: u32, bpl: u32, density: u8) -> Option<Box<Self>> {
        let mock = dev.is_mock();

        log::info!("KsJob::start: {w}x{h}, bpl={bpl}, density={density}, mock={mock}");

        if let Some(ref printer) = dev.printer {
            log::debug!("KsJob::start: CHECK_DEVICE");
            match printer.check_device() {
                Ok(true) => {}
                Ok(false) => {
                    log::error!("KsJob::start: CHECK_DEVICE failed");
                    return None;
                }
                Err(e) => {
                    log::error!("KsJob::start: CHECK_DEVICE error: {e}");
                    return None;
                }
            }

            log::debug!("KsJob::start: waiting for ready");
            match printer.wait_ready(60) {
                Ok(Some(s)) => {
                    if s.has_error() {
                        log::error!(
                            "KsJob::start: printer error: {}",
                            s.error_description().unwrap_or_default()
                        );
                        return None;
                    }
                }
                Ok(None) => {
                    log::error!("KsJob::start: timeout waiting for device ready");
                    return None;
                }
                Err(e) => {
                    log::error!("KsJob::start: wait_ready error: {e}");
                    return None;
                }
            }

            log::debug!("KsJob::start: START_PRINT");
            if let Err(e) = printer.start_print() {
                log::error!("KsJob::start: START_PRINT error: {e}");
                return None;
            }

            log::debug!("KsJob::start: waiting for printing station");
            match printer.wait_printing(60) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    log::error!("KsJob::start: timeout waiting for printing station");
                    return None;
                }
                Err(e) => {
                    log::error!("KsJob::start: wait_printing error: {e}");
                    return None;
                }
            }

            dev.printing.store(true, Ordering::Release);
        } else {
            log::debug!("KsJob::start: mock — skipping printer protocol");
        }

        Some(Box::new(KsJob {
            width: w,
            height: h,
            bytes_per_line: bpl,
            raster_data: vec![0u8; (h * bpl) as usize],
            lines_received: 0,
            density,
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

        if self.lines_received.is_multiple_of(100) {
            log::debug!(
                "KsJob::write_line: received {} / {} lines",
                self.lines_received,
                self.height
            );
        }
        true
    }

    /// Process and transfer a completed page.
    pub fn end_page(&mut self, dev: &KsDevice) -> bool {
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

        // 2. Center in printhead canvas (384 dots = 48mm)
        log::debug!("KsJob::end_page: center_in_printhead");
        let (canvas, canvas_bpl) =
            center_in_printhead(&col_data, num_cols, self.width, PRINTHEAD_WIDTH_DOTS);

        // Dump printhead-sized image (column-major → viewable PBM)
        dump_printhead_pbm(&canvas, num_cols, canvas_bpl, PRINTHEAD_WIDTH_DOTS);

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
                log::error!("KsJob::end_page: compression failed: {e}");
                return false;
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

        // 5+6: Transfer (real mode only)
        if let Some(ref printer) = dev.printer {
            log::debug!("KsJob::end_page: wait_buffer_ready");
            match printer.wait_buffer_ready(200) {
                Ok(Some(s)) => {
                    if s.has_error() {
                        log::error!(
                            "KsJob::end_page: printer error: {}",
                            s.error_description().unwrap_or_default()
                        );
                        return false;
                    }
                }
                Ok(None) => {
                    log::error!("KsJob::end_page: timeout waiting for buffer space");
                    return false;
                }
                Err(e) => {
                    log::error!("KsJob::end_page: wait_buffer_ready error: {e}");
                    return false;
                }
            }

            log::debug!(
                "KsJob::end_page: transfer_compressed ({} bytes)",
                compressed.len()
            );
            if let Err(e) = printer.transfer_compressed(&compressed, speed) {
                log::error!("KsJob::end_page: transfer failed: {e}");
                return false;
            }
        } else {
            log::info!("KsJob::end_page: mock — skipping BT transfer");
        }

        // Clear raster buffer for next page
        self.raster_data.fill(0);
        self.lines_received = 0;

        true
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
