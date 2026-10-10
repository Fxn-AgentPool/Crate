//! Version information for `crate --version` and updates (docs/reference/updates.md).
//! The release workflow sets CRATE_BUILD (its run number) and CRATE_COMMIT; local builds are
//! development builds and get the commit from git, if available.

fn main() {
    println!("cargo:rerun-if-env-changed=CRATE_BUILD");
    println!("cargo:rerun-if-env-changed=CRATE_COMMIT");
    if std::env::var_os("CRATE_COMMIT").is_none() {
        println!("cargo:rerun-if-changed=.git/HEAD");
        println!("cargo:rerun-if-changed=.git/refs/heads");
        let commit = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
        if let Some(c) = commit.filter(|c| !c.is_empty()) {
            println!("cargo:rustc-env=CRATE_COMMIT={c}");
        }
    }
}
