//! Auto-discovery lifecycle: first sighting, device disappears (queue
//! persists), device reappears (no duplicate). Exercises
//! `Server::bootstrap_printers` against a `MockBackend` and a JSON state file
//! in a per-test tempdir.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use ipp_printer_app::{
    DeviceBackend, PersistedState, PrinterConfig, PrinterRegistry, Server,
};
use parking_lot::RwLock;

/// One discovered device as `MockBackend` would emit it.
#[derive(Clone, Debug)]
struct Sighting {
    info: String,
    uri: String,
    device_id: String,
}

impl Sighting {
    fn new(uri: &str) -> Self {
        Self {
            info: format!("Mock {uri}"),
            uri: uri.to_string(),
            device_id: format!("MFG:Mock;MDL:Test;SCH:{uri}"),
        }
    }
}

struct MockBackend {
    devices: Mutex<Vec<Sighting>>,
}

impl MockBackend {
    fn with(initial: Vec<Sighting>) -> Self {
        Self {
            devices: Mutex::new(initial),
        }
    }

    fn set(&self, devices: Vec<Sighting>) {
        *self.devices.lock().unwrap() = devices;
    }
}

impl DeviceBackend for MockBackend {
    fn list(&self, emit: &mut dyn FnMut(&str, &str, &str) -> bool) {
        for s in self.devices.lock().unwrap().iter() {
            if !emit(&s.info, &s.uri, &s.device_id) {
                break;
            }
        }
    }

    fn driver_for_device(&self, _device_id: &str, _device_uri: &str) -> Option<String> {
        Some("mock_driver".into())
    }
}

fn make_config(name: &str, driver: &str, uri: &str, device_id: &str) -> Option<PrinterConfig> {
    Some(PrinterConfig {
        name: name.to_string(),
        driver_name: driver.to_string(),
        make_and_model: "Mock Printer".to_string(),
        device_id: device_id.to_string(),
        device_uri: uri.to_string(),
        dpi: 203,
        printhead_width_dots: 384,
        media_names: vec!["oe_30x20mm_30x20mm".to_string()],
        media_sizes: vec![[3000, 2000]],
        darkness: 50,
    })
}

/// Unique per-test state path under `$TMPDIR` — no external tempfile crate.
fn fresh_state_path() -> std::path::PathBuf {
    static SEQ: AtomicU32 = AtomicU32::new(0);
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    std::env::temp_dir().join(format!("ipp-printer-app-test-{pid}-{n}.state.json"))
}

fn empty_registry() -> PrinterRegistry {
    Arc::new(RwLock::new(Vec::new()))
}

#[test]
fn first_sighting_registers_and_persists() {
    let state = fresh_state_path();
    let _ = std::fs::remove_file(&state);
    let registry = empty_registry();
    let backend = MockBackend::with(vec![Sighting::new("usbhid://bus-5-2")]);

    Server::bootstrap_printers(&registry, &backend, &state, make_config);

    let records = registry.read();
    assert_eq!(records.len(), 1, "one device should be registered");
    assert_eq!(records[0].config.device_uri, "usbhid://bus-5-2");
    assert_eq!(records[0].config.driver_name, "mock_driver");

    // State file should hold the same single entry.
    let persisted = PersistedState::load(&state);
    assert_eq!(persisted.printers.len(), 1);
    assert_eq!(persisted.printers[0].device_uri, "usbhid://bus-5-2");

    let _ = std::fs::remove_file(&state);
}

#[test]
fn device_disappears_but_queue_survives() {
    // Round 1: device on, gets registered.
    let state = fresh_state_path();
    let _ = std::fs::remove_file(&state);
    let backend = MockBackend::with(vec![Sighting::new("usbhid://bus-5-2")]);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &backend, &state, make_config);
    assert_eq!(registry.read().len(), 1);
    drop(registry);

    // Round 2: simulated app restart — fresh in-memory registry, but the
    // state file is still on disk. Device is now OFF (backend lists nothing).
    backend.set(vec![]);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &backend, &state, make_config);

    let records = registry.read();
    assert_eq!(
        records.len(),
        1,
        "queue must persist across an app restart even with the device off",
    );
    assert_eq!(records[0].config.device_uri, "usbhid://bus-5-2");

    let _ = std::fs::remove_file(&state);
}

#[test]
fn device_reappears_no_duplicate() {
    // Round 1: register the device.
    let state = fresh_state_path();
    let _ = std::fs::remove_file(&state);
    let backend = MockBackend::with(vec![Sighting::new("usbhid://bus-5-2")]);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &backend, &state, make_config);
    drop(registry);

    // Round 2: device off, queue survives (covered by previous test).
    backend.set(vec![]);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &backend, &state, make_config);
    drop(registry);

    // Round 3: device on again.
    backend.set(vec![Sighting::new("usbhid://bus-5-2")]);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &backend, &state, make_config);

    let records = registry.read();
    assert_eq!(
        records.len(),
        1,
        "device reappearing must not produce a duplicate entry",
    );

    let _ = std::fs::remove_file(&state);
}

#[test]
fn new_device_added_alongside_existing() {
    // Existing device persists; a freshly-discovered second device is added.
    let state = fresh_state_path();
    let _ = std::fs::remove_file(&state);
    let backend = MockBackend::with(vec![Sighting::new("usbhid://bus-5-2")]);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &backend, &state, make_config);
    drop(registry);

    backend.set(vec![
        Sighting::new("usbhid://bus-5-2"),
        Sighting::new("btrfcomm://hci0/AA:BB:CC:DD:EE:FF"),
    ]);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &backend, &state, make_config);

    let records = registry.read();
    assert_eq!(records.len(), 2);
    let uris: Vec<&str> = records.iter().map(|r| r.config.device_uri.as_str()).collect();
    assert!(uris.contains(&"usbhid://bus-5-2"));
    assert!(uris.contains(&"btrfcomm://hci0/AA:BB:CC:DD:EE:FF"));

    let _ = std::fs::remove_file(&state);
}

#[test]
fn backend_returning_no_driver_skips_device() {
    struct NoDriverBackend;
    impl DeviceBackend for NoDriverBackend {
        fn list(&self, emit: &mut dyn FnMut(&str, &str, &str) -> bool) {
            emit("Mystery", "usbhid://mystery", "");
        }
        fn driver_for_device(&self, _: &str, _: &str) -> Option<String> {
            None
        }
    }

    let state = fresh_state_path();
    let _ = std::fs::remove_file(&state);
    let registry = empty_registry();
    Server::bootstrap_printers(&registry, &NoDriverBackend, &state, make_config);

    assert!(
        registry.read().is_empty(),
        "device with no driver match must be skipped"
    );
    let _ = std::fs::remove_file(&state);
}
