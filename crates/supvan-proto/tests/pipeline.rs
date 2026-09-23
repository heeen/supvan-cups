//! Integration tests for the full raster pipeline.
//!
//! Tests the chain used by `ks_job_end_page`: generate raster →
//! `raster_to_column_major` → `center_in_printhead` → `split_into_buffers` →
//! `compress_buffers` → decompress → verify roundtrip.

use supvan_proto::bitmap::{
    DEFAULT_MARGIN_DOTS, DOTS_PER_MM, PRINTHEAD_BYTES_PER_LINE, PRINTHEAD_WIDTH_DOTS,
    center_in_printhead, create_test_pattern, raster_to_column_major,
};
use supvan_proto::buffer::{
    Density, MAX_BUF_DATA, PRINT_BUF_HEADER, PRINT_BUF_SIZE, PageOptions, split_into_buffers,
};
use supvan_proto::compress::{compress_buffers, decompress_lzma};

/// No block may exceed the firmware's 4096-byte receive buffer, unless it is
/// already down to a single print buffer and cannot be split further.
fn assert_blocks_fit(blocks: &[Vec<u8>]) {
    for (i, b) in blocks.iter().enumerate() {
        let raw = decompress_lzma(b).expect("block roundtrip").len();
        assert!(
            b.len() <= 4096 || raw == PRINT_BUF_SIZE,
            "block {i}: {} compressed bytes spanning {raw} raw — too big to send",
            b.len()
        );
    }
}

/// Decompress each per-buffer block and concatenate, recovering the original
/// print-buffer bytes.
fn decompress_blocks(blocks: &[Vec<u8>]) -> Vec<u8> {
    blocks
        .iter()
        .flat_map(|b| decompress_lzma(b).expect("block roundtrip"))
        .collect()
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a row-major MSB-first 1bpp bitmap filled with `fill`.
fn generate_row_major_1bpp(width: u32, height: u32, fill: u8) -> Vec<u8> {
    let bytes_per_row = width.div_ceil(8) as usize;
    vec![fill; bytes_per_row * height as usize]
}

/// Write a PBM P4 binary image to a `Vec<u8>`.
fn write_pbm_p4(data: &[u8], width: u32, height: u32) -> Vec<u8> {
    let header = format!("P4\n{width} {height}\n");
    let mut buf = Vec::with_capacity(header.len() + data.len());
    buf.extend_from_slice(header.as_bytes());
    buf.extend_from_slice(data);
    buf
}

/// Parse a PBM P4 header, returning (width, height, data_offset).
fn parse_pbm_p4(pbm: &[u8]) -> (u32, u32, usize) {
    // P4\n<width> <height>\n<data>
    // Only parse the header portion as text — pixel data may contain arbitrary bytes.
    assert!(pbm.len() >= 7, "PBM too short");
    assert_eq!(&pbm[..3], b"P4\n", "not a P4 PBM");
    // Find the newline after dimensions
    let nl_pos = pbm[3..]
        .iter()
        .position(|&b| b == b'\n')
        .expect("missing newline after dimensions");
    let dims = std::str::from_utf8(&pbm[3..3 + nl_pos]).expect("dimensions not ASCII");
    let mut parts = dims.split_whitespace();
    let w: u32 = parts.next().unwrap().parse().unwrap();
    let h: u32 = parts.next().unwrap().parse().unwrap();
    let offset = 3 + nl_pos + 1; // "P4\n" + dims + "\n"
    (w, h, offset)
}

/// Run the full pipeline on row-major MSB-first raster data and return
/// (buffers, compressed, decompressed) for verification.
fn run_pipeline(
    raster: &[u8],
    width: u32,
    height: u32,
) -> (Vec<[u8; PRINT_BUF_SIZE]>, Vec<u8>, Vec<u8>) {
    let (col_data, num_cols, _col_bpl) = raster_to_column_major(raster, width, height);

    let canvas_width_dots = PRINTHEAD_WIDTH_DOTS;
    let (canvas, canvas_bpl) = center_in_printhead(&col_data, num_cols, width, canvas_width_dots);

    let buffers = split_into_buffers(
        &canvas,
        canvas_bpl as u8,
        num_cols as u16,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        Density::uniform(4),
        PageOptions::default(),
    );

    let (blocks, _avg) = compress_buffers(&buffers).unwrap();
    let compressed: Vec<u8> = blocks.concat();
    let decompressed = decompress_blocks(&blocks);

    (buffers, compressed, decompressed)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Solid-black 40x30mm label: all 0xFF raster through the full pipeline.
#[test]
fn test_full_pipeline_solid_black() {
    let width = 40 * DOTS_PER_MM; // 320 pixels
    let height = 30 * DOTS_PER_MM; // 240 pixels
    let raster = generate_row_major_1bpp(width, height, 0xFF);

    // Verify PBM write/parse roundtrip on the source raster
    let pbm = write_pbm_p4(&raster, width, height);
    let (pw, ph, offset) = parse_pbm_p4(&pbm);
    assert_eq!(pw, width);
    assert_eq!(ph, height);
    assert_eq!(&pbm[offset..], &raster[..]);

    // Run full pipeline
    let (buffers, compressed, decompressed) = run_pipeline(&raster, width, height);

    // 48 bytes/line, 240 cols, margins 8+8 = 224 image cols
    // max_cols = 4074/48 = 84 → 84+84+56 = 224
    assert_eq!(buffers.len(), 3, "expected 3 buffers for 40x30mm");

    // Compressed data should be non-trivial
    assert!(compressed.len() > 13, "compressed too small");

    // Decompressed should be exactly buffers.len() * 4096
    assert_eq!(
        decompressed.len(),
        buffers.len() * PRINT_BUF_SIZE,
        "decompressed size mismatch"
    );

    // Byte-exact match between buffers and decompressed
    let mut concat = Vec::with_capacity(buffers.len() * PRINT_BUF_SIZE);
    for buf in &buffers {
        concat.extend_from_slice(buf);
    }
    assert_eq!(decompressed, concat, "roundtrip mismatch");

    // Each buffer should have non-zero image data (solid black label)
    for (i, buf) in buffers.iter().enumerate() {
        let has_data = buf[PRINT_BUF_HEADER..].iter().any(|&b| b != 0);
        assert!(
            has_data,
            "buffer {i} has no image data for solid black input"
        );
    }
}

/// Solid-white 40x30mm label: all 0x00 raster.
#[test]
fn test_full_pipeline_solid_white() {
    let width = 40 * DOTS_PER_MM;
    let height = 30 * DOTS_PER_MM;
    let raster = generate_row_major_1bpp(width, height, 0x00);

    let (buffers, _compressed, decompressed) = run_pipeline(&raster, width, height);

    assert_eq!(buffers.len(), 3);

    // Decompressed roundtrip
    let mut concat = Vec::with_capacity(buffers.len() * PRINT_BUF_SIZE);
    for buf in &buffers {
        concat.extend_from_slice(buf);
    }
    assert_eq!(decompressed, concat);

    // Image data region should be all zeros (white)
    for (i, buf) in buffers.iter().enumerate() {
        let all_zero = buf[PRINT_BUF_HEADER..].iter().all(|&b| b == 0);
        assert!(all_zero, "buffer {i} has non-zero data for white input");
    }
}

/// Checkerboard pattern: alternating 0xAA/0x55 rows.
/// Exercises bit-level correctness in MSB→LSB conversion and column rotation.
#[test]
fn test_full_pipeline_checkerboard() {
    let width = 40 * DOTS_PER_MM; // 320
    let height = 30 * DOTS_PER_MM; // 240
    let bytes_per_row = width.div_ceil(8) as usize;

    let mut raster = Vec::with_capacity(bytes_per_row * height as usize);
    for y in 0..height {
        let fill = if y % 2 == 0 { 0xAA } else { 0x55 };
        raster.extend(std::iter::repeat_n(fill, bytes_per_row));
    }

    let (buffers, compressed, decompressed) = run_pipeline(&raster, width, height);

    assert_eq!(buffers.len(), 3);
    assert!(compressed.len() > 13);

    // Roundtrip
    let mut concat = Vec::with_capacity(buffers.len() * PRINT_BUF_SIZE);
    for buf in &buffers {
        concat.extend_from_slice(buf);
    }
    assert_eq!(decompressed, concat);

    // Checkerboard should produce non-trivial data in all buffers
    for (i, buf) in buffers.iter().enumerate() {
        let has_data = buf[PRINT_BUF_HEADER..].iter().any(|&b| b != 0);
        assert!(has_data, "buffer {i} has no data for checkerboard input");
    }
}

/// Test pattern from `create_test_pattern(40, 30)`.
///
/// This produces column-major output directly, so we test it through
/// `split_into_buffers` → `compress_buffers` (skipping raster_to_column_major).
#[test]
fn test_full_pipeline_test_pattern() {
    let (col_data, canvas_width_dots, height_dots, bytes_per_line) = create_test_pattern(40, 30);

    assert_eq!(canvas_width_dots, 384);
    assert_eq!(height_dots, 240);
    assert_eq!(bytes_per_line, 48);

    let buffers = split_into_buffers(
        &col_data,
        bytes_per_line as u8,
        height_dots as u16,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        Density::uniform(4),
        PageOptions::default(),
    );
    assert_eq!(buffers.len(), 3);

    let (blocks, avg) = compress_buffers(&buffers).unwrap();
    assert!(avg > 0, "average compressed size should be > 0");
    assert_blocks_fit(&blocks);

    let decompressed = decompress_blocks(&blocks);
    let mut concat = Vec::with_capacity(buffers.len() * PRINT_BUF_SIZE);
    for buf in &buffers {
        concat.extend_from_slice(buf);
    }
    assert_eq!(decompressed, concat, "test pattern roundtrip mismatch");

    // Test pattern should have interesting data in every buffer
    for (i, buf) in buffers.iter().enumerate() {
        let has_data = buf[PRINT_BUF_HEADER..].iter().any(|&b| b != 0);
        assert!(has_data, "buffer {i} has no data for test pattern");
    }
}

/// Parametric test over various label sizes.
///
/// Verifies: canvas is always 384 dots wide, buffer count matches expected,
/// and compression roundtrip works for each size.
#[test]
fn test_pipeline_various_sizes() {
    let sizes: &[(u32, u32)] = &[(40, 30), (30, 20), (48, 70), (25, 25), (50, 30)];

    for &(w_mm, h_mm) in sizes {
        let width = w_mm * DOTS_PER_MM;
        let height = h_mm * DOTS_PER_MM;
        let raster = generate_row_major_1bpp(width, height, 0xFF);

        let (col_data, num_cols, _) = raster_to_column_major(&raster, width, height);
        let canvas_width_dots = PRINTHEAD_WIDTH_DOTS;
        let (canvas, canvas_bpl) =
            center_in_printhead(&col_data, num_cols, width, canvas_width_dots);

        // Canvas is always 384 dots = 48 bytes per line
        assert_eq!(
            canvas_bpl, PRINTHEAD_BYTES_PER_LINE,
            "{w_mm}x{h_mm}mm: canvas_bpl mismatch"
        );
        assert_eq!(
            canvas.len(),
            num_cols as usize * PRINTHEAD_BYTES_PER_LINE as usize,
            "{w_mm}x{h_mm}mm: canvas size mismatch"
        );

        let buffers = split_into_buffers(
            &canvas,
            canvas_bpl as u8,
            num_cols as u16,
            DEFAULT_MARGIN_DOTS,
            DEFAULT_MARGIN_DOTS,
            Density::uniform(4),
            PageOptions::default(),
        );

        // Verify expected buffer count
        let image_cols = height - DEFAULT_MARGIN_DOTS as u32 * 2;
        let max_cols_per_buf = MAX_BUF_DATA as u32 / PRINTHEAD_BYTES_PER_LINE;
        let expected_bufs = image_cols.div_ceil(max_cols_per_buf) as usize;
        assert_eq!(
            buffers.len(),
            expected_bufs,
            "{w_mm}x{h_mm}mm: buffer count mismatch"
        );

        // Compression roundtrip
        let (blocks, _) = compress_buffers(&buffers).unwrap();
        assert_blocks_fit(&blocks);
        let decompressed = decompress_blocks(&blocks);
        let mut concat = Vec::with_capacity(buffers.len() * PRINT_BUF_SIZE);
        for buf in &buffers {
            concat.extend_from_slice(buf);
        }
        assert_eq!(decompressed, concat, "{w_mm}x{h_mm}mm: roundtrip mismatch");
    }
}

/// A 50×80 mm page is 8 print buffers, and it has to travel as a single
/// compressed block.
///
/// Split across two, the printer lays down only the first block's worth: half
/// a label, no error flag, both blocks acknowledged, and CUPS reporting the job
/// `completed`. Eight buffers sit well under the vendor's per-block cap, so the
/// packing must not split them.
#[test]
fn a_long_page_travels_as_one_block() {
    // The head canvas is 48 mm (384 dots) wide once the page is centred; the
    // feed direction carries the page height.
    let width = PRINTHEAD_WIDTH_DOTS;
    let height = 80 * DOTS_PER_MM;
    let bytes_per_row = width.div_ceil(8) as usize;

    // Text-like rule pattern — mostly white with thin strokes, which is what a
    // real label raster looks like.
    let mut raster = vec![0u8; bytes_per_row * height as usize];
    for y in (4..height as usize).step_by(13) {
        for x in 2..bytes_per_row - 2 {
            raster[y * bytes_per_row + x] = 0xF0;
        }
    }

    let (col_data, num_cols, _) = raster_to_column_major(&raster, width, height);
    let (canvas, canvas_bpl) =
        center_in_printhead(&col_data, num_cols, width, PRINTHEAD_WIDTH_DOTS);
    let buffers = split_into_buffers(
        &canvas,
        canvas_bpl as u8,
        num_cols as u16,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        Density::uniform(4),
        PageOptions::default(),
    );
    assert_eq!(buffers.len(), 8, "a 50x80mm page is 8 print buffers");

    let (blocks, _) = compress_buffers(&buffers).unwrap();
    assert_eq!(blocks.len(), 1, "the page must travel as one block");
    assert_blocks_fit(&blocks);
    assert_eq!(decompress_blocks(&blocks), buffers.concat());
}

/// PBM P4 write/read roundtrip.
#[test]
fn test_pbm_write_read() {
    let width = 40 * DOTS_PER_MM; // 320
    let height = 30 * DOTS_PER_MM; // 240
    let bytes_per_row = width.div_ceil(8) as usize;

    // Generate a recognizable pattern
    let mut data = vec![0u8; bytes_per_row * height as usize];
    for y in 0..height as usize {
        for bx in 0..bytes_per_row {
            data[y * bytes_per_row + bx] = ((y + bx) & 0xFF) as u8;
        }
    }

    let pbm = write_pbm_p4(&data, width, height);
    let (pw, ph, offset) = parse_pbm_p4(&pbm);

    assert_eq!(pw, width);
    assert_eq!(ph, height);
    assert_eq!(
        &pbm[offset..],
        &data[..],
        "PBM pixel data does not match original"
    );
}

/// Every column the caller hands in must reach a buffer.
///
/// `split_into_buffers` reads image data starting `margin_top` columns in and
/// sizes the run as `total_cols - margins`, which is right only when the image
/// itself carries those margin columns. The CUPS path hands over pure
/// printable area, so it passes zero — with the old default of 8 the label came
/// out shifted 1mm and 2mm short at the tail.
#[test]
fn zero_margins_tile_every_column() {
    let cols: u16 = 639; // an 80mm label at 8 dots/mm
    let per_line_byte: u8 = 48; // 384-dot head
    // Distinct byte per column, so a shift or a gap is unmistakable.
    let canvas: Vec<u8> = (0..cols as usize)
        .flat_map(|c| std::iter::repeat_n((c % 251) as u8, per_line_byte as usize))
        .collect();

    let buffers = split_into_buffers(
        &canvas,
        per_line_byte,
        cols,
        0,
        0,
        Density::uniform(4),
        PageOptions::default(),
    );

    let mut sent = Vec::new();
    let mut advertised = 0u32;
    for b in &buffers {
        let n = u16::from_le_bytes([b[4], b[5]]) as usize;
        advertised += n as u32;
        sent.extend_from_slice(&b[PRINT_BUF_HEADER..PRINT_BUF_HEADER + n * per_line_byte as usize]);
    }

    assert_eq!(advertised, cols as u32, "columns dropped between buffers");
    assert_eq!(sent, canvas, "image shifted or truncated");
}
