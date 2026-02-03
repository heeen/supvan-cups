/// Max image data bytes per print buffer (from Android R2.drawable.sf5334_).
pub const MAX_BUF_DATA: usize = 4074;

/// Print buffer size.
pub const PRINT_BUF_SIZE: usize = 4096;

/// Header size in print buffer.
pub const PRINT_BUF_HEADER: usize = 14;

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

/// Parameters for building a print buffer.
pub struct PrintBufferParams<'a> {
    pub image_data: &'a [u8],
    pub per_line_byte: u8,
    pub cols_in_buf: u16,
    pub page_st: bool,
    pub page_end: bool,
    pub prt_end: bool,
    pub margin_top: u16,
    pub margin_bottom: u16,
    pub density: u8,
}

/// Build a 4096-byte print buffer.
///
/// Layout:
///   [0..1]   Checksum (LE)
///   [2..3]   PAGE_REG_BITS
///   [4..5]   Column count (LE)
///   [6]      Bytes per line
///   [7]      Reserved (0)
///   [8..9]   Margin top (LE, 1-900 dots)
///   [10..11] Margin bottom (LE, 1-900 dots)
///   [12]     Density / red deepness (0-15)
///   [13]     0
///   [14..]   Image data
pub fn build_print_buffer(p: &PrintBufferParams) -> [u8; PRINT_BUF_SIZE] {
    let mut buf = [0u8; PRINT_BUF_SIZE];

    // PAGE_REG_BITS
    let page_bits = build_page_reg_bits(&PageRegBits {
        page_st: p.page_st,
        page_end: p.page_end,
        prt_end: p.prt_end,
        nodu: p.density,
        mat: 1,
        ..Default::default()
    });
    buf[2] = page_bits[0];
    buf[3] = page_bits[1];

    // Column count
    buf[4] = (p.cols_in_buf & 0xFF) as u8;
    buf[5] = (p.cols_in_buf >> 8) as u8;

    // Bytes per line
    buf[6] = p.per_line_byte;

    // Margins (clamped 1-900)
    let mt = p.margin_top.clamp(1, 900);
    let mb = p.margin_bottom.clamp(1, 900);
    buf[8] = (mt & 0xFF) as u8;
    buf[9] = (mt >> 8) as u8;
    buf[10] = (mb & 0xFF) as u8;
    buf[11] = (mb >> 8) as u8;

    // Density
    buf[12] = p.density.min(15);

    // Image data at offset 14
    let data_len = p.image_data.len().min(PRINT_BUF_SIZE - PRINT_BUF_HEADER);
    buf[PRINT_BUF_HEADER..PRINT_BUF_HEADER + data_len].copy_from_slice(&p.image_data[..data_len]);

    // Checksum: sum(buf[2..14]) + sum of bytes at each 256-byte boundary
    let data_end = (p.cols_in_buf as usize) * (p.per_line_byte as usize) + PRINT_BUF_HEADER;
    let mut chk: u32 = buf[2..14].iter().map(|&b| b as u32).sum();
    let n_256 = data_end / 256;
    for i in 1..=n_256 {
        let idx = i * 256 - 1;
        if idx < buf.len() {
            chk += buf[idx] as u32;
        }
    }
    buf[0] = (chk & 0xFF) as u8;
    buf[1] = ((chk >> 8) & 0xFF) as u8;

    buf
}

/// Split column-major image data into multiple print buffers.
///
/// Returns a Vec of 4096-byte print buffers ready for LZMA compression.
pub fn split_into_buffers(
    image_data: &[u8],
    per_line_byte: u8,
    total_cols: u16,
    margin_top: u16,
    margin_bottom: u16,
    density: u8,
) -> Vec<[u8; PRINT_BUF_SIZE]> {
    let max_cols = (MAX_BUF_DATA / per_line_byte as usize) as u16;
    let image_cols = total_cols - margin_top - margin_bottom;
    let mut buffers = Vec::new();
    let mut cols_remaining = image_cols;
    let mut current_col: u16 = 0;

    while cols_remaining > 0 {
        let cols_in_buf = cols_remaining.min(max_cols);
        let is_first = current_col == 0;
        let is_last = cols_remaining <= max_cols;

        let img_start = (margin_top + current_col) as usize * per_line_byte as usize;
        let img_end = img_start + cols_in_buf as usize * per_line_byte as usize;
        let img_chunk = &image_data[img_start..img_end.min(image_data.len())];

        let buf = build_print_buffer(&PrintBufferParams {
            image_data: img_chunk,
            per_line_byte,
            cols_in_buf,
            page_st: is_first,
            page_end: is_last,
            prt_end: is_last,
            margin_top,
            margin_bottom,
            density,
        });
        buffers.push(buf);
        current_col += cols_in_buf;
        cols_remaining -= cols_in_buf;
    }

    buffers
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
            density: 4,
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
        let bufs = split_into_buffers(&image_data, per_line_byte, total_cols, 8, 8, 4);
        assert_eq!(bufs.len(), 3);
    }
}
