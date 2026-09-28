//! Per-model print parameters.
//!
//! The wire protocol is not uniform across the range: the BT-only E-series
//! label makers use a smaller print buffer, different `PAGE_REG` constants and
//! a per-buffer transfer handshake. Every E-series value here was recovered by
//! decoding a Bluetooth HCI snoop of the vendor Android app printing to an
//! `E10pro` (see `docs/E10-PROTOCOL.md`) and applies to the whole series;
//! [`PrintProfile::TSeries`] reproduces exactly what this driver did before, so
//! the T50/T80/G/TP path is unchanged.

/// Which model family's print flow to speak.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PrintProfile {
    /// T50/T80/G/TP/SP-series: one combined LZMA stream, 4096-byte buffers.
    ///
    /// Named after its reference family (`docs/PROTOCOL.md` is the T-series
    /// protocol), and covers every non-E model. Split a model out into its
    /// own variant when a capture shows it diverging.
    #[default]
    TSeries,
    /// E-series BT label makers (E10/E10pro/E11/E12/E16). Every constant was
    /// verified against an E10pro (protocol rev 1.4); the rest of the series
    /// is the same BT-only class and is routed here too.
    ESeries,
}

/// The constants that differ between profiles, laid out side by side so a new
/// model is one `const` block rather than a new arm in a dozen `match`es.
///
/// Reached through [`PrintProfile::params`]; the fields are read directly
/// rather than through accessors, so adding a parameter costs one line here
/// and one in each `const` block below.
pub struct ProfileParams {
    /// Print-buffer size. The firmware reads a fixed-size buffer, so this also
    /// bounds how many columns fit in one transfer.
    pub buf_size: usize,
    /// Image bytes the buffer may carry. The T-series' historical cap is 8 bytes
    /// short of `buf_size - PRINT_BUF_HEADER`; the E-series fills the buffer.
    pub max_buf_data: usize,
    /// Blank columns declared before and after the image.
    pub margin_dots: u16,
    /// `PAGE_REG` material selector (bits 6-7 of byte 1).
    pub mat: u8,
    /// `PAGE_REG` `nodu` field (bits 2-5), when the profile pins it to a
    /// constant. `None` means it mirrors the density byte, which is what the
    /// T-series does; the E-series holds it at 4 and carries density only in
    /// `buf[12]`. Callers resolve it with `nodu.unwrap_or(density)`.
    pub nodu: Option<u8>,
    /// Upper bound for the density byte.
    pub max_density: u8,
    /// Whether density is sent only on the first buffer of a page (the
    /// E-series zeroes it on continuation buffers).
    pub density_on_first_buffer_only: bool,
    /// Whether each print buffer is compressed and transferred on its own,
    /// rather than concatenating the page into one LZMA stream.
    pub per_buffer_transfer: bool,
    /// Whether `BUF_FULL` reports the compressed length and speed. The
    /// E-series sends `(0, 0)`.
    pub buf_full_reports_length: bool,
    /// Vendor-specific command issued immediately before `START_PRINT`.
    pub pre_start_cmd: Option<(u8, u16)>,
    /// Vendor-specific command issued after `START_PRINT`, before any data.
    pub post_start_cmd: Option<(u8, u16)>,
    /// Head width of the profile's reference model, in dots.
    ///
    /// Only a default: `TSeries` spans several head widths (T50 384, T80 640,
    /// TP80 960), so `supvan-printer-app` always takes the real width from the
    /// family's `printhead_dots` in `data/models.toml`. This exists for
    /// callers with no registry — [`Printer::test_print`](crate::printer::Printer::test_print)
    /// and `supvan-cli`.
    pub default_printhead_dots: u32,
    /// Whether the `ribbon_end` status bit should abort a job.
    ///
    /// The E10pro asserts MSTA-low `0x20` during prints the vendor app
    /// completes successfully, so treating it as fatal there wedges every job.
    pub ribbon_end_is_fatal: bool,
}

const T_SERIES: ProfileParams = ProfileParams {
    buf_size: crate::buffer::PRINT_BUF_SIZE,
    max_buf_data: crate::buffer::MAX_BUF_DATA,
    margin_dots: 8,
    mat: 1,
    nodu: None,
    max_density: 15,
    density_on_first_buffer_only: false,
    per_buffer_transfer: false,
    buf_full_reports_length: true,
    pre_start_cmd: None,
    post_start_cmd: None,
    default_printhead_dots: crate::bitmap::PRINTHEAD_WIDTH_DOTS,
    ribbon_end_is_fatal: true,
};

const E_SERIES: ProfileParams = ProfileParams {
    buf_size: 4000,
    max_buf_data: 4000 - crate::buffer::PRINT_BUF_HEADER,
    margin_dots: 1,
    mat: 0,
    nodu: Some(4),
    max_density: 19,
    density_on_first_buffer_only: true,
    per_buffer_transfer: true,
    buf_full_reports_length: false,
    // Meaning unknown; the captured parameters did not vary.
    pre_start_cmd: Some((0xC9, 110)),
    post_start_cmd: Some((0xBA, 29)),
    // 96 dots = 12 mm of print on 15 mm tape, not the 384-dot/48 mm head the
    // T50 family uses. Measured on an E10pro; a sibling on wider tape may have
    // more, but under-declaring only narrows the printable band, whereas
    // over-declaring prints nothing at all — so this is the safe default for
    // the whole series until another model is captured.
    default_printhead_dots: 96,
    // The E10pro asserts MSTA-low `0x20` during prints the vendor app
    // completes successfully, so treating it as fatal wedges every job.
    ribbon_end_is_fatal: false,
};

impl PrintProfile {
    /// The wire constants for this profile. Read the fields directly:
    /// `profile.params().buf_size`.
    pub const fn params(self) -> &'static ProfileParams {
        match self {
            Self::TSeries => &T_SERIES,
            Self::ESeries => &E_SERIES,
        }
    }

    /// Pick a profile from the firmware's self-reported model name
    /// (`RD_DEV_NAME`, e.g. `E10pro`).
    ///
    /// Matches the whole `E1x` series — `E10`, `E10pro`, `E11`, `E12`, `E16` —
    /// since they are one BT-only class speaking one print flow.
    ///
    /// This is the fallback for callers with no model registry — chiefly
    /// `supvan-cli`. `supvan-printer-app` resolves the profile from the
    /// driver family in `data/models.toml`, which is authoritative and can be
    /// changed without a rebuild.
    pub fn from_device_name(name: &str) -> Self {
        let b = name.trim().to_ascii_lowercase().into_bytes();
        // `E` then two digits, the first of which is `1`: tight enough that a
        // T-series name or an empty read can't fall onto the E-series flow,
        // which would print blank.
        if matches!(b.as_slice(), [b'e', b'1', d, ..] if d.is_ascii_digit()) {
            Self::ESeries
        } else {
            Self::TSeries
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t_series_is_the_default_and_keeps_its_historical_constants() {
        // Anything that changes here changes what a T50 receives.
        let p = PrintProfile::default();
        assert_eq!(p, PrintProfile::TSeries);
        assert_eq!(p.params().buf_size, 4096);
        assert_eq!(p.params().max_buf_data, 4074);
        assert_eq!(p.params().margin_dots, 8);
        assert_eq!(p.params().mat, 1);
        assert_eq!(p.params().nodu, None, "T-series nodu mirrors density");
        assert!(p.params().buf_full_reports_length);
        assert!(p.params().pre_start_cmd.is_none() && p.params().post_start_cmd.is_none());
        assert!(p.params().ribbon_end_is_fatal);
    }

    #[test]
    fn e_series_holds_nodu_constant_and_zeroes_buf_full() {
        let p = PrintProfile::ESeries;
        assert_eq!(p.params().buf_size, 4000);
        assert_eq!(p.params().max_buf_data, 3986);
        assert_eq!(
            p.params().nodu,
            Some(4),
            "nodu is pinned regardless of density"
        );
        assert!(!p.params().buf_full_reports_length);
        assert!(!p.params().ribbon_end_is_fatal);
    }

    #[test]
    fn device_name_detection_covers_the_whole_series() {
        for name in ["E10pro", " e10 ", "E11", "e12", "E16"] {
            assert_eq!(
                PrintProfile::from_device_name(name),
                PrintProfile::ESeries,
                "{name} is an E-series label maker"
            );
        }
        assert_eq!(
            PrintProfile::from_device_name("T50M Pro"),
            PrintProfile::TSeries
        );
        // An empty or unreadable name must not silently pick the E-series
        // flow for a T50, and neither must a bare `E` or a non-E1x model.
        for name in ["", "E", "E1", "E20", "G15Mini"] {
            assert_eq!(
                PrintProfile::from_device_name(name),
                PrintProfile::TSeries,
                "{name:?} must not be routed to the E-series flow"
            );
        }
    }
}
