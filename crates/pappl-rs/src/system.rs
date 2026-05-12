//! Safe wrapper around `pappl_system_t`.
//!
//! Phase 1 exposes only the bits we need: wrapping a raw pointer from
//! FFI callbacks and the version-gated `auto_add_printers` helper. Full
//! builder + iteration API comes in Phase 4.

use pappl_sys::pappl_system_t;
use std::marker::PhantomData;

use crate::device::DeviceScheme;
use crate::flags::DeviceType;
use crate::{Error, Result};

/// Borrowed handle to a PAPPL system. Lifetime `'a` ties it to whatever
/// produced the pointer (typically a `papplMainloop`-managed system).
#[repr(transparent)]
pub struct System<'a> {
    raw: *mut pappl_system_t,
    _marker: PhantomData<&'a pappl_system_t>,
}

impl<'a> System<'a> {
    /// Wrap a raw PAPPL system pointer.
    ///
    /// # Safety
    /// `raw` must be a valid `*mut pappl_system_t` that lives at least
    /// as long as `'a`. Typically obtained from a PAPPL callback
    /// parameter.
    pub unsafe fn from_raw(raw: *mut pappl_system_t) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    /// Underlying raw pointer for callers still using `pappl-sys`
    /// directly during migration.
    pub fn as_raw(&self) -> *mut pappl_system_t {
        self.raw
    }

    /// Register a custom device scheme. The scheme's URI prefix
    /// (`S::NAME`), device type, and callback set are installed via
    /// `papplDeviceAddScheme`. Generic thunks in `device::install`
    /// bridge the trait methods to PAPPL's C ABI.
    pub fn register_scheme<S: DeviceScheme>(&self) {
        unsafe { crate::device::install::<S>(self.raw) };
    }

    /// Discover and auto-add printers across the registered device
    /// schemes of the given types. Wraps `papplSystemCreatePrinters`
    /// (PAPPL ≥ 1.4). On older PAPPL returns `Err(Error::Unsupported)`.
    pub fn auto_add_printers(&self, types: DeviceType) -> Result<()> {
        let ok = unsafe {
            pappl_sys::try_system_create_printers(
                self.raw,
                types.into(),
                None,
                std::ptr::null_mut(),
            )
        };
        if ok {
            Ok(())
        } else {
            Err(Error::Unsupported)
        }
    }
}
