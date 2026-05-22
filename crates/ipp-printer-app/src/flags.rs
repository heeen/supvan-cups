//! IPP `printer-state-reasons` bit flags (PWG values).

use bitflags::bitflags;

pub type PrinterReasonRaw = u32;

bitflags! {
    /// Printer state reason flags (`printer-state-reasons`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct PrinterReason: PrinterReasonRaw {
        const NONE = 0x0000;
        const OTHER = 0x0001;
        const COVER_OPEN = 0x0002;
        const INPUT_TRAY_MISSING = 0x0004;
        const MARKER_SUPPLY_EMPTY = 0x0008;
        const MARKER_SUPPLY_LOW = 0x0010;
        const MARKER_WASTE_ALMOST_FULL = 0x0020;
        const MARKER_WASTE_FULL = 0x0040;
        const MEDIA_EMPTY = 0x0080;
        const MEDIA_JAM = 0x0100;
        const MEDIA_LOW = 0x0200;
        const MEDIA_NEEDED = 0x0400;
        const OFFLINE = 0x0800;
        const SPOOL_AREA_FULL = 0x1000;
        const TONER_EMPTY = 0x2000;
        const TONER_LOW = 0x4000;
        const DOOR_OPEN = 0x8000;
        const IDENTIFY_PRINTER_REQUESTED = 0x10000;
    }
}

impl PrinterReason {
    /// IPP keyword tokens for this flag set (CUPS / PWG).
    pub fn ipp_keywords(&self) -> Vec<&'static str> {
        let mut out = Vec::new();
        if self.is_empty() {
            out.push("none");
            return out;
        }
        if self.contains(Self::OTHER) {
            out.push("other");
        }
        if self.contains(Self::COVER_OPEN) {
            out.push("cover-open");
        }
        if self.contains(Self::MEDIA_EMPTY) {
            out.push("media-empty");
        }
        if self.contains(Self::MEDIA_JAM) {
            out.push("media-jam");
        }
        if self.contains(Self::MEDIA_LOW) {
            out.push("media-low");
        }
        if self.contains(Self::OFFLINE) {
            out.push("offline-report");
        }
        if self.contains(Self::MARKER_SUPPLY_LOW) {
            out.push("marker-supply-low");
        }
        if self.contains(Self::MARKER_SUPPLY_EMPTY) {
            out.push("marker-supply-empty");
        }
        if out.is_empty() {
            out.push("none");
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_set_is_none() {
        assert_eq!(PrinterReason::empty().ipp_keywords(), vec!["none"]);
    }

    #[test]
    fn single_flag_surfaces() {
        assert_eq!(PrinterReason::COVER_OPEN.ipp_keywords(), vec!["cover-open"]);
    }

    #[test]
    fn multi_flag_surfaces_all() {
        let r = PrinterReason::COVER_OPEN | PrinterReason::MEDIA_EMPTY;
        let kws = r.ipp_keywords();
        assert!(kws.contains(&"cover-open"));
        assert!(kws.contains(&"media-empty"));
        assert!(!kws.contains(&"none"));
    }
}
