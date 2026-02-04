//! Bluetooth RFCOMM transport implementing the Transport trait.
//!
//! Wraps an `RfcommSocket` and uses the existing 0x7E/5A command framing,
//! 512-byte data frames, and BT-specific response parsing.

use crate::cmd::{make_cmd, make_cmd_start_trans};
use crate::data::build_data_frames;
use crate::error::Result;
use crate::rfcomm::RfcommSocket;
use crate::status::{self, MaterialInfo, PrinterStatus};
use crate::transport::Transport;
use std::os::unix::io::RawFd;

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
        let frames = build_data_frames(data);
        let mut last_resp = None;
        for (i, frame) in frames.iter().enumerate() {
            let is_last = i == frames.len() - 1;
            let resp = self.sock.send_data_frame(frame, is_last && read_final_response)?;
            if is_last {
                last_resp = resp;
            }
        }
        Ok(last_resp)
    }

    fn raw_fd(&self) -> RawFd {
        self.sock.raw_fd()
    }

    fn use_socket_io(&self) -> bool {
        true
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
