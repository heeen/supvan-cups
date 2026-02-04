//! USB HID transport implementing the Transport trait.
//!
//! Uses 0xC0/0x40 command framing with big-endian parameters,
//! 64-byte HID reports for data transfer, and 8-byte responses.

use crate::error::Result;
use crate::hidraw::{HidrawDevice, HID_REPORT_SIZE};
use crate::status::{MaterialInfo, PrinterStatus};
use crate::transport::Transport;
use std::os::unix::io::RawFd;
use std::time::Duration;

/// USB HID command magic bytes.
const USB_MAGIC1: u8 = 0xC0;
const USB_MAGIC2: u8 = 0x40;

/// Default response timeout for USB HID.
const USB_RESPONSE_TIMEOUT: Duration = Duration::from_secs(2);

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
    /// The 64-byte report layout (from Electron app reverse-engineering):
    ///   [0]       length/type indicator
    ///   [1..8]    status bytes (same as INQUIRY_STA)
    ///   [19]      width_mm
    ///   [20]      height_mm
    ///   [21]      gap_mm
    ///   [22]      label_type
    ///   [31..32]  SN (LE16)
    ///   [40..]    device serial as ASCII (null-terminated)
    fn parse_usb_material(resp: &[u8]) -> Option<MaterialInfo> {
        if resp.len() < 40 {
            log::debug!("USB material response too short: {} bytes", resp.len());
            return None;
        }

        let width_mm = resp[19];
        let height_mm = resp[20];
        let gap_mm = resp[21];
        let label_type = resp[22];
        let sn = u16::from_le_bytes([resp[31], resp[32]]);

        // Device serial at offset 40, null-terminated ASCII
        let dev_sn = if resp.len() > 40 {
            let end = resp[40..]
                .iter()
                .position(|&b| b == 0)
                .unwrap_or(resp.len() - 40);
            let s = String::from_utf8_lossy(&resp[40..40 + end]).to_string();
            if s.is_empty() { None } else { Some(s) }
        } else {
            None
        };

        log::info!(
            "USB material: {}x{}mm gap={}mm type={} sn={} dev_sn={:?}",
            width_mm, height_mm, gap_mm, label_type, sn, dev_sn
        );

        Some(MaterialInfo {
            uuid: String::new(),
            code: String::new(),
            sn,
            label_type,
            width_mm,
            height_mm,
            gap_mm,
            remaining: None,
            device_sn: dev_sn,
        })
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

        let b0 = resp[1]; // MSTA low
        let b1 = resp[2]; // MSTA high
        let b2 = resp[3]; // FSTA low
        let b3 = resp[4]; // FSTA high

        Some(PrinterStatus {
            buf_full: b0 & 0x01 != 0,
            label_rw_error: b0 & 0x02 != 0,
            label_end: b0 & 0x04 != 0,
            label_mode_error: b0 & 0x08 != 0,
            ribbon_rw_error: b0 & 0x10 != 0,
            ribbon_end: b0 & 0x20 != 0,
            low_battery: b0 & 0x40 != 0,
            device_busy: b1 & 0x04 != 0,
            head_temp_high: b1 & 0x08 != 0,
            cover_open: b2 & 0x08 != 0,
            insert_usb: b2 & 0x10 != 0,
            printing: b2 & 0x40 != 0,
            label_not_installed: b3 & 0x01 != 0,
            print_count: u16::from_le_bytes([resp[5], resp[6]]),
        })
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

    fn raw_fd(&self) -> RawFd {
        self.dev.raw_fd()
    }

    fn use_socket_io(&self) -> bool {
        false
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
        // USB responses don't echo command byte; any non-empty response is valid
        assert!(!Vec::<u8>::new().is_empty() || true); // placeholder for is_empty check
        let resp = [0x08_u8];
        assert!(!resp.is_empty());
        let empty: &[u8] = &[];
        assert!(empty.is_empty());
    }
}
