//! Device open helpers for `btrfcomm://`, `usbhid://`, and `mock://` URIs.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::battery_provider;
use crate::printer_device::KsDevice;
use crate::usb_discover;

static LAST_PRINT_TIME: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

/// Record that a print job is active.
pub fn bt_touch_print_time() {
    LAST_PRINT_TIME.store(now_secs(), Ordering::Relaxed);
}

/// Open `btrfcomm://host/path/AA:BB:CC:DD:EE:FF`.
pub fn open_bt(uri: &str) -> Option<Box<KsDevice>> {
    let addr = uri
        .strip_prefix("btrfcomm://")
        .and_then(|rest| rest.find('/').map(|pos| &rest[pos + 1..]))?;
    bt_touch_print_time();
    let dev = KsDevice::open(addr)?;
    if let Some(h) = battery_provider::handle() {
        h.add_device(addr, 100);
    }
    Some(dev)
}

/// Open `usbhid://SERIAL` or `usbhid://bus-N-path`.
pub fn open_usb(uri: &str) -> Option<KsDevice> {
    let id = uri.strip_prefix("usbhid://")?;
    let hidraw_path = usb_discover::find_device_by_id(id)?;
    log::info!("open_usb: {id} -> {hidraw_path}");
    KsDevice::open_usb(&hidraw_path).map(|b| *b)
}

/// Open `mock://ID`. Always succeeds with a no-connection KsDevice driven by
/// the [`crate::mock`] controller. Only registered when `SUPVAN_MOCK=1`.
pub fn open_mock(_uri: &str) -> Option<KsDevice> {
    Some(KsDevice::open_mock())
}
