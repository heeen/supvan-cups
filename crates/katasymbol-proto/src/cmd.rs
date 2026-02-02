/// Protocol magic bytes.
pub const MAGIC1: u8 = 0x7E;
pub const MAGIC2: u8 = 0x5A;
pub const PROTO_ID: u8 = 0x10;
pub const PROTO_VER: u8 = 0x01;
pub const MARKER_AA: u8 = 0xAA;
pub const DATA_TYPE: u8 = 0x02;

/// Command bytes.
pub const CMD_BUF_FULL: u8 = 0x10;
pub const CMD_INQUIRY_STA: u8 = 0x11;
pub const CMD_CHECK_DEVICE: u8 = 0x12;
pub const CMD_START_PRINT: u8 = 0x13;
pub const CMD_STOP_PRINT: u8 = 0x14;
pub const CMD_RD_DEV_NAME: u8 = 0x16;
pub const CMD_READ_REV: u8 = 0x17;
pub const CMD_PAPER_SKIP: u8 = 0x2E;
pub const CMD_RETURN_MAT: u8 = 0x30;
pub const CMD_NEXT_ZIPPEDBULK: u8 = 0x5C;
pub const CMD_SET_RFID_DATA: u8 = 0x5D;
pub const CMD_READ_FWVER: u8 = 0xC5;

/// Build a standard 16-byte command frame (0x7E 0x5A format).
///
/// Layout:
///   [0]  0x7E   [1]  0x5A
///   [2]  0x0C   [3]  0x00   (payload length = 12)
///   [4]  0x10   [5]  0x01   (protocol ID, version)
///   [6]  0xAA   [7]  CMD
///   [8..9]  checksum LE (sum of bytes 10..15)
///   [10] 0x00  [11] 0x01
///   [12..13] param LE
///   [14..15] 0x0000
pub fn make_cmd(cmd: u8, param: u16) -> [u8; 16] {
    let mut pkt = [0u8; 16];
    pkt[0] = MAGIC1;
    pkt[1] = MAGIC2;
    pkt[2] = 0x0C;
    // pkt[3] = 0x00;
    pkt[4] = PROTO_ID;
    pkt[5] = PROTO_VER;
    pkt[6] = MARKER_AA;
    pkt[7] = cmd;
    // pkt[10] = 0x00;
    pkt[11] = 0x01;
    pkt[12] = (param & 0xFF) as u8;
    pkt[13] = (param >> 8) as u8;
    // pkt[14..15] = 0x0000

    let chk: u16 = pkt[10..16].iter().map(|&b| b as u16).sum();
    pkt[8] = (chk & 0xFF) as u8;
    pkt[9] = (chk >> 8) as u8;
    pkt
}

/// Build a 16-byte start-transfer command.
///
/// Same as `make_cmd` but bytes 12-15 carry block_size and block_count.
/// Used for CMD_NEXT_ZIPPEDBULK (block_size=512, block_count=num_packets)
/// and CMD_BUF_FULL (block_size=compressed_len, block_count=speed).
pub fn make_cmd_start_trans(cmd: u8, block_size: u16, block_count: u16) -> [u8; 16] {
    let mut pkt = [0u8; 16];
    pkt[0] = MAGIC1;
    pkt[1] = MAGIC2;
    pkt[2] = 0x0C;
    pkt[4] = PROTO_ID;
    pkt[5] = PROTO_VER;
    pkt[6] = MARKER_AA;
    pkt[7] = cmd;
    pkt[11] = 0x01;
    pkt[12] = (block_size & 0xFF) as u8;
    pkt[13] = (block_size >> 8) as u8;
    pkt[14] = (block_count & 0xFF) as u8;
    pkt[15] = (block_count >> 8) as u8;

    let chk: u16 = pkt[10..16].iter().map(|&b| b as u16).sum();
    pkt[8] = (chk & 0xFF) as u8;
    pkt[9] = (chk >> 8) as u8;
    pkt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_make_cmd_check_device() {
        let pkt = make_cmd(CMD_CHECK_DEVICE, 0);
        assert_eq!(pkt[0], MAGIC1);
        assert_eq!(pkt[1], MAGIC2);
        assert_eq!(pkt[2], 0x0C);
        assert_eq!(pkt[3], 0x00);
        assert_eq!(pkt[7], CMD_CHECK_DEVICE);
        assert_eq!(pkt[10], 0x00);
        assert_eq!(pkt[11], 0x01);
        assert_eq!(pkt[12], 0x00);
        assert_eq!(pkt[13], 0x00);
        assert_eq!(pkt[14], 0x00);
        assert_eq!(pkt[15], 0x00);
        // checksum: sum of bytes 10..16 = 0+1+0+0+0+0 = 1
        assert_eq!(pkt[8], 0x01);
        assert_eq!(pkt[9], 0x00);
    }

    #[test]
    fn test_make_cmd_with_param() {
        let pkt = make_cmd(CMD_INQUIRY_STA, 0x1234);
        assert_eq!(pkt[12], 0x34);
        assert_eq!(pkt[13], 0x12);
        let chk: u16 = pkt[10..16].iter().map(|&b| b as u16).sum();
        assert_eq!(pkt[8], (chk & 0xFF) as u8);
        assert_eq!(pkt[9], (chk >> 8) as u8);
    }

    #[test]
    fn test_make_cmd_start_trans() {
        let pkt = make_cmd_start_trans(CMD_NEXT_ZIPPEDBULK, 512, 3);
        assert_eq!(pkt[7], CMD_NEXT_ZIPPEDBULK);
        assert_eq!(pkt[12], 0x00); // 512 & 0xFF
        assert_eq!(pkt[13], 0x02); // 512 >> 8
        assert_eq!(pkt[14], 0x03);
        assert_eq!(pkt[15], 0x00);
        let chk: u16 = pkt[10..16].iter().map(|&b| b as u16).sum();
        assert_eq!(pkt[8], (chk & 0xFF) as u8);
        assert_eq!(pkt[9], (chk >> 8) as u8);
    }
}
