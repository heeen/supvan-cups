use crate::cmd::{
    CMD_INQUIRY_STA, CMD_RD_DEV_NAME, CMD_READ_FWVER, CMD_READ_REV, CMD_RETURN_MAT, MAGIC1, MAGIC2,
};

/// Length of the BT response framing that precedes every payload. Parsers slice
/// the payload from `data[BT_RESP_HEADER_LEN..]`.
const BT_RESP_HEADER_LEN: usize = 22;

/// Number of fixed framing bytes the RD_DEV_NAME `payload_len` field counts
/// before the device-name string itself.
const DEV_NAME_PAYLOAD_PREFIX: usize = 18;

/// Parsed printer status from CMD_INQUIRY_STA response.
#[derive(Debug, Clone, Default)]
pub struct PrinterStatus {
    // MSTA_REG low (byte 14)
    pub buf_full: bool,
    pub label_rw_error: bool,
    pub label_end: bool,
    pub label_mode_error: bool,
    pub ribbon_rw_error: bool,
    pub ribbon_end: bool,
    pub low_battery: bool,
    // MSTA_REG high (byte 15)
    pub device_busy: bool,
    pub head_temp_high: bool,
    // FSTA_REG low (byte 16)
    pub cover_open: bool,
    pub insert_usb: bool,
    pub printing: bool,
    // FSTA_REG high (byte 17)
    pub label_not_installed: bool,
    // Bytes 18-19
    pub print_count: u16,
}

impl PrinterStatus {
    /// Error flags paired with their human-readable descriptions, in report
    /// order. Single source for [`has_error`](Self::has_error) and
    /// [`error_description`](Self::error_description).
    fn error_flags(&self) -> [(bool, &'static str); 8] {
        [
            (self.label_rw_error, "label read/write error"),
            (self.label_end, "label roll end"),
            (self.label_mode_error, "label mode mismatch"),
            (self.ribbon_rw_error, "ribbon read/write error"),
            (self.ribbon_end, "ribbon end"),
            (self.cover_open, "cover open"),
            (self.head_temp_high, "printhead temperature too high"),
            (self.label_not_installed, "label not installed"),
        ]
    }

    /// Check if any error flag is set.
    pub fn has_error(&self) -> bool {
        self.error_flags().iter().any(|(set, _)| *set)
    }

    /// Return a human-readable description of any errors.
    pub fn error_description(&self) -> Option<String> {
        let errors: Vec<&str> = self
            .error_flags()
            .into_iter()
            .filter_map(|(set, msg)| set.then_some(msg))
            .collect();
        if errors.is_empty() {
            None
        } else {
            Some(errors.join(", "))
        }
    }
}

/// Fallback label geometry (mm) used when the printer reports no material.
pub const DEFAULT_LABEL_HEIGHT_MM: u8 = 25;
pub const DEFAULT_LABEL_GAP_MM: u8 = 3;

/// Parsed material/consumable info from CMD_RETURN_MAT response.
#[derive(Debug, Clone, Default)]
pub struct MaterialInfo {
    pub uuid: String,
    pub code: String,
    pub sn: u16,
    pub label_type: u8,
    pub width_mm: u8,
    pub height_mm: u8,
    pub gap_mm: u8,
    pub remaining: Option<u32>,
    pub device_sn: Option<String>,
}

/// Parse printer status from CMD_INQUIRY_STA response.
pub fn parse_status(data: &[u8]) -> Option<PrinterStatus> {
    if data.len() < 20 {
        return None;
    }
    if data[0] != MAGIC1 || data[1] != MAGIC2 {
        return None;
    }
    if data[7] != CMD_INQUIRY_STA {
        return None;
    }

    Some(decode_status_bits(
        data[14],
        data[15],
        data[16],
        data[17],
        u16::from_le_bytes([data[18], data[19]]),
    ))
}

/// Decode the four status register bytes (MSTA low/high, FSTA low/high) plus
/// the print counter into a [`PrinterStatus`]. Shared by the BT framing
/// ([`parse_status`]) and the USB HID framing (`parse_usb_status`), which carry
/// the same bit layout at different offsets.
pub(crate) fn decode_status_bits(
    b0: u8,
    b1: u8,
    b2: u8,
    b3: u8,
    print_count: u16,
) -> PrinterStatus {
    PrinterStatus {
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
        print_count,
    }
}

/// Parse material info from CMD_RETURN_MAT response (BT framing).
///
/// The first 22 bytes are the BT response header; the material payload
/// itself starts at offset 22 and is decoded by [`parse_material_payload`].
///
/// BT material responses do **not** carry the printer's device serial —
/// the response ends at the last counter field, before the trailing
/// ASCII serial USB ships. Use the BlueZ `Device1.Name` property for
/// the BT-side serial instead.
pub fn parse_material(data: &[u8]) -> Option<MaterialInfo> {
    if !check_header(data, BT_RESP_HEADER_LEN, CMD_RETURN_MAT) {
        return None;
    }
    parse_material_payload(&data[BT_RESP_HEADER_LEN..], None)
}

/// Decode the per-transport material payload. `device_sn_ascii` is what
/// USB tacks on after the counter fields (16 ASCII bytes null-padded);
/// BT doesn't include it.
///
/// Common payload layout (verified by `crates/supvan-cli/examples/material_probe`
/// against a T50M Pro):
///
/// | Offset | Size | Field                                           |
/// |-------:|-----:|-------------------------------------------------|
/// |   0    | 7    | RFID tag UID (uuid)                             |
/// |   7    | 8    | RFID tag code/signature (code)                  |
/// |  15    | 2    | label SN counter, LE u16                        |
/// |  17    | 1    | label_type                                      |
/// |  18    | 1    | width_mm                                        |
/// |  19    | 1    | height_mm                                       |
/// |  20    | 1    | gap_mm                                          |
/// |  21    | 4    | labels remaining, LE u32                        |
/// |  25+   |      | additional vendor fields (not yet decoded)      |
pub fn parse_material_payload(p: &[u8], device_sn_ascii: Option<String>) -> Option<MaterialInfo> {
    if p.len() < 21 {
        return None;
    }
    let uuid = hex_upper(&p[0..7]);
    let code = hex_upper(&p[7..15]);
    let sn = u16::from_le_bytes([p[15], p[16]]);
    let label_type = p[17];
    let width_mm = p[18];
    let height_mm = p[19];
    let gap_mm = p[20];
    let remaining = if p.len() >= 25 {
        Some(u32::from_le_bytes([p[21], p[22], p[23], p[24]]))
    } else {
        None
    };
    Some(MaterialInfo {
        uuid,
        code,
        sn,
        label_type,
        width_mm,
        height_mm,
        gap_mm,
        remaining,
        device_sn: device_sn_ascii,
    })
}

/// Parse device name from CMD_RD_DEV_NAME response.
pub fn parse_device_name(data: &[u8]) -> Option<String> {
    if !check_header(data, BT_RESP_HEADER_LEN + 1, CMD_RD_DEV_NAME) {
        return None;
    }
    let payload_len = u16::from_le_bytes([data[2], data[3]]) as usize;
    // `payload_len` counts DEV_NAME_PAYLOAD_PREFIX framing bytes before the name.
    let data_len = payload_len.saturating_sub(DEV_NAME_PAYLOAD_PREFIX);
    if data_len == 0 || data_len > data.len() - BT_RESP_HEADER_LEN {
        return None;
    }
    let name = String::from_utf8_lossy(&data[BT_RESP_HEADER_LEN..BT_RESP_HEADER_LEN + data_len])
        .trim_end_matches('\0')
        .to_string();
    if name.is_empty() { None } else { Some(name) }
}

/// Parse firmware version from CMD_READ_FWVER response.
pub fn parse_firmware_version(data: &[u8]) -> Option<u8> {
    if !check_header(data, BT_RESP_HEADER_LEN + 1, CMD_READ_FWVER) {
        return None;
    }
    Some(data[BT_RESP_HEADER_LEN])
}

/// Parse protocol version from CMD_READ_REV response.
pub fn parse_version(data: &[u8]) -> Option<String> {
    if !check_header(data, BT_RESP_HEADER_LEN + 3, CMD_READ_REV) {
        return None;
    }
    let ver = String::from_utf8_lossy(&data[BT_RESP_HEADER_LEN..BT_RESP_HEADER_LEN + 3])
        .trim_end_matches('\0')
        .to_string();
    if ver.is_empty() { None } else { Some(ver) }
}

/// Check a response frame is at least `min_len` bytes, starts with the protocol
/// magic, and echoes `cmd` in the command slot. Shared guard for every BT-framed
/// parser; `min_len` makes each parser's length requirement explicit.
fn check_header(data: &[u8], min_len: usize, cmd: u8) -> bool {
    data.len() >= min_len && data[0] == MAGIC1 && data[1] == MAGIC2 && data[7] == cmd
}

/// Validate a response frame has correct magic and echoes the expected command.
pub fn validate_response(data: &[u8], expected_cmd: u8) -> bool {
    check_header(data, 8, expected_cmd)
}

fn hex_upper(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02X}", b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_status_response(b14: u8, b15: u8, b16: u8, b17: u8, count: u16) -> Vec<u8> {
        let mut resp = vec![0u8; 20];
        resp[0] = MAGIC1;
        resp[1] = MAGIC2;
        resp[7] = CMD_INQUIRY_STA;
        resp[14] = b14;
        resp[15] = b15;
        resp[16] = b16;
        resp[17] = b17;
        resp[18] = (count & 0xFF) as u8;
        resp[19] = (count >> 8) as u8;
        resp
    }

    #[test]
    fn test_parse_status_ready() {
        let resp = make_status_response(0, 0, 0x40, 0, 0);
        let status = parse_status(&resp).unwrap();
        assert!(!status.buf_full);
        assert!(!status.device_busy);
        assert!(status.printing);
        assert!(!status.has_error());
    }

    #[test]
    fn test_parse_status_errors() {
        let resp = make_status_response(0x02, 0, 0x08, 0x01, 5);
        let status = parse_status(&resp).unwrap();
        assert!(status.label_rw_error);
        assert!(status.cover_open);
        assert!(status.label_not_installed);
        assert!(status.has_error());
        assert_eq!(status.print_count, 5);
    }

    #[test]
    fn test_parse_status_too_short() {
        assert!(parse_status(&[0; 10]).is_none());
    }

    #[test]
    fn test_parse_status_wrong_magic() {
        let mut resp = make_status_response(0, 0, 0, 0, 0);
        resp[0] = 0x00;
        assert!(parse_status(&resp).is_none());
    }

    #[test]
    fn test_validate_response() {
        let mut resp = vec![0u8; 16];
        resp[0] = MAGIC1;
        resp[1] = MAGIC2;
        resp[7] = CMD_INQUIRY_STA;
        assert!(validate_response(&resp, CMD_INQUIRY_STA));
        assert!(!validate_response(&resp, 0x12));
        assert!(!validate_response(&[0; 4], CMD_INQUIRY_STA));
    }
}
