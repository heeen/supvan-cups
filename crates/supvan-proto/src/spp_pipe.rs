//! The SPP transport split into a "byte pipe" and a shared codec.
//!
//! An [`SppPipe`] carries already-framed Supvan frames (the 16-byte `0x7E5A`
//! command frame and the 512-byte data frame) over a physical link and returns
//! the device's raw response bytes. It owns only link mechanics — chunking,
//! pacing, draining, response polling — and knows nothing about opcodes or
//! response parsing; to tell when a reply is complete it may ask
//! [`crate::cmd::frame_len`].
//!
//! [`SppCodec`] drives any `SppPipe` to implement the full [`Transport`]
//! surface, so Classic-Bluetooth RFCOMM and BLE GATT share one codec and
//! differ only in their pipe.

use crate::cmd::{CMD_NEXT_ZIPPEDBULK, CmdFrame, make_cmd, make_cmd_start_trans};
use crate::data::{DATA_FRAME_LEN, DataFrame, build_data_frames};
use crate::error::Result;
use crate::status::{self, MaterialInfo, PrinterStatus};
use crate::transport::{Flow, Transport};
use async_trait::async_trait;
use std::time::Duration;

/// The vendor writes each frame as 128-byte pieces, 10 ms apart
/// (`BasePrint.transferSplitData`), on every SPP link.
pub(crate) const SPP_CHUNK: usize = 128;
pub(crate) const SPP_CHUNK_DELAY: Duration = Duration::from_millis(10);

/// Raw transport for pre-framed SPP frames.
#[async_trait]
pub trait SppPipe: Send + Sync {
    /// Send a 16-byte command frame; return the device's raw response.
    async fn send_cmd_frame(&self, frame: &CmdFrame) -> Result<Option<Vec<u8>>>;

    /// Send a 512-byte data frame. When `read_response` is set, read and return
    /// the device's response after the frame.
    async fn send_data_frame(
        &self,
        frame: &DataFrame,
        read_response: bool,
    ) -> Result<Option<Vec<u8>>>;

    /// Whether the link acks every data frame. Classic SPP does; BLE does not
    /// — `BasePrint.transferSplitData` only reads a reply in its
    /// `CLASSIC_BLUETOOTH` branch, so a BLE pipe that waited would stall for
    /// the response timeout on every frame.
    fn acks_data_frames(&self) -> bool {
        true
    }
}

/// Drives an [`SppPipe`] to implement the SPP-framed [`Transport`] protocol
/// (`0x7E5A` command framing, 512-byte data frames, BT response parsing).
pub struct SppCodec<P: SppPipe> {
    pipe: P,
}

impl<P: SppPipe> SppCodec<P> {
    pub fn new(pipe: P) -> Self {
        Self { pipe }
    }
}

#[async_trait]
impl<P: SppPipe> Transport for SppCodec<P> {
    fn flow(&self) -> Flow {
        Flow::Android
    }

    async fn send_cmd(&self, cmd: u8, param: u16) -> Result<Option<Vec<u8>>> {
        self.pipe.send_cmd_frame(&make_cmd(cmd, param)).await
    }

    async fn send_cmd_two(&self, cmd: u8, param1: u16, param2: u16) -> Result<Option<Vec<u8>>> {
        self.pipe
            .send_cmd_frame(&make_cmd_start_trans(cmd, param1, param2))
            .await
    }

    async fn send_bulk_header(
        &self,
        _compressed_len: u16,
        num_packets: usize,
    ) -> Result<Option<Vec<u8>>> {
        self.send_cmd_two(
            CMD_NEXT_ZIPPEDBULK,
            DATA_FRAME_LEN as u16,
            num_packets as u16,
        )
        .await
    }

    async fn send_bulk_data(
        &self,
        data: &[u8],
        read_final_response: bool,
    ) -> Result<Option<Vec<u8>>> {
        // The Android reference (`BasePrint.transferSplitData(..., true, ...)`)
        // reads a response after EVERY data packet, not only the last. The
        // firmware acks each packet and expects us to drain that ack before the
        // next — otherwise leftover RX bytes confuse the next BUF_FULL read.
        // Links that don't ack at all (BLE) opt out via `acks_data_frames`.
        let acked = self.pipe.acks_data_frames();
        let frames = build_data_frames(data)?;
        let mut last_resp = None;
        for (i, frame) in frames.iter().enumerate() {
            let is_last = i == frames.len() - 1;
            let want_resp = acked && (!is_last || read_final_response);
            let resp = self.pipe.send_data_frame(frame, want_resp).await?;
            if is_last {
                last_resp = resp;
            }
        }
        Ok(last_resp)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Records the `read_response` flag the codec asked for on each frame.
    struct RecordingPipe {
        acks: bool,
        asked: Mutex<Vec<bool>>,
    }

    #[async_trait]
    impl SppPipe for RecordingPipe {
        async fn send_cmd_frame(&self, _frame: &CmdFrame) -> Result<Option<Vec<u8>>> {
            Ok(None)
        }

        async fn send_data_frame(
            &self,
            _frame: &DataFrame,
            read_response: bool,
        ) -> Result<Option<Vec<u8>>> {
            self.asked.lock().unwrap().push(read_response);
            Ok(None)
        }

        fn acks_data_frames(&self) -> bool {
            self.acks
        }
    }

    async fn asked_for(acks: bool, read_final_response: bool) -> Vec<bool> {
        let codec = SppCodec::new(RecordingPipe {
            acks,
            asked: Mutex::new(Vec::new()),
        });
        // Three frames' worth of payload.
        let payload = vec![0u8; crate::data::DATA_PAYLOAD_SIZE * 2 + 1];
        codec
            .send_bulk_data(&payload, read_final_response)
            .await
            .unwrap();
        codec.pipe.asked.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn acking_links_drain_every_frame_but_honour_the_final_flag() {
        assert_eq!(asked_for(true, false).await, vec![true, true, false]);
        assert_eq!(asked_for(true, true).await, vec![true, true, true]);
    }

    #[tokio::test]
    async fn non_acking_links_never_wait() {
        // BLE: waiting on a frame the firmware never acks costs one response
        // timeout per frame.
        assert_eq!(asked_for(false, false).await, vec![false, false, false]);
        assert_eq!(asked_for(false, true).await, vec![false, false, false]);
    }
}
