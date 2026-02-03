//! Integration tests for the full raster pipeline.
//!
//! Tests the chain used by `ks_job_end_page`: generate raster →
//! `raster_to_column_major` → `center_in_printhead` → `split_into_buffers` →
//! `compress_buffers` → decompress → verify roundtrip.

use supvan_proto::bitmap::{
    center_in_printhead, create_test_pattern, raster_to_column_major, DEFAULT_MARGIN_DOTS,
    DOTS_PER_MM, PRINTHEAD_BYTES_PER_LINE, PRINTHEAD_WIDTH_DOTS,
};
use supvan_proto::buffer::{split_into_buffers, MAX_BUF_DATA, PRINT_BUF_HEADER, PRINT_BUF_SIZE};
use supvan_proto::compress::compress_buffers;

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

/// Decompress LZMA1-alone data (as produced by `compress_lzma`).
fn decompress_lzma(data: &[u8]) -> Vec<u8> {
    use std::io::Write;
    let stream = xz2::stream::Stream::new_lzma_decoder(u64::MAX).unwrap();
    let mut decompressed = Vec::new();
    let mut decoder = xz2::write::XzDecoder::new_stream(&mut decompressed, stream);
    decoder.write_all(data).unwrap();
    decoder.flush().unwrap();
    drop(decoder);
    decompressed
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
        4,
    );

    let (compressed, _avg) = compress_buffers(&buffers).unwrap();
    let decompressed = decompress_lzma(&compressed);

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
        4,
    );
    assert_eq!(buffers.len(), 3);

    let (compressed, avg) = compress_buffers(&buffers).unwrap();
    assert!(avg > 0, "average compressed size should be > 0");

    let decompressed = decompress_lzma(&compressed);
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
            4,
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
        let (compressed, _) = compress_buffers(&buffers).unwrap();
        let decompressed = decompress_lzma(&compressed);
        let mut concat = Vec::with_capacity(buffers.len() * PRINT_BUF_SIZE);
        for buf in &buffers {
            concat.extend_from_slice(buf);
        }
        assert_eq!(decompressed, concat, "{w_mm}x{h_mm}mm: roundtrip mismatch");
    }
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
