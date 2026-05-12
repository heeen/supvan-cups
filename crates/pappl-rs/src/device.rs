//! Custom device-scheme registration via the `DeviceScheme` trait.
//!
//! PAPPL's `papplDeviceAddScheme` takes six C function pointers
//! (`list`, `open`, `close`, `read`, `write`, `status`). Implementing one
//! scheme idiomatically meant writing six `unsafe extern "C" fn`
//! wrappers, each unmarshalling `*mut c_void` payloads stored via
//! `papplDeviceSetData`. This module collapses that into a single trait
//! plus a set of generic thunks: implement `DeviceScheme` for a unit
//! struct, call `System::register_scheme::<S>()`, done.
//!
//! ## Payload lifecycle
//!
//! `S::open(uri) -> Option<Self::Payload>` constructs the per-device
//! payload. The thunk boxes it and stashes the pointer in PAPPL via
//! `papplDeviceSetData`. Subsequent read/write/status calls receive
//! `&Self::Payload`. On close, the thunk reclaims the box and hands the
//! owned value to `S::close(payload)` — default impl just drops it; the
//! BT scheme overrides this to return the connection to a cache.

use std::ffi::{c_char, c_void, CStr, CString};
use std::marker::PhantomData;

use pappl_sys::{
    papplDeviceAddScheme, papplDeviceGetData, papplDeviceSetData, pappl_device_cb_t,
    pappl_device_t, pappl_deverror_cb_t, pappl_preason_t,
};

use crate::flags::{DeviceType, PrinterReason};

/// A custom PAPPL device scheme.
///
/// Implement on a unit struct (`struct MyScheme;`) and register via
/// [`System::register_scheme`](crate::System::register_scheme).
///
/// ## Thread model
/// PAPPL invokes scheme callbacks from a single thread (its mainloop).
/// Implementations don't need internal locking unless they share state
/// with other threads (e.g. a connection cache accessed by code outside
/// PAPPL's callback path).
pub trait DeviceScheme: 'static {
    /// URI scheme prefix without `://`. E.g. `c"btrfcomm"` for
    /// `btrfcomm://...` URIs.
    const NAME: &'static CStr;

    /// Device-type flag passed to `papplDeviceAddScheme`. Use
    /// [`DeviceType::CUSTOM_LOCAL`] for local-bus schemes,
    /// [`DeviceType::CUSTOM_NETWORK`] for network ones.
    const DEVICE_TYPE: DeviceType;

    /// Per-device payload stored across open/read/write/status/close.
    /// Typically `Box<YourDevice>`.
    type Payload: 'static;

    /// Enumerate devices on this scheme. For each found device, call
    /// `emit(info, uri, device_id)`. The closure returns `true` to
    /// continue, `false` to stop early.
    fn list(emit: &mut dyn FnMut(&str, &str, &str) -> bool);

    /// Open a device. `uri` is the full URI including the scheme
    /// prefix. Return `None` to signal open failure to PAPPL.
    fn open(uri: &str) -> Option<Self::Payload>;

    /// Close a device. Default implementation just drops the payload;
    /// override to e.g. return the connection to a cache.
    fn close(payload: Self::Payload) {
        drop(payload);
    }

    /// Read bytes from the device. Return value follows POSIX
    /// `read(2)`: positive byte count on success, 0 on EOF, negative
    /// on error.
    fn read(payload: &Self::Payload, buf: &mut [u8]) -> isize;

    /// Write bytes to the device. Return value follows POSIX
    /// `write(2)`.
    fn write(payload: &Self::Payload, buf: &[u8]) -> isize;

    /// Query device-level state-reasons. Returned flags are merged
    /// into the printer's `printer-state-reasons` IPP attribute.
    fn status(payload: &Self::Payload) -> PrinterReason;
}

// --- Generic thunks ----------------------------------------------------------
//
// Each thunk is parameterised on `S: DeviceScheme` and converts between
// PAPPL's C ABI and the trait's safe Rust signature. They're all
// `#[doc(hidden)]` because callers should only see them via
// `System::register_scheme`.

#[doc(hidden)]
pub unsafe extern "C" fn list_thunk<S: DeviceScheme>(
    cb: pappl_device_cb_t,
    data: *mut c_void,
    _err_cb: pappl_deverror_cb_t,
    _err_data: *mut c_void,
) -> bool {
    let Some(pappl_cb) = cb else {
        return false;
    };
    S::list(&mut |info, uri, device_id| {
        let c_info = CString::new(info).unwrap_or_default();
        let c_uri = CString::new(uri).unwrap_or_default();
        let c_id = CString::new(device_id).unwrap_or_default();
        // pappl_cb returns true to signal "stop". Forward the inverse
        // so our trait closure can keep using true=continue semantics.
        let stop = pappl_cb(c_info.as_ptr(), c_uri.as_ptr(), c_id.as_ptr(), data);
        !stop
    });
    // Return false: "no error". PAPPL doesn't use this return for
    // "found anything"; that's communicated via the emitted callbacks.
    false
}

#[doc(hidden)]
pub unsafe extern "C" fn open_thunk<S: DeviceScheme>(
    device: *mut pappl_device_t,
    device_uri: *const c_char,
    _name: *const c_char,
) -> bool {
    if device_uri.is_null() {
        return false;
    }
    let uri = match CStr::from_ptr(device_uri).to_str() {
        Ok(s) => s,
        Err(_) => return false,
    };
    let Some(payload) = S::open(uri) else {
        return false;
    };
    let boxed = Box::new(payload);
    papplDeviceSetData(device, Box::into_raw(boxed) as *mut c_void);
    true
}

#[doc(hidden)]
pub unsafe extern "C" fn close_thunk<S: DeviceScheme>(device: *mut pappl_device_t) {
    let ptr = papplDeviceGetData(device) as *mut S::Payload;
    if ptr.is_null() {
        return;
    }
    let payload = *Box::from_raw(ptr);
    S::close(payload);
}

#[doc(hidden)]
pub unsafe extern "C" fn read_thunk<S: DeviceScheme>(
    device: *mut pappl_device_t,
    buffer: *mut c_void,
    bytes: usize,
) -> isize {
    let ptr = papplDeviceGetData(device) as *const S::Payload;
    if ptr.is_null() {
        return -1;
    }
    let buf = std::slice::from_raw_parts_mut(buffer as *mut u8, bytes);
    S::read(&*ptr, buf)
}

#[doc(hidden)]
pub unsafe extern "C" fn write_thunk<S: DeviceScheme>(
    device: *mut pappl_device_t,
    buffer: *const c_void,
    bytes: usize,
) -> isize {
    let ptr = papplDeviceGetData(device) as *const S::Payload;
    if ptr.is_null() {
        return -1;
    }
    let buf = std::slice::from_raw_parts(buffer as *const u8, bytes);
    S::write(&*ptr, buf)
}

#[doc(hidden)]
pub unsafe extern "C" fn status_thunk<S: DeviceScheme>(device: *mut pappl_device_t) -> pappl_preason_t {
    let ptr = papplDeviceGetData(device) as *const S::Payload;
    if ptr.is_null() {
        return PrinterReason::OTHER.into();
    }
    S::status(&*ptr).into()
}

/// Register scheme `S` with the given PAPPL system.
///
/// Called from `System::register_scheme::<S>()`. Splitting it out into
/// a free function keeps the `pappl_system_t` raw pointer plumbing
/// contained.
///
/// # Safety
/// `system` must be a valid `*mut pappl_system_t`.
#[doc(hidden)]
pub unsafe fn install<S: DeviceScheme>(_system: *mut pappl_sys::pappl_system_t) {
    papplDeviceAddScheme(
        S::NAME.as_ptr(),
        S::DEVICE_TYPE.into(),
        Some(list_thunk::<S>),
        Some(open_thunk::<S>),
        Some(close_thunk::<S>),
        Some(read_thunk::<S>),
        Some(write_thunk::<S>),
        Some(status_thunk::<S>),
        None, // id_cb — not currently used by either of our schemes
    );
}

// `PhantomData` placeholder so this file's `S` generic doesn't trip
// `dead_code` if nobody implements `DeviceScheme` yet at compile time.
#[doc(hidden)]
#[allow(dead_code)]
struct _Touch<S: DeviceScheme>(PhantomData<S>);
