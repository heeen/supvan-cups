/// T50 Pro: 8 dots/mm, 48mm printhead.
pub const DOTS_PER_MM: u32 = 8;
pub const PRINTHEAD_WIDTH_MM: u32 = 48;
pub const PRINTHEAD_WIDTH_DOTS: u32 = PRINTHEAD_WIDTH_MM * DOTS_PER_MM;
pub const PRINTHEAD_BYTES_PER_LINE: u32 = PRINTHEAD_WIDTH_DOTS / 8;
pub const DEFAULT_MARGIN_DOTS: u16 = 8;

/// Convert a row-major MSB-first 1bpp bitmap (standard raster format) into
/// column-major LSB-first 1bpp format suitable for the printer.
///
/// The input bitmap is `width` x `height` pixels in row-major order with
/// MSB-first bit packing (standard CUPS/image convention: leftmost pixel
/// is the most significant bit).
///
/// The printer expects column-major LSB-first: each "column" of the output
/// corresponds to a column of dots in the printed label. After a -90 degree
/// rotation, the output has `height` columns, each `ceil(width/8)` bytes
/// wide with LSB-first packing.
///
/// This effectively rotates the image -90 degrees and repacks the bits.
///
/// Returns `(output_data, output_cols, bytes_per_line)`.
pub fn raster_to_column_major(input: &[u8], width: u32, height: u32) -> (Vec<u8>, u32, u32) {
    let in_bytes_per_row = width.div_ceil(8);
    let out_bytes_per_line = width.div_ceil(8); // printhead width packed
    let out_cols = height;

    let mut output = vec![0u8; out_cols as usize * out_bytes_per_line as usize];

    for y in 0..height {
        for x in 0..width {
            // Read pixel from row-major MSB-first input
            let in_byte_idx = y as usize * in_bytes_per_row as usize + (x / 8) as usize;
            let in_bit = 7 - (x % 8); // MSB-first
            if in_byte_idx >= input.len() {
                continue;
            }
            let pixel = (input[in_byte_idx] >> in_bit) & 1;

            if pixel != 0 {
                // Write to column-major LSB-first output
                // After -90 rotation: output column = y, row position = x
                let out_byte_idx = y as usize * out_bytes_per_line as usize + (x / 8) as usize;
                let out_bit = x % 8; // LSB-first
                output[out_byte_idx] |= 1 << out_bit;
            }
        }
    }

    (output, out_cols, out_bytes_per_line)
}

/// Center image data in a full-width printhead canvas.
///
/// The printhead is a fixed physical bar (384 dots / 48 mm on the T50) and the
/// media runs centred under it, so a page is always placed centred in a
/// head-width canvas regardless of the label's own width.
///
/// A page *wider* than the head is cropped symmetrically rather than padded.
/// That is reachable in normal operation — the T50 family advertises 50 mm
/// media on a 48 mm head — and CUPS renders the full media width because the
/// IPP layer declares zero hard margins on all four sides.
///
/// Input: column-major LSB-first data with `input_bytes_per_line` per column.
/// Output: column-major LSB-first data with `canvas_bytes_per_line` per column.
pub fn center_in_printhead(
    input: &[u8],
    num_cols: u32,
    input_width_dots: u32,
    canvas_width_dots: u32,
) -> (Vec<u8>, u32) {
    let canvas_bytes_per_line = (canvas_width_dots / 8) as usize;
    let input_bytes_per_line = input_width_dots.div_ceil(8) as usize;
    // A head whose width isn't a whole number of bytes (the G series is 190
    // dots) can only carry `canvas_bytes_per_line * 8`. Every dot decision
    // below measures against that, not the nominal width, which would walk
    // off the end of the last column.
    let usable_width_dots = (canvas_bytes_per_line * 8) as u32;

    if input_width_dots >= usable_width_dots {
        // Wider than the head: keep the *middle* of the image, because the
        // media runs centred under the printhead. Copying the leading bytes
        // instead drops one whole edge — a 50 mm label on the T50's 48 mm
        // head lost 2 mm off the right rather than 1 mm off each side.
        let lost = input_width_dots - usable_width_dots;
        if lost > 0 {
            let left = lost / 2;
            log::warn!(
                "center_in_printhead: image is {input_width_dots} dots but the head can \
                 carry {usable_width_dots} (nominal {canvas_width_dots}); cropping {lost} \
                 dots ({left} left, {} right)",
                lost - left
            );
        }
        let x_offset_dots = lost / 2;
        let mut output = vec![0u8; num_cols as usize * canvas_bytes_per_line];
        for col in 0..num_cols as usize {
            for dot in 0..usable_width_dots {
                // The crop offset is rarely byte-aligned, so shift bit by bit.
                let src_dot = x_offset_dots + dot;
                let in_byte = col * input_bytes_per_line + (src_dot / 8) as usize;
                if in_byte >= input.len() {
                    continue;
                }
                if (input[in_byte] >> (src_dot % 8)) & 1 != 0 {
                    let out_byte = col * canvas_bytes_per_line + (dot / 8) as usize;
                    output[out_byte] |= 1 << (dot % 8);
                }
            }
        }
        return (output, canvas_bytes_per_line as u32);
    }

    let x_offset_dots = (usable_width_dots - input_width_dots) / 2;
    let mut output = vec![0u8; num_cols as usize * canvas_bytes_per_line];

    for col in 0..num_cols as usize {
        for dot in 0..input_width_dots {
            // Read from input (LSB-first)
            let in_byte = col * input_bytes_per_line + (dot / 8) as usize;
            let in_bit = dot % 8;
            if in_byte >= input.len() {
                continue;
            }
            let pixel = (input[in_byte] >> in_bit) & 1;

            if pixel != 0 {
                // Write to output at offset position (LSB-first)
                let out_dot = x_offset_dots + dot;
                let out_byte = col * canvas_bytes_per_line + (out_dot / 8) as usize;
                let out_bit = out_dot % 8;
                if out_byte < output.len() {
                    output[out_byte] |= 1 << out_bit;
                }
            }
        }
    }

    (output, canvas_bytes_per_line as u32)
}

/// Create a test pattern matching the Python reference implementation.
///
/// Returns (image_bytes, canvas_width_dots, height_dots, bytes_per_line).
pub fn create_test_pattern(label_width_mm: u32, height_mm: u32) -> (Vec<u8>, u32, u32, u32) {
    let canvas_width_dots = PRINTHEAD_WIDTH_DOTS;
    let height_dots = height_mm * DOTS_PER_MM;
    let bytes_per_line = PRINTHEAD_BYTES_PER_LINE;
    let label_width_dots = label_width_mm * DOTS_PER_MM;
    let x_offset = (canvas_width_dots - label_width_dots) / 2;

    let margin_top = DEFAULT_MARGIN_DOTS as u32;
    let margin_bottom = DEFAULT_MARGIN_DOTS as u32;
    let max_cols = (crate::buffer::MAX_BUF_DATA / bytes_per_line as usize) as u32;

    // Compute buffer regions
    let mut buf_regions: Vec<(u32, u32)> = Vec::new();
    let mut col = margin_top;
    while col < height_dots - margin_bottom {
        let end = (col + max_cols).min(height_dots - margin_bottom);
        buf_regions.push((col, end));
        col = end;
    }

    // Column-major LSB-first output
    let mut buf = vec![0u8; bytes_per_line as usize * height_dots as usize];

    for col in 0..height_dots {
        for row in 0..canvas_width_dots {
            let mut pixel = false;

            let label_row = row as i32 - x_offset as i32;
            if label_row >= 0 && (label_row as u32) < label_width_dots {
                let lr = label_row as u32;

                // Outer border (2px)
                if lr < 2 || lr >= label_width_dots - 2 || col < 2 || col >= height_dots - 2 {
                    pixel = true;
                }

                // Per-buffer patterns
                for (i, &(bs, be)) in buf_regions.iter().enumerate() {
                    if col >= bs && col < be {
                        let bh = be - bs;
                        let bw = label_width_dots;
                        let local_col = col - bs;

                        // Buffer top/bottom border
                        if local_col < 2 || local_col >= bh - 2 {
                            pixel = true;
                        }

                        // X cross diagonals
                        if let Some(expected_row_1) = (local_col * bw).checked_div(bh) {
                            if (lr as i32 - expected_row_1 as i32).unsigned_abs() < 2 {
                                pixel = true;
                            }
                            let expected_row_2 = bw - 1 - expected_row_1;
                            if (lr as i32 - expected_row_2 as i32).unsigned_abs() < 2 {
                                pixel = true;
                            }
                        }

                        // Buffer number dots
                        for d in 0..=i as u32 {
                            let dx = 10 + d * 12;
                            let dy: u32 = 10;
                            if lr >= dx && lr < dx + 8 && local_col >= dy && local_col < dy + 8 {
                                pixel = true;
                            }
                        }
                        break;
                    }
                }
            }

            if pixel {
                let byte_idx = col as usize * bytes_per_line as usize + (row / 8) as usize;
                let bit_idx = row % 8; // LSB-first
                buf[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    (buf, canvas_width_dots, height_dots, bytes_per_line)
}

/// Create a test pattern using profile-specific printhead geometry and margins.
///
/// This leaves the existing T-series reference function untouched while
/// allowing E-series test prints to use their 96-dot / 12-byte printhead.
pub fn create_test_pattern_profiled(
    label_width_mm: u32,
    height_mm: u32,
    printhead_dots: u32,
    profile: crate::profile::PrintProfile,
) -> (Vec<u8>, u32, u32, u32) {
    assert!(
        printhead_dots > 0 && printhead_dots.is_multiple_of(8),
        "printhead width {printhead_dots} must be a positive multiple of 8 dots"
    );

    let canvas_width_dots = printhead_dots;
    let height_dots = height_mm * DOTS_PER_MM;
    let bytes_per_line = canvas_width_dots / 8;

    let label_width_dots = (label_width_mm * DOTS_PER_MM).min(canvas_width_dots);
    let x_offset = (canvas_width_dots - label_width_dots) / 2;

    let margin_top = profile.params().margin_dots as u32;
    let margin_bottom = profile.params().margin_dots as u32;
    let max_cols =
        (profile.params().max_buf_data / bytes_per_line as usize) as u32;

    let mut buf_regions: Vec<(u32, u32)> = Vec::new();
    let mut col = margin_top;

    while col < height_dots - margin_bottom {
        let end = (col + max_cols).min(height_dots - margin_bottom);
        buf_regions.push((col, end));
        col = end;
    }

    let mut buf = vec![0u8; bytes_per_line as usize * height_dots as usize];

    for col in 0..height_dots {
        for row in 0..canvas_width_dots {
            let mut pixel = false;

            let label_row = row as i32 - x_offset as i32;

            if label_row >= 0 && (label_row as u32) < label_width_dots {
                let lr = label_row as u32;

                if lr < 2
                    || lr >= label_width_dots - 2
                    || col < 2
                    || col >= height_dots - 2
                {
                    pixel = true;
                }

                for (i, &(bs, be)) in buf_regions.iter().enumerate() {
                    if col >= bs && col < be {
                        let bh = be - bs;
                        let bw = label_width_dots;
                        let local_col = col - bs;

                        if local_col < 2 || local_col >= bh - 2 {
                            pixel = true;
                        }

                        if let Some(expected_row_1) = (local_col * bw).checked_div(bh) {
                            if (lr as i32 - expected_row_1 as i32).unsigned_abs() < 2 {
                                pixel = true;
                            }

                            let expected_row_2 = bw - 1 - expected_row_1;

                            if (lr as i32 - expected_row_2 as i32).unsigned_abs() < 2 {
                                pixel = true;
                            }
                        }

                        for d in 0..=i as u32 {
                            let dx = 10 + d * 12;
                            let dy: u32 = 10;

                            if lr >= dx
                                && lr < dx + 8
                                && local_col >= dy
                                && local_col < dy + 8
                            {
                                pixel = true;
                            }
                        }

                        break;
                    }
                }
            }

            if pixel {
                let byte_idx =
                    col as usize * bytes_per_line as usize + (row / 8) as usize;
                let bit_idx = row % 8;
                buf[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    (buf, canvas_width_dots, height_dots, bytes_per_line)
}

/// Unprinted trailing columns in each swatch band, so neighbouring bands that
/// happen to burn the same shade are still countable.
const SWATCH_SEPARATOR_DOTS: u32 = 4;

/// Build a calibration strip: `steps` solid blocks laid down the feed
/// direction, each to be printed at its own density.
///
/// Bands are equal-width and in order, so a band's position identifies which
/// density produced it — there is no font in this crate to label them with.
///
/// Returns `(image_bytes, height_dots, bytes_per_line, band_cols)`; feed
/// `band_cols` back into a [`DensityBand`](crate::buffer::DensityBand) per step
/// so the buffer split lines up with the ink.
pub fn create_swatch_ladder(
    label_width_mm: u32,
    height_mm: u32,
    steps: u32,
) -> (Vec<u8>, u32, u32, u32) {
    let bytes_per_line = PRINTHEAD_BYTES_PER_LINE;
    let height_dots = height_mm * DOTS_PER_MM;
    let label_width_dots = (label_width_mm * DOTS_PER_MM).min(PRINTHEAD_WIDTH_DOTS);
    let x_offset = (PRINTHEAD_WIDTH_DOTS - label_width_dots) / 2;

    let margin = DEFAULT_MARGIN_DOTS as u32;
    let printable = height_dots.saturating_sub(2 * margin);
    let band_cols = printable / steps.max(1);
    let ink_cols = band_cols.saturating_sub(SWATCH_SEPARATOR_DOTS);

    let mut buf = vec![0u8; bytes_per_line as usize * height_dots as usize];
    for step in 0..steps {
        let start = margin + step * band_cols;
        for col in start..start + ink_cols {
            for dot in x_offset..x_offset + label_width_dots {
                let byte_idx = col as usize * bytes_per_line as usize + (dot / 8) as usize;
                buf[byte_idx] |= 1 << (dot % 8); // LSB-first
            }
        }
    }

    (buf, height_dots, bytes_per_line, band_cols)
}

/// Build an 8bpp grayscale staircase for comparing halftone kernels.
///
/// `steps` bands from white to black down the feed direction. Since the printer
/// is 1bpp, every intermediate tone is whatever the dither makes of it — which
/// is exactly what this is for.
///
/// Returns `(gray, width_dots, height_dots)`, row-major W colorspace.
pub fn create_gray_ramp(label_width_mm: u32, height_mm: u32, steps: u32) -> (Vec<u8>, u32, u32) {
    let width = PRINTHEAD_WIDTH_DOTS;
    let height = height_mm * DOTS_PER_MM;
    let label_width_dots = (label_width_mm * DOTS_PER_MM).min(width);
    let x0 = (width - label_width_dots) / 2;
    let steps = steps.max(2);
    let band = (height / steps).max(1);

    let mut gray = vec![255u8; (width * height) as usize];
    for y in 0..height {
        let step = (y / band).min(steps - 1);
        // White through black, so the top of the label is the lightest tone.
        let level = 255 - (step * 255 / (steps - 1)).min(255);
        let row = (y * width) as usize;
        gray[row + x0 as usize..row + (x0 + label_width_dots) as usize].fill(level as u8);
    }

    (gray, width, height)
}

/// Build **vertical** grey bands — each band a different tone, side by side
/// across the printhead.
///
/// The counterpart to [`create_gray_ramp`], which runs down the feed axis. On
/// two-colour stock the tone reached by a given dot coverage also decides the
/// *colour*: sparse dots cool between firings and develop red, dense ones
/// reinforce each other and go black. Because that is a property of the bitmap
/// rather than the buffer header, it varies freely within a printhead line —
/// unlike heat time or the density trims.
///
/// Returns `(gray, width_dots, height_dots)`, row-major W colorspace.
pub fn create_gray_bands(label_width_mm: u32, height_mm: u32, bands: u32) -> (Vec<u8>, u32, u32) {
    let width = PRINTHEAD_WIDTH_DOTS;
    let height = height_mm * DOTS_PER_MM;
    let label_width_dots = (label_width_mm * DOTS_PER_MM).min(width);
    let x0 = (width - label_width_dots) / 2;
    let bands = bands.max(2);
    let band_dots = (label_width_dots / bands).max(1);

    // Leave the extreme rows blank so the gap sensor still sees clean edges.
    let ink_rows = height / 8..height * 7 / 8;

    let mut gray = vec![255u8; (width * height) as usize];
    for band in 0..bands {
        // White through black across the head.
        let level = (255 - (band * 255 / (bands - 1)).min(255)) as u8;
        let from = x0 + band * band_dots;
        for y in ink_rows.clone() {
            let row = (y * width) as usize;
            gray[row + from as usize..row + (from + band_dots) as usize].fill(level);
        }
    }

    (gray, width, height)
}

/// Which two-colour test card to generate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardPattern {
    /// Horizontal bars: thick red above thin black. Each printhead line holds
    /// one colour, so this works even if the printer only varies energy along
    /// the feed axis.
    Bars,
    /// Vertical stripes alternating red and black across the head. **Every
    /// printhead line contains both colours**, which nothing but genuine
    /// two-plane support can produce — banding by feed-direction energy cannot.
    Stripes,
}

/// Build an RGB test card for two-colour mode.
///
/// [`Stripes`](CardPattern::Stripes) is the decisive one: red and black on the
/// same line is exactly what per-band energy control cannot do, so if the
/// stripes come out in two colours the firmware is genuinely burning the two
/// planes at different trims.
///
/// Returns `(rgb, width_dots, height_dots)`, row-major RGB across the full
/// printhead with the label content centred.
pub fn create_two_colour_pattern(
    label_width_mm: u32,
    height_mm: u32,
    pattern: CardPattern,
) -> (Vec<u8>, u32, u32) {
    match pattern {
        CardPattern::Bars => create_two_colour_card(label_width_mm, height_mm),
        CardPattern::Stripes => create_two_colour_stripes(label_width_mm, height_mm),
    }
}

/// Vertical red/black stripes: four bars across the head, spanning the middle
/// of the label so the gap sensor sees clean leading and trailing edges.
fn create_two_colour_stripes(label_width_mm: u32, height_mm: u32) -> (Vec<u8>, u32, u32) {
    const RED: [u8; 3] = [255, 0, 0];
    const BLACK: [u8; 3] = [0, 0, 0];
    /// Alternating stripes across the head; even ones red, odd ones black.
    const STRIPES: u32 = 4;

    let width = PRINTHEAD_WIDTH_DOTS;
    let height = height_mm * DOTS_PER_MM;
    let label_width_dots = (label_width_mm * DOTS_PER_MM).min(width);
    let x0 = (width - label_width_dots) / 2;
    let stripe_dots = label_width_dots / STRIPES;

    let ink_rows = height / 6..height * 5 / 6;

    let mut rgb = vec![255u8; (width * height * 3) as usize];
    for y in ink_rows {
        for s in 0..STRIPES {
            let colour = if s % 2 == 0 { RED } else { BLACK };
            let from = x0 + s * stripe_dots;
            // Leave a 4-dot unprinted alley so neighbouring stripes can't be
            // merged by lateral heat bleed into looking like one colour.
            for x in from..from + stripe_dots.saturating_sub(4) {
                let px = ((y * width + x) * 3) as usize;
                rgb[px..px + 3].copy_from_slice(&colour);
            }
        }
    }

    (rgb, width, height)
}

/// Build an RGB test card for two-colour mode: a thick red bar above a thin
/// black bar.
///
/// Deliberately asymmetric in *both* colour and thickness, because it has two
/// jobs at once — telling us whether the firmware honours the mode at all, and
/// whether plane 0 is red or black. If the thick bar prints black, the planes
/// are the other way round from what
/// [`twocolor::classify`](crate::twocolor::classify) assumes; if the thin bar
/// lands at the top, the feed direction is inverted.
///
/// Returns `(rgb, width_dots, height_dots)`, row-major RGB across the full
/// printhead with the label content centred.
pub fn create_two_colour_card(label_width_mm: u32, height_mm: u32) -> (Vec<u8>, u32, u32) {
    const WHITE: [u8; 3] = [255, 255, 255];
    const RED: [u8; 3] = [255, 0, 0];
    const BLACK: [u8; 3] = [0, 0, 0];

    let width = PRINTHEAD_WIDTH_DOTS;
    let height = height_mm * DOTS_PER_MM;
    let label_width_dots = (label_width_mm * DOTS_PER_MM).min(width);
    let x0 = (width - label_width_dots) / 2;

    // Thick red across the upper third, thin black in the lower half. The gap
    // between them stays blank so heat bleed can't merge the two.
    let red_rows = height / 10..height * 4 / 10;
    let black_rows = height * 6 / 10..height * 3 / 4;

    let mut rgb = vec![255u8; (width * height * 3) as usize];
    for y in 0..height {
        let colour = if red_rows.contains(&y) {
            RED
        } else if black_rows.contains(&y) {
            BLACK
        } else {
            WHITE
        };
        if colour == WHITE {
            continue;
        }
        for x in x0..x0 + label_width_dots {
            let px = ((y * width + x) * 3) as usize;
            rgb[px..px + 3].copy_from_slice(&colour);
        }
    }

    (rgb, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The ramp must actually span white to black, or it tells us nothing about
    /// the kernel under test.
    #[test]
    fn gray_ramp_spans_the_full_range() {
        let (gray, w, h) = create_gray_ramp(34, 34, 8);
        assert_eq!((w, h), (PRINTHEAD_WIDTH_DOTS, 34 * DOTS_PER_MM));

        let centre = (w / 2) as usize;
        let top = gray[centre];
        let bottom = gray[((h - 1) * w) as usize + centre];
        assert_eq!(top, 255, "first band should be white");
        assert_eq!(bottom, 0, "last band should be black");
    }

    /// Outside the label width the ramp must stay blank, so the printhead
    /// doesn't burn past the media edge.
    #[test]
    fn gray_ramp_leaves_margins_white() {
        let (gray, w, h) = create_gray_ramp(20, 20, 4);
        let last_row = ((h - 1) * w) as usize;
        assert_eq!(gray[last_row], 255, "left margin should be white");
        assert_eq!(gray[last_row + (w - 1) as usize], 255, "right margin");
    }

    #[test]
    fn test_raster_to_column_major_simple() {
        // 8x2 image: first row all black, second row all white
        // MSB-first: 0xFF (row 0), 0x00 (row 1)
        let input = [0xFF, 0x00];
        let (output, cols, bpl) = raster_to_column_major(&input, 8, 2);
        assert_eq!(cols, 2);
        assert_eq!(bpl, 1);
        // Column 0 (y=0): all 8 pixels set -> LSB-first = 0xFF
        assert_eq!(output[0], 0xFF);
        // Column 1 (y=1): all 8 pixels clear -> 0x00
        assert_eq!(output[1], 0x00);
    }

    #[test]
    fn test_center_in_printhead() {
        // 8 dot wide input centered in 24 dot canvas
        let input = vec![0xFF; 2]; // 2 columns, 1 byte each
        let (output, bpl) = center_in_printhead(&input, 2, 8, 24);
        assert_eq!(bpl, 3); // 24/8 = 3 bytes per line
        // 8 dots centered in 24 -> offset = 8 dots = 1 byte
        // Col 0: byte 0 = 0x00, byte 1 = 0xFF, byte 2 = 0x00
        assert_eq!(output[0], 0x00);
        assert_eq!(output[1], 0xFF);
        assert_eq!(output[2], 0x00);
    }

    #[test]
    fn test_create_test_pattern_dimensions() {
        let (data, w, h, bpl) = create_test_pattern(40, 30);
        assert_eq!(w, 384);
        assert_eq!(h, 240);
        assert_eq!(bpl, 48);
        assert_eq!(data.len(), 240 * 48);
    }

    /// A page wider than the head must lose the same amount from both sides,
    /// because the media runs centred under the printhead. Taking the leading
    /// bytes instead drops one edge entirely — a 50 mm label on the T50's
    /// 48 mm head lost 2 mm off the right rather than 1 mm off each side.
    #[test]
    fn oversized_input_is_cropped_symmetrically() {
        // One column, 120 dots wide: set only the outermost dot on each side.
        let input_w = 120u32;
        let head_w = 96u32;
        let bpl = (input_w / 8) as usize;
        let mut input = vec![0u8; bpl];
        let set = |buf: &mut [u8], dot: u32| buf[(dot / 8) as usize] |= 1 << (dot % 8);
        set(&mut input, 0);
        set(&mut input, input_w - 1);
        // ... and one dot just inside the expected crop window on each side.
        let margin = (input_w - head_w) / 2; // 12
        set(&mut input, margin);
        set(&mut input, input_w - 1 - margin);

        let (out, out_bpl) = center_in_printhead(&input, 1, input_w, head_w);
        assert_eq!(out_bpl, head_w / 8);
        let get = |buf: &[u8], dot: u32| (buf[(dot / 8) as usize] >> (dot % 8)) & 1 == 1;

        // The two outermost dots fall outside the window and are dropped.
        // The two just inside it survive, landing at the window's edges.
        assert!(
            get(&out, 0),
            "dot {margin} should map to the first head dot"
        );
        assert!(
            get(&out, head_w - 1),
            "the mirror-side dot should survive too"
        );
        // Nothing else should have been lit.
        let lit = (0..head_w).filter(|d| get(&out, *d)).count();
        assert_eq!(lit, 2, "exactly the two in-window dots should be set");
    }

    /// A head whose width is not a whole number of bytes — the G series is
    /// 190 dots — carries only `190 / 8 * 8 = 184` of them, because that is
    /// all the returned buffer has room for. Walking the nominal 190 wrote
    /// past the end of the last column: silent corruption of the next column
    /// for every column but the last, and an index-out-of-bounds panic on it.
    #[test]
    fn head_width_that_is_not_a_whole_number_of_bytes_stays_in_bounds() {
        const HEAD: u32 = 190;
        const COLS: u32 = 3;
        let in_bpl = HEAD.div_ceil(8) as usize;
        // Light every dot, so any reachable output byte would be written.
        let input = vec![0xffu8; COLS as usize * in_bpl];

        let (out, out_bpl) = center_in_printhead(&input, COLS, HEAD, HEAD);
        assert_eq!(out_bpl, 23, "184 usable dots, not 190");
        assert_eq!(out.len(), COLS as usize * 23);
        // Exactly the usable dots, and no bleed into a neighbouring column.
        assert!(out.iter().all(|&b| b == 0xff));
    }

    /// The same head must also centre a narrower page inside its *usable*
    /// width rather than its nominal one, or the guard in the centring branch
    /// silently eats the dots past 184.
    #[test]
    fn undersized_input_centres_within_the_usable_head_width() {
        const HEAD: u32 = 190;
        let input = vec![0xffu8; 2]; // 16 dots
        let (out, out_bpl) = center_in_printhead(&input, 1, 16, HEAD);
        assert_eq!(out_bpl, 23);
        let lit = (0..23 * 8)
            .filter(|d| (out[d / 8] >> (d % 8)) & 1 == 1)
            .count();
        assert_eq!(lit, 16, "every input dot lands inside the usable width");
    }
}
