//! Supvan IPP Everywhere printer application (Rust IPP stack).

mod battery_provider;
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
mod registrar;
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
