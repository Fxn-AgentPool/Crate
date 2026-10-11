//! User preferences (Edit > Preferences), stored in the user configuration directory:
//! `%APPDATA%\Crate` on Windows, `~/Library/Application Support/Crate` on macOS and
//! `$XDG_CONFIG_HOME/crate` (usually `~/.config/crate`) on Linux. Settings from before the
//! rename (the `Painter` folder) are moved over on first start.
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
    /// Look for a newer release when the application starts (docs/reference/updates.md).
    pub check_updates_at_startup: bool,
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            gpu_cache_budget_gb: 8.0,
            undo_budget_gb: 2.0,
            undo_steps: 64,
            shortcuts: Default::default(),
            check_updates_at_startup: true,
        }
    }
}

/// Per-user directory for preferences, export presets and the library.
pub fn dir() -> PathBuf {
    base_dir(if cfg!(any(windows, target_os = "macos")) { "Crate" } else { "crate" })
}

fn base_dir(name: &str) -> PathBuf {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if cfg!(windows) {
        env("APPDATA").unwrap_or_else(|| PathBuf::from(".")).join(name)
    } else if cfg!(target_os = "macos") {
        env("HOME").unwrap_or_else(|| PathBuf::from(".")).join("Library/Application Support").join(name)
    } else {
        env("XDG_CONFIG_HOME")
            .or_else(|| env("HOME").map(|h| h.join(".config")))
            .unwrap_or_else(|| PathBuf::from("."))
            .join(name)
    }
}

/// Opens a folder in the platform's file browser.
pub fn open_folder(path: &std::path::Path) {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(program).arg(path).spawn() {
        log::warn!("Could not open {}: {e}", path.display());
    }
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
