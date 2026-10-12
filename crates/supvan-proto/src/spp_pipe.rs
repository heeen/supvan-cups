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
use crate::data::{DATA_FRAME_LEN, DATA_PAYLOAD_SIZE, DataFrame, build_data_frames};
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
pub(crate) trait SppPipe: Send + Sync {
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
pub(crate) struct SppCodec<P: SppPipe> {
    pipe: P,
}

impl<P: SppPipe> SppCodec<P> {
    pub(crate) fn new(pipe: P) -> Self {
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

    async fn send_bulk_header(&self, compressed_len: u16) -> Result<Option<Vec<u8>>> {
        let packets = usize::from(compressed_len).div_ceil(DATA_PAYLOAD_SIZE);
        self.send_cmd_two(CMD_NEXT_ZIPPEDBULK, DATA_FRAME_LEN as u16, packets as u16)
            .await
    }

    async fn send_bulk_data(&self, data: &[u8]) -> Result<Option<Vec<u8>>> {
        // `BasePrint.transferSplitData(..., true, ...)` reads the ack after
        // every data frame. We skip the last one: the firmware sends it only
        // once it has digested the whole block, ~0.4 s later on a dense block,
        // and waiting for it halves Bluetooth throughput — the head then
        // outruns the data and stalls. Whatever does arrive is drained before
        // the next command. Links that don't ack at all (BLE) never wait.
        let acked = self.pipe.acks_data_frames();
        let frames = build_data_frames(data)?;
        let last = frames.len().saturating_sub(1);
        for (i, frame) in frames.iter().enumerate() {
            self.pipe.send_data_frame(frame, acked && i < last).await?;
        }
        Ok(None)
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

    async fn asked_for(acks: bool) -> Vec<bool> {
        let codec = SppCodec::new(RecordingPipe {
            acks,
            asked: Mutex::new(Vec::new()),
        });
        // Three frames' worth of payload.
        let payload = vec![0u8; DATA_PAYLOAD_SIZE * 2 + 1];
        codec.send_bulk_data(&payload).await.unwrap();
        codec.pipe.asked.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn acking_links_read_every_ack_but_the_last() {
        assert_eq!(asked_for(true).await, vec![true, true, false]);
    }

    #[tokio::test]
    async fn non_acking_links_never_wait() {
        // BLE: waiting on a frame the firmware never acks costs one response
        // timeout per frame.
        assert_eq!(asked_for(false).await, vec![false, false, false]);
    }
}
