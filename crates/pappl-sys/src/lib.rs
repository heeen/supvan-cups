#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(dead_code)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/bindings.rs"));

/// Whether the linked PAPPL is at least 1.4 (where
/// `papplSystemCreatePrinters` was added). Set by `build.rs` from
/// pkg-config; usable from this crate. Dependent crates can't read the
/// flag directly (cargo `rustc-cfg` doesn't propagate), so call the
/// wrappers below instead of cfg-gating themselves.
#[cfg(pappl_1_4)]
pub const HAS_CREATE_PRINTERS: bool = true;
#[cfg(not(pappl_1_4))]
pub const HAS_CREATE_PRINTERS: bool = false;

/// Wrapper around `papplSystemCreatePrinters` (added in PAPPL 1.4).
///
/// On older PAPPL this is a no-op returning `false`; callers must fall
/// back to whatever discovery was working in pre-1.4 (typically the
/// mainloop-driven auto-add). Wrapping here is the only place a cfg
/// gate works — `cargo:rustc-cfg=pappl_1_4` is emitted by this crate's
/// build script and is invisible to dependents.
///
/// # Safety
/// Same as the underlying PAPPL call: `system` must be a live system.
#[allow(unused_variables)]
pub unsafe fn try_system_create_printers(
    system: *mut pappl_system_t,
    types: pappl_devtype_t,
    cb: pappl_pr_create_cb_t,
    data: *mut std::os::raw::c_void,
) -> bool {
    #[cfg(pappl_1_4)]
    {
        papplSystemCreatePrinters(system, types, cb, data);
        true
    }
    #[cfg(not(pappl_1_4))]
    {
        false
    }
}
