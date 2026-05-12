//! Safe wrapper around `pappl_printer_t`.
//!
//! Phase 1: just the methods needed by `report_job_failure` — wrapping a
//! raw pointer and `set_reasons`. More accessors (`get_driver_data`,
//! `open_device`, `set_ready_media`, …) move in over Phase 2/3.

use pappl_sys::{papplPrinterSetReasons, pappl_printer_t};
use std::marker::PhantomData;

use crate::flags::PrinterReason;

/// Borrowed handle to a PAPPL printer.
#[repr(transparent)]
pub struct Printer<'a> {
    raw: *mut pappl_printer_t,
    _marker: PhantomData<&'a pappl_printer_t>,
}

impl<'a> Printer<'a> {
    /// Wrap a raw PAPPL printer pointer.
    ///
    /// # Safety
    /// `raw` must be a valid `*mut pappl_printer_t` that lives at least
    /// as long as `'a`. Null is permitted but will make the wrapper
    /// methods no-ops.
    pub unsafe fn from_raw(raw: *mut pappl_printer_t) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    /// True if the underlying pointer is null (e.g. a callback handed
    /// us nothing).
    pub fn is_null(&self) -> bool {
        self.raw.is_null()
    }

    pub fn as_raw(&self) -> *mut pappl_printer_t {
        self.raw
    }

    /// Atomic add/remove of state-reason flags.
    ///
    /// Equivalent to `papplPrinterSetReasons(self, add, remove)`. Reasons
    /// in `add` are OR'd into the printer's `printer-state-reasons` IPP
    /// attribute; reasons in `remove` are cleared. Pass `PrinterReason::empty()`
    /// for either to leave it untouched.
    pub fn set_reasons(&self, add: PrinterReason, remove: PrinterReason) {
        if self.raw.is_null() {
            return;
        }
        unsafe { papplPrinterSetReasons(self.raw, add.into(), remove.into()) };
    }
}
