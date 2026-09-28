//! Application entry: IPP server, discovery, state.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex, OnceLock};

use ipp_printer_app::{
    DeviceBackend, DiscoveredDevice, JobContext, JobFailure, JobOutcome, PollStatus, PrinterConfig,
    PrinterReason, PrinterRegistry, ReadyMedia, Server, ServerOptions, default_state_path,
};
use parking_lot::RwLock;
use supvan_proto::rfid::{RfidMaterial, heat_presets};

use crate::ipp_job::{config_from_family, run_cups_raster_job};
use supvan_discover::ble::BleCandidate;
use supvan_discover::bt::BtCandidate;
use supvan_discover::models;
use supvan_discover::usb::UsbCandidate;

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
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let s: String = s
        .split('-')
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if s.is_empty() {
        "printer".to_string()
    } else {
        s
    }
}

#[async_trait::async_trait]
impl DeviceBackend for SupvanDeviceBackend {
    async fn list(&self) -> Vec<DiscoveredDevice> {
        if supvan_discover::util::is_mock_mode() {
            let family = models::default_family();
            let driver = family.driver_name.to_string_lossy();
            let mdl = String::from_utf8_lossy(&family.make_and_model).into_owned();
            let device_id = format!("MFG:Supvan;MDL:{mdl};CMD:KASCRIPT;");
            log::info!("mock discovery: emitted mock://t50-001 (driver={driver})");
            return vec![DiscoveredDevice {
                info: "Supvan Mock".to_string(),
                uri: "mock://t50-001".to_string(),
                device_id,
            }];
        }

        // Collect all candidates. USB probes RD_DEV_NAME silently per device;
        // BT pulls the firmware-reported name from BlueZ; BLE scans for
        // E11/E12-class advertisers (no-op without the `ble` feature).
        let usb = supvan_discover::usb::list_candidates().await;
        let bt = supvan_discover::bt::list_candidates();
        let ble = supvan_discover::ble::list_candidates().await;

        // Group by printer-reported name. USB candidates carry their
        // `device_sn` (parsed from `RETURN_MAT` at offset 40); BT and BLE carry
        // it as the advertised name. When they match, we collapse the
        // transports into one logical printer.
        //
        // If a USB candidate failed to surface its serial (e.g. the device
        // was busy and RETURN_MAT didn't reply), fall back to its bus URI
        // as the group key. A final 1-USB-only + 1-BT-only sweep merges
        // them under the BT name to keep single-printer households tidy.
        type Group = (
            Option<UsbCandidate>,
            Option<BtCandidate>,
            Option<BleCandidate>,
        );
        let mut by_name: BTreeMap<String, Group> = BTreeMap::new();
        for u in usb {
            let key = u.printer_name.clone().unwrap_or_else(|| u.uri_id.clone());
            by_name.entry(key).or_default().0 = Some(u);
        }
        for b in bt {
            let key = b.name.clone();
            by_name.entry(key).or_default().1 = Some(b);
        }
        for e in ble {
            let key = e.name.clone();
            by_name.entry(key).or_default().2 = Some(e);
        }

        let usb_only: Vec<String> = by_name
            .iter()
            .filter(|(_, (u, b, e))| u.is_some() && b.is_none() && e.is_none())
            .map(|(k, _)| k.clone())
            .collect();
        let bt_only: Vec<String> = by_name
            .iter()
            .filter(|(_, (u, b, e))| u.is_none() && b.is_some() && e.is_none())
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

        let mut out = Vec::new();
        for (name, (usb, bt, ble)) in by_name {
            // USB reports its model over the wire; BT and BLE only give us the
            // advertised serial name, which the registry maps back to a model.
            let model = usb
                .as_ref()
                .map(|u| u.model_name.clone())
                .or_else(|| models::bt_model_for_name(&name).map(str::to_string))
                .unwrap_or_else(|| "T50 Series".to_string());
            let info = format!("Supvan {model} {name}");
            let uri = format!("supvan://{}", slug(&name));
            let device_id = format!("MFG:Supvan;MDL:{model};CMD:SUPVAN;");
            log::info!(
                "discover: emitting {uri} (usb={}, bt={}, ble={})",
                usb.is_some(),
                bt.is_some(),
                ble.is_some(),
            );
            // Register the name → transport mapping so open_supvan can resolve it.
            crate::device::register_supvan(
                &slug(&name),
                model.clone(),
                usb.as_ref().map(|u| u.hidraw_path.clone()),
                bt.as_ref().map(|b| b.address.clone()),
                ble.as_ref().map(|e| e.address.clone()),
            );
            out.push(DiscoveredDevice {
                info,
                uri,
                device_id,
            });
        }
        out
    }

    async fn poll_status(&self, config: &PrinterConfig) -> Option<PollStatus> {
        let dev = crate::device::open_uri(&config.device_uri).await;
        let Some(dev) = dev else {
            // Device unreachable (powered off / unplugged / BT down). Report
            // OFFLINE so the framework marks us printer-state=stopped and CUPS
            // holds queued jobs until it's back — instead of accepting a job
            // we can't print and dropping it.
            return Some(PollStatus {
                reasons: PrinterReason::OFFLINE,
                ..Default::default()
            });
        };

        let mut reasons = dev.status().await;
        let mut ready_media = None;
        let mut supply_percent = None;

        // Material query: surfaces labels-remaining + roll-swap detection.
        // Skipped on mock devices (dev.material() returns None).
        if let Some(mat) = dev.material().await {
            let profile = match dev.printer.as_ref() {
                Some(printer) => printer.profile().await,
                None => supvan_proto::profile::PrintProfile::TSeries,
            };

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
                        prev.width_mm,
                        prev.height_mm,
                        prev.uuid,
                        fp.width_mm,
                        fp.height_mm,
                        fp.uuid,
                    );
                }
                None => {
                    log::info!(
                        "{}: roll registered — {}x{}mm uuid={} remaining={:?}",
                        key,
                        fp.width_mm,
                        fp.height_mm,
                        fp.uuid,
                        mat.remaining,
                    );
                }
                _ => {}
            }
            cache.insert(key, fp);

            // Publish the loaded roll as the dynamic media-ready / media-col-ready.
            // PWG self-describing name uses the om_ (metric) class; size in
            // hundredths of a millimetre.
            let (w, h) = (mat.width_mm as i32, mat.height_mm as i32);
            if w > 0 && h > 0 {
                ready_media = Some(ReadyMedia {
                    name: format!("om_{w}x{h}mm_{w}x{h}mm"),
                    size_hmm: [w * 100, h * 100],
                    media_type: "labels".to_string(),
                });
            }

            if let Some(remaining) = mat.remaining {
                if remaining == 0 {
                    if profile == supvan_proto::profile::PrintProfile::TSeries {
                        reasons |= PrinterReason::MEDIA_EMPTY;
                    } else {
                        log::debug!(
                            "{}: ignoring zero remaining-label count for E-series profile",
                            config.name
                        );
                    }
                } else if remaining <= MEDIA_LOW_THRESHOLD {
                    reasons |= PrinterReason::MARKER_SUPPLY_LOW;
                }

                // The firmware reports remaining *labels*, not a percentage, and
                // we don't know the roll's original count. Clamp to 0–100 as a
                // gauge: full while plenty remain, counting down near empty.
                supply_percent = Some(remaining.min(100) as u8);
            }
        }

        Some(PollStatus {
            reasons,
            ready_media,
            supply_percent,
        })
    }

    async fn identify(&self, config: &PrinterConfig, actions: &[String]) {
        // Map Identify-Printer to a physical beep via CHECK_DEVICE. Any action
        // keyword (display/sound/flash) triggers the same buzzer. Mock devices
        // no-op on identify.
        if let Some(dev) = crate::device::open_uri(&config.device_uri).await {
            log::info!("identify {} (actions={actions:?})", config.name);
            dev.identify().await;
        }
    }

    fn driver_for_device(&self, device_id: &str, device_uri: &str) -> Option<String> {
        if !device_id.is_empty()
            && let Some(mdl) = models::parse_mdl(device_id)
        {
            let family = models::family_for_model_hint(mdl);
            return Some(family.driver_name.to_string_lossy().into_owned());
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

    Server::bootstrap_printers(&registry, backend.as_ref(), &state_path, config_from_family).await;

    prune_stale_supvan(&registry);
    Server::persist(&registry, &state_path);

    let registry_print = registry.clone();
    let print_job = Arc::new(
        move |ctx: JobContext, raster: Arc<[u8]>, copies: u32| -> ipp_printer_app::PrintJobFuture {
            let registry_print = registry_print.clone();
            Box::pin(async move {
                let cfg = {
                    let guard = registry_print.read();
                    match guard.iter().find(|p| p.config.name == ctx.printer_name) {
                        Some(p) => p.config.clone(),
                        None => {
                            return JobOutcome::Failed(JobFailure::other(format!(
                                "printer not found: {}",
                                ctx.printer_name
                            )));
                        }
                    }
                };
                // image/jpeg is decoded in-process (run_jpeg_job); everything else
                // is CUPS/PWG raster (CUPS' driverless path already rasterizes).
                let result = if ctx.document_format == "image/jpeg" {
                    // Fallback when the config carries no media size: 40×30 mm,
                    // expressed in hundredths of a millimetre.
                    const DEFAULT_MEDIA_SIZE_HMM: [i32; 2] = [4000, 3000];
                    let media_size = cfg
                        .media_sizes
                        .first()
                        .copied()
                        .unwrap_or(DEFAULT_MEDIA_SIZE_HMM);
                    crate::ipp_job::run_jpeg_job(
                        &cfg.name,
                        &cfg.device_uri,
                        cfg.darkness,
                        cfg.printhead_width_dots,
                        media_size,
                        &raster,
                        copies,
                    )
                    .await
                } else {
                    run_cups_raster_job(
                        &cfg.name,
                        &cfg.device_uri,
                        cfg.darkness,
                        cfg.printhead_width_dots,
                        &cfg.driver_name,
                        &raster,
                        copies,
                    )
                    .await
                };
                match result {
                    Ok(()) => JobOutcome::Completed,
                    // A clearable physical condition — printer off / BT down, paper
                    // jam, out of labels, cover open — should HOLD the job and let
                    // the framework retry until it's resolved, not drop it (the way
                    // a real printer holds a job through a jam). Anything else is a
                    // permanent failure for this document.
                    Err(f) if f.printer_reasons.is_recoverable() => JobOutcome::DeviceUnavailable {
                        reasons: f.printer_reasons,
                    },
                    Err(f) => JobOutcome::Failed(f),
                }
            })
        },
    );

    // CUPS-managed-queue model (IPP Everywhere / Printer Application): we do
    // NOT create or own a CUPS queue. We are a self-contained IPP Everywhere
    // server that advertises over DNS-SD; CUPS discovers us and spins up a
    // temporary on-demand queue (auto-removed when idle), exactly as it does
    // for an AirPrint printer. This requires `cups-browsed` to be off — it
    // would otherwise build a broken same-host `implicitclass://` queue from
    // our advert (it's legacy; modern cupsd does driverless natively).
    let registry_media = registry.clone();
    let media_change: ipp_printer_app::MediaChangeFn = Arc::new(
        move |printer_name: String, media: ipp_printer_app::ReadyMedia| {
            let registry_media = registry_media.clone();
            Box::pin(async move {
                let device_uri = {
                    let guard = registry_media.read();
                    guard
                        .iter()
                        .find(|p| p.config.name == printer_name)
                        .map(|p| p.config.device_uri.clone())
                        .ok_or_else(|| format!("no printer named {printer_name}"))?
                };
                apply_media(&printer_name, &device_uri, &media).await
            })
        },
    );

    Server::run(ServerOptions {
        host: host.to_string(),
        port,
        printers: registry,
        device_backend: backend,
        print_job,
        media_change: Some(media_change),
        state_path,
        // Advertise the DNS-SD service directly at bind time. No queue UUID to
        // stamp (we own no queue), so there's nothing to coordinate first.
        advertise_mdns: true,
    })
    .await
}

/// Heat times to write when provisioning, from `SUPVAN_HEAT` as `heat5:heat40`.
///
/// The material record carries heat but `RETURN_MAT` never returns it
/// (`MaterialInfo` has no such field), so we cannot preserve what is already on
/// the roll — a provision necessarily sets it. Taking it from the environment
/// keeps that an explicit choice rather than a silent reset, and matches how the
/// app takes the rest of its configuration.
fn configured_heat() -> (u16, u16) {
    let Ok(raw) = std::env::var("SUPVAN_HEAT") else {
        return heat_presets::STANDARD;
    };
    let parsed = raw
        .split_once(':')
        .and_then(|(a, b)| Some((a.trim().parse().ok()?, b.trim().parse().ok()?)));
    parsed.unwrap_or_else(|| {
        log::warn!("SUPVAN_HEAT: expected `heat5:heat40`, got {raw:?}; using the standard profile");
        heat_presets::STANDARD
    })
}

/// Whether this material record is ours to overwrite.
///
/// Three cases reach us, and only one must be protected:
///
/// - **Blank stock** — all-zero UUID. The vendor's own gate
///   (`UUID.indexOf("00000000") == -1`) and plainly ours to define.
/// - **A record we wrote** — a synthetic UUID like `30000000000000`. Not
///   all-zero, so the vendor's test alone would refuse it and a roll could be
///   set exactly once, then never corrected.
/// - **A genuine consumable** — carries an 8-byte tag signature in `code`,
///   which is what separates it from the other two: `getT50PlusRFIDData` never
///   populates `MatCode`, so anything we write has `code` all zeros, and blank
///   stock has nothing to read.
///
/// So the signature is the discriminator, not the UUID.
///
/// Caveat worth stating: this assumes a genuine Supvan roll really does carry a
/// non-zero signature. That is what the field is for and what the vendor's
/// anti-counterfeit path reads, but it is unverified here — the bench has only
/// third-party stock. Erring the other way (refusing on any non-zero UUID)
/// would make the feature single-use, so this is the deliberate trade.
fn record_is_ours(uuid: &str, code: &str) -> bool {
    let zeroed = |s: &str| s.is_empty() || s.chars().all(|c| c == '0');

    // A signature is enough on its own to mean hands off, whatever the UUID.
    if !zeroed(code) {
        return false;
    }
    if zeroed(uuid) {
        return true;
    }

    // Ours are `RfidMaterial::uuid_bytes()` output: a decimal catalogue code
    // (u16, so at most five digits) right-padded with zeros to 14 hex chars.
    // Strip the padding and what remains must be those few digits — a real
    // 7-byte tag UID is hex and would not reduce to that.
    let stem = uuid.trim_end_matches('0');
    stem.len() <= 5 && stem.chars().all(|c| c.is_ascii_digit())
}

/// Push operator-set media geometry to the printer as a synthetic material
/// record.
///
/// **Refuses when a genuine RFID tag is present.** A real consumable describes
/// itself, and that description is what `RETURN_MAT` feeds into `media-ready`,
/// the remaining-labels gauge and roll-swap detection. Overwriting it would
/// replace measured truth with a typed-in guess, and 0x5D offers no way back.
/// Only blank stock — which reads as an all-zero UUID — is ours to define.
async fn apply_media(
    printer_name: &str,
    device_uri: &str,
    media: &ipp_printer_app::ReadyMedia,
) -> Result<(), String> {
    let dev = crate::device::open_uri(device_uri)
        .await
        .ok_or_else(|| format!("cannot reach {device_uri}"))?;
    let printer = dev
        .printer
        .as_ref()
        .ok_or("mock device: nothing to provision")?;

    let current = printer
        .query_material()
        .await
        .map_err(|e| format!("cannot read the loaded material: {e}"))?;

    if let Some(ref m) = current
        && !record_is_ours(&m.uuid, &m.code)
    {
        return Err(format!(
            "a genuine RFID roll is loaded (UUID {}, signature {}, {}x{}mm) and describes \
             itself — its own geometry is authoritative. Only blank stock, or a record this \
             app wrote, can be set here.",
            m.uuid, m.code, m.width_mm, m.height_mm
        ));
    }

    let (width_mm, height_mm) = (
        (media.size_hmm[0] / 100) as u8,
        (media.size_hmm[1] / 100) as u8,
    );
    if current
        .as_ref()
        .is_some_and(|m| m.width_mm == width_mm && m.height_mm == height_mm)
    {
        log::info!("{printer_name}: media already {width_mm}x{height_mm}mm, not rewriting");
        return Ok(());
    }

    // Carry over everything the read *does* expose, so setting a size doesn't
    // quietly discard the rest of the record.
    let heat = configured_heat();
    let mut mat = RfidMaterial {
        width_mm,
        length_mm: height_mm,
        heat_time_5: heat.0,
        heat_time_40: heat.1,
        ..Default::default()
    };
    if let Some(ref m) = current {
        if m.gap_mm > 0 {
            mat.gap_mm = m.gap_mm;
        }
        if m.label_type > 0 {
            mat.mat_type = m.label_type;
        }
    }

    let written = printer
        .provision_material(&mat)
        .await
        .map_err(|e| format!("the printer did not take the record: {e}"))?;

    // Record the fingerprint we just wrote, so the status poller doesn't report
    // our own change as an operator swapping the roll.
    roll_cache().lock().unwrap().insert(
        printer_name.to_string(),
        RollFingerprint {
            uuid: written.uuid.clone(),
            code: written.code.clone(),
            width_mm: written.width_mm,
            height_mm: written.height_mm,
        },
    );
    log::info!(
        "{printer_name}: media set to {}x{}mm, heat {}/{}",
        written.width_mm,
        written.height_mm,
        heat.0,
        heat.1
    );
    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The ownership test is the whole safety gate. It has to say yes to blank
    /// stock and to records we wrote — otherwise a roll could be set once and
    /// never corrected — while still refusing a genuine signed consumable.
    #[test]
    fn ownership_gate_protects_only_genuine_tags() {
        // Blank stock.
        assert!(record_is_ours("00000000000000", "0000000000000000"));
        assert!(record_is_ours("", ""));

        // Records we wrote: a synthetic UUID, but never a signature. Observed
        // live on a T50M Pro after `provision --code 30000`.
        assert!(record_is_ours("30000000000000", "0000000000000000"));
        assert!(record_is_ours("56180000000000", "0000000000000000"));

        // A genuine consumable carries a signature — the one case to refuse.
        assert!(!record_is_ours("A1B2C3D4E5F607", "1122334455667788"));
        assert!(
            !record_is_ours("00000000000000", "0000000000000001"),
            "a signature alone is enough to mean hands off"
        );
    }

    /// Heat is unreadable from the device, so a bad `SUPVAN_HEAT` must fall back
    /// rather than fail a media change — and must never silently yield 0/0,
    /// which would print nothing at all.
    #[test]
    fn heat_config_falls_back_on_junk() {
        // Serialised via a mutex-free approach: set, read, restore, since env is
        // process-global and other tests may run in parallel.
        let restore = std::env::var("SUPVAN_HEAT").ok();

        unsafe { std::env::set_var("SUPVAN_HEAT", "1700:1200") };
        assert_eq!(configured_heat(), (1700, 1200));

        unsafe { std::env::set_var("SUPVAN_HEAT", "nonsense") };
        assert_eq!(configured_heat(), heat_presets::STANDARD);

        unsafe { std::env::set_var("SUPVAN_HEAT", "1700") };
        assert_eq!(configured_heat(), heat_presets::STANDARD);

        unsafe { std::env::remove_var("SUPVAN_HEAT") };
        assert_eq!(configured_heat(), heat_presets::STANDARD);

        if let Some(v) = restore {
            unsafe { std::env::set_var("SUPVAN_HEAT", v) };
        }
    }
}
