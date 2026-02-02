//! CUPS filter: rastertokatasymbol
//!
//! Reads CUPS raster from stdin, converts to the intermediate binary format
//! that the katasymbol backend reads.
//!
//! Usage (called by CUPS):
//!   rastertokatasymbol job-id user title copies options [filename]
//!
//! Intermediate format per page:
//!   u32 LE  num_buffers
//!   u32 LE  compressed_len
//!   u16 LE  speed
//!   u16 LE  reserved (0)
//!   [compressed_len bytes]

mod raster;

use katasymbol_proto::bitmap;
use katasymbol_proto::buffer;
use katasymbol_proto::compress;
use katasymbol_proto::speed;
use raster::RasterReader;
use std::io::Write;
use std::process;

/// Parse KatasymbolDensity from CUPS options string.
/// Options format: "key=value key2=value2 ..."
fn parse_density(options: &str) -> u8 {
    for opt in options.split_whitespace() {
        if let Some(val) = opt.strip_prefix("KatasymbolDensity=") {
            if let Ok(d) = val.parse::<u8>() {
                return d.min(15);
            }
        }
    }
    4 // default density
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    // CUPS filter args: job-id user title copies options [filename]
    if args.len() < 6 {
        eprintln!(
            "ERROR: Usage: {} job-id user title copies options [filename]",
            args[0]
        );
        return Err("insufficient arguments".into());
    }

    let options = &args[5];
    let density = parse_density(options);

    eprintln!("INFO: katasymbol filter starting, density={density}");

    // Open raster on stdin (fd 0)
    let mut reader = RasterReader::open(0).ok_or("failed to open CUPS raster on stdin")?;

    let stdout = std::io::stdout();
    let mut out = stdout.lock();

    while let Some(header) = reader.read_header() {
        let width = header.cups_width;
        let height = header.cups_height;
        let bpp = header.cups_bits_per_pixel;
        let bpl = header.cups_bytes_per_line;

        eprintln!(
            "INFO: page {}x{}, {}bpp, {} bytes/line, colorspace={}",
            width, height, bpp, bpl, header.cups_color_space
        );

        if bpp != 1 {
            eprintln!("ERROR: expected 1bpp monochrome, got {}bpp", bpp);
            return Err("unsupported bits per pixel".into());
        }

        // Read all scanlines (row-major, MSB-first, 1bpp)
        let total_bytes = bpl as usize * height as usize;
        let mut raster_data = vec![0u8; total_bytes];
        let mut offset = 0;
        for _y in 0..height {
            let n = reader.read_pixels(&mut raster_data[offset..offset + bpl as usize]);
            if n != bpl as usize {
                eprintln!("WARNING: short read at scanline: got {n}, expected {bpl}");
            }
            offset += bpl as usize;
        }

        // Convert: row-major MSB-first -> column-major LSB-first
        let (col_data, num_cols, col_bpl) =
            bitmap::raster_to_column_major(&raster_data, width, height);

        // Center in printhead canvas (always 48mm = 384 dots)
        let canvas_width = bitmap::PRINTHEAD_WIDTH_MM * bitmap::DPI;
        let (canvas_data, canvas_bpl) =
            bitmap::center_in_printhead(&col_data, num_cols, width, canvas_width);

        eprintln!(
            "INFO: rotated: {} cols, {} bytes/line -> canvas {} bytes/line",
            num_cols, col_bpl, canvas_bpl
        );

        // Split into print buffers
        let buffers = buffer::split_into_buffers(
            &canvas_data,
            canvas_bpl as u8,
            num_cols as u16,
            8,  // margin_top
            8,  // margin_bottom
            density,
        );

        eprintln!("INFO: {} print buffers", buffers.len());

        // Compress
        let (compressed, avg) = compress::compress_buffers(&buffers)
            .map_err(|e| format!("compression failed: {e}"))?;
        let print_speed = speed::calc_speed(avg);

        eprintln!(
            "INFO: compressed {} bytes, avg={}/buf, speed={}",
            compressed.len(),
            avg,
            print_speed
        );

        // Write intermediate format header
        let num_buffers = buffers.len() as u32;
        let compressed_len = compressed.len() as u32;
        out.write_all(&num_buffers.to_le_bytes())?;
        out.write_all(&compressed_len.to_le_bytes())?;
        out.write_all(&print_speed.to_le_bytes())?;
        out.write_all(&0u16.to_le_bytes())?; // reserved

        // Write compressed data
        out.write_all(&compressed)?;
    }

    out.flush()?;
    eprintln!("INFO: katasymbol filter done");
    Ok(())
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    if let Err(e) = run() {
        eprintln!("ERROR: {e}");
        process::exit(1);
    }
}
