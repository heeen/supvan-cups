//! BlueZ D-Bus discovery for Supvan printers.
//!
//! 1. Connect to system D-Bus, find adapter
//! 2. Run a 4-second active BT Classic scan
//! 3. Auto-pair matching unpaired devices
//! 4. Report matching paired devices with SPP UUID

use dbus::blocking::stdintf::org_freedesktop_dbus::ObjectManager;
use dbus::blocking::Connection;
use std::time::Duration;

type PropMap = std::collections::HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>;
type IfaceMap = std::collections::HashMap<String, PropMap>;
type ManagedObjects = std::collections::HashMap<dbus::Path<'static>, IfaceMap>;

const SPP_UUID_PREFIX: &str = "00001101-";
const BLUEZ_SERVICE: &str = "org.bluez";

fn is_matching_printer_name(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.contains("t50")
        || lower.contains("t0117")
        || lower.contains("supvan")
        || lower.contains("katasymbol")
}

fn has_spp_uuid(props: &PropMap) -> bool {
    props
        .get("UUIDs")
        .map(|v| {
            if let Some(iter) = v.0.as_iter() {
                for item in iter {
                    if let Some(uuid) = item.as_str() {
                        if uuid.to_lowercase().starts_with(SPP_UUID_PREFIX) {
                            return true;
                        }
                    }
                }
            }
            false
        })
        .unwrap_or(false)
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
    for (path, ifaces) in objects {
        if ifaces.contains_key("org.bluez.Adapter1") {
            log::debug!("find_adapter: found {path}");
            return Some(path.clone());
        }
    }
    log::warn!("find_adapter: no BlueZ adapter found");
    None
}

fn run_discovery(conn: &Connection, adapter: &dbus::Path<'_>) {
    let proxy = conn.with_proxy(BLUEZ_SERVICE, adapter, Duration::from_secs(5));

    log::debug!("run_discovery: setting filter to bredr");
    let filter: std::collections::HashMap<&str, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>> = {
        let mut m = std::collections::HashMap::new();
        m.insert(
            "Transport",
            dbus::arg::Variant(Box::new("bredr".to_string()) as Box<dyn dbus::arg::RefArg>),
        );
        m
    };
    if let Err(e) =
        proxy.method_call::<(), _, _, _>("org.bluez.Adapter1", "SetDiscoveryFilter", (filter,))
    {
        log::warn!("run_discovery: SetDiscoveryFilter failed: {e}");
    }

    log::info!("run_discovery: starting 4s BT Classic scan");
    match proxy.method_call::<(), _, _, _>("org.bluez.Adapter1", "StartDiscovery", ()) {
        Ok(()) => {}
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("InProgress") {
                log::debug!("run_discovery: scan already in progress");
            } else {
                log::warn!("run_discovery: StartDiscovery failed: {e}");
                return;
            }
        }
    }

    std::thread::sleep(Duration::from_secs(4));

    if let Err(e) = proxy.method_call::<(), _, _, _>("org.bluez.Adapter1", "StopDiscovery", ()) {
        let msg = e.to_string();
        if !msg.contains("NotReady") {
            log::warn!("run_discovery: StopDiscovery failed: {e}");
        }
    }

    log::info!("run_discovery: scan complete");
}

fn auto_pair_device(conn: &Connection, path: &dbus::Path<'_>, addr: &str) {
    let proxy = conn.with_proxy(BLUEZ_SERVICE, path, Duration::from_secs(30));

    log::info!("auto_pair_device: pairing {addr} ({path})");
    match proxy.method_call::<(), _, _, _>("org.bluez.Device1", "Pair", ()) {
        Ok(()) => log::info!("auto_pair_device: paired {addr}"),
        Err(e) => {
            let msg = e.to_string();
            if msg.contains("AlreadyExists") {
                log::debug!("auto_pair_device: {addr} already paired");
            } else if msg.contains("AgentNotAvailable") {
                log::warn!("auto_pair_device: {addr} needs agent for pairing — skipping");
                return;
            } else {
                log::error!("auto_pair_device: Pair({addr}) failed: {e}");
                return;
            }
        }
    }

    use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
    if let Err(e) = proxy.set("org.bluez.Device1", "Trusted", true) {
        log::warn!("auto_pair_device: failed to set Trusted on {addr}: {e}");
    } else {
        log::debug!("auto_pair_device: set Trusted=true on {addr}");
    }
}

/// Discover Supvan printers via BlueZ D-Bus.
///
/// For each found device, calls `cb(device_info, device_uri, device_id)`.
/// Return `false` from the callback to stop enumeration.
/// Returns true if enumeration succeeded (even if no devices found).
pub fn discover<F>(mut cb: F) -> bool
where
    F: FnMut(&str, &str, &str) -> bool,
{
    log::info!("discover: starting");

    let conn = match Connection::new_system() {
        Ok(c) => c,
        Err(e) => {
            log::error!("discover: D-Bus connection failed: {e}");
            return false;
        }
    };

    // Phase 1: Get initial state, find adapter
    log::debug!("discover: phase 1 — GetManagedObjects (pre-scan)");
    let proxy = conn.with_proxy(BLUEZ_SERVICE, "/", Duration::from_secs(5));
    let objects: ManagedObjects = match proxy.get_managed_objects() {
        Ok(o) => o,
        Err(e) => {
            log::error!("discover: GetManagedObjects failed: {e}");
            return false;
        }
    };

    let adapter = match find_adapter(&objects) {
        Some(a) => a,
        None => return true,
    };

    // Phase 2: Active BT Classic scan
    log::debug!("discover: phase 2 — active scan");
    run_discovery(&conn, &adapter);

    // Phase 3: Re-read objects, auto-pair matching unpaired devices
    log::debug!("discover: phase 3 — GetManagedObjects (post-scan), auto-pair");
    let objects: ManagedObjects = match proxy.get_managed_objects() {
        Ok(o) => o,
        Err(e) => {
            log::error!("discover: GetManagedObjects (post-scan) failed: {e}");
            return false;
        }
    };

    for (path, ifaces) in &objects {
        let props = match ifaces.get("org.bluez.Device1") {
            Some(p) => p,
            None => continue,
        };

        let name = get_str_prop(props, "Name").unwrap_or_default();
        if !is_matching_printer_name(&name) {
            continue;
        }

        let paired = get_bool_prop(props, "Paired").unwrap_or(false);
        if paired {
            continue;
        }

        let address = match get_str_prop(props, "Address") {
            Some(a) => a,
            None => continue,
        };

        log::info!("discover: found unpaired match: {name} ({address})");
        auto_pair_device(&conn, path, &address);
    }

    // Phase 4: Final read — report matching paired devices with SPP
    log::debug!("discover: phase 4 — GetManagedObjects (post-pair), report");
    let objects: ManagedObjects = match proxy.get_managed_objects() {
        Ok(o) => o,
        Err(e) => {
            log::error!("discover: GetManagedObjects (post-pair) failed: {e}");
            return false;
        }
    };

    for (path, ifaces) in &objects {
        let path_str = path.to_string();
        if !path_str.starts_with("/org/bluez/hci") || !path_str.contains("/dev_") {
            continue;
        }

        let props = match ifaces.get("org.bluez.Device1") {
            Some(p) => p,
            None => continue,
        };

        let address = match get_str_prop(props, "Address") {
            Some(a) => a,
            None => continue,
        };

        let name = get_str_prop(props, "Name").unwrap_or_default();

        if !has_spp_uuid(props) {
            log::debug!("discover: {name} ({address}) — no SPP UUID, skipping");
            continue;
        }

        if !is_matching_printer_name(&name) {
            continue;
        }

        log::info!("discover: reporting {name} ({address})");

        let device_info = format!("Supvan T50 Pro {name}");
        let device_uri = format!("btrfcomm://bt/{address}");
        let device_id = "MFG:Supvan;MDL:T50 Pro;CMD:SUPVAN;";

        if !cb(&device_info, &device_uri, device_id) {
            break;
        }
    }

    log::info!("discover: done");
    true
}
