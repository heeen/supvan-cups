//! Device open helpers for `supvan://`, `mock://`, and legacy
//! `btrfcomm://` / `usbhid://` URIs.
//!
//! BT connections are cached per address: opening the same address twice
//! reuses the existing RFCOMM socket instead of redialing the printer, which
//! would beep on every connect. Each call validates the cached socket with
//! a CHECK_DEVICE round-trip; if that fails the entry is evicted and a fresh
//! socket is dialed (which beeps once, as expected for "printer came back").
//!
//! `supvan://<name>` is the unified scheme — discovery cross-correlates USB
//! and BT candidates by the printer's self-reported name and registers a
//! per-name transport mapping via [`register_supvan`]. At open time
//! [`open_supvan`] resolves the name to USB (preferred when present) or BT.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use supvan_proto::bt_transport::BtTransport;
use supvan_proto::printer::Printer;
use supvan_proto::rfcomm::RfcommSocket;

use crate::battery_provider;
use crate::printer_device::KsDevice;

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

/// BT printer connection cache, keyed by address. Persists across `open_bt`
/// calls so the status poller and print jobs reuse one RFCOMM socket per
/// printer.
fn bt_cache() -> &'static Mutex<HashMap<String, Arc<Mutex<Printer>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<Mutex<Printer>>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

fn dial_bt(addr: &str) -> Option<Printer> {
    log::info!("device::open_bt: dialing {addr} (no cache entry)");
    let sock = match RfcommSocket::connect_default(addr) {
        Ok(s) => s,
        Err(e) => {
            log::error!("device::open_bt: RFCOMM connect failed for {addr}: {e}");
            return None;
        }
    };
    Some(Printer::new(Box::new(BtTransport::new(sock))))
}

/// Open `btrfcomm://host/path/AA:BB:CC:DD:EE:FF`, reusing a cached RFCOMM
/// socket when one is available. Drops the cache entry and reconnects if the
/// existing socket no longer responds.
pub fn open_bt(uri: &str) -> Option<Box<KsDevice>> {
    let addr = uri
        .strip_prefix("btrfcomm://")
        .and_then(|rest| rest.find('/').map(|pos| &rest[pos + 1..]))?;
    bt_touch_print_time();

    let printer = {
        let cached = bt_cache().lock().unwrap().get(addr).cloned();
        match cached {
            Some(arc) => {
                let alive = arc.lock().unwrap().check_device().unwrap_or(false);
                if alive {
                    log::debug!("device::open_bt: reusing cached socket for {addr}");
                    arc
                } else {
                    log::info!("device::open_bt: cached socket for {addr} is dead, reconnecting");
                    bt_cache().lock().unwrap().remove(addr);
                    let fresh = dial_bt(addr)?;
                    let arced = Arc::new(Mutex::new(fresh));
                    bt_cache()
                        .lock()
                        .unwrap()
                        .insert(addr.to_string(), arced.clone());
                    arced
                }
            }
            None => {
                let fresh = dial_bt(addr)?;
                let arced = Arc::new(Mutex::new(fresh));
                bt_cache()
                    .lock()
                    .unwrap()
                    .insert(addr.to_string(), arced.clone());
                arced
            }
        }
    };

    if let Some(h) = battery_provider::handle() {
        h.add_device(addr, 100);
    }
    Some(Box::new(KsDevice::from_shared(printer)))
}

/// Open `mock://ID`. Always succeeds with a no-connection KsDevice driven by
/// the [`crate::mock`] controller. Only registered when `SUPVAN_MOCK=1`.
pub fn open_mock(_uri: &str) -> Option<KsDevice> {
    // Simulate powered-off / unplugged hardware: the device can't be opened,
    // so poll_status reports OFFLINE and the print path holds the job.
    if crate::mock::controller().is_unreachable() {
        log::info!("mock: device unreachable (SUPVAN_MOCK_UNREACHABLE)");
        return None;
    }
    Some(KsDevice::open_mock())
}

/// Transport map for `supvan://NAME` URIs, populated by discovery and
/// consulted by [`open_supvan`] / [`poll_status`].
#[derive(Clone, Default)]
struct SupvanTransports {
    hidraw_path: Option<String>,
    bt_address: Option<String>,
}

fn supvan_map() -> &'static Mutex<HashMap<String, SupvanTransports>> {
    static MAP: OnceLock<Mutex<HashMap<String, SupvanTransports>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record the active USB and/or BT transports for a `supvan://<slug>` printer.
/// Called from [`crate::ipp_server::SupvanDeviceBackend::list`] after each
/// discovery cycle.
pub fn register_supvan(slug: &str, hidraw_path: Option<String>, bt_address: Option<String>) {
    supvan_map().lock().unwrap().insert(
        slug.to_string(),
        SupvanTransports {
            hidraw_path,
            bt_address,
        },
    );
}

/// Open `supvan://<slug>`. Prefers USB when available, falls back to the
/// cached BT socket. Returns `None` if neither transport is registered or
/// both fail to open.
pub fn open_supvan(uri: &str) -> Option<KsDevice> {
    let slug = uri.strip_prefix("supvan://")?;
    let entry = supvan_map().lock().unwrap().get(slug).cloned()?;

    if let Some(path) = entry.hidraw_path.as_deref() {
        if let Some(dev) = KsDevice::open_usb(path) {
            return Some(*dev);
        }
        log::warn!("open_supvan: USB open failed for {slug} ({path}), falling back to BT");
    }
    if let Some(addr) = entry.bt_address.as_deref() {
        // open_bt expects a full URI; synthesize one.
        let uri = format!("btrfcomm://bt/{addr}");
        return open_bt(&uri).map(|b| *b);
    }
    log::warn!("open_supvan: no transports for {slug}");
    None
}
