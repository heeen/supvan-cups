//! Per-printer configuration and runtime state.

use std::sync::Arc;

use parking_lot::RwLock;

use crate::flags::PrinterReason;

/// Static printer capabilities (from `models.toml` driver family).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PrinterConfig {
    pub name: String,
    pub driver_name: String,
    pub make_and_model: String,
    pub device_id: String,
    pub device_uri: String,
    pub dpi: i32,
    pub printhead_width_dots: u32,
    pub media_names: Vec<String>,
    pub media_sizes: Vec<[i32; 2]>,
    /// Darkness 0–100 (maps to print density).
    pub darkness: i32,
}

impl PrinterConfig {
    pub fn printer_uri(&self, host: &str, port: u16) -> String {
        let h = if host == "0.0.0.0" || host == "::" || host.is_empty() {
            "localhost"
        } else {
            host
        };
        format!("ipp://{h}:{port}/ipp/print/{}", self.name)
    }
}

/// IPP printer state (idle / processing / stopped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum IppPrinterState {
    Idle = 3,
    Processing = 4,
    Stopped = 5,
}

/// Runtime printer entry in the server registry.
#[derive(Debug, Clone)]
pub struct PrinterRecord {
    pub config: PrinterConfig,
    pub state: IppPrinterState,
    pub reasons: PrinterReason,
    pub uuid: String,
}

impl PrinterRecord {
    pub fn new(config: PrinterConfig) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            state: IppPrinterState::Idle,
            reasons: PrinterReason::empty(),
            config,
        }
    }

    pub fn uri_path(&self) -> String {
        format!("/ipp/print/{}", self.config.name)
    }
}

/// Borrowed view passed into raster drivers during a job.
pub struct PrinterHandle<'a> {
    pub record: &'a PrinterRecord,
}

impl<'a> PrinterHandle<'a> {
    pub fn driver_name(&self) -> &str {
        &self.record.config.driver_name
    }

    pub fn darkness(&self) -> i32 {
        self.record.config.darkness
    }

    pub fn printhead_width_dots(&self) -> u32 {
        self.record.config.printhead_width_dots
    }

    pub fn set_reasons(&self, _set: PrinterReason, _clear: PrinterReason) {
        // Updated via AppState in the server; handle is read-only during job.
    }
}

/// Shared printer registry.
pub type PrinterRegistry = Arc<RwLock<Vec<PrinterRecord>>>;
