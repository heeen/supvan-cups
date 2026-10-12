//! Decode CUPS raster and drive [`KsJob`] via [`ipp_printer_app::RasterDriver`].

use std::io::Cursor;
use std::pin::pin;

use std::sync::LazyLock;

use ipp_printer_app::{
    JobFailure, JobOptions, PrinterConfig, PrinterHandle, PrinterReason, PrinterRecord,
    RasterDriver,
};
use print_raster::reader::cups::unified::CupsRasterUnifiedReader;
use print_raster::reader::{RasterPageReader, RasterReader};
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::job::KsJob;
use crate::printer_device::KsDevice;
use supvan_discover::models;
use supvan_proto::bitmap::DOTS_PER_MM;

/// Smallest label we advertise. Below roughly this the gap sensor can't
/// reliably find the die-cut, so offering smaller would promise more than the
/// hardware delivers.
const MIN_LABEL_MM: i32 = 10;

/// Everything a job needs besides the document: the printer's config and the
/// request's options.
pub struct JobSpec<'a> {
    pub config: &'a PrinterConfig,
    /// Copies from the request; 0 falls back to the raster header's count.
    pub copies: u32,
    /// `print-quality` enum, when the client sent one.
    pub print_quality: Option<u8>,
}

/// Open the job's device, or fail it as OFFLINE so the job is held.
async fn open_job_device(uri: &str) -> Result<KsDevice, JobFailure> {
    crate::device::open_uri(uri)
        .await
        .ok_or_else(|| JobFailure::new(PrinterReason::OFFLINE, format!("cannot open device {uri}")))
}

/// The record a job's [`PrinterHandle`] reads: the printer's config at the
/// darkness this job resolved to.
fn job_record(config: &PrinterConfig, darkness: i32) -> PrinterRecord {
    PrinterRecord::new(PrinterConfig {
        darkness,
        ..config.clone()
    })
}

/// Run a full CUPS raster document through [`KsJob`]. Runs on the caller's
/// tokio runtime (the framework's print worker) — no nested runtime.
pub async fn run_cups_raster_job(spec: &JobSpec<'_>, raster: &[u8]) -> Result<(), JobFailure> {
    let dev = open_job_device(&spec.config.device_uri).await?;

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
        spec.config,
        effective_darkness(
            spec.config.darkness,
            page_next
                .as_ref()
                .map(|p| p.header().v1.cups_compression)
                .unwrap_or(0),
            spec.print_quality,
            *DARKNESS_OVERRIDE,
        ),
    );
    let handle = PrinterHandle { record: &record };

    let mut job: Option<KsJob> = None;
    let mut page_num = 0u32;

    while let Some(mut page) = page_next {
        let h = &page.header().v1;
        let copies = if spec.copies > 0 {
            spec.copies
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

/// How far `print-quality` moves darkness either side of the baseline.
///
/// Darkness 0–100 maps onto the protocol's 0–15 density, so ~6.7 points is one
/// density step; 15 is two — enough to see, small enough that Draft on a light
/// baseline doesn't stop marking altogether.
const QUALITY_DARKNESS_STEP: i32 = 15;

/// The 0–100 darkness scale shared with the config and the raster header.
const DARKNESS_RANGE: std::ops::RangeInclusive<i32> = 0..=100;

/// RFC 8011 `print-quality` values (4 is normal).
const QUALITY_DRAFT: u8 = 3;
const QUALITY_HIGH: u8 = 5;

/// `SUPVAN_DARKNESS`, overriding the configured baseline.
static DARKNESS_OVERRIDE: LazyLock<Option<i32>> = LazyLock::new(|| {
    crate::env::parsed("SUPVAN_DARKNESS", "0-100", |s| {
        s.parse().ok().filter(|v| DARKNESS_RANGE.contains(v))
    })
});

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
/// 2. `SUPVAN_DARKNESS` (`env_override`), else the configured value — the
///    baseline.
/// 3. `print-quality`, applied as a *nudge* to that baseline rather than an
///    absolute. CUPS marks `cupsPrintQuality` Normal by default and converts
///    it on every job, so treating quality as absolute would pin darkness to
///    one of three values and make the baseline unreachable.
fn effective_darkness(
    configured: i32,
    cups_compression: u32,
    quality: Option<u8>,
    env_override: Option<i32>,
) -> i32 {
    // 0 means "not set" in the header, so only 1..=100 counts.
    if let Ok(header) = i32::try_from(cups_compression)
        && header > 0
        && DARKNESS_RANGE.contains(&header)
    {
        log::info!("darkness {header} from the raster header");
        return header;
    }

    let base = env_override.unwrap_or(configured);
    let darkness = match quality {
        Some(QUALITY_DRAFT) => base - QUALITY_DARKNESS_STEP,
        Some(QUALITY_HIGH) => base + QUALITY_DARKNESS_STEP,
        _ => base,
    }
    .clamp(*DARKNESS_RANGE.start(), *DARKNESS_RANGE.end());
    if darkness != base {
        log::info!("darkness {darkness} (baseline {base}, print-quality {quality:?})");
    }
    darkness
}

/// Decode an `image/jpeg` document and print it. Decodes to grayscale,
/// contain-fits it onto the loaded label (aspect preserved, centered, white
/// padding), then drives [`KsJob`]'s existing 8bpp path (dither → device).
///
/// JPEG decode + fit is synchronous; the device transfer is awaited like the
/// raster path. Runs on the caller's tokio runtime (the print worker).
pub async fn run_jpeg_job(
    spec: &JobSpec<'_>,
    media_size_hmm: [i32; 2],
    jpeg: &[u8],
) -> Result<(), JobFailure> {
    let config = spec.config;
    let img = image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)
        .map_err(|e| JobFailure::other(format!("jpeg decode: {e}")))?
        .to_luma8();

    let (canvas, label_w, label_h) = fit_luma(&img, media_size_hmm, config.printhead_width_dots);
    if label_w == 0 || label_h == 0 {
        return Err(JobFailure::other(format!(
            "jpeg: empty label geometry from media_size {media_size_hmm:?}"
        )));
    }

    let dev = open_job_device(&config.device_uri).await?;
    let record = job_record(
        config,
        effective_darkness(config.darkness, 0, spec.print_quality, *DARKNESS_OVERRIDE),
    );
    let handle = PrinterHandle { record: &record };

    // 8bpp grayscale, one byte per pixel; KsJob's 8bpp branch dithers each row.
    let options = JobOptions {
        width: label_w,
        height: label_h,
        bits_per_pixel: 8,
        bytes_per_line: label_w,
        copies: spec.copies.max(1),
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
    let mm = |hmm: i32| hmm.max(0) as u32 / 100;
    let label_w = (mm(media_size_hmm[0]) * DOTS_PER_MM).min(printhead_width_dots);
    let label_h = mm(media_size_hmm[1]) * DOTS_PER_MM;
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
    use image::{GrayImage, Luma};

    /// CUPS marks `cupsPrintQuality` Normal by default and converts it on
    /// every job, so quality has to nudge the baseline rather than replace it
    /// — otherwise SUPVAN_DARKNESS and the configured value are unreachable.
    #[test]
    fn print_quality_nudges_the_baseline() {
        assert_eq!(
            effective_darkness(50, 0, Some(4), None),
            50,
            "normal keeps it"
        );
        assert_eq!(effective_darkness(50, 0, None, None), 50, "absent keeps it");
        assert_eq!(
            effective_darkness(50, 0, Some(3), None),
            35,
            "draft lightens"
        );
        assert_eq!(effective_darkness(50, 0, Some(5), None), 65, "high darkens");
    }

    /// The nudge cannot push darkness outside the protocol's range.
    #[test]
    fn quality_nudges_are_clamped() {
        assert_eq!(effective_darkness(5, 0, Some(3), None), 0);
        assert_eq!(effective_darkness(95, 0, Some(5), None), 100);
    }

    /// An explicit header darkness is a per-job absolute, so quality does not
    /// move it.
    #[test]
    fn header_darkness_ignores_quality() {
        assert_eq!(effective_darkness(50, 80, Some(3), None), 80);
        assert_eq!(effective_darkness(50, 80, Some(5), None), 80);
    }

    /// A raster header that carries darkness wins over the configured value —
    /// the CUPS label-driver convention.
    #[test]
    fn header_darkness_overrides_the_config() {
        assert_eq!(effective_darkness(50, 80, None, None), 80);
        assert_eq!(effective_darkness(50, 1, None, None), 1);
        assert_eq!(effective_darkness(50, 100, None, None), 100);
    }

    /// 0 means "unset" (what CUPS 2.4's driverless PPD emits), and anything
    /// past 100 is not a darkness — both fall through to the config.
    #[test]
    fn an_unset_or_bogus_header_falls_through() {
        assert_eq!(effective_darkness(37, 0, None, None), 37);
        assert_eq!(effective_darkness(37, 101, None, None), 37);
        assert_eq!(effective_darkness(37, u32::MAX, None, None), 37);
    }

    /// `SUPVAN_DARKNESS` replaces the configured baseline; quality still
    /// nudges it, and an explicit header value still wins.
    #[test]
    fn env_override_replaces_the_baseline() {
        assert_eq!(effective_darkness(50, 0, None, Some(20)), 20);
        assert_eq!(effective_darkness(50, 0, Some(5), Some(20)), 35);
        assert_eq!(effective_darkness(50, 80, None, Some(20)), 80);
    }

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
