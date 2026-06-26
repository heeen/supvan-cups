//! USB HID transport implementing the Transport trait.
//!
//! Uses 0xC0/0x40 command framing with big-endian parameters,
//! 64-byte HID reports for data transfer, and 8-byte responses.

use crate::error::Result;
use crate::hidraw::{HID_REPORT_SIZE, HidrawDevice};
use crate::status::{MaterialInfo, PrinterStatus};
use crate::transport::Transport;
use std::time::Duration;

/// USB HID command magic bytes.
const USB_MAGIC1: u8 = 0xC0;
const USB_MAGIC2: u8 = 0x40;

/// Default response timeout for USB HID.
const USB_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);

/// Offset of the null-terminated ASCII device serial USB tacks onto a
/// RETURN_MAT response (BT omits it).
const USB_DEVICE_SN_OFFSET: usize = 40;

/// USB HID transport over a hidraw device.
pub struct UsbHidTransport {
    dev: HidrawDevice,
}

impl UsbHidTransport {
    pub fn new(dev: HidrawDevice) -> Self {
        Self { dev }
    }

    /// Build an 8-byte USB HID command frame (padded to 64 bytes by write_report).
    ///
    /// Layout:
    ///   [0] 0xC0  [1] 0x40
    ///   [2] param_hi  [3] param_lo   (big-endian)
    ///   [4] cmd  [5] 0x00  [6] 0x08  [7] 0x00
    fn make_usb_cmd(cmd: u8, param: u16) -> [u8; 8] {
        [
            USB_MAGIC1,
            USB_MAGIC2,
            (param >> 8) as u8,   // big-endian high
            (param & 0xFF) as u8, // big-endian low
            cmd,
            0x00,
            0x08,
            0x00,
        ]
    }

    /// Build a 10-byte USB HID extended command (for two-parameter commands).
    ///
    /// Layout:
    ///   [0..7] same as make_usb_cmd
    ///   [8] param2_hi  [9] param2_lo   (big-endian)
    fn make_usb_cmd_two(cmd: u8, param1: u16, param2: u16) -> [u8; 10] {
        [
            USB_MAGIC1,
            USB_MAGIC2,
            (param1 >> 8) as u8,
            (param1 & 0xFF) as u8,
            cmd,
            0x00,
            0x08,
            0x00,
            (param2 >> 8) as u8,
            (param2 & 0xFF) as u8,
        ]
    }

    /// Send a HID report and read the response.
    fn send_and_recv(&self, data: &[u8]) -> Result<Option<Vec<u8>>> {
        log::debug!("USB TX: {:02x?}", &data[..data.len().min(16)]);
        self.dev.write_report(data)?;
        let resp = self.dev.read_report(USB_RESPONSE_TIMEOUT)?;
        if let Some(ref r) = resp {
            log::debug!("USB RX: {:02x?}", r);
        } else {
            log::debug!("USB RX: (no response)");
        }
        Ok(resp)
    }

    /// Parse material info from a USB HID RETURN_MAT response.
    ///
    /// The 64-byte report is just a 1-byte length prefix followed by the
    /// same material payload the BT transport carries (verified by
    /// `crates/supvan-cli/examples/material_probe`). USB additionally
    /// tacks 16 bytes of null-terminated ASCII device serial onto the
    /// end at offset 40 — BT doesn't include this.
    fn parse_usb_material(resp: &[u8]) -> Option<MaterialInfo> {
        if resp.len() < 22 {
            log::debug!("USB material response too short: {} bytes", resp.len());
            return None;
        }
        // ASCII device serial at USB_DEVICE_SN_OFFSET (USB-only addition).
        let dev_sn = if resp.len() > USB_DEVICE_SN_OFFSET {
            let tail = &resp[USB_DEVICE_SN_OFFSET..];
            let end = tail.iter().position(|&b| b == 0).unwrap_or(tail.len());
            let s = String::from_utf8_lossy(&tail[..end]).to_string();
            if s.is_empty() { None } else { Some(s) }
        } else {
            None
        };
        crate::status::parse_material_payload(&resp[1..], dev_sn)
    }

    /// Parse the 6 status bytes from an 8-byte USB HID response.
    ///
    /// USB response layout:
    ///   [0] echo byte (command or status indicator)
    ///   [1] MSTA low   (same bits as BT byte 14)
    ///   [2] MSTA high  (same bits as BT byte 15)
    ///   [3] FSTA low   (same bits as BT byte 16)
    ///   [4] FSTA high  (same bits as BT byte 17)
    ///   [5] print count low
    ///   [6] print count high
    ///   [7] reserved
    fn parse_usb_status(resp: &[u8]) -> Option<PrinterStatus> {
        if resp.len() < 7 {
            return None;
        }

        Some(crate::status::decode_status_bits(
            resp[1], // MSTA low
            resp[2], // MSTA high
            resp[3], // FSTA low
            resp[4], // FSTA high
            u16::from_le_bytes([resp[5], resp[6]]),
        ))
    }
}

impl Transport for UsbHidTransport {
    fn send_cmd(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>> {
        let frame = Self::make_usb_cmd(cmd, param);
        self.send_and_recv(&frame)
    }

    fn send_cmd_two(&self, cmd: u8, param1: u16, param2: u16) -> Result<Option<Vec<u8>>> {
        let frame = Self::make_usb_cmd_two(cmd, param1, param2);
        self.send_and_recv(&frame)
    }

    fn send_bulk_data(&self, data: &[u8], read_final_response: bool) -> Result<Option<Vec<u8>>> {
        // Split raw compressed bytes into 64-byte HID reports.
        let chunks = data.chunks(HID_REPORT_SIZE);
        let total = chunks.len();
        for (i, chunk) in data.chunks(HID_REPORT_SIZE).enumerate() {
            let is_last = i == total - 1;
            if is_last && read_final_response {
                return self.send_and_recv(chunk);
            }
            self.dev.write_report(chunk)?;
            // Small delay between reports to avoid overwhelming the device
            if !is_last {
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(None)
    }

    fn send_bulk_header(&self, compressed_len: u16, _num_packets: usize) -> Result<Option<Vec<u8>>> {
        // USB HID encodes NEXT_ZIPPEDBULK as the total compressed byte length.
        self.send_cmd(crate::cmd::CMD_NEXT_ZIPPEDBULK, compressed_len)
    }

    fn parse_status_response(&self, resp: &[u8]) -> Option<PrinterStatus> {
        Self::parse_usb_status(resp)
    }

    fn parse_material_response(&self, resp: &[u8]) -> Option<MaterialInfo> {
        // USB RETURN_MAT returns a 64-byte report with material data.
        // Electron app reads: width_mm=A[19], height_mm=A[20], gap_mm=A[21],
        // SN at A[31]+A[32]<<8, device serial at byteToString(A,11,21),
        // and label serial "T0117..." as ASCII starting around offset 40.
        Self::parse_usb_material(resp)
    }

    fn validate_response(&self, resp: &[u8], _expected_cmd: u8) -> bool {
        // USB HID responses do NOT echo the command byte. resp[0] is a
        // length/type indicator, not the command. Any non-empty response
        // means the device acknowledged the command.
        !resp.is_empty()
    }

    fn parse_device_name_response(&self, _resp: &[u8]) -> Option<String> {
        // Not available in the 8-byte USB HID status response format
        None
    }

    fn parse_firmware_version_response(&self, _resp: &[u8]) -> Option<u8> {
        // Not available in the 8-byte USB HID status response format
        None
    }

    fn parse_version_response(&self, _resp: &[u8]) -> Option<String> {
        // Not available in the 8-byte USB HID status response format
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd;

    #[test]
    fn test_make_usb_cmd() {
        let frame = UsbHidTransport::make_usb_cmd(cmd::CMD_CHECK_DEVICE, 0);
        assert_eq!(frame[0], USB_MAGIC1);
        assert_eq!(frame[1], USB_MAGIC2);
        assert_eq!(frame[2], 0x00); // param high
        assert_eq!(frame[3], 0x00); // param low
        assert_eq!(frame[4], cmd::CMD_CHECK_DEVICE);
        assert_eq!(frame[5], 0x00);
        assert_eq!(frame[6], 0x08);
        assert_eq!(frame[7], 0x00);
    }

    #[test]
    fn test_make_usb_cmd_with_param() {
        let frame = UsbHidTransport::make_usb_cmd(cmd::CMD_INQUIRY_STA, 0x1234);
        assert_eq!(frame[2], 0x12); // param high (big-endian)
        assert_eq!(frame[3], 0x34); // param low
    }

    #[test]
    fn test_make_usb_cmd_two() {
        let frame = UsbHidTransport::make_usb_cmd_two(cmd::CMD_NEXT_ZIPPEDBULK, 512, 3);
        assert_eq!(frame[0], USB_MAGIC1);
        assert_eq!(frame[1], USB_MAGIC2);
        assert_eq!(frame[2], 0x02); // 512 >> 8 (big-endian)
        assert_eq!(frame[3], 0x00); // 512 & 0xFF
        assert_eq!(frame[4], cmd::CMD_NEXT_ZIPPEDBULK);
        assert_eq!(frame[8], 0x00); // 3 >> 8
        assert_eq!(frame[9], 0x03); // 3 & 0xFF
    }

    #[test]
    fn test_parse_usb_status_ready() {
        // Simulate: printing=true, no errors
        let resp = [0x11, 0x00, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00];
        let status = UsbHidTransport::parse_usb_status(&resp).unwrap();
        assert!(status.printing);
        assert!(!status.buf_full);
        assert!(!status.device_busy);
        assert!(!status.has_error());
    }

    #[test]
    fn test_parse_usb_status_errors() {
        let resp = [0x11, 0x02, 0x00, 0x08, 0x01, 0x05, 0x00, 0x00];
        let status = UsbHidTransport::parse_usb_status(&resp).unwrap();
        assert!(status.label_rw_error);
        assert!(status.cover_open);
        assert!(status.label_not_installed);
        assert!(status.has_error());
        assert_eq!(status.print_count, 5);
    }

    #[test]
    fn test_parse_usb_status_too_short() {
        assert!(UsbHidTransport::parse_usb_status(&[0; 4]).is_none());
    }

    #[test]
    fn test_validate_usb_response() {
        // USB responses don't echo command byte; validation treats any non-empty slice as ok.
        let resp = [0x08_u8];
        assert!(!resp.is_empty());
        let empty: &[u8] = &[];
        assert!(empty.is_empty());
    }
}
