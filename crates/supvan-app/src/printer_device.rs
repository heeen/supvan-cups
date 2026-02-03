use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, Ordering};

use supvan_proto::printer::Printer;
use supvan_proto::rfcomm::RfcommSocket;

use crate::util::is_mock_mode;

/// Opaque device handle wrapping a connected Printer (or None in mock mode).
pub struct KsDevice {
    pub printer: Option<Printer>,
    /// Guards status queries during active raster transfer.
    pub printing: AtomicBool,
}

/// Material/label info returned by the printer.
pub struct KsMaterial {
    pub width_mm: u8,
    pub height_mm: u8,
    pub remaining: i32,
}

// PAPPL pappl_preason_t bit flags.
pub const PAPPL_PREASON_NONE: u32 = 0x0000;
pub const PAPPL_PREASON_OTHER: u32 = 0x0001;
pub const PAPPL_PREASON_COVER_OPEN: u32 = 0x0002;
pub const PAPPL_PREASON_MEDIA_EMPTY: u32 = 0x0080;
pub const PAPPL_PREASON_MEDIA_JAM: u32 = 0x0100;
pub const PAPPL_PREASON_MEDIA_NEEDED: u32 = 0x0400;

impl KsDevice {
    /// Open an RFCOMM connection to the printer at `addr`.
    ///
    /// If `SUPVAN_MOCK=1`, returns a mock device with no Bluetooth connection.
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

        log::debug!("KsDevice::open: connected to {addr}");
        Some(Box::new(KsDevice {
            printer: Some(Printer::new(sock)),
            printing: AtomicBool::new(false),
        }))
    }

    /// Read raw bytes from the RFCOMM socket. Returns bytes read, or -1 on error.
    ///
    /// In mock mode, returns 0 (EOF).
    pub fn read(&self, buf: *mut u8, len: usize) -> isize {
        let printer = match &self.printer {
            Some(p) => p,
            None => return 0,
        };
        let fd = printer.raw_fd();
        unsafe { libc::recv(fd, buf as *mut c_void, len, 0) }
    }

    /// Write raw bytes to the RFCOMM socket. Returns bytes written, or -1 on error.
    ///
    /// In mock mode, returns `len` (data is discarded).
    pub fn write(&self, buf: *const u8, len: usize) -> isize {
        let printer = match &self.printer {
            Some(p) => p,
            None => return len as isize,
        };
        let fd = printer.raw_fd();
        unsafe { libc::send(fd, buf as *const c_void, len, 0) }
    }

    /// Query printer status and return pappl_preason_t bit flags.
    ///
    /// Returns PAPPL_PREASON_NONE while the printing flag is set.
    pub fn status(&self) -> u32 {
        let printer = match &self.printer {
            Some(p) => p,
            None => return PAPPL_PREASON_NONE,
        };

        if self.printing.load(Ordering::Acquire) {
            return PAPPL_PREASON_NONE;
        }

        let status = match printer.query_status() {
            Ok(Some(s)) => s,
            Ok(None) => return PAPPL_PREASON_OTHER,
            Err(e) => {
                log::warn!("KsDevice::status: query failed: {e}");
                return PAPPL_PREASON_OTHER;
            }
        };

        let mut reasons = PAPPL_PREASON_NONE;
        if status.cover_open {
            reasons |= PAPPL_PREASON_COVER_OPEN;
        }
        if status.label_end || status.label_not_installed {
            reasons |= PAPPL_PREASON_MEDIA_EMPTY;
        }
        if status.label_rw_error || status.label_mode_error {
            reasons |= PAPPL_PREASON_MEDIA_JAM;
        }
        if status.ribbon_end {
            reasons |= PAPPL_PREASON_MEDIA_NEEDED;
        }
        if status.head_temp_high {
            reasons |= PAPPL_PREASON_OTHER;
        }
        reasons
    }

    /// Query material info. Returns Some on success.
    ///
    /// In mock mode, returns a dummy 40x30mm label.
    pub fn material(&self) -> Option<KsMaterial> {
        let printer = match &self.printer {
            Some(p) => p,
            None => {
                log::debug!("KsDevice::material: mock — returning 40x30mm");
                return Some(KsMaterial {
                    width_mm: 40,
                    height_mm: 30,
                    remaining: -1,
                });
            }
        };

        match printer.query_material() {
            Ok(Some(mat)) => {
                log::debug!(
                    "KsDevice::material: {}x{}mm, gap={}mm, remaining={}",
                    mat.width_mm,
                    mat.height_mm,
                    mat.gap_mm,
                    mat.remaining
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "unknown".into()),
                );
                Some(KsMaterial {
                    width_mm: mat.width_mm,
                    height_mm: mat.height_mm,
                    remaining: mat.remaining.map(|r| r as i32).unwrap_or(-1),
                })
            }
            Ok(None) => {
                log::warn!("KsDevice::material: no response");
                None
            }
            Err(e) => {
                log::error!("KsDevice::material: {e}");
                None
            }
        }
    }

    /// Query low-battery flag from printer status.
    ///
    /// Returns `false` in mock mode or on query failure.
    pub fn battery_low(&self) -> bool {
        let printer = match &self.printer {
            Some(p) => p,
            None => return false,
        };
        match printer.query_status() {
            Ok(Some(s)) => s.low_battery,
            _ => false,
        }
    }

    /// Check if this is a mock device (no real printer connection).
    pub fn is_mock(&self) -> bool {
        self.printer.is_none()
    }
}

impl Drop for KsDevice {
    fn drop(&mut self) {
        if self.printer.is_some() {
            log::info!("KsDevice: closing BT connection");
        } else {
            log::info!("KsDevice: closing mock device");
        }
    }
}
