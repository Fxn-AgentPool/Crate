//! Command-line interface for pipelines. See docs/reference/command-line.md.
//!
//! `crate` with no subcommand opens the application. Subcommands run headless:
//! new, reimport, export, run, batch, watch, info and update.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use crate::script::{self, Session, say};

/// Exit codes, documented in the command-line reference.
pub const EXIT_OK: u8 = 0;
pub const EXIT_FAILED: u8 = 1;
pub const EXIT_USAGE: u8 = 2;
pub const EXIT_GPU: u8 = 3;

#[derive(Parser)]
#[command(
    name = "crate",
    version = crate::update::version_str(),
    about = "Realtime UDIM texture painting. Without a subcommand, opens the application.",
    args_conflicts_with_subcommands = true
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Mesh or .crate project to open in the application.
    pub path: Option<PathBuf>,

    /// Script to run in the application after it starts.
    #[arg(long)]
    pub script: Option<PathBuf>,

    /// Print one JSON result on stdout (logs go to stderr with -v).
    #[arg(long, global = true)]
    pub json: bool,

    /// More logging (-v: progress and info, -vv: debug).
    #[arg(short, long, global = true, action = clap::ArgAction::Count)]
    pub verbose: u8,
}

#[derive(Args, Clone, Default)]
pub struct Vars {
    /// Script variable for ${name} (repeatable): --set color=#ff0000
    #[arg(long = "set", value_name = "NAME=VALUE")]
    pub set: Vec<String>,
}

#[derive(Args, Clone, Default)]
pub struct ExportOpts {
    /// Export preset (see the export reference).
    #[arg(long)]
    pub preset: Option<String>,
    /// Override the file format: png8, png16, tga, exr16, exr32.
    #[arg(long)]
    pub format: Option<String>,
    /// Normal map convention: opengl or directx.
    #[arg(long)]
    pub normal: Option<String>,
    /// Padding: infinite, none or a number of texels.
    #[arg(long)]
    pub padding: Option<String>,
    /// Output resolution (downscale only).
    #[arg(long)]
    pub resolution: Option<u32>,
    /// Only these tiles, comma separated (1001,1002).
    #[arg(long)]
    pub tiles: Option<String>,
    /// File name template, e.g. {mesh}_{map}.{udim}.{ext}
    #[arg(long)]
    pub template: Option<String>,
}

impl ExportOpts {
    /// Script `export` options.
    fn script_args(&self) -> Vec<String> {
        let mut v = Vec::new();
        let mut push = |k: &str, val: &Option<String>| {
            if let Some(x) = val {
                v.push(format!("\"{k}={x}\""));
            }
        };
        push("preset", &self.preset);
        push("format", &self.format);
        push("normal", &self.normal);
        push("padding", &self.padding);
        push("resolution", &self.resolution.map(|r| r.to_string()));
        push("tiles", &self.tiles);
        push("template", &self.template);
        v
    }
}

#[derive(Subcommand)]
pub enum Command {
    /// Create a project from a mesh, optionally bake, run a script, export and save.
    New {
        /// Mesh to import (.glb, .gltf or .obj).
        mesh: PathBuf,
        /// Project directory to save (.crate).
        #[arg(short, long)]
        output: PathBuf,
        /// Tile resolution: 1024, 2048 or 4096.
        #[arg(long, default_value_t = 2048)]
        tile_resolution: u32,
        /// High-poly mesh to bake from (.glb, .gltf or .obj).
        #[arg(long)]
        high_poly: Option<PathBuf>,
        /// Measure the high-poly bake's cage per vertex instead of using fixed distances.
        #[arg(long)]
        auto_cage: bool,
        /// Mesh maps to bake: all, or a list such as ao,curvature.
        #[arg(long, value_delimiter = ',')]
        bake: Vec<String>,
        /// Script to run on the new project before saving.
        #[arg(long)]
        script: Option<PathBuf>,
        /// Export textures to this directory after the script.
        #[arg(long)]
        export: Option<PathBuf>,
        #[command(flatten)]
        export_opts: ExportOpts,
        #[command(flatten)]
        vars: Vars,
    },
    /// Replace a project's mesh (for example after a new export from Blender) and save it.
    Reimport {
        /// The .crate project.
        project: PathBuf,
        /// The new mesh.
        mesh: PathBuf,
        /// Do not rebake mesh maps that became out of date.
        #[arg(long)]
        no_bake: bool,
        /// Keep paint where it is in UV space, even if the UVs changed.
        #[arg(long)]
        no_transfer: bool,
        /// Export textures to this directory afterwards.
        #[arg(long)]
        export: Option<PathBuf>,
        #[command(flatten)]
        export_opts: ExportOpts,
    },
    /// Export a project's textures.
    Export {
        /// The .crate project.
        project: PathBuf,
        /// Output directory.
        #[arg(short, long)]
        output: PathBuf,
        #[command(flatten)]
        export_opts: ExportOpts,
    },
    /// Run a script (a file, or - for stdin), optionally on an existing project.
    Run {
        /// Script file, or - to read from stdin. Omit when using -c.
        script: Option<PathBuf>,
        /// Commands separated by ';' (instead of a script file).
        #[arg(short = 'c', long = "commands")]
        commands: Option<String>,
        /// Project to open before the script runs.
        #[arg(long)]
        project: Option<PathBuf>,
        /// Save the project when the script finishes.
        #[arg(long)]
        save: bool,
        #[command(flatten)]
        vars: Vars,
    },
    /// Run one script on many projects, reusing one GPU session.
    Batch {
        /// Script run once per project. ${project} holds the project's name.
        script: PathBuf,
        /// Projects (.crate); wildcards are expanded (e.g. projects/*.crate).
        #[arg(required = true)]
        projects: Vec<String>,
        /// Save each project after its script.
        #[arg(long)]
        save: bool,
        /// Continue with the next project when one fails.
        #[arg(long)]
        keep_going: bool,
        #[command(flatten)]
        vars: Vars,
    },
    /// Watch a mesh file; when it changes, reimport, rebake, save and optionally export.
    Watch {
        /// The .crate project.
        project: PathBuf,
        /// Mesh file to watch (the file your DCC exports).
        #[arg(long)]
        mesh: PathBuf,
        /// Keep paint where it is in UV space, even if the UVs changed.
        #[arg(long)]
        no_transfer: bool,
        /// Export textures to this directory after each reimport.
        #[arg(long)]
        export: Option<PathBuf>,
        #[command(flatten)]
        export_opts: ExportOpts,
        /// Polling interval in milliseconds.
        #[arg(long, default_value_t = 500)]
        interval: u64,
        /// Stop after this many reimports (0 = run until interrupted).
        #[arg(long, default_value_t = 0)]
        count: u32,
    },
    /// Print a project's summary: mesh, tiles, layers, mesh maps.
    Info {
        /// The .crate project.
        project: PathBuf,
    },
    /// Check GitHub for a newer release and install it (no GPU needed).
    Update {
        /// Only report whether a newer release exists.
        #[arg(long)]
        check: bool,
    },
}

fn parse_vars(v: &Vars) -> Result<Vec<(String, String)>> {
    v.set
        .iter()
        .map(|kv| {
            let (k, v) = kv.split_once('=').with_context(|| format!("--set expects NAME=VALUE, got {kv}"))?;
            Ok((k.to_string(), v.to_string()))
        })
        .collect()
}

fn session(defines: Vec<(String, String)>) -> Result<Session> {
    let mut s = Session::new(std::env::current_dir()?)?;
    s.defines.extend(defines);
    Ok(s)
}

/// Quotes a path for a script command.
fn q(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

/// Runs a subcommand. Returns the JSON result fields beyond ok/error/files.
pub fn run(cli: &Cli, cmd: &Command) -> Result<serde_json::Value> {
    match cmd {
        Command::New { mesh, output, tile_resolution, high_poly, auto_cage, bake, script: script_file, export, export_opts, vars } => {
            let mut s = session(parse_vars(vars)?)?;
            s.run(&format!("new {} {tile_resolution}", q(mesh)))?;
            if let Some(h) = high_poly {
                s.run(&format!("high-poly {}", q(h)))?;
            }
            if *auto_cage {
                s.run("bake-settings auto_cage=on")?;
            }
            if !bake.is_empty() {
                s.run(&format!("bake {}", bake.join(" ")))?;
            }
            if let Some(f) = script_file {
                s.run_file(f)?;
            }
            s.run(&format!("save {}", q(output)))?;
            if let Some(dir) = export {
                s.run(&format!("export {} {}", q(dir), export_opts.script_args().join(" ")))?;
            }
            let info = script::project_info(s.project.as_ref().unwrap());
            Ok(serde_json::json!({ "project": info }))
        }
        Command::Reimport { project, mesh, no_bake, no_transfer, export, export_opts } => {
            let mut s = session(Vec::new())?;
            s.run(&format!("open {}", q(project)))?;
            s.run(&format!("reimport {}{}", q(mesh), if *no_transfer { " --no-transfer" } else { "" }))?;
            if !no_bake {
                s.run("bake outdated")?;
            }
            s.run("save")?;
            if let Some(dir) = export {
                s.run(&format!("export {} {}", q(dir), export_opts.script_args().join(" ")))?;
            }
            let p = s.project.as_ref().unwrap();
            let info = script::project_info(p);
            Ok(serde_json::json!({ "project": info, "transfer": transfer_json(p.last_transfer) }))
        }
        Command::Export { project, output, export_opts } => {
            let mut s = session(Vec::new())?;
            s.run(&format!("open {}", q(project)))?;
            s.run(&format!("export {} {}", q(output), export_opts.script_args().join(" ")))?;
            Ok(serde_json::json!({}))
        }
        Command::Run { script: script_file, commands, project, save, vars } => {
            let mut s = session(parse_vars(vars)?)?;
            if let Some(pr) = project {
                s.run(&format!("open {}", q(pr)))?;
            }
            match (script_file, commands) {
                (Some(f), None) => s.run_file(f)?,
                (None, Some(c)) => s.run_text(&split_commands(c).join("\n"), "-c")?,
                (Some(_), Some(_)) => bail!("give either a script or -c, not both"),
                (None, None) => bail!("give a script file, - for stdin, or -c \"commands\""),
            }
            if *save {
                s.run("save")?;
            }
            Ok(serde_json::json!({ "project": s.project.as_ref().map(script::project_info) }))
        }
        Command::Batch { script: script_file, projects, save, keep_going, vars } => {
            let defines = parse_vars(vars)?;
            let text = std::fs::read_to_string(script_file).with_context(|| format!("reading {}", script_file.display()))?;
            let mut s = session(defines.clone())?;
            let paths = expand(projects)?;
            if paths.is_empty() {
                bail!("no projects match {}", projects.join(" "));
            }
            let mut results = Vec::new();
            let mut failed = 0;
            for path in &paths {
                say!("== {}", path.display());
                let t = Instant::now();
                let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                s.defines = defines.iter().cloned().collect();
                s.defines.insert("project".into(), name);
                s.vars.clear();
                let r = (|| -> Result<()> {
                    s.run(&format!("open {}", q(path)))?;
                    s.run_text(&text, &script_file.display().to_string())?;
                    if *save {
                        s.run("save")?;
                    }
                    Ok(())
                })();
                let ok = r.is_ok();
                if let Err(e) = &r {
                    failed += 1;
                    say!("  failed: {e:#}");
                }
                results.push(serde_json::json!({
                    "project": path.display().to_string(),
                    "ok": ok,
                    "error": r.err().map(|e| format!("{e:#}")),
                    "seconds": t.elapsed().as_secs_f32(),
                }));
                if !ok && !*keep_going {
                    break;
                }
            }
            let summary = serde_json::json!({ "projects": results, "failed": failed });
            if failed > 0 {
                return Err(BatchFailed(failed, summary).into());
            }
            Ok(summary)
        }
        Command::Watch { project, mesh, no_transfer, export, export_opts, interval, count } => {
            watch(cli, project, mesh, *no_transfer, export.as_deref(), export_opts, *interval, *count)
        }
        Command::Update { check } => update(*check),
        Command::Info { project } => {
            let mut s = session(Vec::new())?;
            s.run(&format!("open {}", q(project)))?;
            let info = script::project_info(s.project.as_ref().unwrap());
            if !cli.json {
                println!("{}", serde_json::to_string_pretty(&info)?);
            }
            Ok(serde_json::json!({ "project": info }))
        }
    }
}

/// A batch where some projects failed; carries the per-project results.
#[derive(Debug)]
pub struct BatchFailed(pub usize, pub serde_json::Value);

impl std::fmt::Display for BatchFailed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} project(s) failed", self.0)
    }
}

impl std::error::Error for BatchFailed {}

/// Splits `-c` commands on ';' outside quotes.
fn split_commands(c: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in c.chars() {
        match ch {
            '"' => {
                quoted = !quoted;
                cur.push(ch);
            }
            ';' if !quoted => out.push(std::mem::take(&mut cur)),
            _ => cur.push(ch),
        }
    }
    out.push(cur);
    out.into_iter().map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
}

/// Expands `*` and `?` in project arguments (shells on Windows don't).
fn expand(patterns: &[String]) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for pat in patterns {
        if !pat.contains(['*', '?']) {
            out.push(PathBuf::from(pat));
            continue;
        }
        let path = Path::new(pat);
        let dir = path.parent().filter(|d| !d.as_os_str().is_empty()).unwrap_or(Path::new("."));
        let name_pat = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let mut matched: Vec<PathBuf> = std::fs::read_dir(dir)
            .with_context(|| format!("reading {}", dir.display()))?
            .flatten()
            .filter(|e| wildcard(&name_pat, &e.file_name().to_string_lossy()))
            .map(|e| e.path())
            .collect();
        matched.sort();
        out.extend(matched);
    }
    Ok(out)
}

fn wildcard(pat: &str, s: &str) -> bool {
    let (p, t): (Vec<char>, Vec<char>) = (pat.chars().collect(), s.chars().collect());
    fn rec(p: &[char], t: &[char]) -> bool {
        match (p.first(), t.first()) {
            (None, None) => true,
            (Some('*'), _) => rec(&p[1..], t) || (!t.is_empty() && rec(p, &t[1..])),
            (Some('?'), Some(_)) => rec(&p[1..], &t[1..]),
            (Some(a), Some(b)) if a.eq_ignore_ascii_case(b) => rec(&p[1..], &t[1..]),
            _ => false,
        }
    }
    rec(&p, &t)
}

fn modified(path: &Path) -> Option<(SystemTime, u64)> {
    let m = std::fs::metadata(path).ok()?;
    Some((m.modified().ok()?, m.len()))
}

#[allow(clippy::too_many_arguments)]
/// The `transfer` field of reimport output (docs/reference/command-line.md "JSON output").
fn transfer_json(t: Option<crate::transfer::Report>) -> serde_json::Value {
    let t = t.unwrap_or_default();
    serde_json::json!({ "transferred": t.transferred, "missed": t.missed })
}

#[allow(clippy::too_many_arguments)]
fn watch(
    cli: &Cli,
    project: &Path,
    mesh: &Path,
    no_transfer: bool,
    export: Option<&Path>,
    export_opts: &ExportOpts,
    interval: u64,
    count: u32,
) -> Result<serde_json::Value> {
    let mut s = session(Vec::new())?;
    s.run(&format!("open {}", q(project)))?;
    let mut last = modified(mesh);
    let mut done = 0u32;
    say!("watching {} (Ctrl+C to stop)", mesh.display());
    emit(cli, serde_json::json!({ "event": "watching", "mesh": mesh.display().to_string() }));
    loop {
        std::thread::sleep(Duration::from_millis(interval));
        let now = modified(mesh);
        if now.is_none() || now == last {
            continue;
        }
        // Wait until the exporter has finished writing (size and time stable).
        std::thread::sleep(Duration::from_millis(interval.max(200)));
        let settled = modified(mesh);
        if settled != now {
            continue;
        }
        last = settled;
        let t = Instant::now();
        script::report_reset();
        let r = (|| -> Result<()> {
            s.run(&format!("reimport {}{}", q(mesh), if no_transfer { " --no-transfer" } else { "" }))?;
            s.run("bake outdated")?;
            s.run("save")?;
            if let Some(dir) = export {
                s.run(&format!("export {} {}", q(dir), export_opts.script_args().join(" ")))?;
            }
            Ok(())
        })();
        let rep = script::report_take();
        match r {
            Ok(()) => {
                say!("reimported in {:.2}s", t.elapsed().as_secs_f32());
                emit(cli, serde_json::json!({
                    "event": "reimported",
                    "mesh": mesh.display().to_string(),
                    "files": rep.files,
                    "warnings": rep.warnings,
                    "seconds": t.elapsed().as_secs_f32(),
                    "transfer": transfer_json(rep.transfer),
                }));
            }
            Err(e) => {
                say!("reimport failed: {e:#}");
                emit(cli, serde_json::json!({ "event": "error", "error": format!("{e:#}") }));
            }
        }
        done += 1;
        if count > 0 && done >= count {
            return Ok(serde_json::json!({ "reimports": done }));
        }
    }
}

/// Watch mode prints one JSON line per event.
fn emit(cli: &Cli, v: serde_json::Value) {
    if cli.json {
        println!("{v}");
    }
}

/// Entry point for subcommands: runs, reports, and returns the exit code.
pub fn main(cli: Cli) -> u8 {
    let Some(cmd) = &cli.command else { return EXIT_USAGE };
    script::set_output(match (cli.json, cli.verbose) {
        (false, _) => 0,
        (true, 0) => 2,
        (true, _) => 1,
    });
    script::report_reset();
    let name = match cmd {
        Command::New { .. } => "new",
        Command::Reimport { .. } => "reimport",
        Command::Export { .. } => "export",
        Command::Run { .. } => "run",
        Command::Batch { .. } => "batch",
        Command::Watch { .. } => "watch",
        Command::Info { .. } => "info",
        Command::Update { .. } => "update",
    };
    let t = Instant::now();
    let result = run(&cli, cmd);
    let rep = script::report_take();
    let watch = matches!(cmd, Command::Watch { .. });
    let (code, mut out) = match result {
        Ok(extra) => {
            let mut v = serde_json::json!({ "ok": true });
            merge(&mut v, extra);
            (EXIT_OK, v)
        }
        Err(e) => {
            let code = if e.chain().any(|c| c.to_string().contains("does not support")) || e.to_string().contains("No suitable GPU") {
                EXIT_GPU
            } else {
                EXIT_FAILED
            };
            let mut v = serde_json::json!({ "ok": false, "error": format!("{e:#}") });
            if let Some(b) = e.downcast_ref::<BatchFailed>() {
                merge(&mut v, b.1.clone());
            }
            if !cli.json {
                eprintln!("error: {e:#}");
            }
            (code, v)
        }
    };
    if cli.json && !watch {
        merge(&mut out, serde_json::json!({
            "command": name,
            "files": rep.files,
            "saved": rep.saved,
            "warnings": rep.warnings,
            "seconds": t.elapsed().as_secs_f32(),
        }));
        println!("{out}");
    }
    code
}

/// `crate update [--check]`. See docs/reference/command-line.md#update.
fn update(check: bool) -> Result<serde_json::Value> {
    use crate::update as up;
    let release = up::latest()?;
    let newer = release.is_newer();
    let mut out = serde_json::json!({
        "installed": up::version_str(),
        "latest": release.display(),
        "available": newer == Some(true),
        "updated": false,
    });
    match newer {
        None if check => say!("  Crate {} is a development build; the latest release is {}", up::version_str(), release.display()),
        None => bail!("Crate {} doesn't update itself (built from source); the latest release is {}", up::version_str(), release.display()),
        Some(false) => say!("  Crate {} is up to date", up::version_str()),
        Some(true) if check => say!("  Crate {} is available (installed: {})", release.display(), up::version_str()),
        Some(true) => {
            let last = std::cell::Cell::new(-1i64);
            up::install(&release, &|p| match p {
                up::Progress::Downloading { done, total } if total > 0 => {
                    let pct = (done * 100 / total) as i64 / 10 * 10;
                    if pct != last.get() {
                        last.set(pct);
                        say!("  downloading {}: {pct}%", release.asset.as_ref().map(|a| a.name.as_str()).unwrap_or(""));
                    }
                }
                up::Progress::Installing => say!("  installing"),
                _ => {}
            })?;
            say!("  installed Crate {}; it runs the next time Crate starts", release.display());
            out["updated"] = serde_json::json!(true);
        }
    }
    Ok(out)
}

fn merge(into: &mut serde_json::Value, from: serde_json::Value) {
    if let (Some(a), serde_json::Value::Object(b)) = (into.as_object_mut(), from) {
        for (k, v) in b {
            a.insert(k, v);
        }
    }
}
