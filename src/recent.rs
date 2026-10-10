//! Recently opened projects (File > Open Recent), stored in `recent.json` in the user
//! directory. Only the application window records projects; scripts and headless commands
//! don't. See docs/reference/windows-and-panels.md "Open Recent".

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const MAX: usize = 10;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Recent {
    /// Newest first.
    pub projects: Vec<PathBuf>,
}

fn file() -> PathBuf {
    crate::prefs::dir().join("recent.json")
}

impl Recent {
    /// The stored list; empty when there is none or it can't be read.
    pub fn load() -> Self {
        std::fs::read_to_string(file()).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    }

    pub fn save(&self) {
        let path = file();
        let written = std::fs::create_dir_all(path.parent().unwrap_or(Path::new(".")))
            .and_then(|_| std::fs::write(&path, serde_json::to_string_pretty(self).unwrap_or_default()));
        if let Err(e) = written {
            log::warn!("could not save {}: {e}", path.display());
        }
    }

    /// Adds `project` to the stored list and saves it. Rereads the file first, so projects
    /// recorded by another running instance are kept.
    pub fn record(project: &Path) -> Self {
        let mut r = Self::load();
        r.add(project);
        r.save();
        r
    }

    /// Moves `project` to the top, dropping the oldest entry beyond `MAX`.
    pub fn add(&mut self, project: &Path) {
        let p = std::path::absolute(project).unwrap_or_else(|_| project.to_path_buf());
        // Rebuilding from components drops a trailing separator.
        let p: PathBuf = p.components().collect();
        self.projects.retain(|q| !same(q, &p));
        self.projects.insert(0, p);
        self.projects.truncate(MAX);
    }

    pub fn clear(&mut self) {
        self.projects.clear();
    }

    /// Menu labels: the project name, plus its parent folder when two entries share a name.
    pub fn labels(&self) -> Vec<String> {
        let names: Vec<String> = self.projects.iter().map(|p| name(p)).collect();
        names
            .iter()
            .zip(&self.projects)
            .map(|(n, p)| {
                if names.iter().filter(|m| m.eq_ignore_ascii_case(n)).count() > 1 {
                    let parent = p.parent().and_then(|d| d.file_name()).map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                    format!("{n} ({parent})")
                } else {
                    n.clone()
                }
            })
            .collect()
    }
}

/// The project folder's name without `.crate`.
fn name(p: &Path) -> String {
    let n = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.display().to_string());
    match n.rsplit_once('.') {
        Some((stem, ext)) if ext.eq_ignore_ascii_case("crate") && !stem.is_empty() => stem.to_string(),
        _ => n,
    }
}

/// Whether two absolute paths name the same project (case-insensitively on Windows and macOS).
fn same(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        let s = s.trim_end_matches('/').to_string();
        if cfg!(any(windows, target_os = "macos")) { s.to_lowercase() } else { s }
    };
    norm(a) == norm(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn abs(s: &str) -> PathBuf {
        std::path::absolute(s).unwrap()
    }

    #[test]
    fn newest_first_without_duplicates() {
        let mut r = Recent::default();
        r.add(Path::new("a/One.crate"));
        r.add(Path::new("b/Two.crate"));
        r.add(Path::new("a/One.crate/"));
        assert_eq!(r.projects, [abs("a/One.crate"), abs("b/Two.crate")]);
    }

    #[test]
    fn keeps_at_most_ten() {
        let mut r = Recent::default();
        for i in 0..12 {
            r.add(Path::new(&format!("p{i}.crate")));
        }
        assert_eq!(r.projects.len(), MAX);
        assert_eq!(r.projects[0], abs("p11.crate"));
        assert_eq!(r.projects[MAX - 1], abs("p2.crate"));
    }

    #[test]
    fn labels_add_the_parent_for_shared_names() {
        let mut r = Recent::default();
        r.add(Path::new("client/Crate.crate"));
        r.add(Path::new("home/Crate.crate"));
        r.add(Path::new("home/Barrel.crate"));
        r.add(Path::new("home/notes"));
        assert_eq!(r.labels(), ["notes", "Barrel", "Crate (home)", "Crate (client)"]);
    }

    #[test]
    fn reads_old_or_broken_files_as_empty() {
        assert_eq!(serde_json::from_str::<Recent>("{}").unwrap(), Recent::default());
        assert!(serde_json::from_str::<Recent>("not json").is_err());
    }
}
