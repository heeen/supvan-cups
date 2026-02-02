//! CUPS backend print job handler.
//!
//! Reads intermediate format from stdin (produced by rastertokatasymbol filter),
//! connects to the printer via RFCOMM, and executes the print flow.

use katasymbol_proto::printer::Printer;
use katasymbol_proto::rfcomm::RfcommSocket;
use std::io::Read;

/// Parse Bluetooth address from DEVICE_URI environment variable.
///
/// Expected format: `katasymbol://XX:XX:XX:XX:XX:XX`
pub fn parse_device_uri(uri: &str) -> Option<String> {
    uri.strip_prefix("katasymbol://").map(String::from)
}

/// Intermediate format page header (12 bytes).
struct PageHeader {
    num_buffers: u32,
    compressed_len: u32,
    speed: u16,
}

fn read_page_header<R: Read>(reader: &mut R) -> std::io::Result<Option<PageHeader>> {
    let mut buf = [0u8; 12];
    match reader.read_exact(&mut buf) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    Ok(Some(PageHeader {
        num_buffers: u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]),
        compressed_len: u32::from_le_bytes([buf[4], buf[5], buf[6], buf[7]]),
        speed: u16::from_le_bytes([buf[8], buf[9]]),
    }))
}

/// Execute a print job: read intermediate format from stdin, send to printer.
pub fn run_print_job() -> Result<(), Box<dyn std::error::Error>> {
    // Get device URI from environment
    let device_uri = std::env::var("DEVICE_URI")
        .map_err(|_| "DEVICE_URI not set")?;
    let bt_addr = parse_device_uri(&device_uri)
        .ok_or_else(|| format!("invalid DEVICE_URI: {device_uri}"))?;

    log::info!("connecting to {bt_addr}");
    eprintln!("STATE: +connecting-to-device");

    let sock = RfcommSocket::connect_default(&bt_addr)
        .map_err(|e| format!("RFCOMM connect failed: {e}"))?;
    let printer = Printer::new(sock);

    eprintln!("STATE: -connecting-to-device");

    // Query material for supply level reporting
    if let Ok(Some(mat)) = printer.query_material() {
        log::info!(
            "label: {}mm x {}mm, type={}, gap={}mm",
            mat.width_mm,
            mat.height_mm,
            mat.label_type,
            mat.gap_mm
        );
        if let Some(remaining) = mat.remaining {
            // Report supply level to CUPS
            // Approximate percentage (assume 300 label roll)
            let pct = ((remaining as f32 / 300.0) * 100.0).min(100.0) as u32;
            eprintln!("ATTR: marker-names=\"Labels\"");
            eprintln!("ATTR: marker-types=\"labels\"");
            eprintln!("ATTR: marker-levels={pct}");
        }
    }

    // Read pages from stdin (intermediate format from filter)
    let mut stdin = std::io::stdin().lock();
    let mut page_num = 0u32;

    while let Some(header) = read_page_header(&mut stdin)? {
        page_num += 1;
        log::info!(
            "page {}: {} buffers, {} compressed bytes, speed={}",
            page_num,
            header.num_buffers,
            header.compressed_len,
            header.speed
        );
        eprintln!("PAGE: {page_num} 1");

        // Read compressed data
        let mut compressed = vec![0u8; header.compressed_len as usize];
        stdin.read_exact(&mut compressed)?;

        // Execute print
        printer.print_compressed(&compressed, header.speed)
            .map_err(|e| format!("print failed: {e}"))?;
    }

    if page_num == 0 {
        log::warn!("no pages received from filter");
    } else {
        log::info!("{page_num} page(s) printed");
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_device_uri() {
        assert_eq!(
            parse_device_uri("katasymbol://A4:93:40:A0:87:57"),
            Some("A4:93:40:A0:87:57".into())
        );
        assert_eq!(parse_device_uri("other://foo"), None);
        assert_eq!(parse_device_uri("katasymbol://"), Some("".into()));
    }
}
