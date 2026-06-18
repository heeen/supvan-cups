use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use supvan_proto::error::{Error as ProtoError, Result as ProtoResult};
use supvan_proto::hidraw::HidrawDevice;
use supvan_proto::printer::Printer;
use supvan_proto::status::PrinterStatus;
use supvan_proto::usb_transport::UsbHidTransport;

use crate::util::is_mock_mode;

/// A Printer that may be owned by this `KsDevice` (USB, fresh BT) or shared
/// behind a `Mutex` so multiple short-lived `KsDevice` instances can reuse a
/// single open RFCOMM socket without reconnecting (which makes the BT
/// firmware beep).
pub enum PrinterHandle {
    /// `KsDevice` is the only owner; transport closes when dropped.
    Owned(Printer),
    /// Shared with [`crate::device`]'s cache. Drop is a no-op for the
    /// connection.
    Shared(Arc<Mutex<Printer>>),
}

impl PrinterHandle {
    /// Query INQUIRY_STA, returning a parsed [`PrinterStatus`].
    pub fn query_status(&self) -> ProtoResult<Option<PrinterStatus>> {
        match self {
            Self::Owned(p) => p.query_status(),
            Self::Shared(arc) => arc.lock().unwrap().query_status(),
        }
    }

    /// Query RETURN_MAT (loaded label + RFID + remaining count).
    pub fn query_material(&self) -> ProtoResult<Option<supvan_proto::status::MaterialInfo>> {
        match self {
            Self::Owned(p) => p.query_material(),
            Self::Shared(arc) => arc.lock().unwrap().query_material(),
        }
    }

    /// Stream the compressed raster + speed to the device.
    pub fn print_compressed(&self, compressed: &[u8], speed: u16) -> ProtoResult<()> {
        match self {
            Self::Owned(p) => p.print_compressed(compressed, speed),
            Self::Shared(arc) => arc.lock().unwrap().print_compressed(compressed, speed),
        }
    }
}

/// Opaque device handle wrapping a connected Printer (or None in mock mode).
pub struct KsDevice {
    pub printer: Option<PrinterHandle>,
    /// Guards status queries during active raster transfer.
    pub printing: AtomicBool,
}

impl KsDevice {
    /// Wrap a printer that lives in the BT cache.
    pub fn from_shared(printer: Arc<Mutex<Printer>>) -> Self {
        KsDevice {
            printer: Some(PrinterHandle::Shared(printer)),
            printing: AtomicBool::new(false),
        }
    }

    /// Construct a mock device — no transport, status driven by [`crate::mock`].
    pub fn open_mock() -> Self {
        log::info!("KsDevice::open_mock: synthetic mock device");
        KsDevice {
            printer: None,
            printing: AtomicBool::new(false),
        }
    }

    /// Open a USB HID connection to the printer at `hidraw_path` (e.g. "/dev/hidraw7").
    pub fn open_usb(hidraw_path: &str) -> Option<Box<Self>> {
        if is_mock_mode() {
            log::info!("KsDevice::open_usb: MOCK mode — skipping USB open to {hidraw_path}");
            return Some(Box::new(KsDevice {
                printer: None,
                printing: AtomicBool::new(false),
            }));
        }

        log::info!("KsDevice::open_usb: opening {hidraw_path}");
        let dev = match HidrawDevice::open(hidraw_path) {
            Ok(d) => d,
            Err(e) => {
                log::error!("KsDevice::open_usb: hidraw open failed: {e}");
                return None;
            }
        };

        let transport = UsbHidTransport::new(dev);
        let printer = Printer::new(Box::new(transport));

        log::debug!("KsDevice::open_usb: opened {hidraw_path}");
        Some(Box::new(KsDevice {
            printer: Some(PrinterHandle::Owned(printer)),
            printing: AtomicBool::new(false),
        }))
    }

    /// Query printer status and return typed reason flags.
    ///
    /// Returns `PrinterReason::empty()` while the printing flag is set
    /// (so we don't query a printer mid-transfer).
    pub fn status(&self) -> ipp_printer_app::PrinterReason {
        use ipp_printer_app::PrinterReason;

        let printer = match &self.printer {
            Some(p) => p,
            None => return crate::mock::controller().current_reasons(),
        };

        if self.printing.load(Ordering::Acquire) {
            return PrinterReason::empty();
        }

        let status = match printer.query_status() {
            Ok(Some(s)) => s,
            Ok(None) => return PrinterReason::OTHER,
            Err(ProtoError::Io(_)) => {
                // Socket likely dropped under us; let the next open_bt
                // detect it and reconnect rather than reporting OFFLINE
                // on a transient blip.
                log::warn!("KsDevice::status: transport I/O error; will refresh on next open");
                return PrinterReason::empty();
            }
            Err(e) => {
                log::warn!("KsDevice::status: query failed: {e}");
                return PrinterReason::OTHER;
            }
        };

        let mut reasons = PrinterReason::empty();
        if status.cover_open {
            reasons |= PrinterReason::COVER_OPEN;
        }
        if status.label_end || status.label_not_installed {
            reasons |= PrinterReason::MEDIA_EMPTY;
        }
        if status.label_rw_error || status.label_mode_error {
            reasons |= PrinterReason::MEDIA_JAM;
        }
        if status.ribbon_end {
            reasons |= PrinterReason::MEDIA_NEEDED;
        }
        if status.head_temp_high {
            reasons |= PrinterReason::OTHER;
        }
        reasons
    }

    /// Check if this is a mock device (no real printer connection).
    pub fn is_mock(&self) -> bool {
        self.printer.is_none()
    }

    /// Query loaded material / RFID tag info. Returns `None` if mock, mid-print,
    /// or the transport errored.
    pub fn material(&self) -> Option<supvan_proto::status::MaterialInfo> {
        let printer = self.printer.as_ref()?;
        if self.printing.load(Ordering::Acquire) {
            return None;
        }
        match printer.query_material() {
            Ok(m) => m,
            Err(e) => {
                log::debug!("KsDevice::material: query failed: {e}");
                None
            }
        }
    }
}

impl Drop for KsDevice {
    fn drop(&mut self) {
        // Owned connections close their socket here; shared cache entries
        // persist for the next caller, so we don't even log on drop.
        if let Some(PrinterHandle::Owned(_)) = &self.printer {
            log::debug!("KsDevice: closing owned connection");
        }
    }
}
