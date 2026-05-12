//! Safe, idiomatic Rust bindings for PAPPL — Michael Sweet's Printer
//! Application Programming Library.
//!
//! This crate wraps the `pappl-sys` raw bindgen FFI in typed handles and
//! traits so applications don't have to write `unsafe extern "C" fn`
//! callbacks or hand-translate `pappl_*_t` integer flags.
//!
//! ## Threading model
//!
//! PAPPL invokes all registered callbacks (device, raster, driver, status,
//! save) from its single mainloop thread. The traits in this crate take
//! `&self` accordingly. If you need to share mutable state across
//! callbacks, use `RefCell`/`Mutex` inside your trait impl — this crate
//! does not introduce any locking of its own.
//!
//! ## Version compatibility
//!
//! Requires PAPPL ≥ 1.0 at build time (enforced by pkg-config). Features
//! that depend on PAPPL ≥ 1.4 (notably `System::auto_add_printers`) are
//! transparent: on older PAPPL they return `Err(Error::Unsupported)` and
//! the caller can fall back to mainloop-driven auto-add.

pub mod error;
pub mod flags;
pub mod job;
pub mod printer;
pub mod system;

pub use error::{Error, Result};
pub use flags::{DeviceType, JobReason, LogLevel, PrinterReason, SystemOptions};
pub use job::Job;
pub use printer::Printer;
pub use system::System;

/// Re-export of `pappl-sys` for callers that still need raw access during
/// migration. Once the wrapper is feature-complete, dependents should
/// stop using this and depend only on `pappl-rs`.
pub mod sys {
    pub use pappl_sys::*;
}

pub mod prelude {
    pub use crate::{Error, Job, JobReason, LogLevel, Printer, PrinterReason, Result, System};
}
