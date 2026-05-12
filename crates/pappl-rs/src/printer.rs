//! Safe wrapper around `pappl_printer_t`.

use pappl_sys::{
    papplPrinterCloseDevice, papplPrinterGetDriverName, papplPrinterOpenDevice,
    papplPrinterSetReasons, pappl_printer_t,
};
use std::ffi::CStr;
use std::marker::PhantomData;

use crate::device_handle::DeviceHandle;
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

    /// Get the printer's driver name as a `&CStr`.
    pub fn driver_name(&self) -> Option<&'a CStr> {
        if self.raw.is_null() {
            return None;
        }
        let ptr = unsafe { papplPrinterGetDriverName(self.raw) };
        if ptr.is_null() {
            None
        } else {
            Some(unsafe { CStr::from_ptr(ptr) })
        }
    }

    /// Open the printer's device for direct I/O. Returns `None` if the
    /// device is unavailable (offline, busy, etc.). The device must be
    /// closed via [`close_device`](Self::close_device) when done.
    pub fn open_device(&self) -> Option<DeviceHandle<'a>> {
        if self.raw.is_null() {
            return None;
        }
        let dev = unsafe { papplPrinterOpenDevice(self.raw) };
        if dev.is_null() {
            None
        } else {
            Some(unsafe { DeviceHandle::from_raw(dev) })
        }
    }

    /// Close a device previously opened via [`open_device`](Self::open_device).
    pub fn close_device(&self) {
        if !self.raw.is_null() {
            unsafe { papplPrinterCloseDevice(self.raw) };
        }
    }
}
