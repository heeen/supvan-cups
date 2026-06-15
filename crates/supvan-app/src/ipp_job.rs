//! Decode CUPS raster and drive [`KsJob`] via [`ipp_printer_app::RasterDriver`].

use std::io::Cursor;
use std::pin::pin;

use print_raster::reader::cups::unified::CupsRasterUnifiedReader;
use print_raster::reader::{RasterPageReader, RasterReader};
use ipp_printer_app::{JobFailure, JobOptions, PrinterHandle, RasterDriver};
use tokio_util::compat::TokioAsyncReadCompatExt;

use crate::job::KsJob;
use crate::models;
use crate::printer_device::KsDevice;

/// Run a full CUPS raster document through [`KsJob`].
pub fn run_cups_raster_job(
    printer_name: &str,
    device_uri: &str,
    darkness: i32,
    printhead_width_dots: u32,
    driver_name: &str,
    raster: &[u8],
    copies_override: u32,
) -> Result<(), JobFailure> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| JobFailure::other(format!("tokio runtime: {e}")))?;

    rt.block_on(async {
        run_cups_raster_job_async(
            printer_name,
            device_uri,
            darkness,
            printhead_width_dots,
            driver_name,
            raster,
            copies_override,
        )
        .await
    })
}

async fn run_cups_raster_job_async(
    printer_name: &str,
    device_uri: &str,
    darkness: i32,
    printhead_width_dots: u32,
    driver_name: &str,
    raster: &[u8],
    copies_override: u32,
) -> Result<(), JobFailure> {
    let dev = open_device(device_uri).ok_or_else(|| {
        JobFailure::new(
            ipp_printer_app::PrinterReason::OFFLINE,
            format!("cannot open device {device_uri}"),
        )
    })?;

    let record = ipp_printer_app::PrinterRecord::new(ipp_printer_app::PrinterConfig {
        name: printer_name.to_string(),
        driver_name: driver_name.to_string(),
        make_and_model: String::new(),
        device_id: String::new(),
        device_uri: device_uri.to_string(),
        dpi: 203,
        printhead_width_dots,
        media_names: vec![],
        media_sizes: vec![],
        darkness,
    });
    let handle = PrinterHandle {
        record: &record,
    };

    let cursor = Cursor::new(raster);
    let pinned = pin!(cursor.compat());
    let reader = CupsRasterUnifiedReader::new(pinned)
        .await
        .map_err(|e| JobFailure::other(format!("cups raster: {e}")))?;

    let mut page_next = reader
        .next_page()
        .await
        .map_err(|e| JobFailure::other(format!("cups raster page: {e}")))?;

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

        RasterDriver::end_page(state, &options, page_num, &dev)?;
        page_num += 1;

        page_next = page
            .next_page()
            .await
            .map_err(|e| JobFailure::other(format!("cups raster next page: {e}")))?;
    }

    if let Some(j) = job.take() {
        RasterDriver::end_job(j, &dev);
    }

    Ok(())
}

fn open_device(uri: &str) -> Option<KsDevice> {
    if uri.starts_with("btrfcomm://") {
        crate::device::open_bt(uri).map(|b| *b)
    } else if uri.starts_with("usbhid://") {
        crate::device::open_usb(uri)
    } else if uri.starts_with("mock://") {
        crate::device::open_mock(uri)
    } else {
        None
    }
}

/// Build printer config from a driver family name.
pub fn config_from_family(
    name: &str,
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
        driver_name: driver.to_string(),
        make_and_model: make,
        device_id: device_id.to_string(),
        device_uri: uri.to_string(),
        dpi: family.dpi,
        printhead_width_dots: family.printhead_width_dots,
        media_names,
        media_sizes: family.media_sizes.clone(),
        darkness: 50,
    })
}
