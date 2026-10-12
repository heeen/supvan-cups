use crate::cmd::{DATA_TYPE, FRAME_HEADER_LEN, MAGIC1, MAGIC2, PROTO_ID, checksum16};
use crate::error::{Error, Result};

/// Data packet magic bytes.
pub(crate) const DATA_MAGIC1: u8 = 0xAA;
pub(crate) const DATA_MAGIC2: u8 = 0xBB;

/// Firmware-packet marker (byte 1). A firmware transfer reuses the exact
/// packet + frame layout as print data, but marks each packet `0xAA 0xC7`
/// instead of `0xAA 0xBB`. The marker sits outside the checksum range, so it
/// doesn't affect the packet checksum.
pub(crate) const FIRMWARE_MAGIC2: u8 = 0xC7;

/// Max payload per data packet.
pub const DATA_PAYLOAD_SIZE: usize = 500;
/// `AA BB` packet: 6-byte header + payload.
pub(crate) const DATA_PACKET_LEN: usize = 6 + DATA_PAYLOAD_SIZE;
/// A data frame on the wire: frame header, two type bytes, then the packet.
pub(crate) const DATA_FRAME_LEN: usize = 512;
pub(crate) type DataFrame = [u8; DATA_FRAME_LEN];

/// Build one data packet.
///
/// Layout:
///   [0]     0xAA
///   [1]     marker (0xBB print data, 0xC7 firmware)
///   [2..3]  checksum LE (sum of bytes 4..)
///   [4]     packet index (0-based)
///   [5]     total packet count
///   [6..]   payload (500 bytes, zero-padded)
fn make_packet(chunk: &[u8], marker: u8, idx: u8, total: u8) -> [u8; DATA_PACKET_LEN] {
    let mut pkt = [0u8; DATA_PACKET_LEN];
    pkt[0] = DATA_MAGIC1;
    pkt[1] = marker;
    pkt[4] = idx;
    pkt[5] = total;
    let len = chunk.len().min(DATA_PAYLOAD_SIZE);
    pkt[6..6 + len].copy_from_slice(&chunk[..len]);
    let chk = checksum16(&pkt[4..]);
    pkt[2..4].copy_from_slice(&chk.to_le_bytes());
    pkt
}

/// Wrap a packet in a transfer frame: `7E 5A`, payload length, `10 02`, packet.
fn wrap_frame(packet: &[u8; DATA_PACKET_LEN]) -> DataFrame {
    let mut frame = [0u8; DATA_FRAME_LEN];
    frame[0] = MAGIC1;
    frame[1] = MAGIC2;
    frame[2..4].copy_from_slice(&((DATA_FRAME_LEN - FRAME_HEADER_LEN) as u16).to_le_bytes());
    frame[4] = PROTO_ID;
    frame[5] = DATA_TYPE;
    frame[6..].copy_from_slice(packet);
    frame
}

/// Split bytes into numbered packets with the given marker, each in a frame.
fn build_frames(bytes: &[u8], marker: u8) -> Result<Vec<DataFrame>> {
    let chunks = bytes.chunks(DATA_PAYLOAD_SIZE);
    // Index and count are single bytes on the wire.
    let total = u8::try_from(chunks.len()).map_err(|_| {
        Error::InvalidParam(format!(
            "{} bytes need more than 255 data packets",
            bytes.len()
        ))
    })?;
    Ok(chunks
        .enumerate()
        .map(|(i, chunk)| wrap_frame(&make_packet(chunk, marker, i as u8, total)))
        .collect())
}

/// Split compressed print data into transfer frames.
pub(crate) fn build_data_frames(compressed: &[u8]) -> Result<Vec<DataFrame>> {
    build_frames(compressed, DATA_MAGIC2)
}

/// Split **raw firmware** into 512-byte transfer frames for a firmware flash.
///
/// Identical framing to [`build_data_frames`] except each packet carries the
/// firmware marker ([`FIRMWARE_MAGIC2`], `0xC7`) and the bytes are raw (a
/// firmware image is not LZMA-compressed like print buffers).
///
/// A flasher sends `send_cmd_two(`[`cmd::CMD_UPDATE_FW`](crate::cmd::CMD_UPDATE_FW)`, 512, frames.len())`,
/// then each returned frame in order with a per-packet ack (the same drain the
/// print bulk path uses). This is a framing primitive only — the flash is
/// destructive and there is no on-device verification on T50-class printers, so
/// the live send is deliberately left to a caller (see `docs/FIRMWARE.md`).
pub fn build_firmware_frames(firmware: &[u8]) -> Result<Vec<[u8; DATA_FRAME_LEN]>> {
    build_frames(firmware, FIRMWARE_MAGIC2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_data_packet() {
        let data = [0x42u8; 500];
        let pkt = make_packet(&data, DATA_MAGIC2, 0, 3);
        assert_eq!(pkt[0], DATA_MAGIC1);
        assert_eq!(pkt[1], DATA_MAGIC2);
        assert_eq!(pkt[4], 0);
        assert_eq!(pkt[5], 3);
        assert_eq!(&pkt[6..506], &data[..]);
        assert_eq!(pkt[2..4], checksum16(&pkt[4..]).to_le_bytes());
    }

    #[test]
    fn test_make_data_packet_short() {
        let data = [0xFFu8; 100];
        let pkt = make_packet(&data, DATA_MAGIC2, 2, 5);
        assert_eq!(pkt[4], 2);
        assert_eq!(pkt[5], 5);
        // First 100 bytes should be 0xFF, rest 0x00
        assert_eq!(&pkt[6..106], &[0xFF; 100]);
        assert_eq!(&pkt[106..506], &[0x00; 400]);
    }

    #[test]
    fn test_wrap_data_frame() {
        let pkt = [0xAA; DATA_PACKET_LEN];
        let frame = wrap_frame(&pkt);
        assert_eq!(frame[0], MAGIC1);
        assert_eq!(frame[1], MAGIC2);
        assert_eq!(frame[2], 0xFC);
        assert_eq!(frame[3], 0x01);
        assert_eq!(frame[4], PROTO_ID);
        assert_eq!(frame[5], DATA_TYPE);
        assert_eq!(&frame[6..512], &pkt[..]);
    }

    #[test]
    fn test_build_data_frames() {
        // 1100 bytes -> 3 packets (500+500+100)
        let data = vec![0x42u8; 1100];
        let frames = build_data_frames(&data).unwrap();
        assert_eq!(frames.len(), 3);
        // Check packet indices
        assert_eq!(frames[0][6 + 4], 0); // pkt_idx
        assert_eq!(frames[0][6 + 5], 3); // pkt_total
        assert_eq!(frames[1][6 + 4], 1);
        assert_eq!(frames[2][6 + 4], 2);
    }

    #[test]
    fn test_build_firmware_frames_layout() {
        // 1100 raw bytes -> 3 firmware packets (500+500+100).
        let fw = vec![0x42u8; 1100];
        let frames = build_firmware_frames(&fw).unwrap();
        assert_eq!(frames.len(), 3);
        for (i, frame) in frames.iter().enumerate() {
            // Frame header is the standard 0x7E5A data transfer frame.
            assert_eq!(frame[0], MAGIC1);
            assert_eq!(frame[1], MAGIC2);
            assert_eq!(frame[5], DATA_TYPE);
            // Packet is 0xAA 0xC7 (firmware marker, not 0xBB).
            assert_eq!(frame[6], DATA_MAGIC1);
            assert_eq!(frame[6 + 1], FIRMWARE_MAGIC2);
            assert_eq!(frame[6 + 4], i as u8); // pkt_idx
            assert_eq!(frame[6 + 5], 3); // pkt_total
        }
    }

    #[test]
    fn test_firmware_checksum_matches_vendor() {
        // The vendor sums bytes [4..506) of the 506-byte packet (idx, total,
        // payload) — the 0xC7 marker at [1] is excluded, so a firmware packet's
        // checksum equals the same packet's as a print packet.
        let fw = vec![0x37u8; 500];
        let frame = &build_firmware_frames(&fw).unwrap()[0];
        let pkt = &frame[6..512]; // the 506-byte packet
        assert_eq!(pkt[2..4], checksum16(&pkt[4..]).to_le_bytes());
    }

    #[test]
    fn test_firmware_last_packet_zero_padded() {
        let fw = vec![0xABu8; 500 + 3]; // 2 packets, last has 3 bytes
        let frames = build_firmware_frames(&fw).unwrap();
        assert_eq!(frames.len(), 2);
        let last = &frames[1];
        assert_eq!(&last[6 + 6..6 + 6 + 3], &[0xAB; 3]); // 3 payload bytes
        assert_eq!(&last[6 + 6 + 3..512], &[0x00; 497]); // rest zero-padded
    }
}
