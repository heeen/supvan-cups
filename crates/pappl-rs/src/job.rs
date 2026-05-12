//! Safe wrapper around `pappl_job_t`.
//!
//! Phase 1: the methods needed by `report_job_failure` —
//! `set_reasons`, `set_message`, `log`, and `printer()` to reach the
//! owning `Printer`. Job-payload `set_data<T>/take_data<T>` lands in
//! Phase 3 with the `RasterDriver` trait.

use pappl_sys::{
    papplJobGetPrinter, papplJobSetMessage, papplJobSetReasons, papplLogJob, pappl_job_t,
};
use std::ffi::CString;
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
