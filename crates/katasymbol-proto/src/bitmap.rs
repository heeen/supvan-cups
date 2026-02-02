/// T50 Pro: 8 dots/mm, max 48mm printhead.
pub const DPI: u32 = 8;
pub const MAX_WIDTH_MM: u32 = 48;
pub const PRINTHEAD_WIDTH_MM: u32 = 48;

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
pub fn raster_to_column_major(
    input: &[u8],
    width: u32,
    height: u32,
) -> (Vec<u8>, u32, u32) {
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
/// The T50 Pro always uses a 48mm (384 dot) canvas regardless of actual
/// label width. The image content is centered within this canvas.
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

    if input_width_dots >= canvas_width_dots {
        // Input already fills or exceeds canvas - just truncate width
        let mut output = vec![0u8; num_cols as usize * canvas_bytes_per_line];
        for col in 0..num_cols as usize {
            let in_start = col * input_bytes_per_line;
            let out_start = col * canvas_bytes_per_line;
            let copy_len = canvas_bytes_per_line.min(input_bytes_per_line);
            if in_start + copy_len <= input.len() {
                output[out_start..out_start + copy_len]
                    .copy_from_slice(&input[in_start..in_start + copy_len]);
            }
        }
        return (output, canvas_bytes_per_line as u32);
    }

    let x_offset_dots = (canvas_width_dots - input_width_dots) / 2;
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
    let canvas_width_dots = PRINTHEAD_WIDTH_MM * DPI; // 384
    let height_dots = height_mm * DPI;
    let bytes_per_line = canvas_width_dots / 8; // 48
    let label_width_dots = label_width_mm * DPI;
    let x_offset = (canvas_width_dots - label_width_dots) / 2;

    let margin_top: u32 = 8;
    let margin_bottom: u32 = 8;
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
                if lr < 2
                    || lr >= label_width_dots - 2
                    || col < 2
                    || col >= height_dots - 2
                {
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
                        if bh > 0 {
                            let expected_row_1 = (local_col * bw) / bh;
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
                let byte_idx = col as usize * bytes_per_line as usize + (row / 8) as usize;
                let bit_idx = row % 8; // LSB-first
                buf[byte_idx] |= 1 << bit_idx;
            }
        }
    }

    (buf, canvas_width_dots, height_dots, bytes_per_line)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
