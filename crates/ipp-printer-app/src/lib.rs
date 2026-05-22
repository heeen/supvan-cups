//! Rust IPP Everywhere printer-application framework.
//!
//! Replaces PAPPL for CUPS driverless printing: HTTP POST with `application/ipp`
//! bodies on `/ipp/print/<printer-name>`. Device-agnostic; a consumer crate
//! (e.g. `supvan-app`) supplies the [`DeviceBackend`] + [`RasterDriver`] impls.

pub mod attributes;
pub mod device;
pub mod flags;
pub mod job;
pub mod printer;
#[cfg(feature = "mdns")]
pub mod mdns;
pub mod raster;
pub mod server;
pub mod state;
pub mod status;

pub use device::{DeviceBackend, DeviceInfo};
pub use flags::PrinterReason;
pub use job::{JobId, JobRecord, JobRegistry, JobState};
pub use printer::{PrinterConfig, PrinterHandle, PrinterRecord, PrinterRegistry};
pub use raster::{JobFailure, JobOptions, RasterDriver};
pub use server::{JobContext, Server, ServerOptions};
pub use state::{default_state_path, PersistedState};
