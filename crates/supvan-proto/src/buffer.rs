use crate::profile::PrintProfile;

/// Max image data bytes per print buffer (from Android R2.drawable.sf5334_).
pub const MAX_BUF_DATA: usize = 4074;

/// Print buffer size.
pub const PRINT_BUF_SIZE: usize = 4096;

/// Header size in print buffer.
pub const PRINT_BUF_HEADER: usize = 14;

/// Margin clamp range (dots) for the print-buffer header.
const MARGIN_MAX_DOTS: u16 = 900;

/// Maximum density / red-deepness value encoded in the buffer header.
const MAX_DENSITY: u8 = 15;

/// The firmware re-reads the running checksum at every Nth byte; the builder
/// folds in the byte just before each boundary.
const CHECKSUM_STRIDE: usize = 256;

/// Parameters for PAGE_REG_BITS construction.
#[derive(Debug, Clone, Default)]
pub struct PageRegBits {
    pub page_st: bool,
    pub page_end: bool,
    pub prt_end: bool,
    pub cut: u8,
    pub savepaper: bool,
    pub first_cut: u8,
    pub nodu: u8,
    pub mat: u8,
}

/// Build PAGE_REG_BITS (2 bytes) for a print buffer header.
///
/// Byte 0:
///   bit 1: PageSt (first buffer of page)
///   bit 2: PageEnd (last buffer of page)
///   bit 3: PrtEnd (end of print job)
///   bits 4-6: Cut mode (3 bits)
///   bit 7: Savepaper
///
/// Byte 1:
///   bits 0-1: FirstCut
///   bits 2-5: Nodu (density, 0-15)
///   bits 6-7: Mat (material type)
pub fn build_page_reg_bits(p: &PageRegBits) -> [u8; 2] {
    let mut b0: u8 = 0;
    if p.page_st {
        b0 |= 0x02;
    }
    if p.page_end {
        b0 |= 0x04;
    }
    if p.prt_end {
        b0 |= 0x08;
    }
    b0 &= 0x0F;
    b0 |= (p.cut & 0x07) << 4;
    if p.savepaper {
        b0 |= 0x80;
    }

    let mut b1: u8 = 0;
    b1 |= p.first_cut & 0x03;
    b1 |= (p.nodu & 0x0F) << 2;
    b1 |= (p.mat & 0x03) << 6;

    [b0, b1]
}

/// The two independent burn-energy trims a print buffer carries.
///
/// The Android app keeps these as `mDeepness` / `mRedDeepness` and packs them
/// into one int for transport as `(black << 8) | red`, unpacking when the value
/// exceeds 255 (`T50PlusPrint.java:107-112`). They land in different places in
/// the buffer header: black in the PAGE_REG_BITS `nodu` field, red in `buf[12]`.
/// Its print-setup dialog exposes both as separate spinners, so they are meant
/// to be driven independently — setting them equal is a special case, not the
/// rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Density {
    pub black: u8,
    pub red: u8,
}

impl Density {
    /// Both trims at the same value — the vendor's default shape (4/4).
    pub fn uniform(value: u8) -> Self {
        Self {
            black: value,
            red: value,
        }
    }
}

impl Default for Density {
    fn default() -> Self {
        Self::uniform(4)
    }
}

impl std::fmt::Display for Density {
    /// Round-trips the CLI's `N` / `BLACK:RED` forms, so logged sweep settings
    /// can be pasted straight back into a command.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.black == self.red {
            write!(f, "{}", self.black)
        } else {
            write!(f, "{}:{}", self.black, self.red)
        }
    }
}

/// How many bitplanes each printed column carries.
///
/// [`TwoColour`](ColourMode::TwoColour) doubles the data: every column ships a
/// red line followed by a black line (see [`crate::twocolor`]). The firmware is
/// told via the `first_cut` field and a doubled column count, so `cols_in_buf`
/// must already be the doubled figure.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColourMode {
    #[default]
    Mono,
    TwoColour,
}

impl ColourMode {
    /// The `first_cut` value that selects this mode. The vendor sets 2 for
    /// two-colour (`i2 == 3` branch in `T50PlusPrint`); mono leaves it 0.
    fn first_cut(self) -> u8 {
        match self {
            Self::Mono => 0,
            Self::TwoColour => 2,
        }
    }

    /// Bitplanes per printed column.
    pub fn planes(self) -> u16 {
        match self {
            Self::Mono => 1,
            Self::TwoColour => 2,
        }
    }
}

/// Page-level flags shared by every buffer of a page.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PageOptions {
    pub colour: ColourMode,
    /// PAGE_REG_BITS byte 0 bit 7 — 省纸, "save paper".
    ///
    /// **Neither vendor tool ever sets this**; the Android app plumbs
    /// `mSavePaper` through to it but the Linux editor leaves it 0 throughout.
    /// The hypothesis worth testing is that it suppresses the advance to the
    /// tear-off position, which would make a second pass over the same label
    /// possible — and with it, two colours side by side on hardware that
    /// ignores the two-colour mode. Unverified: treat output as suspect until
    /// a label says otherwise.
    pub save_paper: bool,
}

/// Parameters for building a print buffer.
pub struct PrintBufferParams<'a> {
    pub image_data: &'a [u8],
    pub per_line_byte: u8,
    /// Lines in this buffer — already doubled when `colour` is two-colour.
    pub cols_in_buf: u16,
    pub page_st: bool,
    pub page_end: bool,
    pub prt_end: bool,
    pub margin_top: u16,
    pub margin_bottom: u16,
    pub density: Density,
    pub page: PageOptions,
}

/// Build a 4096-byte print buffer.
///
/// Layout:
///   [0..1]   Checksum (LE)
///   [2..3]   PAGE_REG_BITS (black density in the `nodu` field)
///   [4..5]   Column count (LE)
///   [6]      Bytes per line
///   [7]      Reserved (0)
///   [8..9]   Margin top (LE, 1-900 dots)
///   [10..11] Margin bottom (LE, 1-900 dots)
///   [12]     Red deepness (0-15)
///   [13]     0
///   [14..]   Image data
pub fn build_print_buffer(p: &PrintBufferParams) -> [u8; PRINT_BUF_SIZE] {
    let mut buf = [0u8; PRINT_BUF_SIZE];

    // PAGE_REG_BITS
    let page_bits = build_page_reg_bits(&PageRegBits {
        page_st: p.page_st,
        page_end: p.page_end,
        prt_end: p.prt_end,
        nodu: p.density.black,
        mat: 1,
        first_cut: p.page.colour.first_cut(),
        savepaper: p.page.save_paper,
        ..Default::default()
    });
    buf[2] = page_bits[0];
    buf[3] = page_bits[1];

    // Column count
    buf[4..6].copy_from_slice(&p.cols_in_buf.to_le_bytes());

    // Bytes per line
    buf[6] = p.per_line_byte;

    // Margins (clamped 1..=MARGIN_MAX_DOTS)
    let mt = p.margin_top.clamp(1, MARGIN_MAX_DOTS);
    let mb = p.margin_bottom.clamp(1, MARGIN_MAX_DOTS);
    buf[8..10].copy_from_slice(&mt.to_le_bytes());
    buf[10..12].copy_from_slice(&mb.to_le_bytes());

    // Red deepness — the black trim rides in PAGE_REG_BITS above, not here.
    buf[12] = p.density.red.min(MAX_DENSITY);

    // Image data at offset 14
    let data_len = p.image_data.len().min(PRINT_BUF_SIZE - PRINT_BUF_HEADER);
    buf[PRINT_BUF_HEADER..PRINT_BUF_HEADER + data_len].copy_from_slice(&p.image_data[..data_len]);

    // Checksum: sum(buf[2..14]) + sum of bytes at each 256-byte boundary
    let data_end = (p.cols_in_buf as usize) * (p.per_line_byte as usize) + PRINT_BUF_HEADER;
    let mut chk: u32 = buf[2..14].iter().map(|&b| b as u32).sum();
    let n_strides = data_end / CHECKSUM_STRIDE;
    for i in 1..=n_strides {
        let idx = i * CHECKSUM_STRIDE - 1;
        if idx < buf.len() {
            chk += buf[idx] as u32;
        }
    }
    buf[0..2].copy_from_slice(&(chk as u16).to_le_bytes());

    buf
}

/// A run of feed-direction columns printed at its own density.
///
/// Density is a per-buffer field, and buffers tile the label along the feed
/// axis, so energy can vary from one stripe of the label to the next. Within a
/// printhead line it cannot — except by colour: see [`ColourMode::TwoColour`],
/// which splits each line into two independently-trimmed planes.
#[derive(Debug, Clone, Copy)]
pub struct DensityBand {
    pub cols: u16,
    pub density: Density,
}

/// Split column-major image data into print buffers, one density throughout.
///
/// Returns a Vec of 4096-byte print buffers ready for LZMA compression.
pub fn split_into_buffers(
    image_data: &[u8],
    per_line_byte: u8,
    total_cols: u16,
    margin_top: u16,
    margin_bottom: u16,
    density: Density,
    page: PageOptions,
) -> Vec<[u8; PRINT_BUF_SIZE]> {
    let cols = total_cols - margin_top - margin_bottom;
    split_into_banded_buffers(
        image_data,
        per_line_byte,
        &[DensityBand { cols, density }],
        margin_top,
        margin_bottom,
        page,
    )
}

/// Split column-major image data into print buffers, giving each band its own
/// density. Bands are laid down the feed direction in order; a band larger than
/// one buffer's capacity is split across several, all keeping its density.
///
/// `cols` throughout is counted in *printed* columns (printhead lines). Under
/// [`ColourMode::TwoColour`] each of those costs two `per_line_byte` planes, so
/// buffer capacity halves and the header advertises twice the count.
pub fn split_into_banded_buffers(
    image_data: &[u8],
    per_line_byte: u8,
    bands: &[DensityBand],
    margin_top: u16,
    margin_bottom: u16,
    page: PageOptions,
) -> Vec<[u8; PRINT_BUF_SIZE]> {
    let planes = page.colour.planes();
    let col_stride = per_line_byte as usize * planes as usize;
    let max_cols = (MAX_BUF_DATA / col_stride) as u16;

    // Resolve the full chunk list up front: page_end/prt_end must be set on the
    // final buffer, which isn't known until every band has been tiled.
    let mut chunks: Vec<(u16, u16, Density)> = Vec::new();
    let mut current_col: u16 = 0;
    for band in bands {
        let mut cols_remaining = band.cols;
        while cols_remaining > 0 {
            let cols_in_buf = cols_remaining.min(max_cols);
            chunks.push((current_col, cols_in_buf, band.density));
            current_col += cols_in_buf;
            cols_remaining -= cols_in_buf;
        }
    }

    log::debug!(
        "split: {} cols x {per_line_byte}B/line, margins {margin_top}/{margin_bottom}, \
         {} planes -> {} buffers (max {max_cols} cols each)",
        bands.iter().map(|b| b.cols as u32).sum::<u32>(),
        planes,
        chunks.len(),
    );

    let last = chunks.len().saturating_sub(1);
    chunks
        .iter()
        .enumerate()
        .map(|(i, &(start_col, cols_in_buf, density))| {
            let img_start = (margin_top + start_col) as usize * col_stride;
            let img_end = img_start + cols_in_buf as usize * col_stride;
            let img_chunk = image_data
                .get(img_start..img_end.min(image_data.len()))
                .unwrap_or(&[]);

            log::trace!(
                "  buffer {i}: cols {start_col}..{} ({cols_in_buf}), page_st={} page_end={} \
                 prt_end={}",
                start_col as u32 + cols_in_buf as u32,
                i == 0,
                i == last,
                i == last,
            );
            build_print_buffer(&PrintBufferParams {
                image_data: img_chunk,
                per_line_byte,
                cols_in_buf: cols_in_buf * planes,
                page_st: i == 0,
                page_end: i == last,
                prt_end: i == last,
                margin_top,
                margin_bottom,
                density,
                page,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_page_reg_bits_defaults() {
        let bits = build_page_reg_bits(&PageRegBits {
            nodu: 4,
            mat: 1,
            ..Default::default()
        });
        // b0: no flags, cut=0, savepaper=0 -> 0x00
        assert_eq!(bits[0], 0x00);
        // b1: first_cut=0, nodu=4 (<<2 = 0x10), mat=1 (<<6 = 0x40) -> 0x50
        assert_eq!(bits[1], 0x50);
    }

    #[test]
    fn test_build_page_reg_bits_first_last() {
        let bits = build_page_reg_bits(&PageRegBits {
            page_st: true,
            page_end: true,
            prt_end: true,
            nodu: 4,
            mat: 1,
            ..Default::default()
        });
        // b0: PageSt=0x02, PageEnd=0x04, PrtEnd=0x08 = 0x0E
        assert_eq!(bits[0], 0x0E);
        assert_eq!(bits[1], 0x50);
    }

    #[test]
    fn test_build_print_buffer_checksum() {
        let data = vec![0u8; 84 * 48]; // 84 cols * 48 bytes/line
        let buf = build_print_buffer(&PrintBufferParams {
            image_data: &data,
            per_line_byte: 48,
            cols_in_buf: 84,
            page_st: true,
            page_end: true,
            prt_end: true,
            margin_top: 8,
            margin_bottom: 8,
            density: Density::uniform(4),
            page: PageOptions::default(),
        });
        // Verify buffer structure
        assert_eq!(buf[6], 48); // bytes per line
        assert_eq!(buf[4], 84); // cols low
        assert_eq!(buf[5], 0); // cols high
        assert_eq!(buf[8], 8); // margin top
        assert_eq!(buf[12], 4); // density
        // Checksum should be non-zero (at least header bytes contribute)
        let chk = buf[0] as u16 | ((buf[1] as u16) << 8);
        assert!(chk > 0);
    }

    #[test]
    fn test_split_into_buffers() {
        // 48 bytes/line, total 240 cols, margins 8+8 = 224 image cols
        // max_cols = 4074/48 = 84
        // 224 / 84 = 2 full + 56 remainder = 3 buffers
        let per_line_byte = 48u8;
        let total_cols = 240u16;
        let image_data = vec![0u8; total_cols as usize * per_line_byte as usize];
        let bufs = split_into_buffers(
            &image_data,
            per_line_byte,
            total_cols,
            8,
            8,
            Density::uniform(4),
            PageOptions::default(),
        );
        assert_eq!(bufs.len(), 3);
    }

    /// Each band's density must land in its own buffer header, and only the
    /// first/last buffer of the whole strip carry the page flags.
    #[test]
    fn banded_split_keeps_per_band_density() {
        let per_line_byte = 48u8;
        let image_data = vec![0u8; 240 * per_line_byte as usize];
        let bands = [
            DensityBand {
                cols: 40,
                density: Density::uniform(2),
            },
            DensityBand {
                cols: 40,
                density: Density::uniform(9),
            },
            DensityBand {
                cols: 40,
                density: Density::uniform(15),
            },
        ];
        let bufs = split_into_banded_buffers(
            &image_data,
            per_line_byte,
            &bands,
            8,
            8,
            PageOptions::default(),
        );

        assert_eq!(bufs.len(), 3);
        assert_eq!([bufs[0][12], bufs[1][12], bufs[2][12]], [2, 9, 15]);
        // PageSt (0x02) on the first only; PageEnd|PrtEnd (0x0C) on the last only.
        assert_eq!(bufs[0][2] & 0x02, 0x02);
        assert_eq!(bufs[1][2] & 0x0E, 0);
        assert_eq!(bufs[2][2] & 0x0C, 0x0C);
    }

    /// A band wider than one buffer splits, and every piece keeps its density.
    #[test]
    fn banded_split_subdivides_oversized_band() {
        let per_line_byte = 48u8; // max_cols = 4074/48 = 84
        let image_data = vec![0u8; 400 * per_line_byte as usize];
        let bands = [DensityBand {
            cols: 200,
            density: Density::uniform(7),
        }];
        let bufs = split_into_banded_buffers(
            &image_data,
            per_line_byte,
            &bands,
            8,
            8,
            PageOptions::default(),
        );

        assert_eq!(bufs.len(), 3); // 84 + 84 + 32
        assert!(bufs.iter().all(|b| b[12] == 7));
        assert_eq!(u16::from_le_bytes([bufs[2][4], bufs[2][5]]), 32);
    }

    /// Black and red are separate knobs in separate header fields: black in the
    /// PAGE_REG_BITS `nodu` bits, red in `buf[12]`. Driving them in lockstep was
    /// the bug this type exists to prevent.
    #[test]
    fn black_and_red_land_in_different_fields() {
        let data = vec![0u8; 84 * 48];
        let buf = build_print_buffer(&PrintBufferParams {
            image_data: &data,
            per_line_byte: 48,
            cols_in_buf: 84,
            page_st: true,
            page_end: true,
            prt_end: true,
            margin_top: 8,
            margin_bottom: 8,
            density: Density { black: 3, red: 12 },
            page: PageOptions::default(),
        });
        assert_eq!(buf[12], 12); // red deepness
        assert_eq!((buf[3] >> 2) & 0x0F, 3); // nodu = black
    }

    /// Two-colour advertises twice the printed columns and sets first_cut=2;
    /// mono touches neither. Both are how the firmware tells the modes apart.
    #[test]
    fn two_colour_doubles_columns_and_flags_first_cut() {
        let per_line_byte = 48u8;
        let cols = 40u16;
        // Two planes per column, so twice the image bytes.
        let image_data = vec![0u8; (cols as usize + 8) * per_line_byte as usize * 2];
        let bands = [DensityBand {
            cols,
            density: Density { black: 5, red: 9 },
        }];

        let two = split_into_banded_buffers(
            &image_data,
            per_line_byte,
            &bands,
            8,
            8,
            PageOptions {
                colour: ColourMode::TwoColour,
                ..Default::default()
            },
        );
        assert_eq!(u16::from_le_bytes([two[0][4], two[0][5]]), cols * 2);
        assert_eq!(two[0][3] & 0x03, 2); // first_cut
        assert_eq!(two[0][12], 9); // red trim survives
        assert_eq!((two[0][3] >> 2) & 0x0F, 5); // black trim survives

        let mono = split_into_banded_buffers(
            &image_data,
            per_line_byte,
            &bands,
            8,
            8,
            PageOptions::default(),
        );
        assert_eq!(u16::from_le_bytes([mono[0][4], mono[0][5]]), cols);
        assert_eq!(mono[0][3] & 0x03, 0);
    }

    /// Each column costs two planes, so a buffer holds half as many of them.
    #[test]
    fn two_colour_halves_buffer_capacity() {
        let per_line_byte = 48u8; // mono max_cols = 4074/48 = 84 → two-colour 42
        let cols = 100u16;
        let image_data = vec![0u8; (cols as usize + 16) * per_line_byte as usize * 2];
        let bands = [DensityBand {
            cols,
            density: Density::uniform(4),
        }];

        let bufs = split_into_banded_buffers(
            &image_data,
            per_line_byte,
            &bands,
            8,
            8,
            PageOptions {
                colour: ColourMode::TwoColour,
                ..Default::default()
            },
        );
        assert_eq!(bufs.len(), 3); // 42 + 42 + 16
        assert_eq!(u16::from_le_bytes([bufs[0][4], bufs[0][5]]), 42 * 2);
        assert_eq!(u16::from_le_bytes([bufs[2][4], bufs[2][5]]), 16 * 2);
    }

    /// The single-density entry point is the banded one with one band, so the
    /// two must agree exactly.
    #[test]
    fn plain_split_matches_single_band() {
        let per_line_byte = 48u8;
        let image_data = vec![0xA5u8; 240 * per_line_byte as usize];
        let plain = split_into_buffers(
            &image_data,
            per_line_byte,
            240,
            8,
            8,
            Density::uniform(4),
            PageOptions::default(),
        );
        let banded = split_into_banded_buffers(
            &image_data,
            per_line_byte,
            &[DensityBand {
                cols: 224,
                density: Density::uniform(4),
            }],
            8,
            8,
            PageOptions::default(),
        );
        assert_eq!(plain, banded);
    }
}

/// Build a print buffer using model-specific protocol parameters.
///
/// Kept separate from build_print_buffer so the established T-series path
/// remains unchanged while E-series support is verified.
pub fn build_print_buffer_profiled(
    p: &PrintBufferParams,
    profile: PrintProfile,
    first_buffer: bool,
) -> Vec<u8> {
    let params = profile.params();
    let mut buf = vec![0u8; params.buf_size];

    let black = p.density.black.min(params.max_density);
    let red = if first_buffer || !params.density_on_first_buffer_only {
        p.density.red.min(params.max_density)
    } else {
        0
    };

    let page_bits = build_page_reg_bits(&PageRegBits {
        page_st: p.page_st,
        page_end: p.page_end,
        prt_end: p.prt_end,
        nodu: params.nodu.unwrap_or(black),
        mat: params.mat,
        first_cut: p.page.colour.first_cut(),
        savepaper: p.page.save_paper,
        ..Default::default()
    });

    buf[2] = page_bits[0];
    buf[3] = page_bits[1];

    buf[4..6].copy_from_slice(&p.cols_in_buf.to_le_bytes());
    buf[6] = p.per_line_byte;

    let mt = p.margin_top.clamp(1, MARGIN_MAX_DOTS);
    let mb = p.margin_bottom.clamp(1, MARGIN_MAX_DOTS);
    buf[8..10].copy_from_slice(&mt.to_le_bytes());
    buf[10..12].copy_from_slice(&mb.to_le_bytes());

    buf[12] = red;

    let data_len = p.image_data.len().min(params.max_buf_data);
    buf[PRINT_BUF_HEADER..PRINT_BUF_HEADER + data_len]
        .copy_from_slice(&p.image_data[..data_len]);

    let data_end =
        (p.cols_in_buf as usize) * (p.per_line_byte as usize) + PRINT_BUF_HEADER;

    let mut chk: u32 = buf[2..14].iter().map(|&b| b as u32).sum();
    let n_strides = data_end / CHECKSUM_STRIDE;

    for i in 1..=n_strides {
        let idx = i * CHECKSUM_STRIDE - 1;
        if idx < buf.len() {
            chk += buf[idx] as u32;
        }
    }

    buf[0..2].copy_from_slice(&(chk as u16).to_le_bytes());

    buf
}

/// Split a single-density image using model-specific buffer parameters.
///
/// This parallels split_into_buffers but returns dynamically sized buffers,
/// allowing E-series 4000-byte buffers while retaining the existing T-series
/// implementation unchanged.
pub fn split_into_buffers_profiled(
    image_data: &[u8],
    per_line_byte: u8,
    total_cols: u16,
    margin_top: u16,
    margin_bottom: u16,
    density: Density,
    page: PageOptions,
    profile: PrintProfile,
) -> Vec<Vec<u8>> {
    let planes = page.colour.planes();
    let col_stride = per_line_byte as usize * planes as usize;
    let max_cols = (profile.params().max_buf_data / col_stride) as u16;

    let mut buffers = Vec::new();
    let mut cols_remaining = total_cols - margin_top - margin_bottom;
    let mut current_col: u16 = 0;

    while cols_remaining > 0 {
        let cols_in_buf = cols_remaining.min(max_cols);
        let is_first = current_col == 0;
        let is_last = cols_remaining <= max_cols;

        let img_start = (margin_top + current_col) as usize * col_stride;
        let img_end = img_start + cols_in_buf as usize * col_stride;

        let img_chunk = image_data
            .get(img_start..img_end.min(image_data.len()))
            .unwrap_or(&[]);

        let buf = build_print_buffer_profiled(
            &PrintBufferParams {
                image_data: img_chunk,
                per_line_byte,
                cols_in_buf: cols_in_buf * planes,
                page_st: is_first,
                page_end: is_last,
                prt_end: is_last,
                margin_top,
                margin_bottom,
                density,
                page,
            },
            profile,
            is_first,
        );

        buffers.push(buf);
        current_col += cols_in_buf;
        cols_remaining -= cols_in_buf;
    }

    buffers
}

#[cfg(test)]
mod e_series_profile_tests {
    use super::*;
    use crate::profile::PrintProfile;

    #[test]
    fn e_series_split_matches_captured_geometry() {
        const IMAGE_COLS: u16 = 373;
        const PER_LINE: u8 = 12;

        let margin = PrintProfile::ESeries.params().margin_dots;
        let total_cols = IMAGE_COLS + margin * 2;

        let image = vec![0u8; total_cols as usize * PER_LINE as usize];

        let buffers = split_into_buffers_profiled(
            &image,
            PER_LINE,
            total_cols,
            margin,
            margin,
            Density::uniform(19),
            PageOptions::default(),
            PrintProfile::ESeries,
        );

        assert_eq!(buffers.len(), 2);

        assert_eq!(buffers[0].len(), 4000);
        assert_eq!(buffers[1].len(), 4000);

        assert_eq!(
            u16::from_le_bytes([buffers[0][4], buffers[0][5]]),
            332
        );

        assert_eq!(
            u16::from_le_bytes([buffers[1][4], buffers[1][5]]),
            41
        );

        assert_eq!(buffers[0][3], 0x10);
        assert_eq!(buffers[1][3], 0x10);

        assert_eq!(buffers[0][12], 19);
        assert_eq!(buffers[1][12], 0);
    }
}
