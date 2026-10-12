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
use supvan_proto::compress::{CompressedPage, Packing, compress_buffers, decompress_lzma};

const PACKINGS: [Packing; 2] = [Packing::Grouped, Packing::PerBuffer];

/// Every block must fit the firmware's receive buffer, one print buffer's size.
fn assert_blocks_fit(page: &CompressedPage) {
    for (i, b) in page.blocks.iter().enumerate() {
        assert!(
            b.data.len() <= PRINT_BUF_SIZE,
            "block {i}: {} compressed bytes — past firmware limit",
            b.data.len()
        );
    }
}

/// Decompress each block and concatenate, recovering the original print-buffer bytes.
fn decompress_blocks(page: &CompressedPage) -> Vec<u8> {
    page.blocks
        .iter()
        .flat_map(|b| decompress_lzma(&b.data).expect("block roundtrip"))
        .collect()
}

/// Whether every buffer carries some image data.
fn assert_every_buffer_inked(buffers: &[[u8; PRINT_BUF_SIZE]], what: &str) {
    for (i, buf) in buffers.iter().enumerate() {
        assert!(
            buf[PRINT_BUF_HEADER..].iter().any(|&b| b != 0),
            "buffer {i} has no image data for {what}"
        );
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Generate a row-major MSB-first 1bpp bitmap filled with `fill`.
fn generate_row_major_1bpp(width: u32, height: u32, fill: u8) -> Vec<u8> {
    let bytes_per_row = width.div_ceil(8) as usize;
    vec![fill; bytes_per_row * height as usize]
}

/// Run the full pipeline on row-major MSB-first raster data and return
/// (buffers, compressed, decompressed) for verification.
fn run_pipeline(
    raster: &[u8],
    width: u32,
    height: u32,
) -> (Vec<[u8; PRINT_BUF_SIZE]>, Vec<u8>, Vec<u8>) {
    let columns = raster_to_column_major(raster, width, height);
    let canvas = center_in_printhead(&columns, width, PRINTHEAD_WIDTH_DOTS);

    let buffers = split_into_buffers(
        &canvas.data,
        canvas.bpl,
        canvas.cols,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        Density::uniform(4),
        PageOptions::default(),
    );

    let page = compress_buffers(&buffers, Packing::Grouped).unwrap();
    let compressed: Vec<u8> = page.blocks.iter().flat_map(|b| b.data.clone()).collect();
    let decompressed = decompress_blocks(&page);

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
    let (buffers, compressed, decompressed) = run_pipeline(&raster, width, height);

    // 48 bytes/line, 240 cols, margins 8+8 = 224 image cols
    // max_cols = 4074/48 = 84 → 84+84+56 = 224
    assert_eq!(buffers.len(), 3, "expected 3 buffers for 40x30mm");

    assert!(!compressed.is_empty());
    assert_eq!(decompressed, buffers.concat(), "roundtrip mismatch");
    assert_every_buffer_inked(&buffers, "solid black input");
}

/// Solid-white 40x30mm label: all 0x00 raster.
#[test]
fn test_full_pipeline_solid_white() {
    let width = 40 * DOTS_PER_MM;
    let height = 30 * DOTS_PER_MM;
    let raster = generate_row_major_1bpp(width, height, 0x00);

    let (buffers, _compressed, decompressed) = run_pipeline(&raster, width, height);

    assert_eq!(buffers.len(), 3);
    assert_eq!(decompressed, buffers.concat());

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
    assert!(!compressed.is_empty());
    assert_eq!(decompressed, buffers.concat());
    assert_every_buffer_inked(&buffers, "checkerboard input");
}

/// Test pattern from `create_test_pattern(40, 30)`.
///
/// This produces column-major output directly, so we test it through
/// `split_into_buffers` → `compress_buffers` (skipping raster_to_column_major).
#[test]
fn test_full_pipeline_test_pattern() {
    let pattern = create_test_pattern(40, 30);
    assert_eq!((pattern.cols, pattern.bpl), (240, 48));

    let buffers = split_into_buffers(
        &pattern.data,
        pattern.bpl,
        pattern.cols,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        Density::uniform(4),
        PageOptions::default(),
    );
    assert_eq!(buffers.len(), 3);

    for packing in PACKINGS {
        let page = compress_buffers(&buffers, packing).unwrap();
        assert_blocks_fit(&page);
        assert!(page.speed > 0);
        assert_eq!(
            decompress_blocks(&page),
            buffers.concat(),
            "{packing:?}: test pattern roundtrip mismatch"
        );
    }
    assert_every_buffer_inked(&buffers, "the test pattern");
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

        let columns = raster_to_column_major(&raster, width, height);
        let canvas = center_in_printhead(&columns, width, PRINTHEAD_WIDTH_DOTS);

        // The canvas is always the full head: 384 dots = 48 bytes per line.
        assert_eq!(
            u32::from(canvas.bpl),
            PRINTHEAD_BYTES_PER_LINE,
            "{w_mm}x{h_mm}mm: canvas bpl mismatch"
        );
        assert_eq!(
            canvas.data.len(),
            usize::from(canvas.cols) * PRINTHEAD_BYTES_PER_LINE as usize,
            "{w_mm}x{h_mm}mm: canvas size mismatch"
        );

        let buffers = split_into_buffers(
            &canvas.data,
            canvas.bpl,
            canvas.cols,
            DEFAULT_MARGIN_DOTS,
            DEFAULT_MARGIN_DOTS,
            Density::uniform(4),
            PageOptions::default(),
        );

        let image_cols = height - DEFAULT_MARGIN_DOTS as u32 * 2;
        let max_cols_per_buf = MAX_BUF_DATA as u32 / PRINTHEAD_BYTES_PER_LINE;
        let expected_bufs = image_cols.div_ceil(max_cols_per_buf) as usize;
        assert_eq!(
            buffers.len(),
            expected_bufs,
            "{w_mm}x{h_mm}mm: buffer count mismatch"
        );

        for packing in PACKINGS {
            let page = compress_buffers(&buffers, packing).unwrap();
            assert_blocks_fit(&page);
            assert_eq!(
                decompress_blocks(&page),
                buffers.concat(),
                "{w_mm}x{h_mm}mm {packing:?}: roundtrip mismatch"
            );
        }
    }
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
