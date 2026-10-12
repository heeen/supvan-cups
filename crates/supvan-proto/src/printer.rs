//! High-level printer operations.
//!
//! Implements the print flow from T50PlusPrint.doPrint():
//! CHECK_DEVICE -> poll ready -> START_PRINT -> poll printing ->
//! transfer buffers -> poll complete.

use crate::bitmap::DOTS_PER_MM;
use crate::buffer::{ColourMode, Density, PRINT_BUF_SIZE, PageOptions};
use crate::cmd::*;
use crate::compress::{CompressedPage, compress_buffers};
use crate::error::{Error, Result};
use crate::status::{MaterialInfo, PrinterStatus, hex_upper};
use crate::transport::Transport;
use std::time::{Duration, Instant};

/// How often the job-level waits poll status (the Android app's 100 ms).
const STATUS_POLL: Duration = Duration::from_millis(100);
const READY_WAIT: Duration = Duration::from_secs(6);
const PRINTING_WAIT: Duration = Duration::from_secs(6);
const COMPLETION_WAIT: Duration = Duration::from_secs(30);

/// Both vendor tools poll status in a tight loop until `BufSta` clears before
/// each block (Android every 20 ms, Electron every 5 ms). The budget has to
/// cover the firmware printing out whatever it already holds.
const BUFFER_POLL: Duration = Duration::from_millis(20);
const BUFFER_WAIT: Duration = Duration::from_secs(15);

/// Run the head at most this fraction of what the link was measured to
/// deliver, so jitter doesn't starve it.
const LINK_HEADROOM: f32 = 0.85;
/// Round trips each block costs beyond its data: NEXT_ZIPPEDBULK, BUF_FULL
/// and the status poll before the next one.
const BLOCK_ROUND_TRIPS: u32 = 3;

/// How long the first block of a page took to move, measured as it goes.
struct LinkTiming {
    round_trip: Duration,
    per_byte: Duration,
}

/// Strip the `ble://` (or `ble:`) scheme from a target string.
fn ble_target(target: &str) -> Option<&str> {
    target
        .strip_prefix("ble://")
        .or_else(|| target.strip_prefix("ble:"))
}

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
        let sock = crate::rfcomm::RfcommSocket::connect(addr)?;
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

    /// Send a command; whether the device acked it.
    async fn send_acked(&self, cmd: u8, param: u16) -> Result<bool> {
        let resp = self.transport.send_cmd(cmd, param).await?;
        Ok(resp.is_some_and(|r| self.transport.validate_response(&r, cmd)))
    }

    /// CHECK_DEVICE (0x12) - verify printer is present.
    pub async fn check_device(&self) -> Result<bool> {
        log::info!("CHECK_DEVICE");
        self.send_acked(CMD_CHECK_DEVICE, 0).await
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

    /// START_PRINT (0x13), with the parameter this transport's vendor sends.
    async fn start_print(&self) -> Result<Option<Vec<u8>>> {
        let flow = self.transport.flow();
        log::info!("START_PRINT ({flow:?})");
        self.transport
            .send_cmd(CMD_START_PRINT, flow.start_print_param())
            .await
    }

    /// STOP_PRINT (0x14).
    async fn stop_print(&self) -> Result<Option<Vec<u8>>> {
        log::info!("STOP_PRINT");
        self.transport.send_cmd(CMD_STOP_PRINT, 0).await
    }

    /// PAPER_SKIP (0x2E) — feed/advance one blank label. Returns `Ok(())` once
    /// the device acks; errors if there is no response.
    pub async fn paper_skip(&self) -> Result<()> {
        log::info!("PAPER_SKIP");
        if self.send_acked(CMD_PAPER_SKIP, 0).await? {
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

        if !self.send_acked(CMD_SET_RFID_DATA, len).await? {
            return Err(Error::InvalidResponse("SET_RFID_DATA: no ack".into()));
        }
        self.transport.send_bulk_data(record).await?;
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

        let expected = hex_upper(&mat.uuid_bytes());
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

    /// Poll status every `interval` until `done` yields a value, failing on any
    /// error flag or once `budget` runs out. `what` names the wait in errors.
    async fn poll_status<T>(
        &self,
        what: &'static str,
        budget: Duration,
        interval: Duration,
        mut done: impl FnMut(&PrinterStatus) -> Option<T>,
    ) -> Result<T> {
        let started = Instant::now();
        loop {
            if let Some(s) = self.query_status().await? {
                s.ensure_ok(&format!("while waiting for {what}"))?;
                if let Some(v) = done(&s) {
                    return Ok(v);
                }
            }
            if started.elapsed() >= budget {
                return Err(Error::Timeout(what));
            }
            tokio::time::sleep(interval).await;
        }
    }

    /// NEXT_ZIPPEDBULK and the block's data, timed.
    async fn send_block_data(&self, data: &[u8]) -> Result<LinkTiming> {
        let len = u16::try_from(data.len())
            .map_err(|_| Error::InvalidParam("compressed block too large".into()))?;
        let started = Instant::now();
        if self.transport.send_bulk_header(len).await?.is_none() {
            return Err(Error::InvalidResponse(
                "no response to NEXT_ZIPPEDBULK".into(),
            ));
        }
        let round_trip = started.elapsed();
        self.transport.send_bulk_data(data).await?;
        let per_byte = (started.elapsed() - round_trip) / data.len().max(1) as u32;
        Ok(LinkTiming {
            round_trip,
            per_byte,
        })
    }

    /// BUF_FULL — the block is in; print it at `speed`.
    async fn commit_block(&self, len: usize, speed: u16) -> Result<()> {
        let flow = self.transport.flow();
        tokio::time::sleep(flow.pre_buf_full_delay()).await;
        log::info!("BUF_FULL: len={len}, speed={speed}");
        self.transport
            .send_cmd_two(CMD_BUF_FULL, len as u16, speed)
            .await?
            .ok_or_else(|| Error::InvalidResponse("no response to BUF_FULL".into()))?;
        tokio::time::sleep(flow.post_buf_full_delay()).await;
        Ok(())
    }

    /// Compress a page's print buffers the way this transport's vendor tool
    /// does, then print them.
    pub async fn print_buffers(&self, buffers: &[[u8; PRINT_BUF_SIZE]]) -> Result<()> {
        let page = compress_buffers(buffers, self.transport.flow().packing())?;
        self.print_compressed(&page).await
    }

    /// The fastest vendor speed at which `timing` can keep every block of the
    /// page coming before the head finishes the one before.
    ///
    /// The vendor speed rule only looks at density. Over Bluetooth a dense
    /// halftone buffer can arrive slower than the head prints it at that
    /// speed — how much slower varies twofold with the radio link from one
    /// job to the next — and the head stalls mid-label, leaving stripes. The
    /// Android app only dodges it by forcing speed 5 for photos. Measuring the
    /// page's own first block sidesteps guessing: the speed rides on BUF_FULL,
    /// which follows the data.
    fn link_speed_cap(&self, page: &CompressedPage, timing: &LinkTiming) -> u16 {
        let flow = self.transport.flow();
        let overhead = flow.pre_buf_full_delay()
            + flow.post_buf_full_delay()
            + timing.round_trip * BLOCK_ROUND_TRIPS;
        // Each block has to arrive while the head prints the one before it.
        page.blocks
            .windows(2)
            .map(|pair| {
                let printing = pair[0].printed_cols as f32 / DOTS_PER_MM as f32;
                let arriving = timing.per_byte * pair[1].data.len() as u32 + overhead;
                crate::speed::fastest_at_most(printing / arriving.as_secs_f32() * LINK_HEADROOM)
            })
            .min()
            .unwrap_or(page.speed)
    }

    /// Run one print job — the shared skeleton of Android
    /// `T50PlusPrint.doPrint()` and Electron `t5080PrintUtils`: CHECK_DEVICE →
    /// wait idle → START_PRINT → wait printing → per block, wait for
    /// `BufSta == 0` then transfer → wait until printing clears.
    async fn print_compressed(&self, page: &CompressedPage) -> Result<()> {
        if !self.check_device().await? {
            return Err(Error::InvalidResponse("CHECK_DEVICE failed".into()));
        }
        self.poll_status("the device to go idle", READY_WAIT, STATUS_POLL, |s| {
            (!s.device_busy && !s.printing).then_some(())
        })
        .await?;

        self.start_print().await?;
        self.poll_status("printing to start", PRINTING_WAIT, STATUS_POLL, |s| {
            s.printing.then_some(())
        })
        .await?;

        let mut speed = page.speed;
        for (i, block) in page.blocks.iter().enumerate() {
            let before = match self
                .poll_status(
                    "the printer buffer to drain",
                    BUFFER_WAIT,
                    BUFFER_POLL,
                    |s| (!s.buf_full).then(|| s.summary()),
                )
                .await
            {
                Ok(summary) => summary,
                Err(e) => {
                    self.stop_print().await?;
                    return Err(e);
                }
            };
            let timing = self.send_block_data(&block.data).await?;
            // A single-block page is all in before the head starts; only a
            // split page can be starved.
            if i == 0 && page.blocks.len() > 1 {
                let cap = self.link_speed_cap(page, &timing);
                if cap < speed {
                    log::info!(
                        "speed {speed} -> {cap}: link measured at {:?}/KB + {:?} per round trip",
                        timing.per_byte * 1024,
                        timing.round_trip
                    );
                    speed = cap;
                }
            }
            log::info!(
                "block {}/{}: {} bytes ({} bufs), speed={speed}, before: {before}",
                i + 1,
                page.blocks.len(),
                block.data.len(),
                block.source_buffers,
            );
            self.commit_block(block.data.len(), speed).await?;
        }

        // Both vendors call the job done once the printing flag drops.
        let still_buffered = self
            .poll_status("print completion", COMPLETION_WAIT, STATUS_POLL, |s| {
                log::debug!("completion poll: {}", s.summary());
                (!s.printing && !s.device_busy).then_some(s.buf_full)
            })
            .await?;
        if still_buffered {
            return Err(Error::InvalidResponse(
                "printer stopped with data still buffered — the page did not finish".into(),
            ));
        }
        log::info!("print complete");
        Ok(())
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

        let label_width_mm = (mat.width_mm as u32).min(crate::bitmap::PRINTHEAD_WIDTH_MM);
        let height_mm = if mat.height_mm == 0 {
            crate::status::DEFAULT_LABEL_HEIGHT_MM as u32
        } else {
            mat.height_mm as u32
        };

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

        self.print_buffers(&buffers).await
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

        self.print_buffers(&buffers).await
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
        use crate::bitmap::{center_in_printhead, raster_to_column_major};
        use crate::buffer::split_into_buffers;
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
            // Full bleed, like the IPP path: the image is the label.
            0,
            0,
            density,
            PageOptions::default(),
        );
        log::info!(
            "grayscale: {width}x{height}, dither={dither:?}, density={density}, {} buffers",
            buffers.len()
        );

        self.print_buffers(&buffers).await
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

        self.print_buffers(&buffers).await
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
