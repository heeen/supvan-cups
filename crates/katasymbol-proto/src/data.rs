use crate::cmd::{DATA_TYPE, MAGIC1, MAGIC2, PROTO_ID};

/// Data packet magic bytes.
pub const DATA_MAGIC1: u8 = 0xAA;
pub const DATA_MAGIC2: u8 = 0xBB;

/// Max payload per data packet.
pub const DATA_PAYLOAD_SIZE: usize = 500;

/// Build a 506-byte data packet (0xAA 0xBB format).
///
/// Layout:
///   [0]     0xAA
///   [1]     0xBB
///   [2..3]  checksum LE (sum of bytes 4..505)
///   [4]     packet index (0-based)
///   [5]     total packet count
///   [6..505] payload (500 bytes, zero-padded)
pub fn make_data_packet(data_chunk: &[u8], pkt_idx: u8, pkt_total: u8) -> [u8; 506] {
    let mut pkt = [0u8; 506];
    pkt[0] = DATA_MAGIC1;
    pkt[1] = DATA_MAGIC2;
    pkt[4] = pkt_idx;
    pkt[5] = pkt_total;

    let copy_len = data_chunk.len().min(DATA_PAYLOAD_SIZE);
    pkt[6..6 + copy_len].copy_from_slice(&data_chunk[..copy_len]);

    let chk: u16 = pkt[4..506].iter().map(|&b| b as u16).sum();
    pkt[2] = (chk & 0xFF) as u8;
    pkt[3] = (chk >> 8) as u8;
    pkt
}

/// Wrap a 506-byte data packet in a 512-byte transfer frame.
///
/// Layout:
///   [0]     0x7E
///   [1]     0x5A
///   [2..3]  0x01FC (payload length = 508)
///   [4]     0x10 (protocol ID)
///   [5]     0x02 (data transfer type)
///   [6..511] 506 bytes of payload (the AA BB packet)
pub fn wrap_data_frame(payload: &[u8; 506]) -> [u8; 512] {
    let mut frame = [0u8; 512];
    frame[0] = MAGIC1;
    frame[1] = MAGIC2;
    frame[2] = 0xFC;
    frame[3] = 0x01;
    frame[4] = PROTO_ID;
    frame[5] = DATA_TYPE;
    frame[6..512].copy_from_slice(payload);
    frame
}

/// Split compressed data into 506-byte data packets wrapped in 512-byte frames.
///
/// Returns a Vec of 512-byte frames ready to send.
pub fn build_data_frames(compressed: &[u8]) -> Vec<[u8; 512]> {
    let num_packets = compressed.len().div_ceil(DATA_PAYLOAD_SIZE);
    let pkt_total = num_packets as u8;
    let mut frames = Vec::with_capacity(num_packets);

    for i in 0..num_packets {
        let offset = i * DATA_PAYLOAD_SIZE;
        let end = (offset + DATA_PAYLOAD_SIZE).min(compressed.len());
        let chunk = &compressed[offset..end];
        let pkt = make_data_packet(chunk, i as u8, pkt_total);
        frames.push(wrap_data_frame(&pkt));
    }

    frames
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_data_packet() {
        let data = [0x42u8; 500];
        let pkt = make_data_packet(&data, 0, 3);
        assert_eq!(pkt[0], DATA_MAGIC1);
        assert_eq!(pkt[1], DATA_MAGIC2);
        assert_eq!(pkt[4], 0);
        assert_eq!(pkt[5], 3);
        assert_eq!(&pkt[6..506], &data[..]);
        let chk: u16 = pkt[4..506].iter().map(|&b| b as u16).sum();
        assert_eq!(pkt[2], (chk & 0xFF) as u8);
        assert_eq!(pkt[3], (chk >> 8) as u8);
    }

    #[test]
    fn test_make_data_packet_short() {
        let data = [0xFFu8; 100];
        let pkt = make_data_packet(&data, 2, 5);
        assert_eq!(pkt[4], 2);
        assert_eq!(pkt[5], 5);
        // First 100 bytes should be 0xFF, rest 0x00
        assert_eq!(&pkt[6..106], &[0xFF; 100]);
        assert_eq!(&pkt[106..506], &[0x00; 400]);
    }

    #[test]
    fn test_wrap_data_frame() {
        let pkt = [0xAA; 506];
        let frame = wrap_data_frame(&pkt);
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
        let frames = build_data_frames(&data);
        assert_eq!(frames.len(), 3);
        // Check packet indices
        assert_eq!(frames[0][6 + 4], 0); // pkt_idx
        assert_eq!(frames[0][6 + 5], 3); // pkt_total
        assert_eq!(frames[1][6 + 4], 1);
        assert_eq!(frames[2][6 + 4], 2);
    }
}
