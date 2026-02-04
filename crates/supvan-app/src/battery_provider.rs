//! BlueZ BatteryProvider1 integration via D-Bus.
//!
//! Exposes the printer's battery level to UPower/desktop power indicators
//! by registering as a BlueZ BatteryProvider1 on the system D-Bus.

use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use dbus::blocking::SyncConnection;
use dbus::channel::{MatchingReceiver, Sender};
use dbus::message::MatchRule;
use dbus::Path;
use dbus_crossroads::{Crossroads, IfaceToken};

const PROVIDER_ROOT: &str = "/com/supvan/battery";
const BLUEZ_SERVICE: &str = "org.bluez";
const BATTERY_PROVIDER_IFACE: &str = "org.bluez.BatteryProvider1";

/// Commands sent to the battery provider thread.
enum BatteryCmd {
    Add { addr: String, percentage: u8 },
    Update { addr: String, percentage: u8 },
    Remove { addr: String },
    Shutdown,
}

/// Per-device battery state.
struct DeviceState {
    percentage: u8,
    bluez_path: Path<'static>,
}

/// Handle to communicate with the battery provider thread.
pub struct BatteryProviderHandle {
    tx: mpsc::Sender<BatteryCmd>,
}

impl BatteryProviderHandle {
    pub fn add_device(&self, addr: &str, percentage: u8) {
        let _ = self.tx.send(BatteryCmd::Add {
            addr: addr.to_string(),
            percentage,
        });
    }

    pub fn update_battery(&self, addr: &str, percentage: u8) {
        let _ = self.tx.send(BatteryCmd::Update {
            addr: addr.to_string(),
            percentage,
        });
    }

    pub fn remove_device(&self, addr: &str) {
        let _ = self.tx.send(BatteryCmd::Remove {
            addr: addr.to_string(),
        });
    }
}

impl Drop for BatteryProviderHandle {
    fn drop(&mut self) {
        let _ = self.tx.send(BatteryCmd::Shutdown);
    }
}

static HANDLE: OnceLock<Option<BatteryProviderHandle>> = OnceLock::new();

/// Get the global battery provider handle, lazily starting the provider thread.
///
/// Returns `None` if the D-Bus provider could not be started (non-fatal).
pub fn handle() -> Option<&'static BatteryProviderHandle> {
    HANDLE.get_or_init(|| start()).as_ref()
}

/// Convert `"AA:BB:CC:DD:EE:FF"` to `"/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"`.
fn bt_addr_to_bluez_path(addr: &str) -> Path<'static> {
    let mangled = addr.replace(':', "_");
    Path::from(format!("/org/bluez/hci0/dev_{mangled}"))
}

/// Convert `"AA:BB:CC:DD:EE:FF"` to `"/com/supvan/battery/dev_AA_BB_CC_DD_EE_FF"`.
fn bt_addr_to_provider_path(addr: &str) -> Path<'static> {
    let mangled = addr.replace(':', "_");
    Path::from(format!("{PROVIDER_ROOT}/dev_{mangled}"))
}

/// Emit a PropertiesChanged signal for the Percentage property.
fn emit_percentage_changed(conn: &SyncConnection, path: &Path<'_>, percentage: u8) {
    use dbus::blocking::stdintf::org_freedesktop_dbus::PropertiesPropertiesChanged;
    use dbus::arg::Variant;
    use dbus::message::SignalArgs;

    let mut changed = std::collections::HashMap::new();
    changed.insert(
        "Percentage".to_string(),
        Variant(Box::new(percentage) as Box<dyn dbus::arg::RefArg>),
    );

    let signal = PropertiesPropertiesChanged {
        interface_name: BATTERY_PROVIDER_IFACE.to_string(),
        changed_properties: changed,
        invalidated_properties: vec![],
    };

    let msg = signal.to_emit_message(path);
    let _ = conn.send(msg);
}

/// Start the battery provider background thread.
///
/// Returns `None` if the system D-Bus connection or BlueZ registration fails.
fn start() -> Option<BatteryProviderHandle> {
    let conn = match SyncConnection::new_system() {
        Ok(c) => c,
        Err(e) => {
            log::warn!("battery_provider: D-Bus connect failed: {e}");
            return None;
        }
    };

    // Request a well-known name so BlueZ can call us back
    if let Err(e) = conn.request_name("com.supvan.battery", false, false, false) {
        log::warn!("battery_provider: request_name failed: {e}");
        return None;
    }

    // Register with BlueZ BatteryProviderManager1
    let bluez_proxy = conn.with_proxy(BLUEZ_SERVICE, "/org/bluez/hci0", Duration::from_secs(5));
    if let Err(e) = bluez_proxy.method_call::<(), _, _, _>(
        "org.bluez.BatteryProviderManager1",
        "RegisterBatteryProvider",
        (Path::from(PROVIDER_ROOT),),
    ) {
        log::warn!("battery_provider: RegisterBatteryProvider failed: {e}");
        // Non-fatal: BlueZ might not support BatteryProviderManager1
    }

    let (tx, rx) = mpsc::channel::<BatteryCmd>();

    let conn = Arc::new(conn);

    let conn_thread = Arc::clone(&conn);
    std::thread::Builder::new()
        .name("battery-provider".into())
        .spawn(move || {
            run_provider(conn_thread, rx);
        })
        .ok()?;

    log::info!("battery_provider: started");
    Some(BatteryProviderHandle { tx })
}

/// Main loop for the battery provider thread.
fn run_provider(conn: Arc<SyncConnection>, rx: mpsc::Receiver<BatteryCmd>) {
    let mut cr = Crossroads::new();

    // Enable ObjectManager signal emission (InterfacesAdded/Removed)
    cr.set_object_manager_support(Some(conn.clone() as Arc<dyn Sender + Send + Sync>));

    let objmgr_iface = cr.object_manager();

    // Register the BatteryProvider1 interface template
    let battery_iface: IfaceToken<DeviceState> =
        cr.register(BATTERY_PROVIDER_IFACE, |b| {
            b.property("Percentage")
                .get(|_, state: &mut DeviceState| Ok(state.percentage));
            b.property("Device")
                .get(|_, state: &mut DeviceState| Ok(state.bluez_path.clone()));
            b.property("Source")
                .get(|_, _state: &mut DeviceState| Ok("supvan-printer-app".to_string()));
        });

    // Root object with ObjectManager interface
    cr.insert(PROVIDER_ROOT, &[objmgr_iface], ());

    // Wrap Crossroads in Arc<Mutex<>> so we can use it from both the
    // D-Bus message callback and the channel handler
    let cr = Arc::new(Mutex::new(cr));

    let cr_clone = Arc::clone(&cr);
    conn.start_receive(
        MatchRule::new_method_call(),
        Box::new(move |msg, conn| {
            if let Ok(mut cr) = cr_clone.lock() {
                let _ = cr.handle_message(msg, conn);
            }
            true
        }),
    );

    log::info!("battery_provider: entering event loop");

    loop {
        // Process D-Bus messages with a short timeout so we can check the channel
        if let Err(e) = conn.process(Duration::from_millis(200)) {
            log::error!("battery_provider: D-Bus process error: {e}");
            break;
        }

        // Drain all pending commands from the channel
        loop {
            match rx.try_recv() {
                Ok(BatteryCmd::Add { addr, percentage }) => {
                    let provider_path = bt_addr_to_provider_path(&addr);
                    let bluez_path = bt_addr_to_bluez_path(&addr);

                    log::info!("battery_provider: add {addr} at {provider_path} -> {bluez_path}");

                    if let Ok(mut cr) = cr.lock() {
                        cr.insert(
                            provider_path,
                            &[battery_iface],
                            DeviceState {
                                percentage,
                                bluez_path,
                            },
                        );
                    }
                }

                Ok(BatteryCmd::Update { addr, percentage }) => {
                    let provider_path = bt_addr_to_provider_path(&addr);

                    if let Ok(mut cr) = cr.lock() {
                        if let Some(state) = cr.data_mut::<DeviceState>(&provider_path) {
                            if state.percentage != percentage {
                                log::debug!(
                                    "battery_provider: update {addr} {}% -> {percentage}%",
                                    state.percentage
                                );
                                state.percentage = percentage;
                                emit_percentage_changed(&conn, &provider_path, percentage);
                            }
                        }
                    }
                }

                Ok(BatteryCmd::Remove { addr }) => {
                    let provider_path = bt_addr_to_provider_path(&addr);

                    log::info!("battery_provider: remove {addr} at {provider_path}");

                    if let Ok(mut cr) = cr.lock() {
                        let _: Option<DeviceState> = cr.remove(&provider_path);
                    }
                }

                Ok(BatteryCmd::Shutdown) => {
                    log::info!("battery_provider: shutting down");

                    // Unregister from BlueZ
                    let proxy =
                        conn.with_proxy(BLUEZ_SERVICE, "/org/bluez/hci0", Duration::from_secs(5));
                    let _ = proxy.method_call::<(), _, _, _>(
                        "org.bluez.BatteryProviderManager1",
                        "UnregisterBatteryProvider",
                        (Path::from(PROVIDER_ROOT),),
                    );

                    return;
                }

                Err(mpsc::TryRecvError::Empty) => break,

                Err(mpsc::TryRecvError::Disconnected) => {
                    log::info!("battery_provider: channel closed, shutting down");
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bt_addr_to_bluez_path() {
        assert_eq!(
            &*bt_addr_to_bluez_path("AA:BB:CC:DD:EE:FF"),
            "/org/bluez/hci0/dev_AA_BB_CC_DD_EE_FF"
        );
    }

    #[test]
    fn test_bt_addr_to_provider_path() {
        assert_eq!(
            &*bt_addr_to_provider_path("AA:BB:CC:DD:EE:FF"),
            "/com/supvan/battery/dev_AA_BB_CC_DD_EE_FF"
        );
    }
}
