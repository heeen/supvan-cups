//! Central model registry: driver families, USB PIDs, media tables.
//!
//! Loaded at startup from `data/models.toml`. Call [`load()`] once before
//! accessing any other function in this module.

use std::collections::HashMap;
use std::ffi::{CString, c_int};
use std::sync::OnceLock;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public runtime types
// ---------------------------------------------------------------------------

/// A driver family groups models sharing the same printhead and DPI.
pub struct DriverFamily {
    pub driver_name: CString,
    pub make_and_model: Vec<u8>,
    pub dpi: c_int,
    pub printhead_width_dots: u32,
    pub media_names: Vec<CString>,
    pub media_sizes: Vec<[c_int; 2]>,
}

/// A USB model identified by PID (VID is always 0x1820).
pub struct UsbModel {
    pub pid: String,
    pub name: String,
}

// ---------------------------------------------------------------------------
// TOML serde types (private)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct FamilyToml {
    name: String,
    description: String,
    dpi: i32,
    printhead_dots: u32,
    media_mm: Vec<[i32; 2]>,
}

#[derive(Deserialize)]
struct ModelToml {
    pid: String,
    name: String,
    family: String,
}

#[derive(Deserialize)]
struct BtNameToml {
    model: String,
    family: String,
    prefixes: Vec<String>,
}

#[derive(Deserialize)]
struct ModelsToml {
    families: Vec<FamilyToml>,
    models: Vec<ModelToml>,
    bt_names: Vec<BtNameToml>,
}

// ---------------------------------------------------------------------------
// Registry singleton
// ---------------------------------------------------------------------------

/// One advertised-name prefix and the printer it identifies.
struct BtPrefix {
    prefix: String,
    model: String,
    family_idx: usize,
}

struct Registry {
    families: Vec<DriverFamily>,
    models: Vec<UsbModel>,
    /// Longest prefix first, so `t0021b` wins over `t0021`.
    bt_prefixes: Vec<BtPrefix>,
    default_family_idx: usize,
}

static REGISTRY: OnceLock<Registry> = OnceLock::new();

fn registry() -> &'static Registry {
    REGISTRY
        .get()
        .expect("models::load() must be called before accessing the registry")
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load the model registry from TOML. Panics if the file is not found or
/// invalid.
///
/// Must be called exactly once, before any other function in this module.
/// The default model table, baked into the binary so a `cargo install`'d
/// (or otherwise relocated) binary is self-contained. Overridden by
/// `$SUPVAN_MODELS` or a `models.toml` found on disk — see [`find_toml_path`].
const EMBEDDED_MODELS: &str = include_str!("../../../data/models.toml");

pub fn load() {
    let registry = match find_toml_path() {
        Some(path) => match read_and_build(&path) {
            Ok(r) => r,
            Err(e) => {
                // A stale on-disk table must not take the daemon down: an
                // upgrade that adds a field leaves the old /usr/share copy
                // unreadable, and the embedded one is always in step with
                // this binary.
                log::error!("models: ignoring {path}: {e}; falling back to the embedded table");
                embedded_registry()
            }
        },
        None => embedded_registry(),
    };

    if REGISTRY.set(registry).is_err() {
        panic!("models::load() called more than once");
    }
}

fn embedded_registry() -> Registry {
    build_registry(EMBEDDED_MODELS, "<embedded>").expect("the embedded models.toml must be valid")
}

fn read_and_build(path: &str) -> Result<Registry, String> {
    let contents = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    build_registry(&contents, path)
}

fn build_registry(contents: &str, source: &str) -> Result<Registry, String> {
    let toml: ModelsToml =
        toml::from_str(contents).map_err(|e| format!("failed to parse {source}: {e}"))?;

    let families: Vec<DriverFamily> = toml
        .families
        .iter()
        .map(|f| {
            let media_names: Vec<CString> = f
                .media_mm
                .iter()
                // PWG 5101.1 self-describing name: metric dimensions take the
                // `om_` (other-metric) class prefix; `oe_` is for inches and
                // fails the IPP Everywhere media-name regex.
                .map(|[w, h]| CString::new(format!("om_{w}x{h}mm_{w}x{h}mm")).unwrap())
                .collect();
            let media_sizes: Vec<[c_int; 2]> =
                f.media_mm.iter().map(|[w, h]| [w * 100, h * 100]).collect();

            DriverFamily {
                driver_name: CString::new(f.name.as_str()).unwrap(),
                make_and_model: f.description.as_bytes().to_vec(),
                dpi: f.dpi,
                printhead_width_dots: f.printhead_dots,
                media_names,
                media_sizes,
            }
        })
        .collect();

    // Build family name → index map
    let family_index: HashMap<&str, usize> = families
        .iter()
        .enumerate()
        .map(|(i, f)| (f.driver_name.to_str().unwrap(), i))
        .collect();

    let default_family_idx = *family_index
        .get("supvan_t50")
        .ok_or_else(|| format!("{source}: no 'supvan_t50' family"))?;

    let mut models = Vec::with_capacity(toml.models.len());
    for m in &toml.models {
        if !family_index.contains_key(m.family.as_str()) {
            return Err(format!(
                "{source}: model '{}' references unknown family '{}'",
                m.name, m.family
            ));
        }
        models.push(UsbModel {
            pid: m.pid.clone(),
            name: m.name.clone(),
        });
    }

    let mut bt_prefixes: Vec<BtPrefix> = Vec::new();
    for entry in &toml.bt_names {
        let family_idx = *family_index.get(entry.family.as_str()).ok_or_else(|| {
            format!(
                "{source}: bt_names entry '{}' references unknown family '{}'",
                entry.model, entry.family
            )
        })?;
        for prefix in &entry.prefixes {
            bt_prefixes.push(BtPrefix {
                prefix: prefix.to_lowercase(),
                model: entry.model.clone(),
                family_idx,
            });
        }
    }
    bt_prefixes.sort_by_key(|p| std::cmp::Reverse(p.prefix.len()));

    Ok(Registry {
        families,
        models,
        bt_prefixes,
        default_family_idx,
    })
}

/// Locate a `models.toml` override on disk, or `None` to use [`EMBEDDED_MODELS`].
fn find_toml_path() -> Option<String> {
    // 1. Explicit override
    if let Ok(path) = std::env::var("SUPVAN_MODELS") {
        return Some(path);
    }

    // 2. Development / cargo run from workspace root
    // 3. System install
    let candidates = [
        "data/models.toml",
        "/usr/share/supvan-printer-app/models.toml",
    ];
    candidates
        .into_iter()
        .find(|path| std::path::Path::new(path).exists())
        .map(str::to_string)
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// All driver families.
pub fn families() -> &'static [DriverFamily] {
    &registry().families
}

/// The default driver family (supvan_t50).
pub fn default_family() -> &'static DriverFamily {
    &registry().families[registry().default_family_idx]
}

/// Find a USB model by its PID string (lowercase hex, e.g. `"2073"`).
pub fn model_by_pid(pid: &str) -> Option<&'static UsbModel> {
    registry()
        .models
        .iter()
        .find(|m| m.pid.eq_ignore_ascii_case(pid))
}

/// Longest matching advertised-name prefix, if any.
fn match_bt_prefix(name: &str) -> Option<&'static BtPrefix> {
    let lower = name.to_lowercase();
    registry()
        .bt_prefixes
        .iter()
        .find(|p| lower.starts_with(p.prefix.as_str()))
}

/// Determine the driver family from a model name or BT/BLE advertised name.
///
/// Falls back to the default family for unknown names.
pub fn family_for_model_hint(name: &str) -> &'static DriverFamily {
    let reg = registry();
    match match_bt_prefix(name) {
        Some(p) => &reg.families[p.family_idx],
        None => &reg.families[reg.default_family_idx],
    }
}

/// The marketing model name behind an advertised name (`T0182A2507162197` →
/// `E11`), or `None` if the prefix is unknown.
pub fn bt_model_for_name(name: &str) -> Option<&'static str> {
    match_bt_prefix(name).map(|p| p.model.as_str())
}

/// Check if a Bluetooth device name matches any known Supvan printer.
pub fn is_matching_bt_name(name: &str) -> bool {
    let lower = name.to_lowercase();

    if lower.contains("supvan") || lower.contains("katasymbol") {
        return true;
    }

    match_bt_prefix(&lower).is_some()
}

/// Supvan's assigned MAC OUI.
pub fn is_supvan_oui(addr: &str) -> bool {
    addr.get(..8)
        .is_some_and(|oui| oui.eq_ignore_ascii_case("A4:93:40"))
}

/// True for the firmware *serial name* a printer broadcasts: a `T`/`G`/`D`
/// family letter, a hardware code, then the unit serial (`T0143F2408183024`
/// for an E10, `T0117A2410211517` for a T50M Pro).
///
/// The prefix table can only list codes the vendor app knows about, so a unit
/// whose code postdates it would be invisible to discovery. Safe as a generic
/// fallback when paired with [`is_supvan_oui`].
pub fn is_supvan_serial_name(name: &str) -> bool {
    let b = name.as_bytes();
    b.len() >= 3
        && matches!(b[0], b'T' | b'G' | b'D')
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
}

/// Whether a Bluetooth device looks like a Supvan printer, given both its
/// address and advertised name. Accepts known prefixes anywhere, plus unknown
/// firmware serial names inside the Supvan OUI.
pub fn is_matching_bt_device(addr: &str, name: &str) -> bool {
    is_matching_bt_name(name) || (is_supvan_oui(addr) && is_supvan_serial_name(name))
}

/// Parse the MDL field from an IEEE 1284 device ID string.
///
/// Example: `"MFG:Supvan;MDL:T50M Pro;CMD:SUPVAN;"` → `Some("T50M Pro")`
pub fn parse_mdl(device_id: &str) -> Option<&str> {
    device_id
        .split(';')
        .find_map(|field| field.strip_prefix("MDL:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The registry is a process-wide singleton; `load()` panics if called
    /// twice, so every test funnels through here.
    fn init() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        // Always the embedded table, never a stale system install.
        ONCE.call_once(|| {
            let _ = REGISTRY.set(embedded_registry());
        });
    }

    #[test]
    fn advertised_serial_names_resolve_to_models() {
        init();
        // The two names from supvan-cups#1 — an E11 and an E10, as BlueZ
        // reports them.
        assert_eq!(bt_model_for_name("T0182A2507162197"), Some("E11"));
        assert_eq!(bt_model_for_name("T0131F251217E291"), Some("E10/T10"));
        assert_eq!(bt_model_for_name("T0117A2401010001"), Some("T50M Pro"));
    }

    #[test]
    fn discovery_accepts_advertised_serial_names() {
        init();
        assert!(is_matching_bt_name("T0182A2507162197"));
        assert!(is_matching_bt_name("T0131F251217E291"));
        // Brand words are accepted anywhere, not just as a prefix.
        assert!(is_matching_bt_name("My Supvan Printer"));
        assert!(!is_matching_bt_name("Some Headphones"));
    }

    #[test]
    fn a_trailing_letter_selects_a_different_model() {
        init();
        // t0021a and t0021b are distinct printers; longest-prefix-first
        // ordering must not let a shorter pattern swallow either.
        assert_eq!(bt_model_for_name("T0021A0000"), Some("T50M"));
        assert_eq!(bt_model_for_name("T0021B0000"), Some("T50M Plus"));
    }

    #[test]
    fn serials_are_matched_as_prefixes_not_substrings() {
        init();
        // A T50 Max serial embeds "0007", which is an E10 prefix once the
        // leading T is glued on. Substring matching used to call this an E10.
        assert_eq!(bt_model_for_name("T0192T00071234"), Some("T50 Max"));
    }

    #[test]
    fn mdl_strings_still_resolve_a_family() {
        init();
        // driver_for_device feeds the IEEE 1284 MDL through the same table.
        let f = family_for_model_hint("T80M Pro");
        assert_eq!(f.driver_name.to_str().unwrap(), "supvan_t80");
        let f = family_for_model_hint("E11");
        assert_eq!(f.driver_name.to_str().unwrap(), "supvan_e11");
    }

    #[test]
    fn unknown_hardware_codes_still_pass_discovery() {
        init();
        // A code the vendor tables don't list — a unit newer than the app we
        // transcribed. Unknown to the prefix table...
        assert_eq!(bt_model_for_name("T0999X2501010001"), None);
        assert!(!is_matching_bt_name("T0999X2501010001"));
        // ...but still discoverable inside Supvan's OUI.
        assert!(is_matching_bt_device(
            "A4:93:40:11:22:33",
            "T0999X2501010001"
        ));
        // and it lands on the default family rather than being dropped.
        let f = family_for_model_hint("T0999X2501010001");
        assert_eq!(f.driver_name.to_str().unwrap(), "supvan_t50");
    }

    #[test]
    fn the_oui_fallback_does_not_admit_other_vendors() {
        init();
        // Right name shape, wrong OUI.
        assert!(!is_matching_bt_device(
            "00:11:22:33:44:55",
            "T0999X2501010001"
        ));
        // Right OUI, wrong name shape.
        assert!(!is_matching_bt_device(
            "A4:93:40:11:22:33",
            "Some Headphones"
        ));
    }

    #[test]
    fn a_broken_on_disk_table_is_rejected_not_fatal() {
        // What a stale /usr/share copy looks like after a schema change: the
        // daemon must not die on it.
        assert!(build_registry("families = []\nmodels = []\n", "<bad>").is_err());
        assert!(build_registry("this is not toml", "<bad>").is_err());
        // A table whose entries point at families it never defines.
        let orphan = r#"
models = []

[[families]]
name = "supvan_t50"
description = "T50"
dpi = 203
printhead_dots = 384
media_mm = [[40, 30]]

[[bt_names]]
model = "Nope"
family = "supvan_missing"
prefixes = ["x"]
"#;
        let Err(err) = build_registry(orphan, "<bad>") else {
            panic!("an orphan family reference must be rejected");
        };
        assert!(err.contains("unknown family"), "{err}");
    }
}
