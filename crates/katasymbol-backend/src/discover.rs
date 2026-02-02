//! BlueZ D-Bus device discovery.
//!
//! Enumerates paired Bluetooth devices via the BlueZ D-Bus API and filters
//! for Supvan/Katasymbol printers (SPP UUID + name pattern).

use dbus::blocking::Connection;
use std::collections::HashMap;
use std::time::Duration;

type ManagedObjects = HashMap<
    dbus::Path<'static>,
    HashMap<String, HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>>,
>;

const SPP_UUID_PREFIX: &str = "00001101-";
const BLUEZ_SERVICE: &str = "org.bluez";

/// A discovered Supvan printer.
pub struct DiscoveredDevice {
    pub address: String,
    pub name: String,
}

/// Discover paired Supvan printers via BlueZ D-Bus.
///
/// Returns list of devices matching SPP UUID and name patterns.
pub fn discover_devices() -> Vec<DiscoveredDevice> {
    let conn = match Connection::new_system() {
        Ok(c) => c,
        Err(e) => {
            log::error!("D-Bus system bus connection failed: {e}");
            return Vec::new();
        }
    };

    let proxy = conn.with_proxy(BLUEZ_SERVICE, "/", Duration::from_secs(5));

    // Use GetManagedObjects to enumerate all BlueZ objects
    use dbus::blocking::stdintf::org_freedesktop_dbus::ObjectManager;
    let objects: ManagedObjects = match proxy.get_managed_objects() {
        Ok(o) => o,
        Err(e) => {
            log::error!("GetManagedObjects failed: {e}");
            return Vec::new();
        }
    };

    let mut devices = Vec::new();

    for (path, interfaces) in &objects {
        let path_str = path.to_string();
        if !path_str.starts_with("/org/bluez/hci") || !path_str.contains("/dev_") {
            continue;
        }

        let props = match interfaces.get("org.bluez.Device1") {
            Some(p) => p,
            None => continue,
        };

        let address = match props.get("Address") {
            Some(v) => match v.0.as_str() {
                Some(s) => s.to_string(),
                None => continue,
            },
            None => continue,
        };

        let name = props
            .get("Name")
            .and_then(|v| v.0.as_str().map(String::from))
            .unwrap_or_default();

        // Check for SPP UUID
        let has_spp = props
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
            .unwrap_or(false);

        if !has_spp {
            continue;
        }

        // Filter by name pattern
        let name_lower = name.to_lowercase();
        if name_lower.contains("t50")
            || name_lower.contains("t0117")
            || name_lower.contains("supvan")
            || name_lower.contains("katasymbol")
        {
            log::info!("found: {} ({})", name, address);
            devices.push(DiscoveredDevice { address, name });
        }
    }

    devices
}

/// Print device list in CUPS backend discovery format.
///
/// Format: `direct katasymbol://<ADDR> "Katasymbol M50 Pro" "Katasymbol M50 Pro (<name>)"`
pub fn print_discovery() {
    let devices = discover_devices();
    if devices.is_empty() {
        log::info!("no Supvan devices found");
        return;
    }
    for dev in &devices {
        // CUPS backend discovery output goes to stdout
        println!(
            "direct katasymbol://{} \"Katasymbol M50 Pro\" \"Katasymbol M50 Pro ({})\"",
            dev.address, dev.name
        );
    }
}
