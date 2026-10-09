//! User preferences (Edit > Preferences), stored in %APPDATA%/Painter/preferences.json.
//! See docs/reference/limits.md "Memory budgets".

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// GPU memory for paint and caches, in GB.
    pub gpu_cache_budget_gb: f32,
    /// System memory for undo pages, in GB.
    pub undo_budget_gb: f32,
    /// Maximum undo steps.
    pub undo_steps: usize,
    /// Shortcut overrides, e.g. Undo -> "Ctrl+Z".
    pub shortcuts: std::collections::BTreeMap<crate::shortcuts::Action, String>,
}

impl Default for Preferences {
    fn default() -> Self {
        Self { gpu_cache_budget_gb: 8.0, undo_budget_gb: 2.0, undo_steps: 64, shortcuts: Default::default() }
    }
}

pub fn dir() -> PathBuf {
    std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(".")).join("Painter")
}

impl Preferences {
    pub fn load() -> Self {
        std::fs::read_to_string(dir().join("preferences.json"))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let d = dir();
        if std::fs::create_dir_all(&d).is_ok() {
            let _ = std::fs::write(d.join("preferences.json"), serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}
