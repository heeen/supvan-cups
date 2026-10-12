use std::sync::Arc;

use ipp_printer_app::PrinterReason;
use supvan_proto::error::Error as ProtoError;
use supvan_proto::printer::Printer;
use supvan_proto::status::MaterialInfo;
use tokio::sync::{Mutex, MutexGuard};

/// A connected printer, shareable so several short-lived [`KsDevice`]s (status
/// polls, print jobs) reuse one link. Reconnecting a Bluetooth printer makes
/// it beep, so [`crate::device`] caches these per address.
pub type SharedPrinter = Arc<Mutex<Printer>>;

/// The device a job or status poll talks to: a real printer, or the mock.
pub struct KsDevice {
    printer: Option<SharedPrinter>,
}

impl KsDevice {
    pub fn real(printer: SharedPrinter) -> Self {
        Self {
            printer: Some(printer),
        }
    }

    /// A no-connection device driven by the [`crate::mock`] controller.
    pub fn mock() -> Self {
        log::info!("KsDevice::mock: synthetic mock device");
        Self { printer: None }
    }

    pub fn is_mock(&self) -> bool {
        self.printer.is_none()
    }

    /// The printer, locked for the caller's exchange; `None` on the mock.
    /// Waits out any print in progress.
    pub async fn printer(&self) -> Option<MutexGuard<'_, Printer>> {
        Some(self.printer.as_ref()?.lock().await)
    }

    /// The printer if nobody is using it — status and material polls skip a
    /// printer mid-job rather than interleave with the transfer.
    fn idle_printer(&self) -> Option<MutexGuard<'_, Printer>> {
        self.printer.as_ref()?.try_lock().ok()
    }

    /// Printer status as IPP reason flags. Empty while a job holds the
    /// printer.
    pub async fn status(&self) -> PrinterReason {
        if self.printer.is_none() {
            return crate::mock::controller().current_reasons();
        }
        let Some(printer) = self.idle_printer() else {
            return PrinterReason::empty();
        };
        match printer.query_status().await {
            Ok(Some(s)) => crate::job::reasons_from_status(&s),
            Ok(None) => PrinterReason::OTHER,
            Err(ProtoError::Io(_)) => {
                // Socket likely dropped under us; the next open detects it and
                // reconnects, rather than reporting OFFLINE on a transient blip.
                log::warn!("KsDevice::status: transport I/O error; will refresh on next open");
                PrinterReason::empty()
            }
            Err(e) => {
                log::warn!("KsDevice::status: query failed: {e}");
                PrinterReason::OTHER
            }
        }
    }

    /// Loaded material / RFID tag info. `None` on the mock, mid-print, or when
    /// the transport errored.
    pub async fn material(&self) -> Option<MaterialInfo> {
        let printer = self.idle_printer()?;
        printer
            .query_material()
            .await
            .inspect_err(|e| log::debug!("KsDevice::material: query failed: {e}"))
            .ok()
            .flatten()
    }

    /// Identify-Printer: poke the device so it makes itself known. We send
    /// CHECK_DEVICE — the only presence primitive in the protocol; on Supvan
    /// hardware exercising the link makes the unit chirp. No-op on mock.
    pub async fn identify(&self) {
        let Some(printer) = self.printer().await else {
            return;
        };
        match printer.check_device().await {
            Ok(present) => log::info!("KsDevice::identify: check_device -> present={present}"),
            Err(e) => log::warn!("KsDevice::identify: check_device failed: {e}"),
        }
    }
}
