//! BlueZ D-Bus discovery for Supvan printers.
//!
//! 1. Connect to system D-Bus, find adapter
//! 2. Run a short active BT Classic scan
//! 3. Auto-pair matching unpaired devices that could speak SPP
//! 4. Report matching paired devices with SPP UUID

use dbus::blocking::Connection;
use dbus::blocking::stdintf::org_freedesktop_dbus::ObjectManager;
use std::collections::HashMap;
use std::time::Duration;

use crate::models;

type PropMap = HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>;
type IfaceMap = HashMap<String, PropMap>;
type ManagedObjects = HashMap<dbus::Path<'static>, IfaceMap>;

const SPP_UUID_PREFIX: &str = "00001101-";
const BLUEZ_SERVICE: &str = "org.bluez";

fn uuids(props: &PropMap) -> Vec<String> {
    props
        .get("UUIDs")
        .and_then(|v| v.0.as_iter())
        .into_iter()
        .flatten()
        .filter_map(|item| item.as_str())
        .map(|u| u.to_lowercase())
        .collect()
}

fn has_spp_uuid(props: &PropMap) -> bool {
    uuids(props)
        .iter()
        .any(|uuid| uuid.starts_with(SPP_UUID_PREFIX))
}

/// Whether an *unpaired* device is worth a pairing attempt.
///
/// Pairing is intrusive and the generic serial-name fallback in
/// [`models::is_matching_bt_device`] accepts names no prefix lists, so phase
/// 4's SPP check gates pairing too — but only where BlueZ knows the profiles.
/// Before SDP runs, an inquiry result carries only what fits in the EIR, often
/// nothing: an empty list means "unknown", not "no SPP", and refusing on it
/// would make the printer undiscoverable.
fn may_speak_spp(props: &PropMap) -> bool {
    let advertised = uuids(props);
    advertised.is_empty() || advertised.iter().any(|u| u.starts_with(SPP_UUID_PREFIX))
}

fn get_str_prop(props: &PropMap, key: &str) -> Option<String> {
    props.get(key).and_then(|v| v.0.as_str().map(String::from))
}

fn get_bool_prop(props: &PropMap, key: &str) -> Option<bool> {
    props.get(key).and_then(|v| {
        v.0.as_u64()
            .map(|n| n != 0)
            .or_else(|| v.0.as_i64().map(|n| n != 0))
    })
}

fn find_adapter(objects: &ManagedObjects) -> Option<dbus::Path<'static>> {
    let found = objects
        .iter()
        .find(|(_, ifaces)| ifaces.contains_key("org.bluez.Adapter1"))
        .map(|(path, _)| path.clone());
    match &found {
        Some(path) => log::debug!("find_adapter: found {path}"),
        None => log::warn!("find_adapter: no BlueZ adapter found"),
    }
    found
}

/// How long the active BR/EDR inquiry runs.
const SCAN_WINDOW: Duration = Duration::from_secs(4);

fn run_discovery(conn: &Connection, adapter: &dbus::Path<'_>) {
    let proxy = conn.with_proxy(BLUEZ_SERVICE, adapter, Duration::from_secs(5));

    let filter = HashMap::from([(
        "Transport",
        dbus::arg::Variant(Box::new("bredr".to_string()) as Box<dyn dbus::arg::RefArg>),
    )]);
    if let Err(e) =
        proxy.method_call::<(), _, _, _>("org.bluez.Adapter1", "SetDiscoveryFilter", (filter,))
    {
        log::warn!("run_discovery: SetDiscoveryFilter failed: {e}");
    }

    log::info!("run_discovery: starting a {SCAN_WINDOW:?} BT Classic scan");
    if let Err(e) = proxy.method_call::<(), _, _, _>("org.bluez.Adapter1", "StartDiscovery", ()) {
        if e.name() == Some("org.bluez.Error.InProgress") {
            log::debug!("run_discovery: scan already in progress");
        } else {
            log::warn!("run_discovery: StartDiscovery failed: {e}");
            return;
        }
    }

    std::thread::sleep(SCAN_WINDOW);

    if let Err(e) = proxy.method_call::<(), _, _, _>("org.bluez.Adapter1", "StopDiscovery", ())
        && e.name() != Some("org.bluez.Error.NotReady")
    {
        log::warn!("run_discovery: StopDiscovery failed: {e}");
    }
    log::info!("run_discovery: scan complete");
}

fn auto_pair_device(conn: &Connection, path: &dbus::Path<'_>, addr: &str) {
    let proxy = conn.with_proxy(BLUEZ_SERVICE, path, Duration::from_secs(30));

    log::info!("auto_pair_device: pairing {addr} ({path})");
    match proxy.method_call::<(), _, _, _>("org.bluez.Device1", "Pair", ()) {
        Ok(()) => log::info!("auto_pair_device: paired {addr}"),
        Err(e) if e.name() == Some("org.bluez.Error.AlreadyExists") => {
            log::debug!("auto_pair_device: {addr} already paired");
        }
        Err(e) if e.to_string().contains("AgentNotAvailable") => {
            log::warn!("auto_pair_device: {addr} needs an agent for pairing — skipping: {e}");
            return;
        }
        Err(e) => {
            log::error!("auto_pair_device: Pair({addr}) failed: {e}");
            return;
        }
    }

    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    if let Err(e) = proxy.set("org.bluez.Device1", "Trusted", true) {
        log::warn!("auto_pair_device: failed to set Trusted on {addr}: {e}");
    }
}

/// BlueZ devices that look like Supvan printers: `(path, properties,
/// address, name)`.
fn supvan_devices(
    objects: &ManagedObjects,
) -> impl Iterator<Item = (&dbus::Path<'static>, &PropMap, String, String)> {
    objects.iter().filter_map(|(path, ifaces)| {
        let props = ifaces.get("org.bluez.Device1")?;
        let address = get_str_prop(props, "Address")?;
        let name = get_str_prop(props, "Name").unwrap_or_default();
        models::is_matching_bt_device(&address, &name).then_some((path, props, address, name))
    })
}

/// One BT-attached Supvan candidate.
///
/// `name` is the BlueZ device `Name` property — the same string the firmware
/// reports as its self-id, so it cross-correlates with USB's serial.
pub struct BtCandidate {
    pub address: String,
    pub name: String,
}

/// Find Supvan printers over BlueZ: scan, pair any new ones that may speak
/// SPP, and report the paired ones that do. Blocking — the scan alone takes
/// [`SCAN_WINDOW`] — so async callers should run it under `spawn_blocking`.
pub fn list_candidates() -> Vec<BtCandidate> {
    let conn = match Connection::new_system() {
        Ok(c) => c,
        Err(e) => {
            log::error!("bt discover: D-Bus connection failed: {e}");
            return Vec::new();
        }
    };
    let proxy = conn.with_proxy(BLUEZ_SERVICE, "/", Duration::from_secs(5));
    let managed_objects = || {
        proxy
            .get_managed_objects()
            .inspect_err(|e| log::error!("bt discover: GetManagedObjects failed: {e}"))
            .ok()
    };

    let Some(objects) = managed_objects() else {
        return Vec::new();
    };
    let Some(adapter) = find_adapter(&objects) else {
        return Vec::new();
    };
    run_discovery(&conn, &adapter);

    let Some(mut objects) = managed_objects() else {
        return Vec::new();
    };
    let mut paired_any = false;
    for (path, props, address, name) in supvan_devices(&objects) {
        if get_bool_prop(props, "Paired").unwrap_or(false) {
            continue;
        }
        if !may_speak_spp(props) {
            log::debug!("bt discover: {name} ({address}) — advertises no SPP, not pairing");
            continue;
        }
        log::info!("bt discover: found unpaired match: {name} ({address})");
        auto_pair_device(&conn, path, &address);
        paired_any = true;
    }
    if paired_any {
        let Some(after) = managed_objects() else {
            return Vec::new();
        };
        objects = after;
    }

    supvan_devices(&objects)
        .filter(|(_, props, address, name)| {
            let spp = has_spp_uuid(props);
            if !spp {
                log::debug!("bt discover: {name} ({address}) — no SPP UUID, skipping");
            }
            spp
        })
        .map(|(_, _, address, name)| {
            log::info!("bt discover: reporting {name} ({address})");
            BtCandidate { address, name }
        })
        .collect()
}
