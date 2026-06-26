//! Bluetooth RFCOMM transport implementing the Transport trait.
//!
//! Wraps an `RfcommSocket` and uses the existing 0x7E/5A command framing,
//! 512-byte data frames, and BT-specific response parsing.

use crate::cmd::{CMD_NEXT_ZIPPEDBULK, make_cmd, make_cmd_start_trans};
use crate::data::build_data_frames;
use crate::error::Result;
use crate::rfcomm::RfcommSocket;
use crate::status::{self, MaterialInfo, PrinterStatus};
use crate::transport::Transport;

/// Bluetooth RFCOMM transport.
pub struct BtTransport {
    sock: RfcommSocket,
}

impl BtTransport {
    pub fn new(sock: RfcommSocket) -> Self {
        Self { sock }
    }
}

impl Transport for BtTransport {
    fn send_cmd(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>> {
        let frame = make_cmd(cmd, param);
        self.sock.send_cmd(&frame)
    }

    fn send_cmd_two(&self, cmd: u8, param1: u16, param2: u16) -> Result<Option<Vec<u8>>> {
        let frame = make_cmd_start_trans(cmd, param1, param2);
        self.sock.send_cmd(&frame)
    }

    fn send_bulk_data(&self, data: &[u8], read_final_response: bool) -> Result<Option<Vec<u8>>> {
        // The Android reference (`BasePrint.transferSplitData(..., true, ...)`
        // called from `T50PlusPrint.transfer`) reads a response after EVERY
        // 506-byte data packet, not only the last. The printer firmware acks
        // each packet and seems to expect us to drain that ack before the
        // next packet — otherwise the RX buffer accumulates leftover bytes
        // that confuse the next BUF_FULL response read.
        let frames = build_data_frames(data);
        let mut last_resp = None;
        for (i, frame) in frames.iter().enumerate() {
            let is_last = i == frames.len() - 1;
            let want_resp = if is_last { read_final_response } else { true };
            let resp = self.sock.send_data_frame(frame, want_resp)?;
            if is_last {
                last_resp = resp;
            }
        }
        Ok(last_resp)
    }

    fn send_bulk_header(&self, _compressed_len: u16, num_packets: usize) -> Result<Option<Vec<u8>>> {
        // SPP encodes NEXT_ZIPPEDBULK as (block_size=512, packet_count).
        self.send_cmd_two(CMD_NEXT_ZIPPEDBULK, 512, num_packets as u16)
    }

    fn parse_status_response(&self, resp: &[u8]) -> Option<PrinterStatus> {
        status::parse_status(resp)
    }

    fn parse_material_response(&self, resp: &[u8]) -> Option<MaterialInfo> {
        status::parse_material(resp)
    }

    fn validate_response(&self, resp: &[u8], expected_cmd: u8) -> bool {
        status::validate_response(resp, expected_cmd)
    }

    fn parse_device_name_response(&self, resp: &[u8]) -> Option<String> {
        status::parse_device_name(resp)
    }

    fn parse_firmware_version_response(&self, resp: &[u8]) -> Option<u8> {
        status::parse_firmware_version(resp)
    }

    fn parse_version_response(&self, resp: &[u8]) -> Option<String> {
        status::parse_version(resp)
    }
}
