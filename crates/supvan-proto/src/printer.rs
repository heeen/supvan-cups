//! High-level printer operations.
//!
//! Implements the print flow from T50PlusPrint.doPrint():
//! CHECK_DEVICE -> poll ready -> START_PRINT -> poll printing ->
//! transfer buffers -> poll complete.

use crate::cmd::*;
use crate::data::{build_data_frames, DATA_PAYLOAD_SIZE};
use crate::error::{Error, Result};
use crate::rfcomm::RfcommSocket;
use crate::speed::calc_speed;
use crate::status::{self, MaterialInfo, PrinterStatus};
use std::os::unix::io::RawFd;
use std::time::Duration;

/// High-level printer interface over RFCOMM.
pub struct Printer {
    sock: RfcommSocket,
}

impl Printer {
    pub fn new(sock: RfcommSocket) -> Self {
        Self { sock }
    }

    /// Return the raw file descriptor of the underlying RFCOMM socket.
    pub fn raw_fd(&self) -> RawFd {
        self.sock.raw_fd()
    }

    /// Send a standard command and return raw response.
    pub fn send_cmd(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>> {
        let frame = make_cmd(cmd, param);
        self.sock.send_cmd(&frame)
    }

    /// Send a start-transfer command and return raw response.
    pub fn send_cmd_start_trans(
        &self,
        cmd: u8,
        block_size: u16,
        block_count: u16,
    ) -> Result<Option<Vec<u8>>> {
        let frame = make_cmd_start_trans(cmd, block_size, block_count);
        self.sock.send_cmd(&frame)
    }

    /// CHECK_DEVICE (0x12) - verify printer is present.
    pub fn check_device(&self) -> Result<bool> {
        log::info!("CHECK_DEVICE");
        let resp = self.send_cmd(CMD_CHECK_DEVICE, 0)?;
        Ok(resp.is_some_and(|r| status::validate_response(&r, CMD_CHECK_DEVICE)))
    }

    /// INQUIRY_STA (0x11) - query printer status.
    pub fn query_status(&self) -> Result<Option<PrinterStatus>> {
        let resp = self.send_cmd(CMD_INQUIRY_STA, 0)?;
        Ok(resp.and_then(|r| status::parse_status(&r)))
    }

    /// RETURN_MAT (0x30) - query material/label info.
    pub fn query_material(&self) -> Result<Option<MaterialInfo>> {
        log::info!("RETURN_MAT");
        let resp = self.send_cmd(CMD_RETURN_MAT, 0)?;
        Ok(resp.and_then(|r| status::parse_material(&r)))
    }

    /// RD_DEV_NAME (0x16) - read device name.
    pub fn read_device_name(&self) -> Result<Option<String>> {
        log::info!("RD_DEV_NAME");
        let resp = self.send_cmd(CMD_RD_DEV_NAME, 0)?;
        Ok(resp.and_then(|r| status::parse_device_name(&r)))
    }

    /// READ_FWVER (0xC5) - read firmware version.
    pub fn read_firmware_version(&self) -> Result<Option<u8>> {
        log::info!("READ_FWVER");
        let resp = self.send_cmd(CMD_READ_FWVER, 0)?;
        Ok(resp.and_then(|r| status::parse_firmware_version(&r)))
    }

    /// READ_REV (0x17) - read protocol version.
    pub fn read_version(&self) -> Result<Option<String>> {
        log::info!("READ_REV");
        let resp = self.send_cmd(CMD_READ_REV, 0)?;
        Ok(resp.and_then(|r| status::parse_version(&r)))
    }

    /// START_PRINT (0x13).
    pub fn start_print(&self) -> Result<Option<Vec<u8>>> {
        log::info!("START_PRINT");
        self.send_cmd(CMD_START_PRINT, 0)
    }

    /// STOP_PRINT (0x14).
    pub fn stop_print(&self) -> Result<Option<Vec<u8>>> {
        log::info!("STOP_PRINT");
        self.send_cmd(CMD_STOP_PRINT, 0)
    }

    /// Wait for device to be idle (not busy, not printing).
    pub fn wait_ready(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for _ in 0..max_attempts {
            let st = self.query_status()?;
            if let Some(ref s) = st {
                if !s.device_busy && !s.printing {
                    return Ok(st);
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(None)
    }

    /// Wait for printing station to become active.
    pub fn wait_printing(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for _ in 0..max_attempts {
            let st = self.query_status()?;
            if let Some(ref s) = st {
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

    /// Transfer a single compressed buffer: NEXT_ZIPPEDBULK -> data packets -> BUF_FULL.
    pub fn transfer_compressed(&self, compressed: &[u8], speed: u16) -> Result<()> {
        let num_packets = compressed.len().div_ceil(DATA_PAYLOAD_SIZE);
        log::info!(
            "transfer: {} bytes, {} packets, speed={}",
            compressed.len(),
            num_packets,
            speed
        );

        // CMD_NEXT_ZIPPEDBULK (0x5C) with block_size=512, block_count=num_packets
        let resp = self.send_cmd_start_trans(CMD_NEXT_ZIPPEDBULK, 512, num_packets as u16)?;
        if resp.is_none() {
            return Err(Error::InvalidResponse(
                "no response to NEXT_ZIPPEDBULK".into(),
            ));
        }

        // Send data packets
        let frames = build_data_frames(compressed);
        for (i, frame) in frames.iter().enumerate() {
            let is_last = i == frames.len() - 1;
            self.sock.send_data_frame(frame, is_last)?;
        }

        // 20ms delay after last data packet
        std::thread::sleep(Duration::from_millis(20));

        // CMD_BUF_FULL: param=compressed_length, block_count=speed
        let compressed_len = compressed.len() as u16;
        log::info!("BUF_FULL: len={}, speed={}", compressed_len, speed);
        self.send_cmd_start_trans(CMD_BUF_FULL, compressed_len, speed)?;

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
            .wait_ready(60)?
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
        self.wait_printing(60)?
            .ok_or_else(|| Error::InvalidResponse("timeout waiting for printing station".into()))?;

        // Step 5: Wait buffer + transfer
        let buf_status = self
            .wait_buffer_ready(200)?
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
        for _ in 0..300 {
            std::thread::sleep(Duration::from_millis(100));
            if let Some(s) = self.query_status()? {
                if !s.printing && !s.device_busy {
                    log::info!("print complete");
                    return Ok(());
                }
            }
        }

        log::warn!("timeout waiting for print completion");
        Ok(())
    }

    /// Full test print workflow: generate test pattern, build buffers, compress, print.
    pub fn test_print(&self, mat: &MaterialInfo, density: u8) -> Result<()> {
        use crate::bitmap::create_test_pattern;
        use crate::buffer::split_into_buffers;
        use crate::compress::compress_buffers;

        let label_width_mm = (mat.width_mm as u32).min(crate::bitmap::PRINTHEAD_WIDTH_MM);
        let height_mm = if mat.height_mm == 0 {
            25
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
