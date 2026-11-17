//! macOS LE scan over CoreBluetooth (`btleplug`).
//!
//! Mirrors the BlueZ scanner in [`super`]: listen to advertisements for a few
//! seconds, then read each device's properties. Two things are forced by
//! CoreBluetooth:
//!
//! - There is no MAC address, only a per-host UUID, so Supvan's OUI cannot be
//!   checked. The advertised name — the firmware serial-name, `T0…` — is the
//!   discriminator instead.
//! - The T50M Pro advertises no service UUIDs at all, so demanding a Supvan
//!   GATT service here would hide it. The service is resolved on connect.
//!
//! The reported address is the peripheral's UUID string, which is exactly what
//! `supvan_proto::ble::BlePipe::connect` takes on macOS.

use std::collections::BTreeSet;
use std::time::Duration;

use btleplug::api::{
    Central, CentralEvent, CentralState, Manager as _, Peripheral as _, ScanFilter,
};
use btleplug::platform::Manager;
use futures_util::StreamExt;

use super::{BleCandidate, is_supvan_ble_name};

/// How long to listen for advertisements before reporting.
const SCAN_WINDOW: Duration = Duration::from_secs(4);

/// Public only in name: `mod imp_macos` is private, so this surfaces solely
/// through the re-export in [`super`].
pub async fn list_candidates() -> Vec<BleCandidate> {
    match scan().await {
        Ok(found) => found,
        Err(e) => {
            log::warn!("ble_discover: LE scan failed: {e}");
            Vec::new()
        }
    }
}

async fn scan() -> btleplug::Result<Vec<BleCandidate>> {
    let manager = Manager::new().await?;
    // CoreBluetooth exposes a single built-in central; take the first adapter.
    let Some(adapter) = manager.adapters().await?.into_iter().next() else {
        log::debug!("ble_discover: no Bluetooth adapter");
        return Ok(Vec::new());
    };
    // An unauthorized adapter reads as `Unknown`, and scanning it silently
    // yields nothing — make that distinguishable from "printer is off".
    match adapter.adapter_state().await? {
        CentralState::PoweredOn => {}
        state => {
            log::warn!(
                "ble_discover: Bluetooth {state:?}; check power and Privacy & Security -> Bluetooth"
            );
            return Ok(Vec::new());
        }
    }

    // Subscribe before scanning: btleplug only forwards events that arrive
    // after the stream exists, so a printer already advertising would
    // otherwise never appear.
    let mut events = adapter.events().await?;
    adapter.start_scan(ScanFilter::default()).await?;

    let mut seen = BTreeSet::new();
    let collect = async {
        while let Some(ev) = events.next().await {
            match ev {
                CentralEvent::DeviceDiscovered(id) | CentralEvent::DeviceUpdated(id) => {
                    seen.insert(id);
                }
                _ => {}
            }
        }
    };
    let _ = tokio::time::timeout(SCAN_WINDOW, collect).await;
    // Stop before reading anything: a scan left running makes connects slow
    // and flaky, and printing needs the one LE link this host has.
    adapter.stop_scan().await?;

    let mut out = Vec::new();
    for id in seen {
        let Ok(peripheral) = adapter.peripheral(&id).await else {
            continue;
        };
        // Never connect here: reading properties comes from the advertising
        // report, while a connect would occupy the single LE link.
        let Ok(Some(props)) = peripheral.properties().await else {
            continue;
        };
        let name = props
            .local_name
            .or(props.advertisement_name)
            .unwrap_or_default();
        let address = id.to_string();
        log::debug!("ble_discover: advertised {name:?} ({address})");
        if !is_supvan_ble_name(&name) {
            continue;
        }
        log::info!("ble_discover: found {name} ({address})");
        out.push(BleCandidate { address, name });
    }
    Ok(out)
}
