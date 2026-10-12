//! Subcommands that mark or feed paper.

use supvan_proto::bitmap::{
    CardPattern, PRINTHEAD_WIDTH_MM, create_gray_bands, create_gray_ramp, create_two_colour_pattern,
};
use supvan_proto::buffer::{Density, PageOptions};
use supvan_proto::dither::DitherMode;
use supvan_proto::page;
use supvan_proto::printer::Printer;
use supvan_proto::rfid::{RfidMaterial, heat_presets};
use supvan_proto::status::{DEFAULT_LABEL_GAP_MM, DEFAULT_LABEL_HEIGHT_MM, MaterialInfo};

use crate::CliResult;
use crate::args::{LabelArgs, LabelSize};

pub async fn test_print(printer: &Printer, density: Density, save_paper: bool) -> CliResult {
    // Size the pattern to the loaded label, falling back to the full head.
    let mat = match printer.query_material().await? {
        Some(m) => m,
        None => {
            eprintln!(
                "No material info, using defaults ({PRINTHEAD_WIDTH_MM}mm x {DEFAULT_LABEL_HEIGHT_MM}mm)"
            );
            MaterialInfo {
                width_mm: PRINTHEAD_WIDTH_MM as u8,
                height_mm: DEFAULT_LABEL_HEIGHT_MM,
                gap_mm: DEFAULT_LABEL_GAP_MM,
                ..Default::default()
            }
        }
    };
    eprintln!(
        "Printing test pattern on {}mm x {}mm label...",
        mat.width_mm, mat.height_mm
    );
    let page = PageOptions {
        save_paper,
        ..Default::default()
    };
    printer
        .print_buffers(&page::test_pattern(&mat, density, page))
        .await?;
    eprintln!("Done.");
    Ok(())
}

pub async fn feed(printer: &Printer) -> CliResult {
    printer.paper_skip().await?;
    eprintln!("Fed one label.");
    Ok(())
}

/// A material record for `label`; everything not given keeps the vendor's
/// synthetic-roll defaults.
fn material(label: LabelArgs, heat: (u16, u16)) -> RfidMaterial {
    RfidMaterial {
        width_mm: label.size.width,
        length_mm: label.size.length,
        gap_mm: label.gap,
        heat_time_5: heat.0,
        heat_time_40: heat.1,
        ..Default::default()
    }
}

/// Write the record and confirm the printer took it.
async fn write_material(printer: &Printer, mat: &RfidMaterial) -> CliResult {
    let m = printer.provision_material(mat).await?;
    eprintln!(
        "Provisioned: {}mm x {}mm, gap {}mm, heat {}/{}, UUID {}",
        m.width_mm, m.height_mm, m.gap_mm, mat.heat_time_5, mat.heat_time_40, m.uuid
    );
    Ok(())
}

pub async fn provision(
    printer: &Printer,
    label: LabelArgs,
    heat: Option<(u16, u16)>,
    count: u32,
    mat_type: u8,
    code: u16,
) -> CliResult {
    let mat = RfidMaterial {
        remaining: count,
        mat_type,
        code,
        ..material(label, heat.unwrap_or(heat_presets::STANDARD))
    };
    write_material(printer, &mat).await
}

pub async fn heat_sweep(
    printer: &Printer,
    label: LabelArgs,
    heats: Vec<(u16, u16)>,
    densities: Vec<Density>,
) -> CliResult {
    let heats = if heats.is_empty() {
        vec![
            heat_presets::STANDARD,
            heat_presets::BLACK_MARK,
            heat_presets::CARDSTOCK,
        ]
    } else {
        heats
    };
    let LabelSize { width, length } = label.size;
    eprintln!(
        "Sweeping {} heat profiles x {} densities on {width}mm x {length}mm labels.",
        heats.len(),
        densities.len(),
    );
    eprintln!("Bands run top to bottom in the order printed; annotate each strip as it comes out.");

    let listed = densities
        .iter()
        .map(Density::to_string)
        .collect::<Vec<_>>()
        .join(",");
    for (i, &heat) in heats.iter().enumerate() {
        eprintln!(
            "\nStrip {}/{}: heat5={} heat40={}, densities {listed}",
            i + 1,
            heats.len(),
            heat.0,
            heat.1,
        );
        write_material(printer, &material(label, heat)).await?;
        let strip = page::swatch_ladder(u32::from(width), u32::from(length), &densities)?;
        printer.print_buffers(&strip).await?;
    }
    eprintln!("\nSweep complete: {} strips.", heats.len());
    Ok(())
}

pub async fn gray_ramp(
    printer: &Printer,
    label: LabelSize,
    dither: DitherMode,
    steps: u32,
    density: Density,
    vertical: bool,
) -> CliResult {
    let LabelSize { width, length } = label;
    let (gray, w, h, layout) = if vertical {
        let (g, w, h) = create_gray_bands(width.into(), length.into(), steps);
        (g, w, h, "vertical bands across the head")
    } else {
        let (g, w, h) = create_gray_ramp(width.into(), length.into(), steps);
        (g, w, h, "steps down the feed")
    };
    eprintln!(
        "Grey ramp on {width}mm x {length}mm: {steps} {layout}, white->black, \
         dither={dither:?}, density={density}."
    );
    printer
        .print_buffers(&page::grayscale(&gray, w, h, density, dither)?)
        .await?;
    eprintln!("Done.");
    Ok(())
}

pub async fn two_color(
    printer: &Printer,
    label: LabelSize,
    density: Density,
    pattern: CardPattern,
) -> CliResult {
    let LabelSize { width, length } = label;
    let (rgb, w, h) = create_two_colour_pattern(width.into(), length.into(), pattern);
    let Density { black, red } = density;
    match pattern {
        CardPattern::Bars => eprintln!(
            "Bars on {width}mm x {length}mm: thick RED above thin BLACK, black={black} red={red}."
        ),
        CardPattern::Stripes => {
            eprintln!(
                "Stripes on {width}mm x {length}mm: 4 vertical bars, RED BLACK RED BLACK, \
                 black={black} red={red}."
            );
            eprintln!(
                "Both colours share every printhead line — only real two-plane support can do this."
            );
        }
    }
    printer
        .print_buffers(&page::two_colour(&rgb, w, h, density)?)
        .await?;
    eprintln!("Done.");
    Ok(())
}
