//! Updates from GitHub releases: checking, downloading, verifying and installing. Used by the
//! Updates window and `crate update`; needs no GPU. See docs/reference/updates.md.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

/// The repository releases come from.
pub const REPO: &str = "Fxn-AgentPool/Crate";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Release workflow run number; absent in development builds.
pub const BUILD: Option<&str> = option_env!("CRATE_BUILD");
/// Commit the program was built from, when known.
pub const COMMIT: Option<&str> = option_env!("CRATE_COMMIT");

/// What the installer replaces next to the program, besides the program itself.
const PACKAGE_EXTRAS: [&str; 3] = ["README.md", "docs", "assets"];
const STAGING: &str = ".crate-update";

/// The running version: `0.1.0 (build 7)` or `0.1.0 (development build, 1191e6e)`.
pub fn version_str() -> &'static str {
    static S: LazyLock<String> = LazyLock::new(|| match (build_number(), COMMIT) {
        (Some(b), _) => format!("{VERSION} (build {b})"),
        (None, Some(c)) => format!("{VERSION} (development build, {})", &c[..c.len().min(7)]),
        (None, None) => format!("{VERSION} (development build)"),
    });
    &S
}

fn build_number() -> Option<u64> {
    BUILD.and_then(|b| b.trim().parse().ok())
}

pub fn is_development_build() -> bool {
    build_number().is_none()
}

/// The version as comparable numbers, e.g. `0.1.0` -> `[0, 1, 0]`.
fn parse_version(v: &str) -> Option<Vec<u64>> {
    v.split('.').map(|p| p.parse().ok()).collect()
}

/// `v0.1.0-build.7` -> (`[0, 1, 0]`, 7).
pub fn parse_tag(tag: &str) -> Option<(Vec<u64>, u64)> {
    let (v, b) = tag.strip_prefix('v')?.split_once("-build.")?;
    Some((parse_version(v)?, b.parse().ok()?))
}

#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    /// API URL; with `Accept: application/octet-stream` it returns the file.
    pub url: String,
    pub size: u64,
    /// Lowercase hex SHA-256, when GitHub publishes it.
    pub sha256: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub version: Vec<u64>,
    pub build: u64,
    pub notes: String,
    /// Publication date, `YYYY-MM-DD`.
    pub published: String,
    pub html_url: String,
    /// The archive for this platform, if one was published.
    pub asset: Option<Asset>,
}

impl Release {
    /// `0.1.0 (build 7)`.
    pub fn display(&self) -> String {
        let v: Vec<String> = self.version.iter().map(u64::to_string).collect();
        format!("{} (build {})", v.join("."), self.build)
    }

    /// Whether this release is newer than the running program. None for development builds,
    /// which can't be compared.
    pub fn is_newer(&self) -> Option<bool> {
        let build = build_number()?;
        let current = parse_version(VERSION)?;
        Some((&self.version, self.build) > (&current, build))
    }
}

/// Archive name suffix for this platform, or None where no releases are published.
pub fn platform_suffix() -> Option<&'static str> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some("-windows-x64.zip")
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some("-linux-x64.tar.gz")
    } else {
        None
    }
}

/// GET with GitHub's headers. `what` names the resource in error messages.
fn get(url: &str, accept: &str, what: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let mut req = ureq::get(url)
        .header("User-Agent", &format!("crate/{VERSION}"))
        .header("Accept", accept)
        .header("X-GitHub-Api-Version", "2022-11-28");
    if let Some(token) = std::env::var("CRATE_GITHUB_TOKEN").ok().filter(|t| !t.trim().is_empty()) {
        req = req.header("Authorization", &format!("Bearer {}", token.trim()));
    }
    req.call().map_err(|e| match e {
        ureq::Error::StatusCode(403 | 429) => anyhow::anyhow!("GitHub refused the request (rate limit?); try again later or set CRATE_GITHUB_TOKEN"),
        ureq::Error::StatusCode(404) => anyhow::anyhow!("{what} not found on GitHub (github.com/{REPO})"),
        e => anyhow::anyhow!("couldn't download {what} from GitHub: {e}"),
    })
}

/// The latest published release (drafts and pre-releases are excluded by GitHub).
pub fn latest() -> Result<Release> {
    let text = get(&format!("https://api.github.com/repos/{REPO}/releases/latest"), "application/vnd.github+json", "the latest release")?
        .into_body()
        .read_to_string()
        .context("reading the release information")?;
    parse_release(&text)
}

fn parse_release(text: &str) -> Result<Release> {
    let v: serde_json::Value = serde_json::from_str(text).context("the release information is invalid")?;
    let tag = v["tag_name"].as_str().context("the release has no tag")?.to_string();
    let (version, build) = parse_tag(&tag).with_context(|| format!("unexpected release tag {tag}"))?;
    let asset = platform_suffix().and_then(|suffix| {
        v["assets"].as_array()?.iter().find_map(|a| {
            let name = a["name"].as_str()?;
            (name.starts_with("crate-") && name.ends_with(suffix)).then(|| Asset {
                name: name.to_string(),
                url: a["url"].as_str().unwrap_or_default().to_string(),
                size: a["size"].as_u64().unwrap_or(0),
                sha256: a["digest"].as_str().and_then(|d| d.strip_prefix("sha256:")).map(|d| d.to_ascii_lowercase()),
            })
        })
    });
    Ok(Release {
        version,
        build,
        notes: v["body"].as_str().unwrap_or_default().trim().to_string(),
        published: v["published_at"].as_str().unwrap_or_default().chars().take(10).collect(),
        html_url: v["html_url"].as_str().unwrap_or_default().to_string(),
        asset,
    })
}

/// Progress of an install, for the UI and the command line.
#[derive(Clone, Copy, Debug)]
pub enum Progress {
    Downloading { done: u64, total: u64 },
    Installing,
}

/// The program's path, recorded at startup. After an update renames the running program,
/// `current_exe` may follow the rename (Linux), but this still names the installed program.
static PROGRAM: LazyLock<Option<PathBuf>> = LazyLock::new(|| {
    let exe = std::env::current_exe().ok()?;
    // Follow symlinks (e.g. /usr/local/bin/crate) to the real install. Windows paths are kept
    // as they are, to avoid \?\ paths in messages.
    Some(if cfg!(unix) { exe.canonicalize().unwrap_or(exe) } else { exe })
});

/// The installed program, for starting it again after an update.
pub fn program_path() -> Option<PathBuf> {
    PROGRAM.clone()
}

/// The program's own path and folder.
fn program() -> Result<(PathBuf, PathBuf)> {
    let exe = program_path().context("can't locate the program")?;
    let dir = exe.parent().context("can't locate the program folder")?.to_path_buf();
    Ok((exe, dir))
}

fn old_path(exe: &Path) -> PathBuf {
    let mut s = exe.as_os_str().to_owned();
    s.push(".old");
    PathBuf::from(s)
}

/// Downloads, verifies and installs `release` over the running program. The new version runs
/// the next time Crate starts.
pub fn install(release: &Release, progress: &dyn Fn(Progress)) -> Result<()> {
    if is_development_build() {
        bail!("this is a development build, which doesn't update itself; rebuild from source or install a release");
    }
    let asset = release.asset.as_ref().context("this release has no download for this platform")?;
    let (exe, dir) = program()?;
    let staging = dir.join(STAGING);
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).with_context(|| format!("can't write to the program folder {}", dir.display()))?;
    let result = (|| {
        let archive = staging.join(&asset.name);
        download(asset, &archive, progress)?;
        progress(Progress::Installing);
        let unpacked = staging.join("unpacked");
        extract(&archive, &unpacked)?;
        replace(&exe, &dir, &package_root(&unpacked)?)
    })();
    let _ = std::fs::remove_dir_all(&staging);
    result
}

fn download(asset: &Asset, to: &Path, progress: &dyn Fn(Progress)) -> Result<()> {
    let resp = get(&asset.url, "application/octet-stream", &asset.name)?;
    let mut reader = resp.into_body().into_reader();
    let mut file = std::fs::File::create(to).with_context(|| format!("can't write {}", to.display()))?;
    let mut hash = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    progress(Progress::Downloading { done, total: asset.size });
    loop {
        let n = reader.read(&mut buf).context("the download was interrupted")?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if asset.size > 0 && done > asset.size {
            bail!("the download is larger than the release says ({} bytes)", asset.size);
        }
        hash.update(&buf[..n]);
        file.write_all(&buf[..n]).context("writing the download")?;
        progress(Progress::Downloading { done, total: asset.size });
    }
    file.flush()?;
    if asset.size > 0 && done != asset.size {
        bail!("the download is incomplete ({done} of {} bytes)", asset.size);
    }
    let got: String = hash.finalize().iter().map(|b| format!("{b:02x}")).collect();
    match &asset.sha256 {
        Some(want) if *want != got => bail!("the download is damaged (SHA-256 {got}, expected {want})"),
        Some(_) => Ok(()),
        None => {
            log::warn!("GitHub published no checksum for {}; only its size was checked", asset.name);
            Ok(())
        }
    }
}

fn extract(archive: &Path, to: &Path) -> Result<()> {
    std::fs::create_dir_all(to)?;
    let name = archive.file_name().unwrap_or_default().to_string_lossy();
    let file = std::fs::File::open(archive)?;
    if name.ends_with(".zip") {
        zip::ZipArchive::new(file).context("the download is not a valid zip")?.extract(to).context("unpacking the download")?;
    } else if name.ends_with(".tar.gz") {
        tar::Archive::new(flate2::read::GzDecoder::new(file)).unpack(to).context("unpacking the download")?;
    } else {
        bail!("unknown archive type {name}");
    }
    Ok(())
}

/// The folder in the unpacked archive that holds the program (archives have one top folder).
fn package_root(unpacked: &Path) -> Result<PathBuf> {
    let exe_name = if cfg!(windows) { "crate.exe" } else { "crate" };
    if unpacked.join(exe_name).is_file() {
        return Ok(unpacked.to_path_buf());
    }
    for e in std::fs::read_dir(unpacked)? {
        let p = e?.path();
        if p.join(exe_name).is_file() {
            return Ok(p);
        }
    }
    bail!("the download doesn't contain {exe_name}")
}

/// Swaps in the new program (keeping the old one as `<exe>.old`), then the docs and assets.
fn replace(exe: &Path, dir: &Path, package: &Path) -> Result<()> {
    let exe_name = if cfg!(windows) { "crate.exe" } else { "crate" };
    let new_exe = package.join(exe_name);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&new_exe, std::fs::Permissions::from_mode(0o755))?;
    }
    let old = old_path(exe);
    let _ = std::fs::remove_file(&old);
    // A running program can be renamed, but not overwritten (Windows).
    std::fs::rename(exe, &old).with_context(|| format!("can't write to the program folder {}", dir.display()))?;
    if let Err(e) = std::fs::rename(&new_exe, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(e).context("installing the new program");
    }
    // The program is in place; a failure from here on leaves an updated program with older
    // docs or assets, which the message says.
    let extras = || -> Result<()> {
        for extra in PACKAGE_EXTRAS {
            let (from, to) = (package.join(extra), dir.join(extra));
            if !from.exists() {
                continue;
            }
            if to.is_dir() {
                std::fs::remove_dir_all(&to).with_context(|| format!("replacing {}", to.display()))?;
            } else if to.exists() {
                std::fs::remove_file(&to).with_context(|| format!("replacing {}", to.display()))?;
            }
            std::fs::rename(&from, &to).with_context(|| format!("installing {}", to.display()))?;
        }
        Ok(())
    };
    extras().context("the program was updated, but its docs or sample files couldn't be replaced (close any open files from the Crate folder and update again)")
}

/// Deletes the program kept by the last update. The previous process may still be exiting
/// right after a restart, so this retries for a few seconds in the background.
pub fn cleanup_previous() {
    let Ok((exe, _)) = program() else { return };
    // `program()` has now recorded the path, before any update can rename the program.
    let old = old_path(&exe);
    if !old.exists() || std::fs::remove_file(&old).is_ok() {
        return;
    }
    std::thread::spawn(move || {
        for _ in 0..20 {
            if std::fs::remove_file(&old).is_ok() || !old.exists() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
        log::warn!("couldn't delete {}", old.display());
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tags() {
        assert_eq!(parse_tag("v0.1.0-build.7"), Some((vec![0, 1, 0], 7)));
        assert_eq!(parse_tag("v1.2-build.30"), Some((vec![1, 2], 30)));
        assert_eq!(parse_tag("0.1.0-build.7"), None);
        assert_eq!(parse_tag("v0.1.0"), None);
        assert_eq!(parse_tag("v0.x.0-build.7"), None);
    }

    #[test]
    fn release_json() {
        let suffix = platform_suffix().unwrap_or("-linux-x64.tar.gz");
        let json = format!(
            r#"{{"tag_name":"v0.2.0-build.12","body":" notes ","published_at":"2026-10-10T13:05:18Z","html_url":"h",
               "assets":[{{"name":"crate-0.2.0{suffix}","url":"u","size":5,"digest":"sha256:ABCD"}},
                         {{"name":"other.txt","url":"x","size":1}}]}}"#
        );
        let r = parse_release(&json).unwrap();
        assert_eq!((r.version.clone(), r.build), (vec![0, 2, 0], 12));
        assert_eq!(r.display(), "0.2.0 (build 12)");
        assert_eq!(r.notes, "notes");
        assert_eq!(r.published, "2026-10-10");
        if platform_suffix().is_some() {
            let a = r.asset.unwrap();
            assert_eq!((a.url.as_str(), a.size, a.sha256.as_deref()), ("u", 5, Some("abcd")));
        }
    }

    #[test]
    fn package_roundtrip() {
        // Builds a fake install and a fake package, then replaces one with the other.
        let t = std::env::temp_dir().join(format!("crate-update-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&t);
        let (dir, pkg) = (t.join("install"), t.join("pkg"));
        let exe_name = if cfg!(windows) { "crate.exe" } else { "crate" };
        for d in [&dir, &pkg] {
            std::fs::create_dir_all(d.join("docs")).unwrap();
        }
        std::fs::write(dir.join(exe_name), "old").unwrap();
        std::fs::write(dir.join("docs/old.md"), "old").unwrap();
        std::fs::write(dir.join("keep.txt"), "mine").unwrap();
        std::fs::write(pkg.join(exe_name), "new").unwrap();
        std::fs::write(pkg.join("docs/new.md"), "new").unwrap();
        std::fs::write(pkg.join("README.md"), "readme").unwrap();
        replace(&dir.join(exe_name), &dir, &package_root(&pkg).unwrap()).unwrap();
        assert_eq!(std::fs::read_to_string(dir.join(exe_name)).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(old_path(&dir.join(exe_name))).unwrap(), "old");
        assert!(dir.join("docs/new.md").exists() && !dir.join("docs/old.md").exists());
        assert_eq!(std::fs::read_to_string(dir.join("README.md")).unwrap(), "readme");
        assert_eq!(std::fs::read_to_string(dir.join("keep.txt")).unwrap(), "mine");
        let _ = std::fs::remove_dir_all(&t);
    }
}
