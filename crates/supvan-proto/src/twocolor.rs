//! Two-colour (red/black) raster encoding.
//!
//! On two-colour thermal stock the printer develops a different colour depending
//! on burn energy, and the protocol carries that as **two interleaved bitplanes**
//! rather than one — each plane burned at its own trim from
//! [`Density`](crate::buffer::Density). Unlike the per-band density in
//! [`buffer::split_into_banded_buffers`](crate::buffer::split_into_banded_buffers),
//! which only varies energy along the feed axis, this selects colour **per dot**.
//!
//! Recovered from `ImgConverter.GetBytes(int, int, int, List<Color>)` in the
//! vendor Android app, which switches on the colour list having 2 entries.
//!
//! Note the vendor gates this off in the shipped build: `PrintPageData.colors`
//! is read but never populated, and `DeviceManager.isTwoColorDevice()` excludes
//! T50 Pro units whose Bluetooth name contains `A` or `B`. The firmware support
//! is real; whether a given unit honours it is an empirical question.

use crate::bitmap::Canvas;

/// Luminance at or above which a pixel is blank — the vendor's `threshold`,
/// default 125.
pub(crate) const BLANK_THRESHOLD: u8 = 125;

/// Luminance at or below which an inked pixel burns black rather than red.
///
/// Placed so that pure red — luma `255 * 0.3 = 76` — lands on the red side.
/// It is a luminance cut, not a hue test: any mid-grey in the 48..125 window
/// becomes red, so callers should quantise to two ink colours before encoding
/// rather than relying on this to separate an arbitrary image.
pub(crate) const BLACK_THRESHOLD: u8 = 48;

/// Which bitplane an inked pixel belongs to.
///
/// Plane order on the wire is red first, then black, matching the vendor's
/// `luma > 48` branch writing to the even (plane 0) slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Plane {
    Red = 0,
    Black = 1,
}

/// The vendor's luma: `0.3 R + 0.59 G + 0.11 B`, truncated.
///
/// Deliberately not Rec. 601/709 — these are the coefficients the firmware's
/// threshold pair was chosen against, so changing them would move where pure
/// red falls relative to [`BLACK_THRESHOLD`].
pub(crate) fn luma(r: u8, g: u8, b: u8) -> u8 {
    let y = (r as f32) * 0.3 + (g as f32) * 0.59 + (b as f32) * 0.11;
    y as u8
}

/// Classify a pixel by luminance. `None` means no ink.
pub(crate) fn classify(luma: u8) -> Option<Plane> {
    if luma >= BLANK_THRESHOLD {
        None
    } else if luma > BLACK_THRESHOLD {
        Some(Plane::Red)
    } else {
        Some(Plane::Black)
    }
}

/// Encode an RGB image (row-major, 3 bytes per pixel) in the two-colour wire
/// layout: one printed column per image row, each column's red line followed
/// by its black line, both column-major LSB-first like the mono path. The
/// printer sees `2 * cols` lines, which is why the buffer header advertises a
/// doubled column count; `cols` here counts printed columns.
pub(crate) fn rgb_to_interleaved(rgb: &[u8], width: u32, height: u32) -> Canvas {
    let bpl = width.div_ceil(8) as usize;
    let mut data = vec![0u8; 2 * bpl * height as usize];
    for (y, row) in rgb
        .chunks_exact(width as usize * 3)
        .take(height as usize)
        .enumerate()
    {
        for (x, &[r, g, b]) in row.as_chunks::<3>().0.iter().enumerate() {
            let Some(plane) = classify(luma(r, g, b)) else {
                continue;
            };
            let line = 2 * y + plane as usize;
            data[line * bpl + x / 8] |= 1 << (x % 8);
        }
    }
    Canvas {
        data,
        cols: height as u16,
        bpl: bpl as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The red and black lines of printed column `col`.
    fn planes(c: &Canvas, col: usize) -> (&[u8], &[u8]) {
        let bpl = usize::from(c.bpl);
        let start = 2 * col * bpl;
        (
            &c.data[start..start + bpl],
            &c.data[start + bpl..start + 2 * bpl],
        )
    }

    /// The whole reason BLACK_THRESHOLD is 48: pure red must not be classified
    /// as black. If the luma coefficients ever drift, this is what catches it.
    #[test]
    fn pure_red_is_red() {
        assert_eq!(luma(255, 0, 0), 76);
        assert_eq!(classify(luma(255, 0, 0)), Some(Plane::Red));
    }

    #[test]
    fn pure_black_and_white_classify() {
        assert_eq!(classify(luma(0, 0, 0)), Some(Plane::Black));
        assert_eq!(classify(luma(255, 255, 255)), None);
    }

    /// A luminance cut, not a hue test — mid-grey lands on red. Callers must
    /// quantise first; this pins the caveat so it can't be forgotten.
    #[test]
    fn midgrey_lands_on_red() {
        assert_eq!(classify(luma(100, 100, 100)), Some(Plane::Red));
    }

    #[test]
    fn threshold_boundaries() {
        assert_eq!(classify(BLANK_THRESHOLD), None);
        assert_eq!(classify(BLANK_THRESHOLD - 1), Some(Plane::Red));
        assert_eq!(classify(BLACK_THRESHOLD + 1), Some(Plane::Red));
        assert_eq!(classify(BLACK_THRESHOLD), Some(Plane::Black));
    }

    /// End-to-end on the real test card: the thick bar must be red and the thin
    /// bar black, in different planes. This is the encoder half of the hardware
    /// check — if it passes and the label still comes out wrong, the fault is
    /// the firmware's plane order, not ours.
    #[test]
    fn test_card_puts_thick_bar_in_red_plane() {
        let (rgb, w, h) =
            crate::bitmap::create_two_colour_pattern(34, 34, crate::bitmap::CardPattern::Bars);
        let c = rgb_to_interleaved(&rgb, w, h);
        assert_eq!((u32::from(c.cols), u32::from(c.bpl)), (h, w / 8));

        let ink = |line: &[u8]| line.iter().map(|b| b.count_ones() as usize).sum::<usize>();
        let (red_dots, black_dots) = (0..usize::from(c.cols))
            .map(|col| planes(&c, col))
            .fold((0, 0), |(r, b), (red, black)| {
                (r + ink(red), b + ink(black))
            });
        assert!(red_dots > 0 && black_dots > 0);
        // The red bar is 3/10 of the height, the black bar 3/20 — twice as thick.
        assert!(
            red_dots > black_dots,
            "red {red_dots} should exceed black {black_dots}"
        );
    }

    /// Red and black pixels must land in different planes at the same bit
    /// position — that co-location is what makes them one dot in two colours —
    /// with the red line first on the wire.
    #[test]
    fn rgb_splits_colours_into_red_then_black_lines() {
        // 8x1: red at x=0, black at x=1, white elsewhere.
        let mut rgb = vec![255u8; 8 * 3];
        rgb[0..3].copy_from_slice(&[255, 0, 0]);
        rgb[3..6].copy_from_slice(&[0, 0, 0]);

        let c = rgb_to_interleaved(&rgb, 8, 1);
        assert_eq!((c.cols, c.bpl), (1, 1));
        assert_eq!(c.data, [0b0000_0001, 0b0000_0010]);
    }
}
