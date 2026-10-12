//! Device open helpers for `supvan://` and `mock://` URIs.
//!
//! `supvan://<name>` is the unified scheme — discovery cross-correlates USB,
//! Bluetooth and BLE candidates by the printer's self-reported name and
//! registers a per-name transport map via [`register_supvan`]. At open time
//! [`open_uri`] resolves the name to USB (preferred when present), then
//! Bluetooth, then BLE.
//!
//! Wireless connections are cached per address: reopening reuses the link
//! instead of redialing the printer, which would beep on every connect. Each
//! open validates the cached link with a CHECK_DEVICE round-trip; if that
//! fails the entry is evicted and a fresh link is dialed (which beeps once,
//! as expected for "printer came back").

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use supvan_proto::printer::Printer;

use crate::battery_provider;
use crate::printer_device::{KsDevice, SharedPrinter};

pub const SUPVAN_SCHEME: &str = "supvan://";
pub const MOCK_SCHEME: &str = "mock://";

/// Whether this app serves `uri`.
pub fn is_ours(uri: &str) -> bool {
    uri.starts_with(SUPVAN_SCHEME) || uri.starts_with(MOCK_SCHEME)
}

/// Open connections, keyed by address. The map lock is held only briefly;
/// each printer sits behind its own async mutex.
type ConnCache = LazyLock<Mutex<HashMap<String, SharedPrinter>>>;

static BT_CONNS: ConnCache = LazyLock::new(Default::default);
#[cfg(feature = "ble")]
static BLE_CONNS: ConnCache = LazyLock::new(Default::default);

/// Reuse the cached connection to `addr` if it still answers, else `dial` a
/// new one and cache it.
async fn cached_or_dial(
    cache: &ConnCache,
    addr: &str,
    dial: impl AsyncFnOnce() -> Option<Printer>,
) -> Option<SharedPrinter> {
    let cached = cache.lock().unwrap().get(addr).cloned();
    if let Some(printer) = cached {
        if printer.lock().await.check_device().await.unwrap_or(false) {
            log::debug!("device: reusing the cached link to {addr}");
            return Some(printer);
        }
        log::info!("device: cached link to {addr} is dead, reconnecting");
        cache.lock().unwrap().remove(addr);
    }
    log::info!("device: dialing {addr}");
    let printer = Arc::new(tokio::sync::Mutex::new(dial().await?));
    cache
        .lock()
        .unwrap()
        .insert(addr.to_string(), printer.clone());
    Some(printer)
}

/// A Classic-Bluetooth printer by address.
async fn open_bt(addr: &str) -> Option<KsDevice> {
    let printer = cached_or_dial(&BT_CONNS, addr, async || {
        // RFCOMM connect blocks for up to the BlueZ page timeout.
        let target = addr.to_string();
        match tokio::task::spawn_blocking(move || Printer::open_bt(&target)).await {
            Ok(Ok(p)) => {
                if let Some(h) = battery_provider::handle() {
                    h.add_device(addr, 100);
                }
                Some(p)
            }
            Ok(Err(e)) => {
                log::error!("device: RFCOMM connect to {addr} failed: {e}");
                None
            }
            Err(e) => {
                log::error!("device: RFCOMM connect task for {addr} failed: {e}");
                None
            }
        }
    })
    .await?;
    Some(KsDevice::real(printer))
}

/// A BLE printer by address.
#[cfg(feature = "ble")]
async fn open_ble(addr: &str) -> Option<KsDevice> {
    let printer = cached_or_dial(&BLE_CONNS, addr, async || {
        Printer::open_ble(addr)
            .await
            .inspect_err(|e| log::error!("device: GATT connect to {addr} failed: {e}"))
            .ok()
    })
    .await?;
    Some(KsDevice::real(printer))
}

#[cfg(not(feature = "ble"))]
async fn open_ble(addr: &str) -> Option<KsDevice> {
    log::warn!("device: BLE address {addr} registered but the `ble` feature is off");
    None
}

/// A USB printer at a hidraw path. Not cached: reopening USB is silent.
fn open_usb(path: &str) -> Option<KsDevice> {
    log::info!("device: opening {path}");
    let printer = Printer::open_usb(path)
        .inspect_err(|e| log::error!("device: hidraw open of {path} failed: {e}"))
        .ok()?;
    Some(KsDevice::real(Arc::new(tokio::sync::Mutex::new(printer))))
}

/// Open a device from its URI. `None` for an unknown scheme or a printer
/// that can't be reached.
pub async fn open_uri(uri: &str) -> Option<KsDevice> {
    if let Some(slug) = uri.strip_prefix(SUPVAN_SCHEME) {
        open_supvan(slug).await
    } else if uri.starts_with(MOCK_SCHEME) {
        open_mock()
    } else {
        None
    }
}

/// The mock device, unless the simulator says the hardware is unplugged —
/// then the open fails like real hardware would, so status reports OFFLINE
/// and the print path holds the job.
fn open_mock() -> Option<KsDevice> {
    if crate::mock::controller().is_unreachable() {
        log::info!("mock: device unreachable (SUPVAN_MOCK_UNREACHABLE)");
        return None;
    }
    Some(KsDevice::mock())
}

/// Transports discovery found for one `supvan://NAME` printer.
#[derive(Clone, Default)]
struct SupvanTransports {
    hidraw_path: Option<String>,
    bt_address: Option<String>,
    ble_address: Option<String>,
}

static SUPVAN_MAP: LazyLock<Mutex<HashMap<String, SupvanTransports>>> =
    LazyLock::new(Default::default);

/// Record the transports discovery found for `supvan://<slug>`. Called after
/// each discovery cycle.
pub fn register_supvan(
    slug: &str,
    hidraw_path: Option<String>,
    bt_address: Option<String>,
    ble_address: Option<String>,
) {
    SUPVAN_MAP.lock().unwrap().insert(
        slug.to_string(),
        SupvanTransports {
            hidraw_path,
            bt_address,
            ble_address,
        },
    );
}

/// Open `supvan://<slug>`: USB when present, else Bluetooth, else BLE.
async fn open_supvan(slug: &str) -> Option<KsDevice> {
    let entry = SUPVAN_MAP.lock().unwrap().get(slug).cloned()?;

    if let Some(path) = entry.hidraw_path.as_deref() {
        if let Some(dev) = open_usb(path) {
            return Some(dev);
        }
        log::warn!("open_supvan: USB open failed for {slug} ({path}), falling back to BT");
    }
    if let Some(addr) = entry.bt_address.as_deref() {
        if let Some(dev) = open_bt(addr).await {
            return Some(dev);
        }
        log::warn!("open_supvan: BT open failed for {slug} ({addr}), trying BLE");
    }
    if let Some(addr) = entry.ble_address.as_deref() {
        return open_ble(addr).await;
    }
    log::warn!("open_supvan: no transports for {slug}");
    None
}
