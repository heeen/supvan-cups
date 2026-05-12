//! PAPPL device schemes for `btrfcomm://` and `usbhid://` URIs.
//!
//! Implements [`DeviceScheme`] for [`BtScheme`] and [`UsbScheme`], replacing
//! the previous 12 `unsafe extern "C"` callback functions with trait impls.

use std::ffi::CStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pappl_rs::device::DeviceScheme;
use pappl_rs::flags::{DeviceType, PrinterReason};

use crate::battery_provider;
use crate::discover;
use crate::printer_device::KsDevice;
use crate::usb_discover;

// --- BT connection cache ---
//
// PAPPL opens/closes the device for every status poll and print job.
// For BT, each cycle is a full RFCOMM connect/disconnect which destabilizes
// the link. We cache the last connection and reuse it if the address matches.
//
// To avoid draining the printer battery, the cache has an idle timeout:
// connections are only cached while a print job is recent. After the timeout,
// connections are dropped so the printer can sleep. Configurable via
// SUPVAN_BT_IDLE_TIMEOUT (seconds, default 120).

static BT_CONN_CACHE: Mutex<Option<Box<KsDevice>>> = Mutex::new(None);

/// Epoch seconds of the last print job start. 0 = no job yet.
static LAST_PRINT_TIME: AtomicU64 = AtomicU64::new(0);

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or(Duration::ZERO)
        .as_secs()
}

fn bt_idle_timeout() -> u64 {
    static CACHED: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("SUPVAN_BT_IDLE_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(120)
    })
}

/// Record that a print job is active (call from rstartjob).
pub fn bt_touch_print_time() {
    LAST_PRINT_TIME.store(now_secs(), Ordering::Relaxed);
}

/// Take a cached BT connection for `addr`, or open a new one.
fn bt_conn_open(addr: &str) -> Option<Box<KsDevice>> {
    if let Ok(mut cache) = BT_CONN_CACHE.lock() {
        if let Some(dev) = cache.take() {
            if dev.addr.as_deref() == Some(addr) {
                if dev.is_alive() {
                    log::info!("bt_conn_open: reusing cached connection to {addr}");
                    return Some(dev);
                }
                log::info!("bt_conn_open: cached connection to {addr} is dead, reconnecting");
                drop(dev);
            } else {
                log::info!("bt_conn_open: dropping cached connection (different addr)");
                drop(dev);
            }
        }
    }
    KsDevice::open(addr)
}

/// Return a BT connection to the cache instead of dropping it.
/// Drops the connection if the socket is dead or idle too long (so the
/// printer can sleep and save battery).
fn bt_conn_close(dev: Box<KsDevice>) {
    let addr = dev.addr.as_deref().unwrap_or("?");
    if !dev.is_alive() {
        log::info!("bt_conn_close: connection to {addr} is dead, dropping");
        return;
    }
    let last = LAST_PRINT_TIME.load(Ordering::Relaxed);
    let idle = now_secs().saturating_sub(last);
    if last == 0 || idle > bt_idle_timeout() {
        log::info!(
            "bt_conn_close: idle {idle}s > timeout {}s, dropping connection to {addr}",
            bt_idle_timeout()
        );
        return;
    }
    if let Ok(mut cache) = BT_CONN_CACHE.lock() {
        log::info!("bt_conn_close: caching connection to {addr} (idle {idle}s)");
        *cache = Some(dev);
    }
}

// --- Bluetooth RFCOMM scheme -------------------------------------------------

/// `btrfcomm://` device scheme. Discovers Supvan BT printers via BlueZ D-Bus
/// and communicates over RFCOMM. Skips reporting BT devices that are also
/// available over USB HID, since USB is more reliable.
pub struct BtScheme;

impl DeviceScheme for BtScheme {
    const NAME: &'static CStr = c"btrfcomm";
    const DEVICE_TYPE: DeviceType = DeviceType::CUSTOM_LOCAL;
    type Payload = KsDevice;

    fn list(emit: &mut dyn FnMut(&str, &str, &str) -> bool) {
        let usb_available = usb_discover::has_device();

        discover::discover(|device_info, device_uri, device_id| {
            if usb_available {
                log::info!("BtScheme::list: skipping BT device (USB HID available): {device_uri}");
                return true; // continue enumeration but don't report
            }
            emit(device_info, device_uri, device_id)
        });
    }

    fn open(uri: &str) -> Option<Self::Payload> {
        let addr = uri
            .strip_prefix("btrfcomm://")
            .and_then(|rest| rest.find('/').map(|pos| &rest[pos + 1..]))?;

        let dev = bt_conn_open(addr)?;

        if let Some(h) = battery_provider::handle() {
            h.add_device(addr, 100);
        }

        Some(*dev)
    }

    fn close(payload: Self::Payload) {
        bt_conn_close(Box::new(payload));
    }

    fn read(payload: &Self::Payload, buf: &mut [u8]) -> isize {
        payload.read(buf.as_mut_ptr(), buf.len())
    }

    fn write(payload: &Self::Payload, buf: &[u8]) -> isize {
        payload.write(buf.as_ptr(), buf.len())
    }

    fn status(payload: &Self::Payload) -> PrinterReason {
        payload.status()
    }
}

// --- USB HID scheme ----------------------------------------------------------

/// `usbhid://` device scheme. Discovers Supvan USB HID printers via sysfs
/// and communicates over `/dev/hidrawN`.
pub struct UsbScheme;

impl DeviceScheme for UsbScheme {
    const NAME: &'static CStr = c"usbhid";
    const DEVICE_TYPE: DeviceType = DeviceType::CUSTOM_LOCAL;
    type Payload = KsDevice;

    fn list(emit: &mut dyn FnMut(&str, &str, &str) -> bool) {
        usb_discover::discover(|device_info, device_uri, device_id| {
            emit(device_info, device_uri, device_id)
        });
    }

    fn open(uri: &str) -> Option<Self::Payload> {
        let id = uri.strip_prefix("usbhid://")?;

        log::info!("UsbScheme::open: resolving id '{id}' to hidraw path");
        let hidraw_path = match usb_discover::find_device_by_id(id) {
            Some(p) => p,
            None => {
                log::warn!("UsbScheme::open: device id '{id}' not found");
                return None;
            }
        };

        log::info!("UsbScheme::open: resolved to {hidraw_path}");
        KsDevice::open_usb(&hidraw_path).map(|b| *b)
    }

    fn read(payload: &Self::Payload, buf: &mut [u8]) -> isize {
        payload.read(buf.as_mut_ptr(), buf.len())
    }

    fn write(payload: &Self::Payload, buf: &[u8]) -> isize {
        payload.write(buf.as_ptr(), buf.len())
    }

    fn status(payload: &Self::Payload) -> PrinterReason {
        payload.status()
    }
}
