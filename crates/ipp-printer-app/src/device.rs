//! Device discovery hooks (USB HID / Bluetooth).

use crate::flags::PrinterReason;
use crate::printer::PrinterConfig;

/// One discovered device.
#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub info: String,
    pub uri: String,
    pub device_id: String,
}

/// Enumerate and open physical printers, and report their live health.
pub trait DeviceBackend: Send + Sync {
    /// Call `emit(info, uri, device_id)` for each device. Return false from emit to stop.
    fn list(&self, emit: &mut dyn FnMut(&str, &str, &str) -> bool);

    /// Resolve driver name from device_id MDL / URI (e.g. `supvan_t50`).
    fn driver_for_device(&self, device_id: &str, device_uri: &str) -> Option<String>;

    /// Query live printer-state-reasons for a registered printer. The background
    /// status loop calls this on each registered printer; returning `None` means
    /// "no change" (keep whatever reasons the registry already holds).
    fn poll_status(&self, _config: &PrinterConfig) -> Option<PrinterReason> {
        None
    }
}
