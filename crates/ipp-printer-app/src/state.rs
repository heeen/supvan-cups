//! Persisted printer list (JSON).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::printer::PrinterConfig;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PersistedState {
    pub printers: Vec<PrinterConfig>,
}

impl PersistedState {
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_default(),
            Err(_) => PersistedState::default(),
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        fs::write(path, json)
    }
}

/// Application state path: `$XDG_STATE_HOME/<app_id>.state.json`,
/// falling back to `$HOME/.local/state/<app_id>.state.json`.
pub fn default_state_path(app_id: &str) -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_STATE_HOME") {
        if !dir.is_empty() {
            return PathBuf::from(format!("{dir}/{app_id}.state.json"));
        }
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
    PathBuf::from(format!("{home}/.local/state/{app_id}.state.json"))
}
