//! High-level printer operations.
//!
//! Implements the print flow from T50PlusPrint.doPrint():
//! CHECK_DEVICE -> poll ready -> START_PRINT -> poll printing ->
//! transfer buffers -> poll complete.

use crate::buffer::{ColourMode, Density, PageOptions};
use crate::cmd::*;
use crate::data::DATA_PAYLOAD_SIZE;
use crate::error::{Error, Result};
use crate::profile::PrintProfile;
use crate::speed::calc_speed;
use crate::status::{MaterialInfo, PrinterStatus};
use crate::transport::Transport;
use std::time::Duration;

/// Status-poll attempt budgets for the print state machine; each is multiplied
/// by the poll interval inside its wait loop.
const READY_ATTEMPTS: usize = 60;
const PRINTING_ATTEMPTS: usize = 60;
const BUFFER_READY_ATTEMPTS: usize = 200;

/// Print speed for a page that needs more than one compressed block. The
/// vendor's fixed value; the head must not outrun the transfers.
const MULTI_BLOCK_SPEED: u16 = 20;

/// Pause around each block transfer, matching the vendor's own pacing
/// (`sendMatrix`/`cmdbuffull` both wait this long before acting).
const BLOCK_SETTLE: Duration = Duration::from_millis(100);

/// Wait-for-completion budget: COMPLETION_POLLS × COMPLETION_POLL_INTERVAL = 30s.
const COMPLETION_POLL_INTERVAL: Duration = Duration::from_millis(100);
const COMPLETION_POLLS: usize = 300;

/// Strip the `ble://` (or `ble:`) scheme from a target string.
fn ble_target(target: &str) -> Option<&str> {
    target
        .strip_prefix("ble://")
        .or_else(|| target.strip_prefix("ble:"))
}

/// High-level printer interface over a pluggable transport.
pub struct Printer {
    transport: Box<dyn Transport>,
    profile: PrintProfile,
}

impl Printer {
    pub fn new(transport: Box<dyn Transport>) -> Self {
        Self { transport, profile: PrintProfile::default() }
    }

    pub fn set_profile(&mut self, profile: PrintProfile) {
        self.profile = profile;
    }

    pub fn profile(&self) -> PrintProfile {
        self.profile
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

    /// Open a BLE GATT printer by address (E11/E12-class hardware). Async
    /// because the `bluer` GATT client is natively async. Requires the `ble`
    /// feature.
    #[cfg(feature = "ble")]
    pub async fn open_ble(addr: &str) -> Result<Self> {
        let pipe = crate::ble::BlePipe::connect(addr).await?;
        Ok(Self::new(Box::new(crate::spp_pipe::SppCodec::new(pipe))))
    }

    /// Open a printer from a target string:
    /// - `/dev/hidrawN` — USB HID
    /// - `ble://AA:BB:CC:DD:EE:FF` — BLE GATT (needs the `ble` feature)
    /// - anything else — a Classic Bluetooth RFCOMM address
    pub async fn open_target(target: &str) -> Result<Self> {
        if target.starts_with("/dev/hidraw") {
            return Self::open_usb(target);
        }
        if let Some(addr) = ble_target(target) {
            #[cfg(feature = "ble")]
            return Self::open_ble(addr).await;
            #[cfg(not(feature = "ble"))]
            return Err(Error::InvalidParam(format!(
                "cannot open {addr} over BLE: built without the `ble` feature"
            )));
        }
        Self::open_bt(target)
    }

    /// CHECK_DEVICE (0x12) - verify printer is present.
    pub async fn check_device(&self) -> Result<bool> {
        log::info!("CHECK_DEVICE");
        let resp = self.transport.send_cmd(CMD_CHECK_DEVICE, 0).await?;
        Ok(resp.is_some_and(|r| self.transport.validate_response(&r, CMD_CHECK_DEVICE)))
    }

    /// INQUIRY_STA (0x11) - query printer status.
    pub async fn query_status(&self) -> Result<Option<PrinterStatus>> {
        let resp = self.transport.send_cmd(CMD_INQUIRY_STA, 0).await?;
        Ok(resp.and_then(|r| self.transport.parse_status_response(&r)))
    }

    /// RETURN_MAT (0x30) - query material/label info.
    pub async fn query_material(&self) -> Result<Option<MaterialInfo>> {
        log::info!("RETURN_MAT");
        let resp = self.transport.send_cmd(CMD_RETURN_MAT, 0).await?;
        Ok(resp.and_then(|r| self.transport.parse_material_response(&r)))
    }

    /// RD_DEV_NAME (0x16) - read device name.
    pub async fn read_device_name(&self) -> Result<Option<String>> {
        log::info!("RD_DEV_NAME");
        let resp = self.transport.send_cmd(CMD_RD_DEV_NAME, 0).await?;
        Ok(resp.and_then(|r| self.transport.parse_device_name_response(&r)))
    }

    /// READ_FWVER (0xC5) - read firmware version.
    pub async fn read_firmware_version(&self) -> Result<Option<u8>> {
        log::info!("READ_FWVER");
        let resp = self.transport.send_cmd(CMD_READ_FWVER, 0).await?;
        Ok(resp.and_then(|r| self.transport.parse_firmware_version_response(&r)))
    }

    /// READ_REV (0x17) - read protocol version.
    pub async fn read_version(&self) -> Result<Option<String>> {
        log::info!("READ_REV");
        let resp = self.transport.send_cmd(CMD_READ_REV, 0).await?;
        Ok(resp.and_then(|r| self.transport.parse_version_response(&r)))
    }

    /// START_PRINT (0x13).
    pub async fn start_print(&self) -> Result<Option<Vec<u8>>> {
        log::info!("START_PRINT");
        self.transport.send_cmd(CMD_START_PRINT, 0).await
    }

    /// STOP_PRINT (0x14).
    pub async fn stop_print(&self) -> Result<Option<Vec<u8>>> {
        log::info!("STOP_PRINT");
        self.transport.send_cmd(CMD_STOP_PRINT, 0).await
    }

    /// PAPER_SKIP (0x2E) — feed/advance one blank label. Returns `Ok(())` once
    /// the device acks; errors if there is no response.
    pub async fn paper_skip(&self) -> Result<()> {
        log::info!("PAPER_SKIP");
        let resp = self.transport.send_cmd(CMD_PAPER_SKIP, 0).await?;
        if resp.is_some_and(|r| self.transport.validate_response(&r, CMD_PAPER_SKIP)) {
            Ok(())
        } else {
            Err(Error::InvalidResponse("PAPER_SKIP: no ack".into()))
        }
    }

    /// Push a synthetic label-material record, standing in for the RFID tag
    /// third-party stock doesn't have. Clears `label_rw_error` and gives the
    /// firmware the geometry and heat times to print with.
    ///
    /// Two-step, mirroring `t5080PrintUtils.setDivRfidData()`: announce the
    /// payload length under 0x5D, then bulk-write the record. The vendor's
    /// `CMD_SET_RFID_DATA_WRITE: 999` is an internal step marker, not a second
    /// opcode — nothing by that name goes on the wire.
    ///
    /// Only meaningful on blank stock. Genuine consumables carry a real tag and
    /// the vendor deliberately skips this for them; overwriting one is not
    /// something the protocol offers a way back from.
    pub async fn set_rfid_data(&self, record: &[u8]) -> Result<()> {
        let len = u16::try_from(record.len())
            .map_err(|_| Error::InvalidParam(format!("RFID record too long: {}", record.len())))?;
        log::info!("SET_RFID_DATA: {len} bytes");

        let resp = self.transport.send_cmd(CMD_SET_RFID_DATA, len).await?;
        if !resp.is_some_and(|r| self.transport.validate_response(&r, CMD_SET_RFID_DATA)) {
            return Err(Error::InvalidResponse("SET_RFID_DATA: no ack".into()));
        }

        // Unlike the print path, the vendor does read a response after the last
        // frame here: there is no BUF_FULL to ack the transfer instead.
        self.transport.send_bulk_data(record, true).await?;
        Ok(())
    }

    /// Write a material record and confirm the printer took it, returning the
    /// material as read back.
    ///
    /// The commit is asynchronous: for a short window after the bulk write the
    /// printer still serves a half-updated record (observed on a T50M Pro as a
    /// UUID of `00001000000000`, between the old all-zero value and the new
    /// one), so a single read straight after the write reports a false failure.
    /// Poll until the synthetic UUID echoes back.
    pub async fn provision_material(
        &self,
        mat: &crate::rfid::RfidMaterial,
    ) -> Result<MaterialInfo> {
        /// ~2s total, comfortably past the observed few-hundred-ms window.
        const SETTLE_POLLS: usize = 10;
        const SETTLE_INTERVAL: Duration = Duration::from_millis(200);

        self.set_rfid_data(&mat.encode()).await?;

        let expected: String = mat
            .uuid_bytes()
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect();
        let mut last = None;
        for _ in 0..SETTLE_POLLS {
            tokio::time::sleep(SETTLE_INTERVAL).await;
            match self.query_material().await? {
                Some(m) if m.uuid == expected => {
                    log::info!(
                        "provisioned: {}x{}mm gap={}mm heat={}/{} uuid={}",
                        m.width_mm,
                        m.height_mm,
                        m.gap_mm,
                        mat.heat_time_5,
                        mat.heat_time_40,
                        m.uuid
                    );
                    return Ok(m);
                }
                other => last = other,
            }
        }

        Err(Error::InvalidResponse(match last {
            Some(m) => format!(
                "record not taken: printer reports UUID {} (wrote {expected})",
                m.uuid
            ),
            None => "record not taken: no material info after write".into(),
        }))
    }

    /// Send an arbitrary opcode and hand back the raw response, for probing
    /// commands whose frame shape we don't know yet.
    ///
    /// Deliberately does no validation — the point is to see what comes back,
    /// including nothing. `Ok(None)` means the read timed out, which is the
    /// expected shape of "firmware doesn't implement this".
    ///
    /// Callers own the safety question. Some opcodes write, move paper, or enter
    /// firmware update; see the range warnings in [`crate::cmd`].
    pub async fn probe_raw(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>> {
        log::info!("PROBE 0x{cmd:02X} param={param}");
        self.transport.send_cmd(cmd, param).await
    }

    /// Wait for device to be idle (not busy, not printing).
    pub async fn wait_ready(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for _ in 0..max_attempts {
            let st = self.query_status().await?;
            if let Some(ref s) = st
                && !s.device_busy
                && !s.printing
            {
                return Ok(st);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(None)
    }

    /// Apply profile-specific interpretation to printer error flags.
    ///
    /// E-series captures show `ribbon_end` asserted during normal thermal
    /// printing, so it is not fatal for that profile. All other existing
    /// error flags retain their current meaning.
    fn has_fatal_error(&self, s: &PrinterStatus) -> bool {
        if !matches!(self.profile, PrintProfile::ESeries) {
            return s.has_error();
        }

        s.label_rw_error
            || s.label_end
            || s.label_mode_error
            || s.ribbon_rw_error
            || s.cover_open
            || s.head_temp_high
            || s.label_not_installed
    }

    /// Wait for printing station to become active.
    ///
    /// Aborts early via `Error::InvalidResponse` if the printer raises an
    /// error flag (label end, cover open, mode mismatch, etc.) — those
    /// states cause the firmware to drop the BT link and beep, and there's
    /// no point continuing the print.
    pub async fn wait_printing(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for _ in 0..max_attempts {
            let st = self.query_status().await?;
            if let Some(ref s) = st {
                if self.has_fatal_error(s) {
                    return Err(Error::InvalidResponse(format!(
                        "printer error after START_PRINT: {}",
                        s.error_description().unwrap_or_default()
                    )));
                }
                if s.printing {
                    return Ok(st);
                }
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Ok(None)
    }

    /// Wait for buffer space available (buf_full == false).
    pub async fn wait_buffer_ready(&self, max_attempts: usize) -> Result<Option<PrinterStatus>> {
        for i in 0..max_attempts {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let st = self.query_status().await?;
            if let Some(ref s) = st {
                if self.has_fatal_error(s) {
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
    pub async fn transfer_compressed(&self, compressed: &[u8], speed: u16) -> Result<()> {
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
        let resp = self
            .transport
            .send_bulk_header(compressed_len, num_packets)
            .await?;
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
        self.transport.send_bulk_data(compressed, false).await?;

        // Let the firmware drain the packets before telling it the buffer is
        // full. The vendor's `cmdbuffull()` waits for every data packet to be
        // acknowledged and then delays again before sending BUF_FULL; without
        // the pause the printer can take the whole page and never print it,
        // leaving `buf_full` stuck set.
        tokio::time::sleep(BLOCK_SETTLE).await;

        // CMD_BUF_FULL parameters are profile-dependent.
        let (buf_len, buf_speed) = if self.profile.params().buf_full_reports_length {
            (compressed_len, speed)
        } else {
            (0, 0)
        };
        log::info!("BUF_FULL: len={}, speed={}", buf_len, buf_speed);
        self.transport
            .send_cmd_two(CMD_BUF_FULL, buf_len, buf_speed)
            .await?;

        // `buf_full` only rises once the firmware has taken the block in.
        // Reading it too soon sees the *previous* state, so the next block goes
        // out against a stale "there is room" — which is how eight blocks left
        // in a second and the buffer never drained.
        tokio::time::sleep(BLOCK_SETTLE).await;

        Ok(())
    }

    /// Execute a full print job with pre-compressed data.
    ///
    /// This is the main print flow from T50PlusPrint.doPrint():
    /// 1. CHECK_DEVICE
    /// 2. Wait ready
    /// 3. START_PRINT
    /// 4. Wait printing station
    /// 5. Per block: wait for buffer room, then transfer
    /// 6. Wait completion
    pub async fn print_compressed(&self, blocks: &[Vec<u8>], speed: u16) -> Result<()> {
        // A page split across transfers has to print slower. The vendor sets
        // the speed from the block count alone, not from how well the raster
        // compressed (`t5080PrintUtils.js`, the CMD_BUF_FULL handler):
        //
        //     if (this.imageDataList.length > 1) { this.speed = 20; }
        //     else                               { this.speed = 60; }
        //
        // At full speed the head reaches the end of the first block before the
        // next one has been decompressed, and the printer stops there — half a
        // label, no error flag, both blocks acknowledged.
        let speed = if blocks.len() > 1 {
            log::info!(
                "{} blocks: dropping speed {speed} -> {MULTI_BLOCK_SPEED} for a split page",
                blocks.len()
            );
            MULTI_BLOCK_SPEED
        } else {
            speed
        };
        // Step 1: Check device
        if !self.check_device().await? {
            return Err(Error::InvalidResponse("CHECK_DEVICE failed".into()));
        }

        // Step 2: Wait ready
        let status = self
            .wait_ready(READY_ATTEMPTS)
            .await?
            .ok_or_else(|| Error::InvalidResponse("timeout waiting for device ready".into()))?;
        if self.has_fatal_error(&status) {
            return Err(Error::InvalidResponse(format!(
                "printer error: {}",
                status.error_description().unwrap_or_default()
            )));
        }

        // Step 3: Optional profile-specific pre-start command, then START_PRINT.
        if let Some((cmd, param)) = self.profile.params().pre_start_cmd {
            self.transport.send_cmd(cmd, param).await?;
        }
        self.start_print().await?;

        // Step 4: Wait printing station
        self.wait_printing(PRINTING_ATTEMPTS)
            .await?
            .ok_or_else(|| Error::InvalidResponse("timeout waiting for printing station".into()))?;

        if let Some((cmd, param)) = self.profile.params().post_start_cmd {
            self.transport.send_cmd(cmd, param).await?;
        }

        // Step 5: Transfer one block per print buffer, waiting for room before
        // each. `buf_full` is the firmware's flow control — it decompresses
        // into a single page buffer, so sending the next block before the
        // previous one has drained overruns it, and the label prints garbled
        // and stops short. Mirrors the vendor's send loop, which re-checks
        // status and waits for `MSTA_REG.BufSta == 0` between blocks.
        for (i, block) in blocks.iter().enumerate() {
            let buf_status = self
                .wait_buffer_ready(BUFFER_READY_ATTEMPTS)
                .await?
                .ok_or_else(|| {
                    Error::InvalidResponse(format!(
                        "timeout waiting for buffer space before block {}/{}",
                        i + 1,
                        blocks.len()
                    ))
                })?;
            if self.has_fatal_error(&buf_status) {
                self.stop_print().await?;
                return Err(Error::InvalidResponse(format!(
                    "printer error: {}",
                    buf_status.error_description().unwrap_or_default()
                )));
            }
            log::info!(
                "block {}/{}: {} bytes, before: {}",
                i + 1,
                blocks.len(),
                block.len(),
                buf_status.summary()
            );
            self.transfer_compressed(block, speed).await?;
            // What the firmware made of it. A block that is accepted and then
            // silently dropped looks identical to one that printed unless the
            // registers are on record either side of the transfer.
            if let Some(after) = self.query_status().await? {
                log::info!(
                    "block {}/{}: after:  {}",
                    i + 1,
                    blocks.len(),
                    after.summary()
                );
            }
        }

        // Step 6: Wait completion.
        //
        // `!printing && !device_busy` alone is not completion: right after the
        // last block the firmware may not have started yet, so both read false
        // and we would report success on a page that never printed. The buffer
        // has to have drained too — a stuck `buf_full` is exactly the state a
        // printer wedges in when it has been fed faster than it can print.
        for _ in 0..COMPLETION_POLLS {
            tokio::time::sleep(COMPLETION_POLL_INTERVAL).await;
            if let Some(s) = self.query_status().await? {
                log::debug!("completion poll: {}", s.summary());
                if self.has_fatal_error(&s) {
                    return Err(Error::InvalidResponse(format!(
                        "printer error while printing: {}",
                        s.error_description().unwrap_or_default()
                    )));
                }
                if !s.printing && !s.device_busy && !s.buf_full {
                    log::info!("print complete");
                    return Ok(());
                }
            }
        }
        // Falling out here with the buffer still full means the firmware took
        // the page and never printed it; say so rather than blaming the poll.
        if let Some(s) = self.query_status().await?
            && s.buf_full
        {
            return Err(Error::InvalidResponse(
                "printer buffer still full after the print budget — the page was accepted but \
                 never printed"
                    .into(),
            ));
        }

        log::warn!("timeout waiting for print completion");
        Err(Error::Timeout("print completion"))
    }

    /// Full test print workflow: generate test pattern, build buffers, compress, print.
    pub async fn test_print(
        &self,
        mat: &MaterialInfo,
        density: Density,
        page: PageOptions,
    ) -> Result<()> {
        use crate::bitmap::create_test_pattern;
        use crate::buffer::split_into_buffers;
        use crate::compress::compress_buffers;

        let label_width_mm = (mat.width_mm as u32).min(crate::bitmap::PRINTHEAD_WIDTH_MM);
        let height_mm = if mat.height_mm == 0 {
            crate::status::DEFAULT_LABEL_HEIGHT_MM as u32
        } else {
            mat.height_mm as u32
        };

        if matches!(self.profile, PrintProfile::ESeries) {
            let printhead_dots = self.profile.params().default_printhead_dots;
            let label_width_mm = (mat.width_mm as u32)
                .min(printhead_dots / crate::bitmap::DOTS_PER_MM);
            let margin = self.profile.params().margin_dots;

            let (image_data, _w, h, bpl) =
                crate::bitmap::create_test_pattern_profiled(
                    label_width_mm,
                    height_mm,
                    printhead_dots,
                    self.profile,
                );

            let buffers = crate::buffer::split_into_buffers_profiled(
                &image_data,
                bpl as u8,
                h as u16,
                margin,
                margin,
                density,
                page,
                self.profile,
            );

            log::info!(
                "E-series test print: {}mm x {}mm, {} buffers",
                label_width_mm,
                height_mm,
                buffers.len()
            );

            let (compressed, avg) =
                crate::compress::compress_buffers_individually(&buffers)?;
            let speed = calc_speed(avg);

            return self.print_compressed(&compressed, speed).await;
        }

        log::info!(
            "test print: {}mm x {}mm, black={} red={}, save_paper={}",
            label_width_mm,
            height_mm,
            density.black,
            density.red,
            page.save_paper
        );

        let (image_data, _w, h, bpl) = create_test_pattern(label_width_mm, height_mm);
        let buffers = split_into_buffers(&image_data, bpl as u8, h as u16, 8, 8, density, page);
        log::info!("{} print buffers", buffers.len());

        let (compressed, avg) = compress_buffers(&buffers)?;
        let speed = calc_speed(avg);
        log::info!(
            "compressed: {} bytes, avg={}/buf, speed={}",
            compressed.len(),
            avg,
            speed
        );

        self.print_compressed(&compressed, speed).await
    }

    /// Print one calibration strip: a solid block per entry in `densities`, laid
    /// down the feed direction in order, each burned at its own density.
    ///
    /// Each band carries its own black *and* red trim, so one strip can walk the
    /// two against each other. Pair with [`set_rfid_data`](Self::set_rfid_data)
    /// to vary heat times between strips: heat time sets absolute energy, the
    /// density pair trims it, and on two-colour stock that decides the colour.
    pub async fn print_swatch_ladder(
        &self,
        label_width_mm: u32,
        height_mm: u32,
        densities: &[Density],
    ) -> Result<()> {
        use crate::bitmap::create_swatch_ladder;
        use crate::buffer::{DensityBand, split_into_banded_buffers};
        use crate::compress::compress_buffers;

        if densities.is_empty() {
            return Err(Error::InvalidParam("no densities to sweep".into()));
        }
        let steps = densities.len() as u32;
        let (image_data, _h, bpl, band_cols) =
            create_swatch_ladder(label_width_mm, height_mm, steps);
        if band_cols == 0 {
            return Err(Error::InvalidParam(format!(
                "{height_mm}mm label too short for {steps} bands"
            )));
        }

        let bands: Vec<DensityBand> = densities
            .iter()
            .map(|&density| DensityBand {
                cols: band_cols as u16,
                density,
            })
            .collect();
        let margin = crate::bitmap::DEFAULT_MARGIN_DOTS;
        let buffers = split_into_banded_buffers(
            &image_data,
            bpl as u8,
            &bands,
            margin,
            margin,
            PageOptions::default(),
        );
        log::info!(
            "swatch ladder: {}mm x {}mm, {} bands of {} cols, densities={:?}",
            label_width_mm,
            height_mm,
            steps,
            band_cols,
            densities
        );

        let (compressed, avg) = compress_buffers(&buffers)?;
        let speed = calc_speed(avg);
        self.print_compressed(&compressed, speed).await
    }

    /// Print an 8bpp grayscale image, halftoned with `dither`.
    ///
    /// The printer has no grayscale mode — the raster is 1bpp — so tone is
    /// entirely a product of the halftone kernel. `gray` is row-major
    /// `width * height`, W colorspace (0 = black, 255 = white).
    ///
    /// This is the batch counterpart of the IPP path, which dithers
    /// line-by-line as CUPS streams them. Both share [`crate::dither`].
    pub async fn print_grayscale(
        &self,
        gray: &[u8],
        width: u32,
        height: u32,
        density: Density,
        dither: crate::dither::DitherMode,
    ) -> Result<()> {
        use crate::bitmap::{DEFAULT_MARGIN_DOTS, center_in_printhead, raster_to_column_major};
        use crate::buffer::split_into_buffers;
        use crate::compress::compress_buffers;
        use crate::dither::Ditherer;

        let expected = (width as usize) * (height as usize);
        if gray.len() < expected {
            return Err(Error::InvalidParam(format!(
                "grayscale buffer too small: {} < {expected}",
                gray.len()
            )));
        }

        let bpl = width.div_ceil(8) as usize;
        let mut mono = vec![0u8; bpl * height as usize];
        let mut ditherer = Ditherer::new(dither, width);
        for y in 0..height {
            let row = &gray[(y * width) as usize..((y + 1) * width) as usize];
            let out = &mut mono[y as usize * bpl..(y as usize + 1) * bpl];
            ditherer.line(row, y, out);
        }

        let (col_data, num_cols, _) = raster_to_column_major(&mono, width, height);
        let (canvas, canvas_bpl) = center_in_printhead(
            &col_data,
            num_cols,
            width,
            crate::bitmap::PRINTHEAD_WIDTH_DOTS,
        );
        let buffers = split_into_buffers(
            &canvas,
            canvas_bpl as u8,
            num_cols as u16,
            DEFAULT_MARGIN_DOTS,
            DEFAULT_MARGIN_DOTS,
            density,
            PageOptions::default(),
        );
        log::info!(
            "grayscale: {width}x{height}, dither={dither:?}, density={density}, {} buffers",
            buffers.len()
        );

        let (compressed, avg) = compress_buffers(&buffers)?;
        let speed = calc_speed(avg);
        self.print_compressed(&compressed, speed).await
    }

    /// Print an RGB image in two-colour mode, red and black on one pass.
    ///
    /// `rgb` is row-major `width * height * 3`. Pixels are sorted into the two
    /// planes by luminance ([`crate::twocolor::classify`]) — a cut, not a hue
    /// test, so quantise to two ink colours first rather than feeding a
    /// photograph.
    ///
    /// `density.red` and `density.black` trim the two planes independently.
    ///
    /// Whether a given unit honours this is not guaranteed: the vendor's own
    /// capability check excludes T50 Pro units whose Bluetooth name contains
    /// `A` or `B`, and the shipped app never enables the path at all.
    pub async fn print_two_colour(
        &self,
        rgb: &[u8],
        width: u32,
        height: u32,
        density: Density,
    ) -> Result<()> {
        use crate::buffer::{DensityBand, split_into_banded_buffers};
        use crate::compress::compress_buffers;
        use crate::twocolor::{interleave_planes, rgb_to_planes};

        let expected = (width as usize) * (height as usize) * 3;
        if rgb.len() < expected {
            return Err(Error::InvalidParam(format!(
                "RGB buffer too small: {} < {expected}",
                rgb.len()
            )));
        }

        let (red, black, cols, bpl) = rgb_to_planes(rgb, width, height);
        let interleaved = interleave_planes(&red, &black, bpl as usize, cols as usize)
            .ok_or_else(|| Error::InvalidParam("plane interleave failed".into()))?;

        let margin = crate::bitmap::DEFAULT_MARGIN_DOTS;
        let printable = (cols as u16).saturating_sub(2 * margin);
        if printable == 0 {
            return Err(Error::InvalidParam(format!(
                "{height} rows leaves nothing after margins"
            )));
        }

        let bands = [DensityBand {
            cols: printable,
            density,
        }];
        let buffers = split_into_banded_buffers(
            &interleaved,
            bpl as u8,
            &bands,
            margin,
            margin,
            PageOptions {
                colour: ColourMode::TwoColour,
                ..Default::default()
            },
        );
        log::info!(
            "two-colour: {width}x{height}, {} cols, black={} red={}, {} buffers",
            printable,
            density.black,
            density.red,
            buffers.len()
        );

        let (compressed, avg) = compress_buffers(&buffers)?;
        let speed = calc_speed(avg);
        self.print_compressed(&compressed, speed).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ble_targets_are_recognised_by_scheme() {
        assert_eq!(
            ble_target("ble://A4:93:40:AF:B0:B5"),
            Some("A4:93:40:AF:B0:B5")
        );
        assert_eq!(
            ble_target("ble:A4:93:40:AF:B0:B5"),
            Some("A4:93:40:AF:B0:B5")
        );
        // A bare address stays a Classic Bluetooth target.
        assert_eq!(ble_target("A4:93:40:AF:B0:B5"), None);
        assert_eq!(ble_target("/dev/hidraw0"), None);
    }
}
