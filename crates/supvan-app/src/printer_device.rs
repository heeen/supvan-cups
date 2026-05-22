use std::sync::atomic::{AtomicBool, Ordering};

use supvan_proto::bt_transport::BtTransport;
use supvan_proto::hidraw::HidrawDevice;
use supvan_proto::printer::Printer;
use supvan_proto::rfcomm::RfcommSocket;
use supvan_proto::usb_transport::UsbHidTransport;

use crate::util::is_mock_mode;

/// Opaque device handle wrapping a connected Printer (or None in mock mode).
pub struct KsDevice {
    pub printer: Option<Printer>,
    /// Guards status queries during active raster transfer.
    pub printing: AtomicBool,
}

impl KsDevice {
    /// Open a Bluetooth RFCOMM connection to the printer at `addr`.
    ///
    /// If `SUPVAN_MOCK=1`, returns a mock device with no connection.
    pub fn open(addr: &str) -> Option<Box<Self>> {
        if is_mock_mode() {
            log::info!("KsDevice::open: MOCK mode — skipping BT connect to {addr}");
            return Some(Box::new(KsDevice {
                printer: None,
                printing: AtomicBool::new(false),
            }));
        }

        log::info!("KsDevice::open: connecting to {addr}");
        let sock = match RfcommSocket::connect_default(addr) {
            Ok(s) => s,
            Err(e) => {
                log::error!("KsDevice::open: RFCOMM connect failed: {e}");
                return None;
            }
        };

        let transport = BtTransport::new(sock);
        let printer = Printer::new(Box::new(transport));

        log::debug!("KsDevice::open: connected to {addr}");
        Some(Box::new(KsDevice {
            printer: Some(printer),
            printing: AtomicBool::new(false),
        }))
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
            printer: Some(printer),
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
            None => return PrinterReason::empty(),
        };

        if self.printing.load(Ordering::Acquire) {
            return PrinterReason::empty();
        }

        let status = match printer.query_status() {
            Ok(Some(s)) => s,
            Ok(None) => return PrinterReason::OTHER,
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
}

impl Drop for KsDevice {
    fn drop(&mut self) {
        if self.printer.is_some() {
            log::info!("KsDevice: closing connection");
        } else {
            log::info!("KsDevice: closing mock device");
        }
    }
}
