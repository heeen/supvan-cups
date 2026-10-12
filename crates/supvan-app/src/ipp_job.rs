//! Decode CUPS raster and drive [`KsJob`] via [`ipp_printer_app::RasterDriver`].

use std::io::Cursor;
use std::pin::pin;

use ipp_printer_app::{JobFailure, JobOptions, PrinterHandle, RasterDriver};
use print_raster::reader::cups::unified::CupsRasterUnifiedReader;
use print_raster::reader::{RasterPageReader, RasterReader};
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::job::KsJob;
use supvan_discover::models;

/// Printhead resolution in dots per millimetre (matches supvan-proto).
const DOTS_PER_MM: i32 = 8;

/// Smallest label we advertise. Below roughly this the gap sensor can't
/// reliably find the die-cut, so offering smaller would promise more than the
/// hardware delivers.
const MIN_LABEL_MM: i32 = 10;

/// Everything a job needs from the printer config and the IPP request, minus
/// the document itself.
pub struct JobSpec<'a> {
    pub printer_name: &'a str,
    pub device_uri: &'a str,
    pub driver_name: &'a str,
    pub darkness: i32,
    pub printhead_width_dots: u32,
    /// Copies from the request; 0 falls back to the raster header's count.
    pub copies: u32,
    /// `print-quality` enum, when the client sent one.
    pub print_quality: Option<u8>,
}

/// Run a full CUPS raster document through [`KsJob`]. Runs on the caller's
/// tokio runtime (the framework's print worker) — no nested runtime.
pub async fn run_cups_raster_job(spec: &JobSpec<'_>, raster: &[u8]) -> Result<(), JobFailure> {
    let JobSpec {
        printer_name,
        device_uri,
        driver_name,
        darkness,
        printhead_width_dots,
        copies: copies_override,
        print_quality,
    } = *spec;
    let dev = crate::device::open_uri(device_uri).await.ok_or_else(|| {
        JobFailure::new(
            ipp_printer_app::PrinterReason::OFFLINE,
            format!("cannot open device {device_uri}"),
        )
    })?;

    let cursor = Cursor::new(raster);
    let pinned = pin!(cursor.compat());
    let reader = CupsRasterUnifiedReader::new(pinned)
        .await
        .map_err(|e| JobFailure::other(format!("cups raster: {e}")))?;

    let mut page_next = reader
        .next_page()
        .await
        .map_err(|e| JobFailure::other(format!("cups raster page: {e}")))?;

    // Darkness can ride in the first page header, so the record waits for it.
    let record = job_record(
        printer_name,
        device_uri,
        driver_name,
        printhead_width_dots,
        effective_darkness(
            darkness,
            page_next
                .as_ref()
                .map(|p| p.header().v1.cups_compression)
                .unwrap_or(0),
            print_quality,
        ),
    );
    let handle = PrinterHandle { record: &record };

    let mut job: Option<KsJob> = None;
    let mut page_num = 0u32;

    while let Some(mut page) = page_next {
        let h = &page.header().v1;
        let copies = if copies_override > 0 {
            copies_override
        } else if h.num_copies < 1 {
            1
        } else {
            h.num_copies
        };
        let options = JobOptions::from_cups_v1(
            h.width,
            h.height,
            h.bits_per_pixel,
            h.bytes_per_line,
            copies,
        );

        if job.is_none() {
            job = Some(RasterDriver::start_job(&handle, &options, &dev)?);
        }

        let state = job.as_mut().unwrap();
        RasterDriver::start_page(state, &options, page_num, &dev)?;

        let bpl = options.bytes_per_line as usize;
        let height = options.height as usize;
        let mut line = vec![0u8; bpl];
        let content = page.content_mut();
        for y in 0..height {
            futures::AsyncReadExt::read_exact(content, &mut line)
                .await
                .map_err(|e| JobFailure::other(format!("raster line {y}: {e}")))?;
            RasterDriver::write_line(state, &options, y as u32, &line)?;
        }

        RasterDriver::end_page(state, &options, page_num, &dev).await?;
        page_num += 1;

        page_next = page
            .next_page()
            .await
            .map_err(|e| JobFailure::other(format!("cups raster next page: {e}")))?;
    }

    if let Some(j) = job.take() {
        RasterDriver::end_job(j, &dev).await;
    }

    Ok(())
}

/// Build the throwaway [`PrinterRecord`] that backs the [`PrinterHandle`] a
/// [`KsJob`] reads (only `darkness` + `printhead_width_dots` matter). Shared by
/// the raster and JPEG paths.
fn job_record(
    printer_name: &str,
    device_uri: &str,
    driver_name: &str,
    printhead_width_dots: u32,
    darkness: i32,
) -> ipp_printer_app::PrinterRecord {
    ipp_printer_app::PrinterRecord::new(ipp_printer_app::PrinterConfig {
        name: printer_name.to_string(),
        display_name: String::new(),
        driver_name: driver_name.to_string(),
        make_and_model: String::new(),
        device_id: String::new(),
        device_uri: device_uri.to_string(),
        dpi: 203,
        printhead_width_dots,
        media_names: vec![],
        media_sizes: vec![],
        // Throwaway record: only darkness and printhead width are read from it.
        media_size_min: [0, 0],
        media_size_max: [0, 0],
        darkness,
        document_formats: vec![],
    })
}

/// Decode an `image/jpeg` document and print it. Decodes to grayscale,
/// contain-fits it onto the loaded label (aspect preserved, centered, white
/// How far `print-quality` moves darkness either side of the baseline.
///
/// Darkness 0–100 maps onto the protocol's 0–15 density, so ~6.7 points is one
/// density step; 15 is two — enough to see, small enough that Draft on a light
/// baseline doesn't stop marking altogether.
const QUALITY_DARKNESS_STEP: i32 = 15;

/// Print darkness for a job, 0–100.
///
/// Three sources, and the order is deliberate:
///
/// 1. `cupsCompression` in the raster header. This is the convention CUPS' own
///    label drivers follow — `rastertolabel.c` scales it as
///    `15 * cupsCompression / 100`, exactly the 0..15 the Supvan protocol
///    takes — so a value here is an explicit per-job darkness and wins
///    outright. CUPS 2.4's driverless PPD hardcodes the field to 0, so nothing
///    sets it today; a custom PPD or a newer CUPS would.
/// 2. `SUPVAN_DARKNESS`, else the configured value — the baseline.
/// 3. `print-quality`, applied as a *nudge* to that baseline rather than an
///    absolute. CUPS marks `cupsPrintQuality` Normal by default and converts
///    it on every job, so treating quality as absolute would pin darkness to
///    one of three values and make the baseline unreachable.
fn effective_darkness(configured: i32, cups_compression: u32, quality: Option<u8>) -> i32 {
    if (1..=100).contains(&cups_compression) {
        log::info!("darkness {cups_compression} from the raster header");
        return cups_compression as i32;
    }

    let base = match std::env::var("SUPVAN_DARKNESS") {
        Ok(raw) => match raw.trim().parse::<i32>() {
            Ok(v) if (0..=100).contains(&v) => v,
            _ => {
                log::warn!("SUPVAN_DARKNESS: expected 0-100, got {raw:?}; using {configured}");
                configured
            }
        },
        Err(_) => configured,
    };

    // RFC 8011: 3 draft, 4 normal, 5 high.
    let darkness = match quality {
        Some(3) => base - QUALITY_DARKNESS_STEP,
        Some(5) => base + QUALITY_DARKNESS_STEP,
        _ => base,
    }
    .clamp(0, 100);
    if darkness != base {
        log::info!("darkness {darkness} (baseline {base}, print-quality {quality:?})");
    }
    darkness
}

/// padding), then drives [`KsJob`]'s existing 8bpp path (dither → device).
///
/// JPEG decode + fit is synchronous; the device transfer is awaited like the
/// raster path. Runs on the caller's tokio runtime (the print worker).
pub async fn run_jpeg_job(
    spec: &JobSpec<'_>,
    media_size_hmm: [i32; 2],
    jpeg: &[u8],
) -> Result<(), JobFailure> {
    let JobSpec {
        printer_name,
        device_uri,
        darkness,
        printhead_width_dots,
        copies,
        print_quality,
        ..
    } = *spec;
    let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .map_err(|e| JobFailure::other(format!("jpeg decode: {e}")))?
        .to_luma8();

    let (canvas, label_w, label_h) = fit_luma(&img, media_size_hmm, printhead_width_dots);
    if label_w == 0 || label_h == 0 {
        return Err(JobFailure::other(format!(
            "jpeg: empty label geometry from media_size {media_size_hmm:?}"
        )));
    }

    let dev = crate::device::open_uri(device_uri).await.ok_or_else(|| {
        JobFailure::new(
            ipp_printer_app::PrinterReason::OFFLINE,
            format!("cannot open device {device_uri}"),
        )
    })?;
    // The throwaway record only feeds KsJob's darkness + printhead width; the
    // driver name is irrelevant on this path.
    let record = job_record(
        printer_name,
        device_uri,
        "",
        printhead_width_dots,
        effective_darkness(darkness, 0, print_quality),
    );
    let handle = PrinterHandle { record: &record };

    // 8bpp grayscale, one byte per pixel; KsJob's 8bpp branch dithers each row.
    let options = JobOptions {
        width: label_w,
        height: label_h,
        bits_per_pixel: 8,
        bytes_per_line: label_w,
        copies: copies.max(1),
    };

    let mut job: KsJob = RasterDriver::start_job(&handle, &options, &dev)?;
    RasterDriver::start_page(&mut job, &options, 0, &dev)?;
    let w = label_w as usize;
    for y in 0..label_h as usize {
        RasterDriver::write_line(&mut job, &options, y as u32, &canvas[y * w..(y + 1) * w])?;
    }
    // end_page transfers `options.copies` times internally — do not loop here.
    RasterDriver::end_page(&mut job, &options, 0, &dev).await?;
    RasterDriver::end_job(job, &dev).await;
    Ok(())
}

/// Contain-fit a grayscale image onto the label canvas: scale preserving aspect
/// to fit inside the label (W×H dots, capped at the printhead width), then
/// center on a white (`0xFF` luma) ground. Returns `(row-major 8bpp canvas,
/// label_w, label_h)`. Pure function — unit-tested.
fn fit_luma(
    img: &image::GrayImage,
    media_size_hmm: [i32; 2],
    printhead_width_dots: u32,
) -> (Vec<u8>, u32, u32) {
    let label_w = ((media_size_hmm[0].max(0) / 100 * DOTS_PER_MM) as u32).min(printhead_width_dots);
    let label_h = (media_size_hmm[1].max(0) / 100 * DOTS_PER_MM) as u32;
    if label_w == 0 || label_h == 0 {
        return (Vec::new(), 0, 0);
    }

    let canvas_len = (label_w * label_h) as usize;
    let (iw, ih) = img.dimensions();
    if iw == 0 || ih == 0 {
        return (vec![0xFF; canvas_len], label_w, label_h);
    }

    let scale = (label_w as f32 / iw as f32).min(label_h as f32 / ih as f32);
    let rw = ((iw as f32 * scale).round() as u32).clamp(1, label_w);
    let rh = ((ih as f32 * scale).round() as u32).clamp(1, label_h);
    let resized = image::imageops::resize(img, rw, rh, image::imageops::FilterType::Triangle);

    let mut canvas = vec![0xFFu8; canvas_len];
    let ox = (label_w - rw) / 2;
    let oy = (label_h - rh) / 2;
    let src = resized.as_raw();
    for y in 0..rh as usize {
        let dst_start = ((oy as usize + y) * label_w as usize) + ox as usize;
        let src_start = y * rw as usize;
        canvas[dst_start..dst_start + rw as usize]
            .copy_from_slice(&src[src_start..src_start + rw as usize]);
    }
    (canvas, label_w, label_h)
}

/// Build printer config from a driver family name.
pub fn config_from_family(
    name: &str,
    info: &str,
    driver: &str,
    uri: &str,
    device_id: &str,
) -> Option<ipp_printer_app::PrinterConfig> {
    let family = models::families()
        .iter()
        .find(|f| f.driver_name.to_string_lossy() == driver)?;
    let make = String::from_utf8_lossy(&family.make_and_model).into_owned();
    let media_names: Vec<String> = family
        .media_names
        .iter()
        .map(|n| n.to_string_lossy().into_owned())
        .collect();
    Some(ipp_printer_app::PrinterConfig {
        name: name.to_string(),
        // Human-readable label the backend built ("Supvan T50 Series <serial>")
        // — surfaced as the DNS-SD instance name, printer-info, and web UI.
        display_name: info.to_string(),
        driver_name: driver.to_string(),
        make_and_model: make,
        device_id: device_id.to_string(),
        device_uri: uri.to_string(),
        dpi: family.dpi,
        printhead_width_dots: family.printhead_width_dots,
        media_names,
        media_sizes: family.media_sizes.clone(),
        // Custom-size bounds, so a print dialog can offer sizes the family
        // table doesn't enumerate. The ceiling is the material record's own
        // clamp (`rfid::MAX_LABEL_*`) rather than a separate number, so the two
        // cannot drift apart and advertise a size the record would truncate.
        media_size_min: [MIN_LABEL_MM * 100, MIN_LABEL_MM * 100],
        media_size_max: [
            supvan_proto::rfid::MAX_LABEL_WIDTH_MM as i32 * 100,
            supvan_proto::rfid::MAX_LABEL_LENGTH_MM as i32 * 100,
        ],
        darkness: 50,
        // We accept PWG/CUPS raster (CUPS' driverless path) and decode
        // image/jpeg ourselves (run_jpeg_job) — the last IPP Everywhere
        // required format.
        document_formats: vec![
            "image/pwg-raster".to_string(),
            "application/vnd.cups-raster".to_string(),
            "application/octet-stream".to_string(),
            "image/jpeg".to_string(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CUPS marks `cupsPrintQuality` Normal by default and converts it on
    /// every job, so quality has to nudge the baseline rather than replace it
    /// — otherwise SUPVAN_DARKNESS and the configured value are unreachable.
    #[test]
    fn print_quality_nudges_the_baseline() {
        assert_eq!(effective_darkness(50, 0, Some(4)), 50, "normal keeps it");
        assert_eq!(effective_darkness(50, 0, None), 50, "absent keeps it");
        assert_eq!(effective_darkness(50, 0, Some(3)), 35, "draft lightens");
        assert_eq!(effective_darkness(50, 0, Some(5)), 65, "high darkens");
    }

    /// The nudge cannot push darkness outside the protocol's range.
    #[test]
    fn quality_nudges_are_clamped() {
        assert_eq!(effective_darkness(5, 0, Some(3)), 0);
        assert_eq!(effective_darkness(95, 0, Some(5)), 100);
    }

    /// An explicit header darkness is a per-job absolute, so quality does not
    /// move it.
    #[test]
    fn header_darkness_ignores_quality() {
        assert_eq!(effective_darkness(50, 80, Some(3)), 80);
        assert_eq!(effective_darkness(50, 80, Some(5)), 80);
    }

    /// A raster header that carries darkness wins over the configured value —
    /// the CUPS label-driver convention.
    #[test]
    fn header_darkness_overrides_the_config() {
        assert_eq!(effective_darkness(50, 80, None), 80);
        assert_eq!(effective_darkness(50, 1, None), 1);
        assert_eq!(effective_darkness(50, 100, None), 100);
    }

    /// 0 means "unset" (what CUPS 2.4's driverless PPD emits), and anything
    /// past 100 is not a darkness — both fall through to the config.
    #[test]
    fn an_unset_or_bogus_header_falls_through() {
        assert_eq!(effective_darkness(37, 0, None), 37);
        assert_eq!(effective_darkness(37, 101, None), 37);
        assert_eq!(effective_darkness(37, u32::MAX, None), 37);
    }
    use image::{GrayImage, Luma};

    #[test]
    fn fit_luma_contains_and_centers() {
        // 10x10 all-black image onto a 40x30mm label (320x240 dots).
        let img = GrayImage::from_pixel(10, 10, Luma([0]));
        let (canvas, w, h) = fit_luma(&img, [4000, 3000], 384);
        assert_eq!((w, h), (320, 240));
        assert_eq!(canvas.len(), 320 * 240);
        // Square image contain-fits to 240x240, centered at x-offset 40.
        assert_eq!(canvas[0], 0xFF, "top-left corner is white padding");
        let center = 120 * 320 + 160;
        assert_eq!(canvas[center], 0, "center is black content");
    }

    #[test]
    fn fit_luma_caps_at_printhead_width() {
        let img = GrayImage::from_pixel(4, 4, Luma([0]));
        // 60mm label = 480 dots, capped to the 384-dot printhead.
        let (_canvas, w, _h) = fit_luma(&img, [6000, 3000], 384);
        assert_eq!(w, 384);
    }

    #[test]
    fn fit_luma_white_image_stays_white() {
        let img = GrayImage::from_pixel(8, 8, Luma([255]));
        let (canvas, _w, _h) = fit_luma(&img, [4000, 3000], 384);
        assert!(canvas.iter().all(|&p| p == 0xFF));
    }

    #[test]
    fn fit_luma_zero_geometry_is_empty() {
        let img = GrayImage::from_pixel(4, 4, Luma([0]));
        let (canvas, w, h) = fit_luma(&img, [0, 0], 384);
        assert!(canvas.is_empty());
        assert_eq!((w, h), (0, 0));
    }
}
