//! Synthetic label-material records for `CMD_SET_RFID_DATA` (0x5D).
//!
//! Genuine Supvan consumables carry an RFID tag the printer reads via
//! [`CMD_RETURN_MAT`](crate::cmd::CMD_RETURN_MAT). Third-party stock has no
//! tag, so that read comes back all zeros and the printer raises
//! `label_rw_error`. The vendor's own editor handles this by *synthesising* the
//! material record in software and pushing it to the printer over 0x5D — see
//! `MatCtrlFunc.sendRfid()`, which switches on an all-zero UUID:
//!
//! ```text
//! if (UUID.indexOf("00000000") == -1) { isRFIDLabel = false }  // real tag, leave alone
//! else                                { isRFIDLabel = true  }  // blank stock, send one
//! ```
//!
//! The record is what tells the firmware how hard to burn: [`heat_time_5`] and
//! [`heat_time_40`] are the printhead pulse widths at 5 °C and 40 °C ambient,
//! which the firmware interpolates between. They are the only *absolute* energy
//! control in the protocol — the 0–15 per-buffer density is a relative trim on
//! top. For energy-selected two-colour stock this is the knob that picks the
//! colour.
//!
//! Layout recovered from `t5080imageEncodeUtils.getT50PlusRFIDData()` in the
//! vendor Linux editor's source map. Note that the shipped editor leaves the
//! anti-counterfeit fields empty: `Cipertext` is `null`, and its `deepCopy()`
//! turns that into `{}`, so `Cipertext[i]` is `undefined` and lands in the
//! `Uint8Array` as zero. `TimeStamp` and `MatCode` are likewise empty strings.
//! The `Password` that `generalPassword()` computes is never serialised at all.
//! So the firmware accepts a record with all of those zeroed, and we do the
//! same rather than reimplementing dead crypto.

/// Serialised record length. The vendor allocates a fixed 80-byte buffer and
/// announces that length over 0x5D; trailing bytes past the last field are zero.
pub(crate) const RFID_RECORD_LEN: usize = 80;

/// Every populated field sits at this offset within the record; only the 7-byte
/// UUID precedes it. The vendor writes `databuf[N + offset]` throughout.
const FIELD_BASE: usize = 16;

/// Printer-imposed ceilings from `getT50PlusRFIDData()`.
pub const MAX_LABEL_WIDTH_MM: u8 = 50;
pub const MAX_LABEL_LENGTH_MM: u8 = 120;

/// Vendor heat-time presets, as `(heat_time_5, heat_time_40)` in printhead
/// pulse units. The editor picks between these by consumable catalogue code;
/// the spread across material families is what makes these useful starting
/// points for calibrating unknown stock.
pub mod heat_presets {
    /// Standard die-cut and continuous label stock — the catch-all default.
    pub const STANDARD: (u16, u16) = (1700, 1200);
    /// Index tabs, transparent black-mark, and black-mark-with-hole stock.
    pub const BLACK_MARK: (u16, u16) = (1800, 1300);
    /// Black-mark cardstock — the hottest profile the vendor ships.
    pub const CARDSTOCK: (u16, u16) = (2500, 2000);
}

/// A label-material record to hand the printer in place of a real RFID tag.
///
/// [`Default`] reproduces the vendor's own defaults for unbranded stock: a
/// `"30000"` catalogue code (the editor's placeholder for "not a catalogue
/// item"), 480 labels remaining, and the [`heat_presets::STANDARD`] profile.
#[derive(Debug, Clone)]
pub struct RfidMaterial {
    /// Catalogue code. Becomes both the label SN and the leading digits of the
    /// synthetic UUID. The vendor falls back to 30000 for anything non-numeric.
    pub code: u16,
    /// Material type — a [`PaperType`](crate::rfid::PaperType) discriminant.
    pub mat_type: u8,
    /// Label extent across the printhead, millimetres. Clamped to
    /// [`MAX_LABEL_WIDTH_MM`].
    pub width_mm: u8,
    /// Label extent along the feed direction, millimetres. Clamped to
    /// [`MAX_LABEL_LENGTH_MM`].
    pub length_mm: u8,
    /// Inter-label gap, millimetres.
    pub gap_mm: u8,
    /// Tail length past the last label, millimetres.
    pub tail_mm: u8,
    /// Brand/customer ID. The vendor uses 0 for Supvan, 1 for 贴博士.
    pub custom_id: u8,
    /// Labels remaining on the roll. The printer decrements this and reports it
    /// back through `RETURN_MAT`.
    pub remaining: u32,
    /// Printhead pulse width at 5 °C ambient.
    pub heat_time_5: u16,
    /// Printhead pulse width at 40 °C ambient.
    pub heat_time_40: u16,
    /// Gap-sensor thresholds. Zero lets the firmware use its own calibration.
    pub opt_threshold: (u16, u16),
    /// Gap-sensor profile index (newer models only).
    pub opt_index: u8,
    /// Heat-curve profile index (newer models only). The vendor ships 2.
    pub heat_index: u8,
}

impl Default for RfidMaterial {
    fn default() -> Self {
        let (heat_time_5, heat_time_40) = heat_presets::STANDARD;
        Self {
            code: 30000,
            mat_type: PaperType::DieCut as u8,
            width_mm: 40,
            length_mm: 30,
            gap_mm: 3,
            tail_mm: 0,
            custom_id: 1,
            remaining: 480,
            heat_time_5,
            heat_time_40,
            opt_threshold: (0, 0),
            opt_index: 0,
            heat_index: 2,
        }
    }
}

impl RfidMaterial {
    /// The synthetic UUID the record advertises: the decimal catalogue code,
    /// zero-padded on the right to 7 bytes. `RETURN_MAT` echoes this back, so
    /// it is how you confirm a record took.
    ///
    /// A code of 30000 yields `30000000000000`, matching the vendor's
    /// `code + uuid.substring(0, uuid.length - code.length)`.
    pub fn uuid_bytes(&self) -> [u8; 7] {
        // The vendor pads from a 14-char zero string, so an over-long code
        // truncates rather than overflowing the field. Each decimal digit is
        // read back as a hex nibble.
        let mut nibbles = self
            .code
            .to_string()
            .into_bytes()
            .into_iter()
            .map(|d| d - b'0')
            .chain(std::iter::repeat(0));
        std::array::from_fn(|_| {
            let hi = nibbles.next().unwrap_or(0);
            let lo = nibbles.next().unwrap_or(0);
            hi << 4 | lo
        })
    }

    /// Serialise to the wire record sent as the 0x5D bulk payload.
    pub(crate) fn encode(&self) -> [u8; RFID_RECORD_LEN] {
        let mut buf = [0u8; RFID_RECORD_LEN];
        buf[..7].copy_from_slice(&self.uuid_bytes());

        // MatCode at FIELD_BASE..FIELD_BASE+8 stays zero — the vendor never
        // populates it for synthesised records.
        let f = FIELD_BASE;
        buf[f + 8..f + 10].copy_from_slice(&self.code.to_le_bytes());
        buf[f + 10] = self.mat_type;
        buf[f + 11] = self.width_mm.min(MAX_LABEL_WIDTH_MM);
        buf[f + 12] = self.length_mm.min(MAX_LABEL_LENGTH_MM);
        buf[f + 13] = self.custom_id;
        buf[f + 14] = self.gap_mm;
        buf[f + 15] = self.tail_mm;
        buf[f + 16..f + 20].copy_from_slice(&self.remaining.to_le_bytes());
        buf[f + 20..f + 22].copy_from_slice(&self.heat_time_5.to_le_bytes());
        buf[f + 22..f + 24].copy_from_slice(&self.heat_time_40.to_le_bytes());
        // f+24..f+30 TimeStamp, f+30..f+32 padding, f+32..f+48 Cipertext: all
        // zero, as the vendor ships them.
        buf[f + 48..f + 50].copy_from_slice(&self.opt_threshold.0.to_le_bytes());
        buf[f + 50..f + 52].copy_from_slice(&self.opt_threshold.1.to_le_bytes());
        buf[f + 52] = self.opt_index;
        buf[f + 53] = self.heat_index;
        buf
    }
}

/// Material types the firmware understands, from the vendor's `PaperTypeEnum`.
///
/// There is deliberately no two-colour variant: the firmware has no notion of
/// colour, only burn energy. Two-colour stock is driven by heat time and
/// density, not by picking a type here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum PaperType {
    Continuous = 0,
    DieCut = 1,
    MarkerCard = 3,
    Flag = 4,
    Plate = 5,
    Tube = 6,
    ShrinkTube = 7,
    MarkerTap = 8,
    ContinuousHole = 9,
    ReflectLight = 10,
    LaminatedAluminum = 11,
    BlackMark = 12,
    BlackCard = 13,
    BlackSticker = 14,
    LaminatedWrapContinuous = 21,
    LaminatedWrapDieCut = 22,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_uuid_matches_vendor_padding() {
        let m = RfidMaterial::default();
        assert_eq!(m.uuid_bytes(), [0x30, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn short_code_pads_right() {
        let m = RfidMaterial {
            code: 5,
            ..Default::default()
        };
        // "5" padded to "50000000000000".
        assert_eq!(m.uuid_bytes(), [0x50, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn heat_times_land_at_vendor_offsets() {
        let m = RfidMaterial {
            heat_time_5: 0x1234,
            heat_time_40: 0x5678,
            ..Default::default()
        };
        let buf = m.encode();
        // databuf[20 + offset] .. databuf[23 + offset], little-endian.
        assert_eq!(&buf[36..38], &[0x34, 0x12]);
        assert_eq!(&buf[38..40], &[0x78, 0x56]);
    }

    #[test]
    fn geometry_is_clamped() {
        let m = RfidMaterial {
            width_mm: 200,
            length_mm: 200,
            ..Default::default()
        };
        let buf = m.encode();
        assert_eq!(buf[FIELD_BASE + 11], MAX_LABEL_WIDTH_MM);
        assert_eq!(buf[FIELD_BASE + 12], MAX_LABEL_LENGTH_MM);
    }

    #[test]
    fn anti_counterfeit_fields_stay_zero() {
        let buf = RfidMaterial::default().encode();
        // TimeStamp + padding + Cipertext, as the vendor ships them.
        assert!(
            buf[FIELD_BASE + 24..FIELD_BASE + 48]
                .iter()
                .all(|&b| b == 0)
        );
    }

    #[test]
    fn record_is_fixed_length() {
        assert_eq!(RfidMaterial::default().encode().len(), RFID_RECORD_LEN);
    }
}
