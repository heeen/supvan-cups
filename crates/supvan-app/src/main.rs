//! Supvan IPP Everywhere printer application.
//!
//! Wires the `ipp-printer-app` framework (the IPP/HTTP server + job model) to
//! the Supvan device layer (`supvan-proto`): discovers USB + Bluetooth printers
//! and unifies them into one `supvan://` device, decodes incoming jobs
//! (PWG/CUPS raster or `image/jpeg`) to the printhead bitmap, and drives the
//! transfer. An in-process registrar auto-creates the direct CUPS queue and
//! coexists with `cups-browsed` via a matching mDNS `UUID=` key. The binary
//! takes no arguments; it is configured via `SUPVAN_*` environment variables.

mod battery_provider;
mod ble_discover;
mod device;
mod discover;
mod dither;
mod dump;
mod ipp_job;
mod ipp_server;
mod job;
mod mock;
mod models;
mod printer_device;
mod usb_discover;
mod util;

use std::env;

#[tokio::main]
async fn main() {
    let _ = env_logger::try_init();

    let host = env::var("SUPVAN_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = env::var("SUPVAN_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(8631);

    if let Err(e) = ipp_server::run_server(&host, port).await {
        log::error!("server error: {e}");
        std::process::exit(1);
    }
}
