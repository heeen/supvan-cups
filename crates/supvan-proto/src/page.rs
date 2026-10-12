//! Page builders: turn an image into the print buffers
//! [`Printer::print_buffers`](crate::printer::Printer::print_buffers) sends.
//!
//! Pure — no I/O — so the IPP job, the CLI and tests compose pages the same way.

use crate::bitmap::{
    DEFAULT_MARGIN_DOTS, PRINTHEAD_WIDTH_DOTS, PRINTHEAD_WIDTH_MM, center_in_printhead,
    create_swatch_ladder, create_test_pattern, raster_to_column_major,
};
use crate::buffer::{
    ColourMode, Density, DensityBand, PRINT_BUF_SIZE, PageOptions, split_into_banded_buffers,
    split_into_buffers,
};
use crate::dither::{DitherMode, Ditherer};
use crate::error::{Error, Result};
use crate::status::{DEFAULT_LABEL_HEIGHT_MM, MaterialInfo};
use crate::twocolor::{interleave_planes, rgb_to_planes};

pub type PrintBuffer = [u8; PRINT_BUF_SIZE];

/// A column-major 1bpp image laid across the printhead: `cols` feed columns
/// of `bpl` bytes, one bit per head dot.
pub struct Canvas {
    pub data: Vec<u8>,
    pub cols: u16,
    pub bpl: u8,
}

/// Lay a row-major, MSB-first 1bpp raster across a `head_dots`-wide head,
/// centred, one image row per feed column.
pub fn printhead_canvas(mono: &[u8], width: u32, height: u32, head_dots: u32) -> Canvas {
    let (col_data, cols, _) = raster_to_column_major(mono, width, height);
    let (data, bpl) = center_in_printhead(&col_data, cols, width, head_dots);
    Canvas {
        data,
        cols: cols as u16,
        bpl: bpl as u8,
    }
}

/// Buffers for a canvas that already *is* the label — no margin columns are
/// skipped. This is what CUPS hands over (the IPP layer declares zero hard
/// margins); skipping the default 8 shifted labels 1 mm and lost 2 mm of tail.
pub fn full_bleed_buffers(canvas: &Canvas, density: Density) -> Vec<PrintBuffer> {
    split_into_buffers(
        &canvas.data,
        canvas.bpl,
        canvas.cols,
        0,
        0,
        density,
        PageOptions::default(),
    )
}

/// An 8bpp grayscale image (row-major, 0 = black), halftoned with `dither`.
/// The printer has no grayscale mode, so tone is entirely the kernel's doing.
pub fn grayscale(
    gray: &[u8],
    width: u32,
    height: u32,
    density: Density,
    dither: DitherMode,
) -> Result<Vec<PrintBuffer>> {
    let expected = width as usize * height as usize;
    if gray.len() < expected {
        return Err(Error::InvalidParam(format!(
            "grayscale buffer too small: {} < {expected}",
            gray.len()
        )));
    }
    let bpl = width.div_ceil(8) as usize;
    let mut mono = vec![0u8; bpl * height as usize];
    let mut ditherer = Ditherer::new(dither, width);
    for ((y, row), out) in gray
        .chunks_exact(width as usize)
        .enumerate()
        .zip(mono.chunks_exact_mut(bpl))
    {
        ditherer.line(row, y as u32, out);
    }
    let canvas = printhead_canvas(&mono, width, height, PRINTHEAD_WIDTH_DOTS);
    let buffers = full_bleed_buffers(&canvas, density);
    log::info!(
        "grayscale: {width}x{height}, dither={dither:?}, density={density}, {} buffers",
        buffers.len()
    );
    Ok(buffers)
}

/// The built-in test pattern, sized to the loaded label.
pub fn test_pattern(mat: &MaterialInfo, density: Density, page: PageOptions) -> Vec<PrintBuffer> {
    let width_mm = u32::from(mat.width_mm).min(PRINTHEAD_WIDTH_MM);
    let height_mm = u32::from(if mat.height_mm == 0 {
        DEFAULT_LABEL_HEIGHT_MM
    } else {
        mat.height_mm
    });
    log::info!(
        "test print: {width_mm}mm x {height_mm}mm, black={} red={}, save_paper={}",
        density.black,
        density.red,
        page.save_paper
    );
    let (image, _, cols, bpl) = create_test_pattern(width_mm, height_mm);
    split_into_buffers(
        &image,
        bpl as u8,
        cols as u16,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        density,
        page,
    )
}

/// One calibration strip: a solid band per entry in `densities`, down the feed
/// in order, each burned at its own black/red trims. Pair with
/// [`Printer::provision_material`](crate::printer::Printer::provision_material)
/// to vary heat time between strips.
pub fn swatch_ladder(
    label_width_mm: u32,
    height_mm: u32,
    densities: &[Density],
) -> Result<Vec<PrintBuffer>> {
    if densities.is_empty() {
        return Err(Error::InvalidParam("no densities to sweep".into()));
    }
    let steps = densities.len() as u32;
    let (image, _, bpl, band_cols) = create_swatch_ladder(label_width_mm, height_mm, steps);
    if band_cols == 0 {
        return Err(Error::InvalidParam(format!(
            "{height_mm}mm label too short for {steps} bands"
        )));
    }
    let bands: Vec<DensityBand> = densities
        .iter()
        .map(|&density| DensityBand {
            cols: band_cols as u16,
            density,
        })
        .collect();
    log::info!(
        "swatch ladder: {label_width_mm}mm x {height_mm}mm, {steps} bands of {band_cols} cols, \
         densities={densities:?}"
    );
    Ok(split_into_banded_buffers(
        &image,
        bpl as u8,
        &bands,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        PageOptions::default(),
    ))
}

/// An RGB image (row-major, 3 bytes per pixel) in two-colour mode, red and
/// black on one pass. Pixels are sorted into the planes by luminance
/// ([`crate::twocolor::classify`]) — a cut, not a hue test, so quantise to two
/// ink colours first rather than feeding a photograph. `density.red` and
/// `density.black` trim the planes independently.
///
/// Whether a unit honours this is not guaranteed: the vendor's own capability
/// check excludes T50 Pro units whose Bluetooth name contains `A` or `B`, and
/// the shipped app never enables the path at all.
pub fn two_colour(
    rgb: &[u8],
    width: u32,
    height: u32,
    density: Density,
) -> Result<Vec<PrintBuffer>> {
    let expected = width as usize * height as usize * 3;
    if rgb.len() < expected {
        return Err(Error::InvalidParam(format!(
            "RGB buffer too small: {} < {expected}",
            rgb.len()
        )));
    }
    let (red, black, cols, bpl) = rgb_to_planes(rgb, width, height);
    let interleaved = interleave_planes(&red, &black, bpl as usize, cols as usize)
        .ok_or_else(|| Error::InvalidParam("plane interleave failed".into()))?;

    let printable = (cols as u16).saturating_sub(2 * DEFAULT_MARGIN_DOTS);
    if printable == 0 {
        return Err(Error::InvalidParam(format!(
            "{height} rows leaves nothing after margins"
        )));
    }
    let bands = [DensityBand {
        cols: printable,
        density,
    }];
    let buffers = split_into_banded_buffers(
        &interleaved,
        bpl as u8,
        &bands,
        DEFAULT_MARGIN_DOTS,
        DEFAULT_MARGIN_DOTS,
        PageOptions {
            colour: ColourMode::TwoColour,
            ..Default::default()
        },
    );
    log::info!(
        "two-colour: {width}x{height}, {printable} cols, black={} red={}, {} buffers",
        density.black,
        density.red,
        buffers.len()
    );
    Ok(buffers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::DOTS_PER_MM;

    #[test]
    fn full_bleed_canvas_tiles_every_row() {
        // 48 x 80 mm: 640 feed columns, 84 per buffer -> 8 buffers.
        let (w, h) = (48 * DOTS_PER_MM, 80 * DOTS_PER_MM);
        let mono = vec![0xFF; w.div_ceil(8) as usize * h as usize];
        let canvas = printhead_canvas(&mono, w, h, PRINTHEAD_WIDTH_DOTS);
        assert_eq!(canvas.cols, 640);
        let buffers = full_bleed_buffers(&canvas, Density::uniform(4));
        assert_eq!(buffers.len(), 8);
        let printed: u32 = buffers.iter().map(crate::buffer::printed_cols).sum();
        assert_eq!(printed, 640);
    }

    #[test]
    fn grayscale_rejects_a_short_image() {
        let err = grayscale(&[0; 10], 8, 8, Density::uniform(4), DitherMode::Bayer).unwrap_err();
        assert!(matches!(err, Error::InvalidParam(_)));
    }

    #[test]
    fn swatch_ladder_needs_densities() {
        assert!(swatch_ladder(40, 30, &[]).is_err());
        let bufs = swatch_ladder(40, 30, &[Density::uniform(2), Density::uniform(9)]).unwrap();
        assert!(!bufs.is_empty());
    }
}
