//! Background `printer-state-reasons` poller.
//!
//! A single tokio task per server walks the printer registry every
//! [`POLL_INTERVAL`] and asks the backend for fresh status. Backends that don't
//! implement [`DeviceBackend::poll_status`] return `None` and leave the
//! registry untouched.

use std::sync::Arc;
use std::time::Duration;

use crate::device::DeviceBackend;
use crate::printer::PrinterRegistry;

/// Default cadence (configurable via `IPP_PRINTER_APP_POLL_SECS`).
const POLL_INTERVAL: Duration = Duration::from_secs(30);

/// Spawn the polling task. Returns immediately. Drop the returned
/// [`tokio::task::JoinHandle`] to abort the loop on server shutdown.
pub fn spawn(
    backend: Arc<dyn DeviceBackend>,
    registry: PrinterRegistry,
) -> tokio::task::JoinHandle<()> {
    let interval = std::env::var("IPP_PRINTER_APP_POLL_SECS")
        .ok()
        .and_then(|s| s.parse().ok())
        .map(Duration::from_secs)
        .unwrap_or(POLL_INTERVAL);

    tokio::spawn(async move {
        // First poll happens after `interval` so the server has time to
        // bootstrap printers before the first status query lands on a device.
        let mut ticker = tokio::time::interval(interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        ticker.tick().await; // immediate first tick
        loop {
            ticker.tick().await;
            poll_once(backend.as_ref(), &registry).await;
        }
    })
}

async fn poll_once(backend: &dyn DeviceBackend, registry: &PrinterRegistry) {
    use crate::printer::IppPrinterState;
    // Snapshot the idle printers; skip ones with a job in flight so we don't
    // contend with the device.
    let configs: Vec<_> = {
        let g = registry.read();
        g.iter()
            .filter(|r| r.state == IppPrinterState::Idle)
            .map(|r| r.config.clone())
            .collect()
    };

    for cfg in configs {
        // `poll_status` may block on a HID read; let it run in the blocking
        // section so other tasks (Get-Printer-Attributes, etc.) stay responsive.
        let reasons = tokio::task::block_in_place(|| backend.poll_status(&cfg));
        if let Some(reasons) = reasons {
            let mut g = registry.write();
            if let Some(rec) = g.iter_mut().find(|r| r.config.name == cfg.name) {
                if rec.reasons != reasons {
                    log::debug!(
                        "status: {} reasons {:?} -> {:?}",
                        cfg.name,
                        rec.reasons,
                        reasons
                    );
                    rec.reasons = reasons;
                }
            }
        }
    }
}
