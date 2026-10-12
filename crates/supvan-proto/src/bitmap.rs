/// T50 Pro: 8 dots/mm, 48mm printhead.
pub const DOTS_PER_MM: u32 = 8;
pub const PRINTHEAD_WIDTH_MM: u32 = 48;
pub const PRINTHEAD_WIDTH_DOTS: u32 = PRINTHEAD_WIDTH_MM * DOTS_PER_MM;
pub const PRINTHEAD_BYTES_PER_LINE: u32 = PRINTHEAD_WIDTH_DOTS / 8;
pub const DEFAULT_MARGIN_DOTS: u16 = 8;

/// A column-major, LSB-first 1bpp image — the printer's layout: `cols` feed
/// columns of `bpl` bytes, bit `d` of a column being printhead dot `d`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Canvas {
    pub data: Vec<u8>,
    pub cols: u16,
    pub bpl: u8,
}

/// Set dot `dot` of feed column `col` in a column-major LSB-first image.
fn set_dot(buf: &mut [u8], bpl: usize, col: usize, dot: usize) {
    buf[col * bpl + dot / 8] |= 1 << (dot % 8);
}

/// Where a label sits on the printhead: the full head width, the label's own
/// width centred within it, and its length in feed dots.
struct LabelGeom {
    head: u32,
    label: u32,
    x0: u32,
    height: u32,
}

impl LabelGeom {
    fn new(label_width_mm: u32, height_mm: u32) -> Self {
        let label = (label_width_mm * DOTS_PER_MM).min(PRINTHEAD_WIDTH_DOTS);
        Self {
            head: PRINTHEAD_WIDTH_DOTS,
            label,
            x0: (PRINTHEAD_WIDTH_DOTS - label) / 2,
            height: height_mm * DOTS_PER_MM,
        }
    }

    /// The label's span of head dots.
    fn dots(&self) -> std::ops::Range<u32> {
        self.x0..self.x0 + self.label
    }
}

/// Repack a row-major, MSB-first 1bpp raster (the CUPS/image convention) into
/// the printer's column-major, LSB-first layout.
///
/// No pixels move: raster row `y` is already feed column `y`, and pixel `x`
/// of a row is printhead dot `x`. Only the bit order within each byte flips —
/// plus the padding bits past `width` are cleared, since they'd now sit on
/// real dots.
pub fn raster_to_column_major(input: &[u8], width: u32, height: u32) -> Canvas {
    let bpl = width.div_ceil(8) as usize;
    let mut data = vec![0u8; bpl * height as usize];
    if bpl > 0 {
        let tail_mask = match width % 8 {
            0 => 0xFF,
            used => (1u8 << used) - 1,
        };
        for (out, row) in data.chunks_exact_mut(bpl).zip(input.chunks(bpl)) {
            out.iter_mut()
                .zip(row)
                .for_each(|(o, &i)| *o = i.reverse_bits());
            out[bpl - 1] &= tail_mask;
        }
    }
    Canvas {
        data,
        cols: height as u16,
        bpl: bpl as u8,
    }
}

/// Place a `width_dots`-wide image centred on a `head_dots`-wide printhead.
///
/// The printhead is a fixed physical bar (384 dots / 48 mm on the T50) and the
/// media runs centred under it, so a page always goes in the middle of a
/// head-width canvas whatever the label's own width.
///
/// A page *wider* than the head is cropped symmetrically, not padded. That is
/// reachable in normal operation — the T50 family advertises 50 mm media on a
/// 48 mm head — and CUPS renders the full media width because the IPP layer
/// declares zero hard margins on all four sides.
pub fn center_in_printhead(input: &Canvas, width_dots: u32, head_dots: u32) -> Canvas {
    let bpl = (head_dots / 8) as usize;
    // A head whose width isn't a whole number of bytes (the G series is 190
    // dots) can only carry `bpl * 8`; measure against that, not the nominal
    // width, which would walk off the end of the last column.
    let usable = (bpl * 8) as i64;
    let width = i64::from(width_dots);
    if width > usable {
        let lost = width - usable;
        log::warn!(
            "center_in_printhead: image is {width_dots} dots but the head can carry {usable} \
             (nominal {head_dots}); cropping {lost} dots ({} left, {} right)",
            lost / 2,
            lost - lost / 2
        );
    }
    // Head dot `d` shows image dot `d - offset`: negative offset crops, positive pads.
    let offset = (usable - width) / 2;
    let in_bpl = usize::from(input.bpl);
    let mut data = vec![0u8; usize::from(input.cols) * bpl];
    for col in 0..usize::from(input.cols) {
        for dot in 0..usable {
            let src = dot - offset;
            if !(0..width).contains(&src) {
                continue;
            }
            let src = src as usize;
            let Some(&byte) = input.data.get(col * in_bpl + src / 8) else {
                continue;
            };
            if byte >> (src % 8) & 1 != 0 {
                set_dot(&mut data, bpl, col, dot as usize);
            }
        }
    }
    Canvas {
        data,
        cols: input.cols,
        bpl: bpl as u8,
    }
}

/// Create a test pattern matching the Python reference implementation.
///
/// A border, plus per print buffer an X and as many dots as its index, so a
/// dropped or misplaced buffer is obvious on paper.
pub fn create_test_pattern(label_width_mm: u32, height_mm: u32) -> Canvas {
    let geom = LabelGeom::new(label_width_mm, height_mm);
    let height_dots = geom.height;
    let bytes_per_line = PRINTHEAD_BYTES_PER_LINE;
    let label_width_dots = geom.label;

    let margin_top = DEFAULT_MARGIN_DOTS as u32;
    let margin_bottom = DEFAULT_MARGIN_DOTS as u32;
    let max_cols = (crate::buffer::MAX_BUF_DATA / bytes_per_line as usize) as u32;

    let mut buf_regions: Vec<(u32, u32)> = Vec::new();
    let mut col = margin_top;
    while col < height_dots - margin_bottom {
        let end = (col + max_cols).min(height_dots - margin_bottom);
        buf_regions.push((col, end));
        col = end;
    }

    let mut buf = vec![0u8; bytes_per_line as usize * height_dots as usize];

    for col in 0..height_dots {
        for row in geom.dots() {
            let mut pixel = false;
            let lr = row - geom.x0;

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

            if pixel {
                set_dot(
                    &mut buf,
                    bytes_per_line as usize,
                    col as usize,
                    row as usize,
                );
            }
        }
    }

    Canvas {
        data: buf,
        cols: height_dots as u16,
        bpl: bytes_per_line as u8,
    }
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
/// Returns the image and `band_cols`; feed `band_cols` back into a
/// [`DensityBand`](crate::buffer::DensityBand) per step so the buffer split
/// lines up with the ink.
pub(crate) fn create_swatch_ladder(
    label_width_mm: u32,
    height_mm: u32,
    steps: u32,
) -> (Canvas, u32) {
    let geom = LabelGeom::new(label_width_mm, height_mm);
    let bpl = PRINTHEAD_BYTES_PER_LINE as usize;
    let margin = DEFAULT_MARGIN_DOTS as u32;
    let band_cols = geom.height.saturating_sub(2 * margin) / steps.max(1);
    let ink_cols = band_cols.saturating_sub(SWATCH_SEPARATOR_DOTS);

    let mut data = vec![0u8; bpl * geom.height as usize];
    for step in 0..steps {
        let start = margin + step * band_cols;
        for col in start..start + ink_cols {
            for dot in geom.dots() {
                set_dot(&mut data, bpl, col as usize, dot as usize);
            }
        }
    }
    let canvas = Canvas {
        data,
        cols: geom.height as u16,
        bpl: bpl as u8,
    };
    (canvas, band_cols)
}

/// Build an 8bpp grayscale staircase for comparing halftone kernels.
///
/// `steps` bands from white to black down the feed direction. Since the printer
/// is 1bpp, every intermediate tone is whatever the dither makes of it — which
/// is exactly what this is for.
///
/// Returns `(gray, width_dots, height_dots)`, row-major W colorspace.
pub fn create_gray_ramp(label_width_mm: u32, height_mm: u32, steps: u32) -> (Vec<u8>, u32, u32) {
    let LabelGeom {
        head: width,
        label: label_width_dots,
        x0,
        height,
    } = LabelGeom::new(label_width_mm, height_mm);
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
    let LabelGeom {
        head: width,
        label: label_width_dots,
        x0,
        height,
    } = LabelGeom::new(label_width_mm, height_mm);
    let bands = bands.max(2);
    let band_dots = (label_width_dots / bands).max(1);

    let mut gray = vec![255u8; (width * height) as usize];
    for band in 0..bands {
        // White through black across the head, the full label length.
        let level = (255 - (band * 255 / (bands - 1)).min(255)) as u8;
        let from = x0 + band * band_dots;
        for y in 0..height {
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

    let LabelGeom {
        head: width,
        label: label_width_dots,
        x0,
        height,
    } = LabelGeom::new(label_width_mm, height_mm);
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
fn create_two_colour_card(label_width_mm: u32, height_mm: u32) -> (Vec<u8>, u32, u32) {
    const WHITE: [u8; 3] = [255, 255, 255];
    const RED: [u8; 3] = [255, 0, 0];
    const BLACK: [u8; 3] = [0, 0, 0];

    let LabelGeom {
        head: width,
        label: label_width_dots,
        x0,
        height,
    } = LabelGeom::new(label_width_mm, height_mm);

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

    fn canvas(data: Vec<u8>, cols: u16, width_dots: u32) -> Canvas {
        Canvas {
            data,
            cols,
            bpl: width_dots.div_ceil(8) as u8,
        }
    }

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
        let out = raster_to_column_major(&input, 8, 2);
        assert_eq!((out.cols, out.bpl), (2, 1));
        assert_eq!(out.data, [0xFF, 0x00]);
    }

    /// MSB-first to LSB-first is a bit reversal; padding past the width is
    /// cleared because it would now land on real dots.
    #[test]
    fn raster_bits_reverse_and_padding_is_cleared() {
        // 10 dots wide: dot 0 and dot 9 set, plus junk in the 6 padding bits.
        let input = [0b1000_0000, 0b0111_1111];
        let out = raster_to_column_major(&input, 10, 1);
        assert_eq!(out.data, [0b0000_0001, 0b0000_0010]);
    }

    #[test]
    fn test_center_in_printhead() {
        // 8 dot wide input centered in 24 dot canvas
        let input = canvas(vec![0xFF; 2], 2, 8);
        let out = center_in_printhead(&input, 8, 24);
        assert_eq!(out.bpl, 3);
        // 8 dots centred in 24: one byte of padding either side.
        assert_eq!(out.data[..3], [0x00, 0xFF, 0x00]);
    }

    #[test]
    fn test_create_test_pattern_dimensions() {
        let pattern = create_test_pattern(40, 30);
        assert_eq!((pattern.cols, pattern.bpl), (240, 48));
        assert_eq!(pattern.data.len(), 240 * 48);
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

        let out = center_in_printhead(&canvas(input, 1, input_w), input_w, head_w);
        assert_eq!(u32::from(out.bpl), head_w / 8);
        let out = out.data;
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

        let out = center_in_printhead(&canvas(input, COLS as u16, HEAD), HEAD, HEAD);
        assert_eq!(out.bpl, 23, "184 usable dots, not 190");
        let out = out.data;
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
        let out = center_in_printhead(&canvas(input, 1, 16), 16, HEAD);
        assert_eq!(out.bpl, 23);
        let out = out.data;
        let lit = (0..23 * 8)
            .filter(|d| (out[d / 8] >> (d % 8)) & 1 == 1)
            .count();
        assert_eq!(lit, 16, "every input dot lands inside the usable width");
    }
}
