//! Transport trait abstracting Bluetooth RFCOMM vs USB HID communication.

use crate::error::Result;
use crate::status::{MaterialInfo, PrinterStatus};
use std::os::unix::io::RawFd;

/// Abstraction over the physical transport to the printer.
///
/// Both Bluetooth (RFCOMM socket, 0x7E/5A framing, 512-byte data frames)
/// and USB HID (hidraw device, 0xC0/40 framing, 64-byte reports) implement
/// this trait with their own command encoding, data framing, and response parsing.
pub trait Transport: Send {
    /// Send a command with one parameter, return raw response.
    fn send_cmd(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>>;

    /// Send a command with two parameters (used for NEXT_ZIPPEDBULK, BUF_FULL).
    fn send_cmd_two(&self, cmd: u8, param1: u16, param2: u16) -> Result<Option<Vec<u8>>>;

    /// Send bulk compressed data as transport-native frames.
    /// If `read_final_response` is true, reads and returns the response after the last frame.
    fn send_bulk_data(&self, data: &[u8], read_final_response: bool) -> Result<Option<Vec<u8>>>;

    /// Return the raw file descriptor of the underlying device.
    fn raw_fd(&self) -> RawFd;

    /// Whether this transport uses socket I/O (recv/send) vs file I/O (read/write).
    fn use_socket_io(&self) -> bool;

    /// Parse a status response into PrinterStatus.
    fn parse_status_response(&self, resp: &[u8]) -> Option<PrinterStatus>;

    /// Parse a material response into MaterialInfo.
    fn parse_material_response(&self, resp: &[u8]) -> Option<MaterialInfo>;

    /// Validate response echoes the expected command byte.
    fn validate_response(&self, resp: &[u8], expected_cmd: u8) -> bool;

    /// Parse device name from response.
    fn parse_device_name_response(&self, resp: &[u8]) -> Option<String>;

    /// Parse firmware version from response.
    fn parse_firmware_version_response(&self, resp: &[u8]) -> Option<u8>;

    /// Parse protocol version from response.
    fn parse_version_response(&self, resp: &[u8]) -> Option<String>;
}
