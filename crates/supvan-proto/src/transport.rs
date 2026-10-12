//! Transport trait abstracting Bluetooth RFCOMM vs USB HID communication.

use crate::compress::Packing;
use crate::error::Result;
use crate::status::{MaterialInfo, PrinterStatus};
use async_trait::async_trait;
use std::time::Duration;

/// Which vendor tool's print flow a transport follows. Supvan ships one tool
/// per transport and they disagree on several job-level details; each is
/// mirrored for the transport it actually drives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Android app (`T50PlusPrint`) over Classic SPP / BLE.
    Android,
    /// Electron desktop editor (`t5080PrintUtils`) over USB HID.
    Electron,
}

impl Flow {
    pub(crate) fn packing(self) -> Packing {
        match self {
            Self::Android => Packing::Grouped,
            Self::Electron => Packing::PerBuffer,
        }
    }

    /// START_PRINT parameter: Android sends 0, Electron 1.
    pub(crate) fn start_print_param(self) -> u16 {
        match self {
            Self::Android => 0,
            Self::Electron => 1,
        }
    }

    /// Pause between the last data packet and BUF_FULL: Android sleeps 20 ms;
    /// Electron waits for the bulk ack, then 100 ms + 50 ms of timers.
    pub(crate) fn pre_buf_full_delay(self) -> Duration {
        match self {
            Self::Android => Duration::from_millis(20),
            Self::Electron => Duration::from_millis(150),
        }
    }

    /// Pause after the BUF_FULL reply before polling for the next block
    /// (Electron's 100 ms timer; Android goes straight to its poll loop).
    pub(crate) fn post_buf_full_delay(self) -> Duration {
        match self {
            Self::Android => Duration::ZERO,
            Self::Electron => Duration::from_millis(100),
        }
    }
}

/// Abstraction over the physical transport to the printer.
///
/// Both Bluetooth (RFCOMM socket, 0x7E/5A framing, 512-byte data frames)
/// and USB HID (hidraw device, 0xC0/40 framing, 64-byte reports) implement
/// this trait with their own command encoding, data framing, and response parsing.
///
/// The I/O methods are async because the BLE GATT pipe is natively async (via
/// `bluer`); the blocking RFCOMM/HID transports bridge their
/// FFI calls with `tokio::task::block_in_place`. Response parsing stays sync —
/// it's pure byte work with no I/O.
#[async_trait]
pub trait Transport: Send + Sync {
    /// The vendor flow this transport's framing belongs to.
    fn flow(&self) -> Flow;

    /// Send a command with one parameter, return raw response.
    async fn send_cmd(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>>;

    /// Send a command with two parameters (used for NEXT_ZIPPEDBULK, BUF_FULL).
    async fn send_cmd_two(&self, cmd: u8, param1: u16, param2: u16) -> Result<Option<Vec<u8>>>;

    /// Send the NEXT_ZIPPEDBULK (0x5C) bulk-transfer header, encoded for this
    /// transport: SPP framing carries `(block_size=512, packet_count)`, USB HID
    /// carries the total compressed byte length. Returns the device ack.
    async fn send_bulk_header(&self, compressed_len: u16) -> Result<Option<Vec<u8>>>;

    /// Send bulk data as transport-native frames. Returns the device's reply
    /// to the transfer when the transport waits for one (USB does; SPP leaves
    /// it to be drained, see `SppCodec`).
    async fn send_bulk_data(&self, data: &[u8]) -> Result<Option<Vec<u8>>>;

    /// Parse a status response into PrinterStatus.
    fn parse_status_response(&self, resp: &[u8]) -> Option<PrinterStatus>;

    /// Parse a material response into MaterialInfo.
    fn parse_material_response(&self, resp: &[u8]) -> Option<MaterialInfo>;

    /// Validate response echoes the expected command byte.
    fn validate_response(&self, resp: &[u8], expected_cmd: u8) -> bool;

    /// Parse device name from response. Only SPP framing carries one.
    fn parse_device_name_response(&self, _resp: &[u8]) -> Option<String> {
        None
    }

    /// Parse firmware version from response. Only SPP framing carries one.
    fn parse_firmware_version_response(&self, _resp: &[u8]) -> Option<u8> {
        None
    }

    /// Parse protocol version from response. Only SPP framing carries one.
    fn parse_version_response(&self, _resp: &[u8]) -> Option<String> {
        None
    }
}
