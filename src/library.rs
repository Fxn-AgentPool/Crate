//! The user library: one folder per Library category under `<user dir>/library`, plus any
//! folders the user adds. See docs/reference/windows-and-panels.md "Library" and
//! docs/explanation/about-the-user-library.md.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "tga", "tif", "tiff", "exr"];
pub const ENVIRONMENT_EXTS: &[&str] = &["hdr", "exr"];

/// What a library folder holds, which decides the files listed and what they can be used for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    SmartMaterials,
    /// Created but not used yet: every file is listed.
    Reserved,
    Textures,
    Alphas,
    Stencils,
    Environments,
    /// A folder added by the user.
    Images,
}

impl Kind {
    pub fn accepts(self, path: &Path) -> bool {
        let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
        match self {
            Kind::SmartMaterials => ext == crate::smartmat::EXT,
            Kind::Reserved => true,
            Kind::Textures | Kind::Alphas | Kind::Stencils | Kind::Images => IMAGE_EXTS.contains(&ext.as_str()),
            Kind::Environments => ENVIRONMENT_EXTS.contains(&ext.as_str()),
        }
    }
}

/// Built-in folders: (folder, category name, kind), in Library row order.
pub const BUILT_IN: [(&str, &str, Kind); 8] = [
    ("smart-materials", "Smart Materials", Kind::SmartMaterials),
    ("materials", "Materials", Kind::Reserved),
    ("textures", "Textures", Kind::Textures),
    ("smart-masks", "Smart Masks", Kind::Reserved),
    ("brushes", "Brushes", Kind::Reserved),
    ("alphas", "Alphas", Kind::Alphas),
    ("stencils", "Stencils", Kind::Stencils),
    ("environments", "Environments", Kind::Environments),
];

pub fn root() -> PathBuf {
    crate::prefs::dir().join("library")
}

pub fn dir(folder: &str) -> PathBuf {
    root().join(folder)
}

fn built_in(folder: &str) -> Option<&'static (&'static str, &'static str, Kind)> {
    // On Windows and macOS `Alphas` and `alphas` are the same folder; on Linux they're two.
    let same = |a: &str, b: &str| if cfg!(any(windows, target_os = "macos")) { a.eq_ignore_ascii_case(b) } else { a == b };
    BUILT_IN.iter().find(|(f, _, _)| same(f, folder))
}

pub fn kind(folder: &str) -> Kind {
    built_in(folder).map_or(Kind::Images, |b| b.2)
}

/// Category name shown in the Library: the built-in name, or the folder name.
pub fn title(folder: &str) -> String {
    built_in(folder).map_or_else(|| folder.to_string(), |b| b.1.to_string())
}

/// Creates the built-in folders that are missing.
pub fn ensure_folders() -> Result<()> {
    for (folder, _, _) in BUILT_IN {
        let d = dir(folder);
        std::fs::create_dir_all(&d).with_context(|| format!("creating {}", d.display()))?;
    }
    Ok(())
}

fn hidden(name: &std::ffi::OsStr) -> bool {
    name.to_string_lossy().starts_with('.')
}

/// Folders the user added to the library, in alphabetical order.
pub fn extra_folders() -> Vec<String> {
    extra_folders_in(&root())
}

fn extra_folders_in(root: &Path) -> Vec<String> {
    let mut out: Vec<String> = std::fs::read_dir(root)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !hidden(&e.file_name()))
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|f| built_in(f).is_none())
        .collect();
    out.sort_by_key(|f| (f.to_lowercase(), f.clone()));
    out
}

#[derive(Clone, Debug)]
pub struct Item {
    /// File name without its extension.
    pub name: String,
    pub path: PathBuf,
}

/// The items of a library folder, sorted by name.
pub fn scan(folder: &str) -> Vec<Item> {
    scan_dir(&dir(folder), kind(folder))
}

pub fn scan_dir(dir: &Path, kind: Kind) -> Vec<Item> {
    let mut items: Vec<Item> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()) && !hidden(&e.file_name()))
        .map(|e| e.path())
        .filter(|p| kind.accepts(p))
        .map(|path| Item { name: path.file_stem().unwrap_or_default().to_string_lossy().into_owned(), path })
        .collect();
    // By name; files sharing a name by full file name.
    items.sort_by_key(|it| (it.name.to_lowercase(), it.path.file_name().map(|n| n.to_os_string())));
    items
}

/// Resolves a script argument: a file path when it has an extension, otherwise a library item,
/// `<name>` in `default_folder` or `<folder>/<name>`.
/// See docs/reference/script-commands.md "Library items".
pub fn resolve(arg: &str, default_folder: &str, base: impl Fn(&str) -> PathBuf) -> Result<PathBuf> {
    if Path::new(arg).extension().is_some() {
        return Ok(base(arg));
    }
    resolve_in(&root(), arg, default_folder)
}

fn resolve_in(root: &Path, arg: &str, default_folder: &str) -> Result<PathBuf> {
    let (folder, name) = arg.split_once(['/', '\\']).unwrap_or((default_folder, arg));
    let plain = |s: &str| !s.is_empty() && s != "." && s != ".." && !s.contains(['/', '\\', ':']);
    if !plain(folder) || !plain(name) {
        bail!("{arg:?} is not a library item (<name> or <folder>/<name>)");
    }
    let d = root.join(folder);
    if !d.is_dir() {
        bail!("no library folder {folder} ({})", d.display());
    }
    // The folder's kind decides which files count, as in the Library panel.
    let k = built_in(folder).map_or(Kind::Images, |b| b.2);
    scan_dir(&d, k)
        .into_iter()
        .find(|it| it.name == name)
        .map(|it| it.path)
        .with_context(|| format!("no item {name} in library folder {folder}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("crate-library-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"x").unwrap();
    }

    fn names(items: &[Item]) -> Vec<String> {
        items.iter().map(|i| i.path.file_name().unwrap().to_string_lossy().into_owned()).collect()
    }

    #[test]
    fn scan_filters_and_sorts() {
        let r = temp_root("scan");
        for f in ["b.hdr", "A.exr", "a.hdr", "c.png", ".hidden.hdr", "notes.txt"] {
            touch(&r.join("environments").join(f));
        }
        touch(&r.join("environments").join("sub").join("d.hdr"));
        let items = scan_dir(&r.join("environments"), Kind::Environments);
        assert_eq!(names(&items), ["A.exr", "a.hdr", "b.hdr"]);

        touch(&r.join("brushes").join("anything.bin"));
        assert_eq!(names(&scan_dir(&r.join("brushes"), Kind::Reserved)), ["anything.bin"]);
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn extra_folders_skip_built_in_and_hidden() {
        let r = temp_root("extra");
        for f in ["zebra", "Grunge", "alphas", "Stencils", ".git", "apple"] {
            std::fs::create_dir_all(r.join(f)).unwrap();
        }
        touch(&r.join("loose.png"));
        if cfg!(any(windows, target_os = "macos")) {
            assert_eq!(extra_folders_in(&r), ["apple", "Grunge", "zebra"]);
        } else {
            assert_eq!(extra_folders_in(&r), ["apple", "Grunge", "Stencils", "zebra"]);
        }
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn resolve_names() {
        let r = temp_root("resolve");
        touch(&r.join("stencils").join("rust.tga"));
        touch(&r.join("stencils").join("rust.png"));
        touch(&r.join("grunge").join("scratches.jpg"));
        touch(&r.join("grunge").join("readme.txt"));
        let file = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
        // First by full file name.
        assert_eq!(file(&resolve_in(&r, "rust", "stencils").unwrap()), "rust.png");
        assert_eq!(file(&resolve_in(&r, "grunge/scratches", "stencils").unwrap()), "scratches.jpg");
        assert!(resolve_in(&r, "grunge/readme", "stencils").is_err(), "not an image");
        assert!(resolve_in(&r, "missing", "stencils").is_err());
        assert!(resolve_in(&r, "nofolder/x", "stencils").is_err());
        assert!(resolve_in(&r, "../stencils", "stencils").is_err());
        assert!(resolve_in(&r, "a/b/c", "stencils").is_err());
        let _ = std::fs::remove_dir_all(&r);
    }

    #[test]
    fn resolve_paths_with_extension() {
        let p = resolve("files/x.png", "stencils", |s| PathBuf::from("base").join(s)).unwrap();
        assert_eq!(p, PathBuf::from("base").join("files/x.png"));
    }
}
