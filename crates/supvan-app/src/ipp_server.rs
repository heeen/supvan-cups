//! Application entry: IPP server, discovery, state.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use parking_lot::RwLock;
use ipp_printer_app::{
    default_state_path, DeviceBackend, JobContext, JobFailure, PrinterConfig, PrinterReason,
    PrinterRegistry, Server, ServerOptions,
};

use crate::discover::BtCandidate;
use crate::ipp_job::{config_from_family, run_cups_raster_job};
use crate::models;
use crate::usb_discover::UsbCandidate;

/// Threshold below which the printer-state-reasons gets the MEDIA_LOW flag.
/// Conservative — most label-printer ops want a few minutes of warning.
const MEDIA_LOW_THRESHOLD: u32 = 20;

/// Per-printer last-seen RFID tag identifiers; used to log roll swaps and
/// (in a future phase) refresh `media-col-ready`.
#[derive(Default, Clone, PartialEq)]
struct RollFingerprint {
    uuid: String,
    code: String,
    width_mm: u8,
    height_mm: u8,
}

fn roll_cache() -> &'static Mutex<HashMap<String, RollFingerprint>> {
    static C: OnceLock<Mutex<HashMap<String, RollFingerprint>>> = OnceLock::new();
    C.get_or_init(|| Mutex::new(HashMap::new()))
}

pub struct SupvanDeviceBackend;

/// Slugify a printer-reported name into something CUPS can use as a queue
/// name. Lowercase ASCII alphanumerics; everything else becomes a hyphen.
fn slug(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c.to_ascii_lowercase() } else { '-' })
        .collect();
    let s: String = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "printer".to_string() } else { s }
}

impl DeviceBackend for SupvanDeviceBackend {
    fn list(&self, emit: &mut dyn FnMut(&str, &str, &str) -> bool) {
        if crate::util::is_mock_mode() {
            let family = models::default_family();
            let driver = family.driver_name.to_string_lossy();
            let mdl = String::from_utf8_lossy(&family.make_and_model).into_owned();
            let device_id = format!("MFG:Supvan;MDL:{mdl};CMD:KASCRIPT;");
            emit("Supvan Mock", "mock://t50-001", &device_id);
            log::info!("mock discovery: emitted mock://t50-001 (driver={driver})");
            return;
        }

        // Collect all candidates. USB probes RD_DEV_NAME silently per device;
        // BT pulls the firmware-reported name straight from BlueZ.
        let usb = crate::usb_discover::list_candidates();
        let bt = crate::discover::list_candidates();

        // Group by printer-reported name. USB candidates carry their
        // `device_sn` (parsed from `RETURN_MAT` at offset 40); BT carries
        // it as the BlueZ `Name` property. When both match, we collapse
        // the two transports into one logical printer.
        //
        // If a USB candidate failed to surface its serial (e.g. the device
        // was busy and RETURN_MAT didn't reply), fall back to its bus URI
        // as the group key. A final 1-USB-only + 1-BT-only sweep merges
        // them under the BT name to keep single-printer households tidy.
        type Group = (Option<UsbCandidate>, Option<BtCandidate>);
        let mut by_name: BTreeMap<String, Group> = BTreeMap::new();
        for u in usb {
            let key = u.printer_name.clone().unwrap_or_else(|| u.uri_id.clone());
            by_name.entry(key).or_default().0 = Some(u);
        }
        for b in bt {
            let key = b.name.clone();
            by_name.entry(key).or_default().1 = Some(b);
        }

        let usb_only: Vec<String> = by_name
            .iter()
            .filter(|(_, (u, b))| u.is_some() && b.is_none())
            .map(|(k, _)| k.clone())
            .collect();
        let bt_only: Vec<String> = by_name
            .iter()
            .filter(|(_, (u, b))| u.is_none() && b.is_some())
            .map(|(k, _)| k.clone())
            .collect();
        if usb_only.len() == 1 && bt_only.len() == 1 {
            let usb_key = usb_only.into_iter().next().unwrap();
            let bt_key = bt_only.into_iter().next().unwrap();
            log::info!(
                "discover: USB probe failed; cardinality fallback merging {usb_key} + {bt_key} under {bt_key}"
            );
            let usb_entry = by_name.remove(&usb_key).unwrap().0;
            by_name.get_mut(&bt_key).unwrap().0 = usb_entry;
        }

        for (name, (usb, bt)) in by_name {
            let model = usb
                .as_ref()
                .map(|u| u.model_name.clone())
                .or_else(|| bt.as_ref().map(|_| "T50 Series".to_string()))
                .unwrap_or_else(|| "T50 Series".to_string());
            let info = format!("Supvan {model} {name}");
            let uri = format!("supvan://{}", slug(&name));
            let device_id = format!("MFG:Supvan;MDL:{model};CMD:SUPVAN;");
            log::info!(
                "discover: emitting {uri} (usb={}, bt={})",
                usb.is_some(),
                bt.is_some(),
            );
            // Register the name → transport mapping so open_supvan can resolve it.
            crate::device::register_supvan(
                &slug(&name),
                usb.as_ref().map(|u| u.hidraw_path.clone()),
                bt.as_ref().map(|b| b.address.clone()),
            );
            if !emit(&info, &uri, &device_id) {
                break;
            }
        }
    }

    fn poll_status(&self, config: &PrinterConfig) -> Option<PrinterReason> {
        let dev = if config.device_uri.starts_with("supvan://") {
            crate::device::open_supvan(&config.device_uri)
        } else if config.device_uri.starts_with("mock://") {
            crate::device::open_mock(&config.device_uri)
        } else {
            return None;
        }?;

        let mut reasons = dev.status();

        // Material query: surfaces labels-remaining + roll-swap detection.
        // Skipped on mock devices (dev.material() returns None).
        if let Some(mat) = dev.material() {
            let fp = RollFingerprint {
                uuid: mat.uuid.clone(),
                code: mat.code.clone(),
                width_mm: mat.width_mm,
                height_mm: mat.height_mm,
            };
            let mut cache = roll_cache().lock().unwrap();
            let key = config.name.clone();
            match cache.get(&key) {
                Some(prev) if *prev != fp && !prev.uuid.is_empty() => {
                    log::info!(
                        "{}: roll swap detected — was {}x{}mm uuid={} -> now {}x{}mm uuid={}",
                        key,
                        prev.width_mm, prev.height_mm, prev.uuid,
                        fp.width_mm, fp.height_mm, fp.uuid,
                    );
                }
                None => {
                    log::info!(
                        "{}: roll registered — {}x{}mm uuid={} remaining={:?}",
                        key, fp.width_mm, fp.height_mm, fp.uuid, mat.remaining,
                    );
                }
                _ => {}
            }
            cache.insert(key, fp);

            if let Some(remaining) = mat.remaining {
                if remaining == 0 {
                    reasons |= PrinterReason::MEDIA_EMPTY;
                } else if remaining <= MEDIA_LOW_THRESHOLD {
                    reasons |= PrinterReason::MARKER_SUPPLY_LOW;
                }
            }
        }

        Some(reasons)
    }

    fn driver_for_device(&self, device_id: &str, device_uri: &str) -> Option<String> {
        if !device_id.is_empty() {
            if let Some(mdl) = models::parse_mdl(device_id) {
                let family = models::family_for_model_hint(mdl);
                return Some(family.driver_name.to_string_lossy().into_owned());
            }
        }
        if device_uri.starts_with("supvan://") || device_uri.starts_with("mock://") {
            return Some(
                models::default_family()
                    .driver_name
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        None
    }
}

pub async fn run_server(host: &str, port: u16) -> std::io::Result<()> {
    models::load();

    let registry: PrinterRegistry = Arc::new(RwLock::new(Vec::new()));
    let state_path = default_state_path("supvan-printer-app");
    let backend = Arc::new(SupvanDeviceBackend);

    Server::bootstrap_printers(
        &registry,
        backend.as_ref(),
        &state_path,
        config_from_family,
    );

    prune_stale_supvan(&registry);
    Server::persist(&registry, &state_path);

    let registry_print = registry.clone();
    let print_job = Arc::new(
        move |ctx: JobContext, raster: Vec<u8>, copies: u32| -> Result<(), JobFailure> {
            let cfg = {
                let guard = registry_print.read();
                guard
                    .iter()
                    .find(|p| p.config.name == ctx.printer_name)
                    .ok_or_else(|| {
                        JobFailure::other(format!("printer not found: {}", ctx.printer_name))
                    })?
                    .config
                    .clone()
            };
            run_cups_raster_job(
                &cfg.name,
                &cfg.device_uri,
                cfg.darkness,
                cfg.printhead_width_dots,
                &cfg.driver_name,
                &raster,
                copies,
            )
        },
    );

    Server::run(ServerOptions {
        host: host.to_string(),
        port,
        printers: registry,
        device_backend: backend,
        print_job,
        state_path,
    })
    .await
}

/// Drop persisted entries whose URI scheme this build no longer recognises
/// (e.g. legacy `usbhid://` / `btrfcomm://` from before the supvan:// unification).
/// Live `supvan://` entries are kept; the next discovery cycle re-registers
/// the transport mapping.
fn prune_stale_supvan(registry: &PrinterRegistry) {
    let mut guard = registry.write();
    guard.retain(|p| {
        let uri = &p.config.device_uri;
        let keep = uri.starts_with("supvan://") || uri.starts_with("mock://");
        if !keep {
            log::info!("pruning legacy-scheme printer: {uri}");
        }
        keep
    });
}
