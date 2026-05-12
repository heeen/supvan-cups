//! Borrowed handle to a `pappl_device_t` received in raster callbacks.
//!
//! Provides typed access to the per-device payload stored by a
//! [`DeviceScheme`](crate::device::DeviceScheme) implementation.

use pappl_sys::{papplDeviceGetData, pappl_device_t};
use std::marker::PhantomData;

/// Borrowed handle to an open PAPPL device.
///
/// Obtained from raster callbacks or via [`Printer::open_device`].
/// The `'a` lifetime ties it to the enclosing callback scope.
#[repr(transparent)]
pub struct DeviceHandle<'a> {
    raw: *mut pappl_device_t,
    _marker: PhantomData<&'a pappl_device_t>,
}

impl<'a> DeviceHandle<'a> {
    /// Wrap a raw PAPPL device pointer.
    ///
    /// # Safety
    /// `raw` must be a valid, non-null `*mut pappl_device_t`.
    pub unsafe fn from_raw(raw: *mut pappl_device_t) -> Self {
        Self {
            raw,
            _marker: PhantomData,
        }
    }

    pub fn as_raw(&self) -> *mut pappl_device_t {
        self.raw
    }

    /// Get a typed reference to the per-device payload stored by the
    /// device scheme's `open` callback.
    ///
    /// # Safety
    /// The caller must ensure `T` matches the actual type stored via
    /// `papplDeviceSetData` (i.e. `DeviceScheme::Payload`).
    pub unsafe fn data<T: 'static>(&self) -> Option<&'a T> {
        let ptr = papplDeviceGetData(self.raw) as *const T;
        if ptr.is_null() {
            None
        } else {
            Some(&*ptr)
        }
    }

    /// Get a typed mutable reference to the per-device payload.
    ///
    /// # Safety
    /// Same as [`data`](Self::data), plus exclusive access must be
    /// guaranteed (PAPPL's single-threaded callback model ensures this).
    pub unsafe fn data_mut<T: 'static>(&self) -> Option<&'a mut T> {
        let ptr = papplDeviceGetData(self.raw) as *mut T;
        if ptr.is_null() {
            None
        } else {
            Some(&mut *ptr)
        }
    }
}
