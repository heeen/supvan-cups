//! High-level printer operations.
//!
//! Implements the print flow from T50PlusPrint.doPrint():
//! CHECK_DEVICE -> poll ready -> START_PRINT -> poll printing ->
//! transfer buffers -> poll complete.

use crate::cmd::*;
use crate::data::DATA_PAYLOAD_SIZE;
use crate::error::{Error, Result};
use crate::speed::calc_speed;
use crate::status::{MaterialInfo, PrinterStatus};
use crate::transport::Transport;
use std::time::Duration;

/// Status-poll attempt budgets for the print state machine; each is multiplied
/// by the poll interval inside its wait loop.
const READY_ATTEMPTS: usize = 60;
const PRINTING_ATTEMPTS: usize = 60;
const BUFFER_READY_ATTEMPTS: usize = 200;

/// Wait-for-completion budget: COMPLETION_POLLS × COMPLETION_POLL_INTERVAL = 30s.
const COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const COMPLETION_POLLS: usize = 300;

/// High-level printer interface over a pluggable transport.
pub struct Printer {
    transport: Box<dyn Transport>,
}

impl Printer {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self { transport }
    }

    /// Open a USB HID printer at the given `/dev/hidrawN` path.
    pub fn open_usb(path: &str) -> Result<Self> {
        let dev = crate::hidraw::HidrawDevice::open(path)?;
        Ok(Self::new(Box::new(
            crate::usb_transport::UsbHidTransport::new(dev),
        )))
    }

    /// Open a Bluetooth printer at the given RFCOMM address (`AA:BB:CC:DD:EE:FF`).
    pub fn open_bt(addr: &str) -> Result<Self> {
        let sock = crate::rfcomm::RfcommSocket::connect_default(addr)?;
        Ok(Self::new(Box::new(crate::spp_pipe::SppCodec::new(sock))))
    }

    /// Open a printer from a target string: a `/dev/hidrawN` path selects USB
    /// HID, anything else is treated as a Bluetooth address.
    pub fn open_target(target: &str) -> Result<Self> {
        if target.starts_with("/dev/hidraw") {
            Self::open_usb(target)
        } else {
            Self::open_bt(target)
        }
    }

    /// CHECK_DEVICE (0x12) - verify printer is present.
    pub fn check_device(&self) -> Result<bool> {
        log::info!("CHECK_DEVICE");
        let resp = self.transport.send_cmd(CMD_CHECK_DEVICE, 0)?;
        Ok(resp.is_some_and(|r| self.transport.validate_response(&r, CMD_CHECK_DEVICE)))
    }

    /// INQUIRY_STA (0x11) - query printer status.
    pub fn query_status(&self) -> Result<Option<PrinterStatus>> {
        let resp = self.transport.send_cmd(CMD_INQUIRY_STA, 0)?;
        Ok(resp.and_then(|r| self.transport.parse_status_response(&r)))
    }

    /// RETURN_MAT (0x30) - query material/label info.
    pub fn query_material(&self) -> Result<Option<MaterialInfo>> {
        log::info!("RETURN_MAT");
        let resp = self.transport.send_cmd(CMD_RETURN_MAT, 0)?;
        Ok(resp.and_then(|r| self.transport.parse_material_response(&r)))
    }

    /// RD_DEV_NAME (0x16) - read device name.
    pub fn read_device_name(&self) -> Result<Option<String>> {
        log::info!("RD_DEV_NAME");
        let resp = self.transport.send_cmd(CMD_RD_DEV_NAME, 0)?;
        Ok(resp.and_then(|r| self.transport.parse_device_name_response(&r)))
    }

    /// READ_FWVER (0xC5) - read firmware version.
    pub fn read_firmware_version(&self) -> Result<Option<u8>> {
        log::info!("READ_FWVER");
        let resp = self.transport.send_cmd(CMD_READ_FWVER, 0)?;
        Ok(resp.and_then(|r| self.transport.parse_firmware_version_response(&r)))
    }

    /// READ_REV (0x17) - read protocol version.
    pub fn read_version(&self) -> Result<Option<String>> {
        log::info!("READ_REV");
        let resp = self.transport.send_cmd(CMD_READ_REV, 0)?;
        Ok(resp.and_then(|r| self.transport.parse_version_response(&r)))
    }

    /// START_PRINT (0x13).
    pub fn start_print(&self) -> Result<Option<Vec<u8>>> {
        log::info!("START_PRINT");
        self.transport.send_cmd(CMD_START_PRINT, 0)
    }

    /// STOP_PRINT (0x14).
    pub fn stop_print(&self) -> Result<Option<Vec<u8>>> {
        log::info!("STOP_PRINT");
        self.transport.send_cmd(CMD_STOP_PRINT, 0)
    }

    /// PAPER_SKIP (0x2E) — feed/advance one blank label. Returns `Ok(())` once
    /// the device acks; errors if there is no response.
    pub fn paper_skip(&self) -> Result<()> {
        log::info!("PAPER_SKIP");
        let resp = self.transport.send_cmd(CMD_PAPER_SKIP, 0)?;
        if resp.is_some_and(|r| self.transport.validate_response(&r, CMD_PAPER_SKIP)) {
            Ok(())
        } else {
            Err(Error::InvalidResponse("PAPER_SKIP: no ack".into()))
        }
    }

    /// Wait for device to be idle (not busy, not printing).
    pub fn wait_ready(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for _ in 0..max_attempts {
            let st = self.query_status()?;
            if let Some(ref s) = st
                && !s.device_busy
                && !s.printing
            {
                return Ok(st);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(None)
    }

    /// Wait for printing station to become active.
    ///
    /// Aborts early via `Error::InvalidResponse` if the printer raises an
    /// error flag (label end, cover open, mode mismatch, etc.) — those
    /// states cause the firmware to drop the BT link and beep, and there's
    /// no point continuing the print.
    pub fn wait_printing(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for _ in 0..max_attempts {
            let st = self.query_status()?;
            if let Some(ref s) = st {
                if s.has_error() {
                    return Err(Error::InvalidResponse(format!(
                        "printer error after START_PRINT: {}",
                        s.error_description().unwrap_or_default()
                    )));
                }
                if s.printing {
                    return Ok(st);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(None)
    }

    /// Wait for buffer space available (buf_full == false).
    pub fn wait_buffer_ready(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for i in 0..max_attempts {
            std::thread::sleep(Duration::from_millis(20));
            let st = self.query_status()?;
            if let Some(ref s) = st {
                if s.has_error() {
                    return Err(Error::InvalidResponse(format!(
                        "printer error while waiting for buffer: {}",
                        s.error_description().unwrap_or_default()
                    )));
                }
                if !s.buf_full {
                    return Ok(st);
                }
            }
            if i % 10 == 0 && i > 0 {
                log::debug!("waiting for buffer space... ({i})");
            }
        }
        Ok(None)
    }

    /// Transfer the compressed print buffers as a single LZMA stream:
    /// NEXT_ZIPPEDBULK -> data packets -> BUF_FULL.
    ///
    /// The printer's decoder splits the decompressed stream on 4096-byte
    /// boundaries internally, so one transfer covers all the page's buffers.
    pub fn transfer_compressed(&self, compressed: &[u8], speed: u16) -> Result<()> {
        let compressed_len = compressed.len() as u16;

        // CMD_NEXT_ZIPPEDBULK (0x5C): each transport encodes the header in its
        // own convention (SPP: block_size=512 + packet count; USB: total length).
        let num_packets = compressed.len().div_ceil(DATA_PAYLOAD_SIZE);
        log::info!(
            "transfer: {} bytes, {} packets, speed={}",
            compressed.len(),
            num_packets,
            speed
        );
        let resp = self.transport.send_bulk_header(compressed_len, num_packets)?;
        if resp.is_none() {
            return Err(Error::InvalidResponse(
                "no response to NEXT_ZIPPEDBULK".into(),
            ));
        }

        // Send data packets via the transport. We do NOT read a response
        // after the last frame: the protocol acks the bulk only via the
        // BUF_FULL reply that follows. Polling for a non-existent response
        // here blocks for the read timeout (2s on BT), during which the
        // printer queues the bytes, times out waiting for BUF_FULL, errors
        // (3-beep) and drops the RFCOMM link before BUF_FULL arrives.
        self.transport.send_bulk_data(compressed, false)?;

        // 20ms delay after last data packet
        std::thread::sleep(Duration::from_millis(20));

        // CMD_BUF_FULL: param=compressed_length, param2=speed
        log::info!("BUF_FULL: len={}, speed={}", compressed_len, speed);
        self.transport
            .send_cmd_two(CMD_BUF_FULL, compressed_len, speed)?;

        Ok(())
    }

    /// Execute a full print job with pre-compressed data.
    ///
    /// This is the main print flow from T50PlusPrint.doPrint():
    /// 1. CHECK_DEVICE
    /// 2. Wait ready
    /// 3. START_PRINT
    /// 4. Wait printing station
    /// 5. Wait buffer ready + transfer
    /// 6. Wait completion
    pub fn print_compressed(&self, compressed: &[u8], speed: u16) -> Result<()> {
        // Step 1: Check device
        if !self.check_device()? {
            return Err(Error::InvalidResponse("CHECK_DEVICE failed".into()));
        }

        // Step 2: Wait ready
        let status = self
            .wait_ready(READY_ATTEMPTS)?
            .ok_or_else(|| Error::InvalidResponse("timeout waiting for device ready".into()))?;
        if status.has_error() {
            return Err(Error::InvalidResponse(format!(
                "printer error: {}",
                status.error_description().unwrap_or_default()
            )));
        }

        // Step 3: Start print
        self.start_print()?;

        // Step 4: Wait printing station
        self.wait_printing(PRINTING_ATTEMPTS)?
            .ok_or_else(|| Error::InvalidResponse("timeout waiting for printing station".into()))?;

        // Step 5: Wait buffer + transfer
        let buf_status = self
            .wait_buffer_ready(BUFFER_READY_ATTEMPTS)?
            .ok_or_else(|| Error::InvalidResponse("timeout waiting for buffer space".into()))?;
        if buf_status.has_error() {
            self.stop_print()?;
            return Err(Error::InvalidResponse(format!(
                "printer error: {}",
                buf_status.error_description().unwrap_or_default()
            )));
        }
        self.transfer_compressed(compressed, speed)?;

        // Step 6: Wait completion
        for _ in 0..COMPLETION_POLLS {
            std::thread::sleep(COMPLETION_POLL_INTERVAL);
            if let Some(s) = self.query_status()?
                && !s.printing
                && !s.device_busy
            {
                log::info!("print complete");
                return Ok(());
            }
        }

        log::warn!("timeout waiting for print completion");
        Err(Error::Timeout("print completion"))
    }

    /// Full test print workflow: generate test pattern, build buffers, compress, print.
    pub fn test_print(&self, mat: &MaterialInfo, density: u8) -> Result<()> {
        use crate::bitmap::create_test_pattern;
        use crate::buffer::split_into_buffers;
        use crate::compress::compress_buffers;

        let label_width_mm = (mat.width_mm as u32).min(crate::bitmap::PRINTHEAD_WIDTH_MM);
        let height_mm = if mat.height_mm == 0 {
            crate::status::DEFAULT_LABEL_HEIGHT_MM as u32
        } else {
            mat.height_mm as u32
        };

        log::info!(
            "test print: {}mm x {}mm, density={}",
            label_width_mm,
            height_mm,
            density
        );

        let (image_data, _w, h, bpl) = create_test_pattern(label_width_mm, height_mm);
        let buffers = split_into_buffers(&image_data, bpl as u8, h as u16, 8, 8, density);
        log::info!("{} print buffers", buffers.len());

        let (compressed, avg) = compress_buffers(&buffers)?;
        let speed = calc_speed(avg);
        log::info!(
            "compressed: {} bytes, avg={}/buf, speed={}",
            compressed.len(),
            avg,
            speed
        );

        self.print_compressed(&compressed, speed)
    }
}
