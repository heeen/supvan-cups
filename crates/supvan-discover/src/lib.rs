//! Device discovery and the model registry, shared by the printer application
//! and the CLI.
//!
//! The three transports each have their own scanner — [`usb`] over sysfs,
//! [`bt`] over BlueZ D-Bus, [`ble`] over an LE scan — and all three agree on
//! what counts as a Supvan printer by going through [`models`]. Keeping them
//! in one crate is what stops the CLI and the daemon disagreeing about which
//! devices exist.

pub mod ble;
pub mod bt;
pub mod models;
pub mod usb;

/// Whether `SUPVAN_MOCK=1`: discovery reports a synthetic printer and USB
/// probing is skipped.
pub fn is_mock_mode() -> bool {
    std::env::var("SUPVAN_MOCK").is_ok_and(|v| v == "1")
}

/// Everything one discovery pass found, per transport.
pub struct Candidates {
    pub usb: Vec<usb::UsbCandidate>,
    pub bt: Vec<bt::BtCandidate>,
    pub ble: Vec<ble::BleCandidate>,
}

/// Scan USB, Bluetooth and BLE at once. The Bluetooth scan is blocking D-Bus
/// plus a multi-second inquiry, so it runs on the blocking pool.
pub async fn scan_all() -> Candidates {
    let (usb, bt, ble) = tokio::join!(
        usb::list_candidates(),
        tokio::task::spawn_blocking(bt::list_candidates),
        ble::list_candidates(),
    );
    let bt = bt.unwrap_or_else(|e| {
        log::warn!("discover: BT scan task failed: {e}");
        Vec::new()
    });
    Candidates { usb, bt, ble }
}
