//! Safe wrapper around `pappl_job_t`.
//!
//! Provides `set_reasons`, `set_message`, `log`, `printer()`, and typed
//! per-job data management (`set_data<T>`, `data<T>`, `data_mut<T>`,
//! `take_data<T>`).

use pappl_sys::{
    papplJobGetData, papplJobGetPrinter, papplJobSetData, papplJobSetMessage, papplJobSetReasons,
    papplLogJob, pappl_job_t,
};
use std::ffi::{c_void, CString};
use std::marker::PhantomData;

use crate::flags::{JobReason, LogLevel};
use crate::printer::Printer;

/// Borrowed handle to a PAPPL job.
#[repr(transparent)]
pub struct Job<'a> {
    raw: *mut pappl_job_t,
    _marker: PhantomData<&'a pappl_job_t>,
}

impl<'a> Job<'a> {
    /// Wrap a raw PAPPL job pointer.
    ///
    /// # Safety
    /// `raw` must be a valid `*mut pappl_job_t` that lives at least as
    /// long as `'a`. Null is permitted but makes the wrapper methods
    /// no-ops.
    pub unsafe fn from_raw(raw: *mut pappl_job_t) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    pub fn is_null(&self) -> bool {
        self.raw.is_null()
    }

    pub fn as_raw(&self) -> *mut pappl_job_t {
        self.raw
    }

    /// The `Printer` that owns this job. Returns a `Printer<'a>` with
    /// the same lifetime; if PAPPL returns NULL the wrapper's
    /// `is_null()` will reflect that.
    pub fn printer(&self) -> Printer<'a> {
        let raw = if self.raw.is_null() {
            std::ptr::null_mut()
        } else {
            unsafe { papplJobGetPrinter(self.raw) }
        };
        unsafe { Printer::from_raw(raw) }
    }

    /// Atomic add/remove of job-state-reason flags. Equivalent to
    /// `papplJobSetReasons(self, add, remove)`.
    pub fn set_reasons(&self, add: JobReason, remove: JobReason) {
        if self.raw.is_null() {
            return;
        }
        unsafe { papplJobSetReasons(self.raw, add.into(), remove.into()) };
    }

    /// Set `job-state-message` shown by CUPS clients and the PAPPL web
    /// UI. Strings with interior NUL bytes are silently truncated.
    pub fn set_message(&self, msg: &str) {
        if self.raw.is_null() {
            return;
        }
        let cmsg = CString::new(msg)
            .unwrap_or_else(|_| CString::new("print failed").expect("static literal has no NUL"));
        unsafe { papplJobSetMessage(self.raw, cmsg.as_ptr()) };
    }

    /// Append a message to the job log at the given level. NUL bytes
    /// in `msg` are silently truncated.
    pub fn log(&self, level: LogLevel, msg: &str) {
        if self.raw.is_null() {
            return;
        }
        let cmsg = CString::new(msg)
            .unwrap_or_else(|_| CString::new("log message failed").expect("static literal has no NUL"));
        unsafe { papplLogJob(self.raw, level.into(), cmsg.as_ptr()) };
    }

    // -- Per-job typed data management ----------------------------------
    //
    // PAPPL stores a single `void*` per job via papplJobSetData/GetData.
    // These methods provide typed access, boxing the value on `set` and
    // reclaiming it on `take`.

    /// Store per-job data. The value is boxed and stored via
    /// `papplJobSetData`. Any previous data is **leaked** — call
    /// `take_data::<T>()` first if you need to reclaim it.
    ///
    /// # Safety
    /// Must not be called while another reference (from `data` /
    /// `data_mut`) is live.
    pub unsafe fn set_data<T: 'static>(&self, val: T) {
        if self.raw.is_null() {
            return;
        }
        let boxed = Box::new(val);
        papplJobSetData(self.raw, Box::into_raw(boxed) as *mut c_void);
    }

    /// Get a shared reference to the per-job data.
    ///
    /// # Safety
    /// `T` must match the type previously stored via `set_data`.
    pub unsafe fn data<T: 'static>(&self) -> Option<&'a T> {
        if self.raw.is_null() {
            return None;
        }
        let ptr = papplJobGetData(self.raw) as *const T;
        if ptr.is_null() {
            None
        } else {
            Some(&*ptr)
        }
    }

    /// Get a mutable reference to the per-job data.
    ///
    /// # Safety
    /// `T` must match the type previously stored via `set_data`, and
    /// no other reference may be live.
    pub unsafe fn data_mut<T: 'static>(&self) -> Option<&'a mut T> {
        if self.raw.is_null() {
            return None;
        }
        let ptr = papplJobGetData(self.raw) as *mut T;
        if ptr.is_null() {
            None
        } else {
            Some(&mut *ptr)
        }
    }

    /// Reclaim the per-job data, returning the owned value and clearing
    /// the PAPPL slot. Returns `None` if the slot was null.
    ///
    /// # Safety
    /// `T` must match the type previously stored via `set_data`. Must
    /// not be called while any reference from `data`/`data_mut` is live.
    pub unsafe fn take_data<T: 'static>(&self) -> Option<T> {
        if self.raw.is_null() {
            return None;
        }
        let ptr = papplJobGetData(self.raw) as *mut T;
        if ptr.is_null() {
            return None;
        }
        papplJobSetData(self.raw, std::ptr::null_mut());
        Some(*Box::from_raw(ptr))
    }

    /// Clear the per-job data slot without reclaiming (leaks the value).
    /// Use `take_data` instead if you need proper cleanup.
    ///
    /// # Safety
    /// Must not be called while a reference from `data`/`data_mut` is live.
    pub unsafe fn clear_data(&self) {
        if !self.raw.is_null() {
            papplJobSetData(self.raw, std::ptr::null_mut());
        }
    }

    /// Convenience for the canonical "job failed at device" surface:
    ///   - clear `error` and `canceled-at-device` reasons on the
    ///     job (in case they were stale)
    ///   - set `JOB_CANCELED_AT_DEVICE | ERRORS_DETECTED`
    ///   - apply `add` to the parent printer's state-reasons
    ///   - set `job-state-message` to `msg`
    ///   - log the message at ERROR level
    ///
    /// Replaces the hand-rolled `report_job_failure` helper that lived
    /// in `supvan-app::raster`.
    pub fn fail(&self, printer_reasons: crate::PrinterReason, msg: &str) {
        self.printer().set_reasons(printer_reasons, crate::PrinterReason::empty());
        self.set_reasons(
            JobReason::JOB_CANCELED_AT_DEVICE | JobReason::ERRORS_DETECTED,
            JobReason::NONE,
        );
        self.set_message(msg);
        self.log(LogLevel::Error, msg);
    }
}
