/// Protocol magic bytes.
pub(crate) const MAGIC1: u8 = 0x7E;
pub(crate) const MAGIC2: u8 = 0x5A;
pub(crate) const PROTO_ID: u8 = 0x10;
pub(crate) const PROTO_VER: u8 = 0x01;
pub(crate) const MARKER_AA: u8 = 0xAA;
pub(crate) const DATA_TYPE: u8 = 0x02;
/// Byte 11 of every command frame. The vendor always sets it to 1
/// (`bArr2[11] = 1`); its meaning is unknown.
const CMD_FLAGS: u8 = 0x01;

/// Every SPP frame opens with the two magic bytes and a little-endian payload
/// length; the frame is that many bytes plus this header.
pub(crate) const FRAME_HEADER_LEN: usize = 4;
/// A command frame, host to device.
pub(crate) const CMD_FRAME_LEN: usize = 16;
pub(crate) type CmdFrame = [u8; CMD_FRAME_LEN];
/// Where frames in both directions carry the command byte.
pub(crate) const CMD_OFFSET: usize = 7;

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
pub const CMD_READ_FWVER: u8 = 0xC5;
/// Firmware-transfer start (`sendCmdStartTrans(0xC6, 512, num_chunks)`), the
/// firmware analogue of `NEXT_ZIPPEDBULK`. Followed by `0xAA 0xC7` firmware
/// packets. See [`crate::data::build_firmware_frames`] and `docs/FIRMWARE.md`.
pub const CMD_UPDATE_FW: u8 = 0xC6;

// --- Additional opcodes recovered from the vendor Linux tool (Electron source
// map, `com.supvan.supvaneditor` 1.1.4). Not all are exercised by the T50
// family — model applicability and used/reserved status noted per opcode. We
// don't drive these yet; kept as a complete vocabulary. See docs/PROTOCOL.md.

/// CHECK_RIB (0x19) — check ribbon. Defined in the vendor tool; no active call
/// site observed.
pub const CMD_CHECK_RIB: u8 = 0x19;

/// RD_LAB_DPI (0x22) — read the loaded label's DPI (response carries DPI×100 as
/// a little-endian u16; the offset varies per material). Used by the G/TP/MP50
/// plugins. The Android `BasePrint` calls this one `CMD_READ_DPI`.
pub const CMD_RD_LAB_DPI: u8 = 0x22;
/// Per-material DPI read variant from the `sp` plugin. `BasePrint` does not name
/// 0x24, so this reading is from the Linux tool alone.
pub const CMD_RD_LAB_DPI_24: u8 = 0x24;

/// SET_PRTMODE (0x33) — set print mode. MP50/P70-family in the vendor tool.
pub const CMD_SET_PRTMODE: u8 = 0x33;

/// SEND_INF (0x35) — set print density. MP50/P70-family in the vendor tool.
pub const CMD_SEND_INF: u8 = 0x35;

/// SET_RFID_DATA (0x5D) — authenticate/write label RFID data; the `RfidData`
/// payload follows as a bulk write. Used by the T50 and MP50 plugins. Payload
/// format not yet decoded (see docs/PROTOCOL.md known gaps).
pub const CMD_SET_RFID_DATA: u8 = 0x5D;

/// TRANSFER (0xF0) — "传输字模" (dot-pattern transfer). **Reserved**: defined
/// in the vendor tool but never sent; the live bitmap path is
/// [`CMD_NEXT_ZIPPEDBULK`]. Possibly an alternate transfer mode/model.
pub const CMD_TRANSFER: u8 = 0xF0;

// --- Opcodes from the Android app (`BasePrint.java`, plus `AdjustManager`).
//
// The Electron editor exposes a far smaller vocabulary than the Android app;
// an earlier reading of it alone concluded the command set was complete, which
// was wrong. `BasePrint` declares 54 opcodes against the editor's ~20.
//
// **Almost none of these have a call site.** The app declares them and never
// sends them, so for most we have a number and a name but no observed frame
// shape, no parameter semantics, and no evidence this firmware implements them.
// Treat every one as unverified until a device round-trip says otherwise. The
// handful that *are* exercised say so individually.

/// STRD_MAT (0x18) — standard/stored material query. One of the few with call
/// sites: `sendCmd(cmd, 0, buf)` in `D15TPrint`, `sendCmdRefreshStatus(cmd, 0)`
/// in `TP76iPrint`. Not seen on the T50 path.
pub const CMD_STRD_MAT: u8 = 0x18;

/// Heat-time read/set — the same printhead pulse widths the RFID material
/// record carries at offsets 36/38, but addressable directly instead of through
/// an 80-byte record. If the firmware honours these, per-band heat becomes
/// possible without rewriting the material. No call sites.
pub const BLTCMD_HTIME_RD: u8 = 0x2B;
pub const BLTCMD_HTIME_SET: u8 = 0x2C;

/// Print-position (印位, "yin wei") adjustment: label origin, printhead offset,
/// left/right and top/bottom shift. Only `SET_TB_YINWEI` has call sites, in the
/// G-series (`sendCmd(cmd, value, [0u8; 64])` — a value plus a 64-byte payload).
pub const CMD_SET_LAB_YINWEI: u8 = 0x25;
pub const CMD_RD_HD_YINWEI: u8 = 0x26;
pub const CMD_SET_HD_YINWEI: u8 = 0x27;
/// 0x31 carries two names in `BasePrint` — `RD_COnLAB_YINWEI` and `RD_DEV_DPI`.
pub const CMD_RD_CONLAB_YINWEI: u8 = 0x31;
pub const CMD_SET_RL_YINWEI: u8 = 0x38;
pub const CMD_SET_TB_YINWEI: u8 = 0x39;

/// PAPER_BACK (0xBA) — reverse feed.
///
/// Its existence corrects the earlier claim in `docs/PROTOCOL.md` that the
/// protocol has no backfeed. **But the app never sends it**, so the parameter
/// (distance? units?) is unknown, as is whether this firmware implements it.
/// Backfeeding blind can jam the roll — establish the parameter before use.
pub const CMD_PAPER_BACK: u8 = 0xBA;

/// Gap/optical sensor level. `CHECK_OPTLEVEL` is the only one of the three with
/// a call site, and notably it is in `T50PlusPrint` — our own model's class:
/// `sendCmdStartTrans(0xBC, i, i2, [0u8; 64])`, i.e. the two-parameter
/// [`make_cmd_start_trans`] shape.
pub const CMD_CHECK_OPTLEVEL: u8 = 0xBC;
pub const CMD_READ_OPTLEVEL: u8 = 0xBD;
pub const CMD_SET_OPTLEVEL: u8 = 0xBE;

/// Bluetooth link control and device parameter/option blocks. `RD_DEV_OPT` and
/// `WR_DEV_OPT` have call sites; the `BLTCMD_*_DEV_PAR` pair does not.
pub const CMD_SET_BLTCONTROL: u8 = 0x37;
pub const CMD_RD_DEV_OPT: u8 = 0x67;
pub const CMD_WR_DEV_OPT: u8 = 0x68;
pub const BLTCMD_RD_DEV_PAR: u8 = 0x69;
pub const BLTCMD_WR_DEV_PAR: u8 = 0x6A;

/// Power-off timeout and buzzer/key configuration. No call sites.
pub const CMD_READ_POWER_OFF_TIME: u8 = 0x41;
pub const CMD_SET_POWER_OFF_TIME: u8 = 0x42;
pub const CMD_READ_BUZZER_KEY: u8 = 0x43;
pub const CMD_SET_BUZZER_KEY: u8 = 0x44;

/// RD_USER_INF (0x58) — user information block. No call sites.
pub const CMD_RD_USER_INF: u8 = 0x58;

/// Consumable anti-counterfeit challenge/response and timestamps — the
/// device-side half of the authentication the RFID record's `Cipertext` and
/// `TimeStamp` fields belong to. Since the vendor ships those fields zeroed
/// (see [`crate::rfid`]), this whole family is presumably dormant. No call sites.
pub const CMD_SET_TIMESTAMP: u8 = 0xB0;
pub const CMD_RD_TIMESTAMP: u8 = 0xB1;
pub const CMD_MAT_AUTHEN_RESULT: u8 = 0xB6;
pub const CMD_READ_RANDOM: u8 = 0xD5;
pub const CMD_VERIFY_RANDOM: u8 = 0xD6;

/// Density and printhead rate as standalone commands, rather than the
/// per-buffer trims in [`crate::buffer::Density`]. No call sites.
pub const CMD_BLTCMD_SET_HEADRATE: u8 = 0xC9;
pub const CMD_BLTCMD_SET_DENSITY: u8 = 0xD9;

/// BLE firmware-update path: buffer-oriented, distinct from the
/// [`CMD_UPDATE_FW`] (0xC6) chunked transfer. `FORCEUPDATE` presumably bypasses
/// the version check.
///
/// **Do not probe this range.** An unrecognised opcode here risks leaving the
/// unit in a bootloader waiting for an image.
pub const BLTCMD_START_FW_BUFFER_DATA: u8 = 0xD0;
pub const BLTCMD_SEND_FW_BUFFER_DATA: u8 = 0xD1;
pub const BLTCMD_UPDATA_FINISH: u8 = 0xD2;
pub const CMD_FORCEUPDATE: u8 = 0xD3;

/// BLE bulk transfer (0x5A) — the GATT-side analogue of
/// [`CMD_NEXT_ZIPPEDBULK`].
pub const BLTCMD_NEXTFRM_BULK: u8 = 0x5A;

// --- `AdjustManager` (calibration/service menu). A second constant table that
// fills part of what looked like empty opcode space — a reminder that the holes
// listed in docs/PROTOCOL.md are "unknown", not "unused".

pub const CMD_ADJ_READ_DATA: u8 = 0x60;
pub const CMD_ADJ_WRITE_DATA: u8 = 0x61;
/// **Destructive.** Excluded from any probe sweep.
pub const CMD_ADJ_RESTORE_FACTORY: u8 = 0x62;
pub const CMD_ADJ_WRITE_START: u8 = 0x6F;
pub const CMD_ADJ_RD_CONTINUE: u8 = 0xB2;

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
pub(crate) fn make_cmd(cmd: u8, param: u16) -> CmdFrame {
    // A plain command is a start-transfer frame with no block_count; the param
    // occupies the block_size field at bytes 12-13.
    make_cmd_start_trans(cmd, param, 0)
}

/// Build a 16-byte start-transfer command.
///
/// Same as `make_cmd` but bytes 12-15 carry block_size and block_count.
/// Used for CMD_NEXT_ZIPPEDBULK (block_size=512, block_count=num_packets)
/// and CMD_BUF_FULL (block_size=compressed_len, block_count=speed).
pub(crate) fn make_cmd_start_trans(cmd: u8, block_size: u16, block_count: u16) -> CmdFrame {
    let mut pkt = [0u8; CMD_FRAME_LEN];
    pkt[0] = MAGIC1;
    pkt[1] = MAGIC2;
    pkt[2..4].copy_from_slice(&((CMD_FRAME_LEN - FRAME_HEADER_LEN) as u16).to_le_bytes());
    pkt[4] = PROTO_ID;
    pkt[5] = PROTO_VER;
    pkt[6] = MARKER_AA;
    pkt[CMD_OFFSET] = cmd;
    pkt[11] = CMD_FLAGS;
    pkt[12..14].copy_from_slice(&block_size.to_le_bytes());
    pkt[14..16].copy_from_slice(&block_count.to_le_bytes());
    let chk = checksum16(&pkt[10..]);
    pkt[8..10].copy_from_slice(&chk.to_le_bytes());
    pkt
}

/// The protocol's checksum: the byte sum, truncated to 16 bits. Command frames
/// sum bytes 10..16, data packets bytes 4..506.
pub(crate) fn checksum16(bytes: &[u8]) -> u16 {
    bytes
        .iter()
        .fold(0u16, |acc, &b| acc.wrapping_add(u16::from(b)))
}

/// Total length of a `7E 5A` frame, from its header's length field — `None`
/// until the header is in or if `resp` isn't one.
pub(crate) fn frame_len(resp: &[u8]) -> Option<usize> {
    match resp {
        [MAGIC1, MAGIC2, lo, hi, ..] => {
            Some(usize::from(u16::from_le_bytes([*lo, *hi])) + FRAME_HEADER_LEN)
        }
        _ => None,
    }
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
        assert_eq!(pkt[8..10], checksum16(&pkt[10..16]).to_le_bytes());
    }

    #[test]
    fn test_make_cmd_start_trans() {
        let pkt = make_cmd_start_trans(CMD_NEXT_ZIPPEDBULK, 512, 3);
        assert_eq!(pkt[7], CMD_NEXT_ZIPPEDBULK);
        assert_eq!(pkt[12], 0x00); // 512 & 0xFF
        assert_eq!(pkt[13], 0x02); // 512 >> 8
        assert_eq!(pkt[14], 0x03);
        assert_eq!(pkt[15], 0x00);
        assert_eq!(pkt[8..10], checksum16(&pkt[10..16]).to_le_bytes());
    }
}
