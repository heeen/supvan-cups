//! Central model registry: driver families, USB PIDs, media tables.
//!
//! Loaded at startup from `data/models.toml`. Call [`load()`] once before
//! accessing any other function in this module.

use std::collections::HashMap;
use std::ffi::{c_int, CStr, CString};
use std::sync::OnceLock;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Public runtime types
// ---------------------------------------------------------------------------

/// A driver family groups models sharing the same printhead and DPI.
pub struct DriverFamily {
    pub driver_name: CString,
    pub description: CString,
    pub make_and_model: Vec<u8>,
    pub device_id: CString,
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
struct ModelsToml {
    families: Vec<FamilyToml>,
    models: Vec<ModelToml>,
    bt_patterns: HashMap<String, Vec<String>>,
}

// ---------------------------------------------------------------------------
// Registry singleton
// ---------------------------------------------------------------------------

struct Registry {
    families: Vec<DriverFamily>,
    models: Vec<UsbModel>,
    /// (pattern, family_idx) — longest patterns first for correct matching.
    bt_patterns: Vec<(String, usize)>,
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
pub fn load() {
    let path = find_toml_path();
    let contents =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("failed to read {path}: {e}"));
    let toml: ModelsToml =
        toml::from_str(&contents).unwrap_or_else(|e| panic!("failed to parse {path}: {e}"));

    let families: Vec<DriverFamily> = toml
        .families
        .iter()
        .map(|f| {
            let media_names: Vec<CString> = f
                .media_mm
                .iter()
                .map(|[w, h]| CString::new(format!("oe_{w}x{h}mm_{w}x{h}mm")).unwrap())
                .collect();
            let media_sizes: Vec<[c_int; 2]> =
                f.media_mm.iter().map(|[w, h]| [w * 100, h * 100]).collect();

            let mdl = f
                .description
                .strip_prefix("Supvan ")
                .unwrap_or(&f.description);
            let device_id = format!("MFG:Supvan;MDL:{mdl};CMD:SUPVAN;");

            DriverFamily {
                driver_name: CString::new(f.name.as_str()).unwrap(),
                description: CString::new(f.description.as_str()).unwrap(),
                make_and_model: f.description.as_bytes().to_vec(),
                device_id: CString::new(device_id).unwrap(),
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
        .expect("models.toml must define a 'supvan_t50' family");

    let models: Vec<UsbModel> = toml
        .models
        .iter()
        .map(|m| {
            assert!(
                family_index.contains_key(m.family.as_str()),
                "model '{}' references unknown family '{}'",
                m.name,
                m.family
            );
            UsbModel {
                pid: m.pid.clone(),
                name: m.name.clone(),
            }
        })
        .collect();

    // Flatten bt_patterns: (pattern, family_idx), sorted longest-first
    let mut bt_patterns: Vec<(String, usize)> = Vec::new();
    for (family_name, patterns) in &toml.bt_patterns {
        let idx = *family_index
            .get(family_name.as_str())
            .unwrap_or_else(|| panic!("bt_patterns references unknown family '{family_name}'"));
        for pattern in patterns {
            bt_patterns.push((pattern.clone(), idx));
        }
    }
    bt_patterns.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

    if REGISTRY
        .set(Registry {
            families,
            models,
            bt_patterns,
            default_family_idx,
        })
        .is_err()
    {
        panic!("models::load() called more than once");
    }
}

fn find_toml_path() -> String {
    // 1. Explicit override
    if let Ok(path) = std::env::var("SUPVAN_MODELS") {
        return path;
    }

    // 2. Development / cargo run from workspace root
    // 3. System install
    let candidates = [
        "data/models.toml",
        "/usr/share/supvan-printer-app/models.toml",
    ];

    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return path.to_string();
        }
    }

    panic!(
        "models.toml not found; searched: $SUPVAN_MODELS, {}\n\
         Set SUPVAN_MODELS=/path/to/models.toml to override.",
        candidates.join(", ")
    );
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

/// Find a driver family by its driver name (e.g. `c"supvan_t50"`).
pub fn family_by_driver_name(name: &CStr) -> Option<&'static DriverFamily> {
    registry()
        .families
        .iter()
        .find(|f| f.driver_name.as_c_str() == name)
}

/// Find a USB model by its PID string (lowercase hex, e.g. `"2073"`).
pub fn model_by_pid(pid: &str) -> Option<&'static UsbModel> {
    registry()
        .models
        .iter()
        .find(|m| m.pid.eq_ignore_ascii_case(pid))
}

/// Determine the driver family from a model name or BT broadcast name.
///
/// Uses substring matching against bt_patterns (longest first).
/// Falls back to the default family for unknown names.
pub fn family_for_model_hint(name: &str) -> &'static DriverFamily {
    let lower = name.to_lowercase();
    let reg = registry();

    for (pattern, idx) in &reg.bt_patterns {
        if lower.contains(pattern.as_str()) {
            return &reg.families[*idx];
        }
    }

    &reg.families[reg.default_family_idx]
}

/// Check if a Bluetooth device name matches any known Supvan printer pattern.
pub fn is_matching_bt_name(name: &str) -> bool {
    let lower = name.to_lowercase();

    if lower.contains("supvan") || lower.contains("katasymbol") {
        return true;
    }

    registry()
        .bt_patterns
        .iter()
        .any(|(pattern, _)| lower.contains(pattern.as_str()))
}

/// Parse the MDL field from an IEEE 1284 device ID string.
///
/// Example: `"MFG:Supvan;MDL:T50M Pro;CMD:SUPVAN;"` → `Some("T50M Pro")`
pub fn parse_mdl(device_id: &str) -> Option<&str> {
    for field in device_id.split(';') {
        if let Some(mdl) = field.strip_prefix("MDL:") {
            return Some(mdl);
        }
    }
    None
}
