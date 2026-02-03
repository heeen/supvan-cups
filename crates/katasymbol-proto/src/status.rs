use crate::cmd::{
    CMD_INQUIRY_STA, CMD_RD_DEV_NAME, CMD_READ_FWVER, CMD_READ_REV, CMD_RETURN_MAT, MAGIC1, MAGIC2,
};

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
    /// Check if any error flag is set.
    pub fn has_error(&self) -> bool {
        self.label_rw_error
            || self.label_end
            || self.label_mode_error
            || self.ribbon_rw_error
            || self.ribbon_end
            || self.cover_open
            || self.head_temp_high
            || self.label_not_installed
    }

    /// Return a human-readable description of any errors.
    pub fn error_description(&self) -> Option<String> {
        let mut errors = Vec::new();
        if self.label_rw_error {
            errors.push("label read/write error");
        }
        if self.label_end {
            errors.push("label roll end");
        }
        if self.label_mode_error {
            errors.push("label mode mismatch");
        }
        if self.ribbon_rw_error {
            errors.push("ribbon read/write error");
        }
        if self.ribbon_end {
            errors.push("ribbon end");
        }
        if self.cover_open {
            errors.push("cover open");
        }
        if self.head_temp_high {
            errors.push("printhead temperature too high");
        }
        if self.label_not_installed {
            errors.push("label not installed");
        }
        if errors.is_empty() {
            None
        } else {
            Some(errors.join(", "))
        }
    }
}

/// Parsed material/consumable info from CMD_RETURN_MAT response.
#[derive(Debug, Clone)]
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

    let b0 = data[14];
    let b1 = data[15];
    let b2 = data[16];
    let b3 = data[17];

    Some(PrinterStatus {
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
        print_count: u16::from_le_bytes([data[18], data[19]]),
    })
}

/// Parse material info from CMD_RETURN_MAT response.
pub fn parse_material(data: &[u8]) -> Option<MaterialInfo> {
    if data.len() < 43 {
        return None;
    }
    if data[0] != MAGIC1 || data[1] != MAGIC2 {
        return None;
    }
    if data[7] != CMD_RETURN_MAT {
        return None;
    }

    let uuid = hex_upper(&data[22..29]);
    let code = hex_upper(&data[29..37]);
    // SN: big-endian from bytes 37-38
    let sn = ((data[38] as u16) << 8) | (data[37] as u16);
    let label_type = data[39];
    let width_mm = data[40];
    let height_mm = data[41];
    let gap_mm = data[42];

    let remaining = if data.len() >= 47 {
        Some(u32::from_le_bytes([data[43], data[44], data[45], data[46]]))
    } else {
        None
    };

    let device_sn = if data.len() >= 57 {
        let sn_str: String = (0..6).map(|i| format!("{:02}", data[51 + i])).collect();
        Some(sn_str)
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
        device_sn,
    })
}

/// Parse device name from CMD_RD_DEV_NAME response.
pub fn parse_device_name(data: &[u8]) -> Option<String> {
    if data.len() <= 22 || data[0] != MAGIC1 || data[1] != MAGIC2 {
        return None;
    }
    if data[7] != CMD_RD_DEV_NAME {
        return None;
    }
    let payload_len = u16::from_le_bytes([data[2], data[3]]) as usize;
    let data_len = payload_len.saturating_sub(18);
    if data_len == 0 || data_len > data.len() - 22 {
        return None;
    }
    let name = String::from_utf8_lossy(&data[22..22 + data_len])
        .trim_end_matches('\0')
        .to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

/// Parse firmware version from CMD_READ_FWVER response.
pub fn parse_firmware_version(data: &[u8]) -> Option<u8> {
    if data.len() <= 22 || data[0] != MAGIC1 || data[1] != MAGIC2 {
        return None;
    }
    if data[7] != CMD_READ_FWVER {
        return None;
    }
    Some(data[22])
}

/// Parse protocol version from CMD_READ_REV response.
pub fn parse_version(data: &[u8]) -> Option<String> {
    if data.len() <= 24 || data[0] != MAGIC1 || data[1] != MAGIC2 {
        return None;
    }
    if data[7] != CMD_READ_REV {
        return None;
    }
    let ver = String::from_utf8_lossy(&data[22..25])
        .trim_end_matches('\0')
        .to_string();
    if ver.is_empty() {
        None
    } else {
        Some(ver)
    }
}

/// Validate a response frame has correct magic and echoes the expected command.
pub fn validate_response(data: &[u8], expected_cmd: u8) -> bool {
    data.len() >= 8 && data[0] == MAGIC1 && data[1] == MAGIC2 && data[7] == expected_cmd
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
