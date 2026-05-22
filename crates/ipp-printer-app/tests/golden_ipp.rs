//! Build minimal IPP requests and optional live-server checks.

use std::io::Cursor;

use ipp::model::Operation;
use ipp::prelude::*;
use ipp::request::IppRequestResponse;

#[test]
fn build_get_printer_attributes_request() {
    let uri: Uri = "ipp://localhost:8631/ipp/print/test"
        .parse()
        .expect("uri");
    let req = IppRequestResponse::new(
        IppVersion::v2_0(),
        Operation::GetPrinterAttributes,
        Some(uri),
    )
    .expect("request");

    let bytes = req.to_bytes();
    assert!(bytes.len() > 8);

    // Write fixture for capture_ipp_golden.sh curl path
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/ipp/get-printer-attributes.req.bin");
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&path, &bytes).expect("write fixture");
}

#[test]
fn parse_get_printer_attributes_roundtrip() {
    let uri: Uri = "ipp://localhost:631/ipp/print/x"
        .parse()
        .unwrap();
    let req = IppRequestResponse::new(
        IppVersion::v2_0(),
        Operation::GetPrinterAttributes,
        Some(uri),
    )
    .unwrap();
    let bytes = req.to_bytes();

    let parsed = ipp::parser::IppParser::new(ipp::reader::IppReader::new(Cursor::new(bytes.to_vec())))
        .parse()
        .expect("parse");
    assert_eq!(
        parsed.header().operation_or_status,
        Operation::GetPrinterAttributes as u16
    );
}
