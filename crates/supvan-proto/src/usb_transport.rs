//! USB HID transport implementing the Transport trait.
//!
//! Uses 0xC0/0x40 command framing with big-endian parameters,
//! 64-byte HID reports for data transfer, and 8-byte responses.

use crate::error::Result;
use crate::hidraw::{HID_REPORT_SIZE, HidrawDevice};
use crate::status::{MaterialInfo, PrinterStatus};
use crate::transport::{Flow, Transport};
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

    /// Send a HID report and read the response.
    fn send_and_recv(&self, data: &[u8]) -> Result<Option<Vec<u8>>> {
        log::debug!("USB TX: {:02x?}", &data[..data.len().min(16)]);
        self.dev.drain()?;
        self.dev.write_report(data)?;
        let resp = self.dev.read_report(USB_RESPONSE_TIMEOUT)?;
        match &resp {
            Some(r) => log::debug!("USB RX: {r:02x?}"),
            None => log::debug!("USB RX: (no response)"),
        }
        Ok(resp)
    }
}

/// An 8-byte USB command: `C0 40`, the parameter **big-endian** (BT is LE),
/// the command byte, then `00 08 00` — constant in every vendor frame.
fn make_usb_cmd(cmd: u8, param: u16) -> [u8; 8] {
    let [hi, lo] = param.to_be_bytes();
    [USB_MAGIC1, USB_MAGIC2, hi, lo, cmd, 0x00, 0x08, 0x00]
}

/// A two-parameter command: the 8-byte form plus the second parameter, BE.
fn make_usb_cmd_two(cmd: u8, param1: u16, param2: u16) -> [u8; 10] {
    let mut frame = [0u8; 10];
    frame[..8].copy_from_slice(&make_usb_cmd(cmd, param1));
    frame[8..].copy_from_slice(&param2.to_be_bytes());
    frame
}

#[async_trait::async_trait]
impl Transport for UsbHidTransport {
    fn flow(&self) -> Flow {
        Flow::Electron
    }

    async fn send_cmd(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>> {
        let frame = make_usb_cmd(cmd, param);
        tokio::task::block_in_place(|| self.send_and_recv(&frame))
    }

    async fn send_cmd_two(&self, cmd: u8, param1: u16, param2: u16) -> Result<Option<Vec<u8>>> {
        let frame = make_usb_cmd_two(cmd, param1, param2);
        tokio::task::block_in_place(|| self.send_and_recv(&frame))
    }

    async fn send_bulk_data(
        &self,
        data: &[u8],
        read_final_response: bool,
    ) -> Result<Option<Vec<u8>>> {
        tokio::task::block_in_place(|| {
            // Raw compressed bytes in 64-byte reports, no per-report header
            // (Electron `BulkWriteType`). The device answers once, after the
            // byte count announced in NEXT_ZIPPEDBULK has arrived.
            self.dev.drain()?;
            for chunk in data.chunks(HID_REPORT_SIZE) {
                self.dev.write_report(chunk)?;
            }
            if read_final_response {
                self.dev.read_report(USB_RESPONSE_TIMEOUT)
            } else {
                Ok(None)
            }
        })
    }

    async fn send_bulk_header(
        &self,
        compressed_len: u16,
        _num_packets: usize,
    ) -> Result<Option<Vec<u8>>> {
        // USB HID encodes NEXT_ZIPPEDBULK as the total compressed byte length.
        self.send_cmd(crate::cmd::CMD_NEXT_ZIPPEDBULK, compressed_len)
            .await
    }

    /// `[0]` is a length/type byte, then the status block.
    fn parse_status_response(&self, resp: &[u8]) -> Option<PrinterStatus> {
        crate::status::decode_status_regs(resp.get(1..)?)
    }

    /// A 1-byte prefix, then the same material payload BT carries — plus the
    /// printer's ASCII serial at [`USB_DEVICE_SN_OFFSET`], which BT omits.
    fn parse_material_response(&self, resp: &[u8]) -> Option<MaterialInfo> {
        let device_sn = resp
            .get(USB_DEVICE_SN_OFFSET..)
            .and_then(crate::status::nul_str);
        crate::status::parse_material_payload(resp.get(1..)?, device_sn)
    }

    /// USB replies don't echo the command byte (`[0]` is a length/type byte),
    /// so any reply is the ack.
    fn validate_response(&self, resp: &[u8], _expected_cmd: u8) -> bool {
        !resp.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cmd;

    #[test]
    fn test_make_usb_cmd() {
        let frame = make_usb_cmd(cmd::CMD_CHECK_DEVICE, 0);
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
        let frame = make_usb_cmd(cmd::CMD_INQUIRY_STA, 0x1234);
        assert_eq!(frame[2], 0x12); // param high (big-endian)
        assert_eq!(frame[3], 0x34); // param low
    }

    #[test]
    fn test_make_usb_cmd_two() {
        let frame = make_usb_cmd_two(cmd::CMD_NEXT_ZIPPEDBULK, 512, 3);
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
        let status = crate::status::decode_status_regs(&resp[1..]).unwrap();
        assert!(status.printing);
        assert!(!status.buf_full);
        assert!(!status.device_busy);
        assert!(!status.has_error());
    }

    #[test]
    fn test_parse_usb_status_errors() {
        let resp = [0x11, 0x02, 0x00, 0x08, 0x01, 0x05, 0x00, 0x00];
        let status = crate::status::decode_status_regs(&resp[1..]).unwrap();
        assert!(status.label_rw_error);
        assert!(status.cover_open);
        assert!(status.label_not_installed);
        assert!(status.has_error());
        assert_eq!(status.print_count, 5);
    }

    #[test]
    fn test_parse_usb_status_too_short() {
        assert!(crate::status::decode_status_regs(&[0; 4]).is_none());
    }
}
