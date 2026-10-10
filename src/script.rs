//! Headless script runner: `painter run <script.txt>` (and `painter --headless run`).
//!
//! Each line is a command mirroring a user action, so the tutorials can be replayed and
//! checked automatically, and pipelines can drive Painter. Lines starting with `#` are
//! comments; `${name}` is replaced by a variable (`--set name=value` or the environment).

use std::sync::Mutex;
use std::sync::atomic::{AtomicU8, Ordering};

/// Where `say!` output goes: 0 = stdout, 1 = stderr, 2 = nowhere.
pub static OUTPUT: AtomicU8 = AtomicU8::new(0);

macro_rules! say {
    ($($arg:tt)*) => {
        match $crate::script::OUTPUT.load(std::sync::atomic::Ordering::Relaxed) {
            0 => println!($($arg)*),
            1 => eprintln!($($arg)*),
            _ => {}
        }
    };
}
pub(crate) use say;

/// What a run produced, for `--json` reports.
#[derive(Default, Debug, Clone, serde::Serialize)]
pub struct Report {
    pub files: Vec<String>,
    pub warnings: Vec<String>,
    pub saved: Vec<String>,
}

pub static REPORT: Mutex<Option<Report>> = Mutex::new(None);

pub fn report_reset() {
    *REPORT.lock().unwrap() = Some(Report::default());
}

pub fn report_take() -> Report {
    REPORT.lock().unwrap().take().unwrap_or_default()
}

/// Path as text with the platform's separators (no mixed / and \).
pub fn norm(p: &Path) -> String {
    p.components().collect::<PathBuf>().display().to_string()
}

fn report(f: impl FnOnce(&mut Report)) {
    if let Some(r) = REPORT.lock().unwrap().as_mut() {
        f(r);
    }
}

pub fn set_output(mode: u8) {
    OUTPUT.store(mode, Ordering::Relaxed);
}

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use glam::Vec2;

use crate::camera::Camera;
use crate::channels::Channel;
use crate::gpu::Gpu;
use crate::graph::{BlendMode, ParamKind, Value};
use crate::mesh::{self, ImportSettings};
use crate::meshmaps::MeshMap;
use crate::ops::{self, Selection};
use crate::paint::{BakeMode, OnCameraMove, PaintEngine, SizeSpace, Tool};
use crate::project::Project;
use crate::viewport::{DisplayMode, ViewSettings, Viewport};

pub struct Session {
    pub gpu: Gpu,
    pub project: Option<Project>,
    pub viewport: Option<Viewport>,
    pub camera: Camera,
    pub view: ViewSettings,
    pub base_dir: PathBuf,
    pub size: (u32, u32),
    pub paint: PaintEngine,
    pub stencil_overlay: crate::stencil::StencilOverlay,
    pub evaluator: crate::export::UvEvaluator,
    /// Script variables naming nodes (`node <var> <type>`).
    pub vars: std::collections::HashMap<String, crate::graph::NodeId>,
    /// `${name}` substitutions (`--set name=value`); the environment is the fallback.
    pub defines: std::collections::HashMap<String, String>,
}

impl Session {
    pub fn new(base_dir: PathBuf) -> Result<Self> {
        Ok(Self::with_gpu(Gpu::headless()?, base_dir))
    }

    pub fn with_gpu(gpu: Gpu, base_dir: PathBuf) -> Self {
        Self {
            paint: PaintEngine::new(&gpu),
            stencil_overlay: crate::stencil::StencilOverlay::new(&gpu),
            evaluator: crate::export::UvEvaluator::new(&gpu),
            vars: Default::default(),
            defines: Default::default(),
            gpu,
            project: None,
            viewport: None,
            camera: Camera::default(),
            view: ViewSettings::default(),
            base_dir,
            size: (960, 720),
        }
    }

    fn project(&mut self) -> Result<&mut Project> {
        self.project.as_mut().context("no project; use `new <mesh>` first")
    }

    fn path(&self, p: &str) -> PathBuf {
        let p = Path::new(p);
        if p.is_absolute() { p.to_path_buf() } else { self.base_dir.join(p) }
    }

    pub fn run_file(&mut self, path: &Path) -> Result<()> {
        let text = if path == Path::new("-") {
            let mut t = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut t)?;
            t
        } else {
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?
        };
        self.run_text(&text, &path.display().to_string())
    }

    /// Runs script text; `origin` names it in error messages.
    pub fn run_text(&mut self, text: &str, origin: &str) -> Result<()> {
        for (n, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let line = self.substitute(line).with_context(|| format!("{origin}:{}", n + 1))?;
            say!("> {line}");
            self.run(&line).with_context(|| format!("{origin}:{}: {line}", n + 1))?;
        }
        Ok(())
    }

    /// Replaces `${name}` with a variable from `--set`, then the environment.
    fn substitute(&self, line: &str) -> Result<String> {
        let mut out = String::new();
        let mut rest = line;
        while let Some(start) = rest.find("${") {
            out.push_str(&rest[..start]);
            let end = rest[start..].find('}').with_context(|| format!("unterminated ${{ in: {line}"))? + start;
            let name = &rest[start + 2..end];
            let value = self
                .defines
                .get(name)
                .cloned()
                .or_else(|| std::env::var(name).ok())
                .with_context(|| format!("variable ${{{name}}} is not set (use --set {name}=...)"))?;
            out.push_str(&value);
            rest = &rest[end + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    pub fn run(&mut self, line: &str) -> Result<()> {
        let args = split_args(line);
        let cmd = args[0].as_str();
        let rest = &args[1..];
        let gpu = self.gpu.clone();
        match cmd {
            "new" => {
                let path = self.path(rest.first().context("new <mesh> [resolution]")?);
                let res = rest.get(1).map(|r| r.parse()).transpose()?.unwrap_or(2048);
                let settings = ImportSettings { tile_resolution: res, ..Default::default() };
                let mesh = mesh::import(&path, &settings)?;
                for d in &mesh.diagnostics {
                    say!("  [{:?}] {}: {}", d.severity, d.code, d.message);
                }
                if mesh.has_errors() {
                    bail!("import aborted");
                }
                say!("  tiles {:?}, materials {:?}", mesh.tiles, mesh.materials);
                self.camera = Camera::default();
                self.camera.frame(mesh.center(), mesh.radius());
                let p = Project::new(&gpu, mesh, settings);
                let mut vp = Viewport::new(&gpu, &p.scene.layout);
                vp.set_mesh(&gpu, &p.gpu_mesh);
                self.viewport = Some(vp);
                self.project = Some(p);
            }
            "bake" => {
                let p = self.project()?;
                let maps: Vec<MeshMap> = if rest.is_empty() || rest[0] == "all" {
                    vec![MeshMap::AmbientOcclusion, MeshMap::Curvature, MeshMap::Thickness, MeshMap::Id]
                } else if rest[0] == "outdated" {
                    MeshMap::ALL
                        .into_iter()
                        .filter(|m| p.maps.status(*m) == crate::meshmaps::MapStatus::OutOfDate)
                        .collect()
                } else {
                    rest.iter().map(|m| parse_map(m)).collect::<Result<_>>()?
                };
                if maps.is_empty() {
                    say!("  nothing to bake");
                    return Ok(());
                }
                let t = std::time::Instant::now();
                p.start_bake(&gpu, &maps);
                p.finish_background_work(&gpu);
                say!("  baked in {:.2}s", t.elapsed().as_secs_f32());
            }
            "add-fill" | "add-paint" => {
                let p = self.project()?;
                let above = p.selection.map(|s| s.layer());
                let l = p.edit(&gpu, cmd, |g| {
                    if cmd == "add-fill" { ops::add_fill_layer(g, above) } else { ops::add_paint_layer(g, above) }
                });
                if let Some(name) = rest.first() {
                    let name = name.clone();
                    p.edit(&gpu, "rename", |g| ops::slot_mut(g, l).unwrap().name = name);
                }
                p.selection = Some(Selection::Layer(l));
            }
            "select" => {
                let p = self.project()?;
                let name = rest.join(" ");
                let l = ops::find_layer(&p.graph, &name).with_context(|| format!("no layer named {name}"))?;
                p.selection = Some(Selection::Layer(l));
            }
            "select-mask" => {
                let p = self.project()?;
                let l = p.selection.context("no selection")?.layer();
                let i: usize = rest.first().context("select-mask <index>")?.parse()?;
                ops::mask_slot(&p.graph, l, i).context("no such mask layer")?;
                p.selection = Some(Selection::MaskLayer(l, i));
            }
            "rename" => {
                let p = self.project()?;
                let l = p.selection.context("no selection")?.layer();
                let name = rest.join(" ");
                p.edit(&gpu, "rename", |g| ops::slot_mut(g, l).unwrap().name = name);
            }
            "set" => {
                let p = self.project()?;
                for kv in rest {
                    let (k, v) = kv.split_once('=').context("set key=value")?;
                    set_value(p, &gpu, k, v)?;
                }
            }
            "add-mask" | "add-black-mask" => {
                let p = self.project()?;
                let l = p.selection.context("no selection")?.layer();
                p.edit(&gpu, cmd, |g| ops::add_mask(g, l, cmd == "add-black-mask"));
            }
            "add-generator" => {
                let p = self.project()?;
                let l = p.selection.context("no selection")?.layer();
                let ty = rest.first().context("add-generator <type>")?.replace('-', "_");
                crate::graph::def(&ty).with_context(|| format!("unknown generator {ty}"))?;
                let sel = p.edit(&gpu, cmd, |g| ops::add_mask_generator(g, l, &ty));
                p.selection = Some(sel);
            }
            "add-mask-paint" | "add-mask-fill" | "add-mask-levels" => {
                let p = self.project()?;
                let l = p.selection.context("no selection")?.layer();
                let sel = p.edit(&gpu, cmd, |g| match cmd {
                    "add-mask-paint" => ops::add_mask_paint(g, l),
                    "add-mask-fill" => ops::add_mask_fill(g, l),
                    _ => ops::add_mask_levels(g, l),
                });
                p.selection = Some(sel);
            }
            "move-below" | "move-above" => {
                let p = self.project()?;
                let l = p.selection.context("no selection")?.layer();
                let name = rest.join(" ");
                let t = ops::find_layer(&p.graph, &name).with_context(|| format!("no layer named {name}"))?;
                let nl = p.edit(&gpu, cmd, |g| {
                    if cmd == "move-below" { ops::move_below(g, l, t) } else { ops::move_above(g, l, t) }
                });
                p.selection = Some(Selection::Layer(nl));
            }
            "group" => {
                // group [layer names...]: groups the named layers, or the selected layer.
                let p = self.project()?;
                let layers: Vec<ops::LayerRef> = if rest.is_empty() {
                    vec![p.selection.context("no selection")?.layer()]
                } else {
                    rest.iter()
                        .map(|n| ops::find_layer(&p.graph, n).with_context(|| format!("no layer named {n}")))
                        .collect::<Result<_>>()?
                };
                let nl = p.edit(&gpu, cmd, |g| ops::group(g, &layers)).context("cannot group (layers must share a stack)")?;
                p.selection = Some(Selection::Layer(nl));
            }
            "delete" => {
                let p = self.project()?;
                let l = p.selection.context("no selection")?.layer();
                p.edit(&gpu, cmd, |g| ops::delete(g, l));
                p.selection = None;
            }
            "undo" => {
                let p = self.project()?;
                say!("  undid {:?}", p.undo(&gpu));
            }
            "redo" => {
                let p = self.project()?;
                say!("  redid {:?}", p.redo(&gpu));
            }
            "layers" => {
                let p = self.project()?;
                print_layers(p);
            }
            "expect-layers" => {
                let p = self.project()?;
                let names: Vec<String> = p.graph.node(p.graph.root).slots.iter().rev().map(|s| s.name.clone()).collect();
                let want: Vec<String> = rest.to_vec();
                if names != want {
                    bail!("expected layers {want:?} (top to bottom), found {names:?}");
                }
            }
            "view" => {
                let what = rest.join(" ");
                self.view.display = parse_display(&what)?;
            }
            "tiles" => self.view.show_tiles = rest.first().map(|s| s == "on").unwrap_or(true),
            "camera" => {
                let p = self.project.as_mut().context("no project")?;
                if !self.paint.camera_moving(&gpu, p) {
                    bail!("camera is locked (unbaked paint, On camera move = Lock camera)");
                }
                let (c, r) = (p.mesh.center(), p.mesh.radius());
                let yaw: f32 = rest.first().context("camera <yaw> <pitch> [zoom]")?.parse()?;
                let pitch: f32 = rest.get(1).context("camera <yaw> <pitch> [zoom]")?.parse()?;
                let zoom: f32 = rest.get(2).map(|z| z.parse()).transpose()?.unwrap_or(1.0);
                self.camera.yaw = yaw.to_radians();
                self.camera.pitch = pitch.to_radians();
                self.camera.frame(c, r);
                self.camera.distance /= zoom;
            }
            "tool" => {
                self.paint.tool = match rest.first().map(String::as_str) {
                    Some("paint") => Tool::Paint,
                    Some("erase") => Tool::Erase,
                    Some("paint-through") => Tool::PaintThrough,
                    _ => bail!("tool paint|erase|paint-through"),
                };
            }
            "mode" => {
                let mode = match rest.first().map(String::as_str) {
                    Some("live") => BakeMode::Live,
                    Some("manual") => BakeMode::Manual,
                    _ => bail!("mode live|manual"),
                };
                let p = self.project.as_mut().context("no project")?;
                self.paint.set_mode(&gpu, p, mode);
            }
            "on-camera-move" => {
                self.paint.on_camera_move = match rest.first().map(String::as_str) {
                    Some("bake") => OnCameraMove::Bake,
                    Some("clear") => OnCameraMove::Clear,
                    Some("lock") => OnCameraMove::LockCamera,
                    _ => bail!("on-camera-move bake|clear|lock"),
                };
            }
            "brush" => {
                for kv in rest {
                    let (k, v) = kv.split_once('=').context("brush key=value")?;
                    let b = &mut self.paint.brush;
                    match k {
                        "size" => b.size = v.parse()?,
                        "hardness" => b.hardness = v.parse()?,
                        "flow" => b.flow = v.parse()?,
                        "opacity" => b.opacity = v.parse()?,
                        "spacing" => b.spacing = v.parse()?,
                        "angle" => b.angle = v.parse()?,
                        "space" => b.size_space = if v == "world" { SizeSpace::World } else { SizeSpace::Screen },
                        "jitter_size" => b.jitter_size = v.parse()?,
                        "jitter_angle" => b.jitter_angle = v.parse()?,
                        "jitter_position" => b.jitter_position = v.parse()?,
                        "jitter_flow" => b.jitter_flow = v.parse()?,
                        _ => bail!("unknown brush property {k}"),
                    }
                }
            }
            "brush-values" => {
                for kv in rest {
                    let (k, v) = kv.split_once('=').context("brush-values channel=value|off")?;
                    if k == "mask" {
                        self.paint.values.mask_value = v.parse()?;
                        continue;
                    }
                    let c = channel_by_key(k).with_context(|| format!("unknown channel {k}"))?;
                    if v == "off" {
                        self.paint.values.enabled[c.index()] = false;
                    } else {
                        self.paint.values.enabled[c.index()] = true;
                        self.paint.values.values[c.index()] = parse_color(v)?;
                    }
                }
            }
            "stroke" => {
                // stroke x,y x,y ... in viewport fractions (0..1); pressure optional as x,y,p.
                let pts: Vec<(Vec2, f32)> = rest
                    .iter()
                    .map(|s| {
                        let v: Vec<f32> = s.split(',').map(|x| x.parse()).collect::<Result<_, _>>()?;
                        anyhow::ensure!(v.len() >= 2, "points are x,y[,pressure]");
                        let size = Vec2::new(self.size.0 as f32, self.size.1 as f32);
                        Ok((Vec2::new(v[0], v[1]) * size, v.get(2).copied().unwrap_or(1.0)))
                    })
                    .collect::<Result<_>>()?;
                anyhow::ensure!(!pts.is_empty(), "stroke needs points");
                let p = self.project.as_mut().context("no project")?;
                p.finish_background_work(&gpu);
                if !self.paint.camera_moving(&gpu, p) {
                    bail!("camera is locked");
                }
                let stencil = p.stencils.sample(|n| p.scene.image_layer(n), self.size);
                if self.paint.tool == Tool::PaintThrough && stencil.is_none() {
                    bail!("Paint Through needs a stencil");
                }
                self.paint
                    .begin_stroke(&gpu, p, &self.camera, self.size, pts[0].0, pts[0].1)
                    .map_err(|e| anyhow::anyhow!(e))?;
                for &(pt, pr) in &pts[1..] {
                    self.paint.stroke_to(&gpu, pt, pr);
                    self.paint.flush(&gpu, p, stencil);
                }
                self.paint.end_stroke(&gpu, p, stencil);
            }
            "stencil-add" => {
                let path = self.path(rest.first().context("stencil-add <image>")?);
                let p = self.project.as_mut().context("no project")?;
                p.add_stencil(&gpu, &path)?;
            }
            "stencil" => {
                let p = self.project.as_mut().context("no project")?;
                let s = p.stencils.active_mut().context("no active stencil")?;
                for kv in rest {
                    let (k, v) = kv.split_once('=').context("stencil key=value")?;
                    match k {
                        "opacity" => s.opacity = v.parse()?,
                        "visible" => s.visible = v.parse()?,
                        "use_alpha" => s.use_alpha = v.parse()?,
                        "tile" => s.tile = v.parse()?,
                        "flip" => s.flip = v.parse()?,
                        "targets" => {
                            s.targets = [false; 7];
                            for t in v.split(',') {
                                let c = channel_by_key(t).with_context(|| format!("unknown channel {t}"))?;
                                s.targets[c.index()] = true;
                            }
                        }
                        _ => bail!("unknown stencil property {k}"),
                    }
                }
            }
            "stencil-transform" => {
                // stencil-transform dx dy rotation_deg scale (dx, dy in viewport fractions)
                let v: Vec<f32> = rest.iter().map(|x| x.parse()).collect::<Result<_, _>>()?;
                anyhow::ensure!(v.len() == 4, "stencil-transform dx dy rotation scale");
                let size = Vec2::new(self.size.0 as f32, self.size.1 as f32);
                let p = self.project.as_mut().context("no project")?;
                let s = p.stencils.active_mut().context("no active stencil")?;
                s.offset = Vec2::new(v[0], v[1]) * size;
                s.rotation = v[2].to_radians();
                s.scale = v[3];
            }
            "export" => {
                // export <dir> [preset="glTF Metal/Rough"] [normal=directx] [padding=infinite|none|N]
                //        [format=png16|png8|tga|exr16|exr32] [resolution=N] [tiles=1001,1002]
                let dir = self.path(rest.first().context("export <dir> [options]")?);
                let mut s = crate::export::ExportSettings::new(dir);
                for kv in &rest[1..] {
                    let (k, v) = kv.split_once('=').context("export option key=value")?;
                    match k {
                        "preset" => s.preset = v.to_string(),
                        "template" => s.template = v.to_string(),
                        "normal" => {
                            s.normal_format = if v.eq_ignore_ascii_case("directx") {
                                crate::export::NormalFormat::DirectX
                            } else {
                                crate::export::NormalFormat::OpenGl
                            }
                        }
                        "padding" => {
                            s.padding = match v {
                                "infinite" => crate::export::Padding::Infinite,
                                "none" => crate::export::Padding::None,
                                n => crate::export::Padding::Texels(n.parse()?),
                            }
                        }
                        "format" => {
                            use crate::export::FileFormat::*;
                            s.format = Some(match v {
                                "png8" | "png" => (Png, 8),
                                "png16" => (Png, 16),
                                "tga" => (Tga, 8),
                                "exr16" | "exr" => (Exr, 16),
                                "exr32" => (Exr, 32),
                                _ => bail!("unknown format {v}"),
                            });
                        }
                        "resolution" => s.resolution = v.parse()?,
                        "tiles" => s.tiles = v.split(',').map(|t| t.parse()).collect::<Result<_, _>>()?,
                        _ => bail!("unknown export option {k}"),
                    }
                }
                let p = self.project.as_mut().context("no project")?;
                let t = std::time::Instant::now();
                let report = crate::export::export(&gpu, p, &mut self.evaluator, &s)?;
                for w in &report.warnings {
                    say!("  warning: {w}");
                }
                for f in &report.files {
                    say!("  {}", f.file_name().unwrap_or_default().to_string_lossy());
                }
                let (files, warnings) = (report.files.clone(), report.warnings.clone());
                self::report(|r| {
                    r.files.extend(files.iter().map(|f| norm(f)));
                    r.warnings.extend(warnings);
                });
                say!("  exported {} files in {:.2}s", report.files.len(), t.elapsed().as_secs_f32());
            }
            "expect-files" => {
                // expect-files <dir> <name>... : exactly these files exist in dir
                let dir = self.path(rest.first().context("expect-files <dir> <names>")?);
                let mut found: Vec<String> = std::fs::read_dir(&dir)?
                    .flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect();
                found.sort();
                let mut want: Vec<String> = rest[1..].to_vec();
                want.sort();
                anyhow::ensure!(found == want, "expected files {want:?}, found {found:?}");
            }
            "save" => {
                // save [dir.painter]: without a path, saves where the project was opened or saved.
                let dir = match rest.first() {
                    Some(d) => crate::projectfile::with_extension(self.path(d)),
                    None => self
                        .project
                        .as_ref()
                        .and_then(|p| p.path.clone())
                        .context("the project has never been saved; use save <dir.painter>")?,
                };
                let vs = crate::projectfile::ViewportState { camera: Some(self.camera), ..Default::default() };
                let p = self.project.as_mut().context("no project")?;
                let t = std::time::Instant::now();
                let pages = crate::projectfile::save(&gpu, p, &dir, &vs, None)?;
                say!("  saved {} ({pages} paint pages written) in {:.2}s", dir.display(), t.elapsed().as_secs_f32());
                report(|r| r.saved.push(norm(&dir)));
            }
            "reimport" => {
                let path = self.path(rest.first().context("reimport <mesh>")?);
                let p = self.project.as_mut().context("no project")?;
                p.reimport(&gpu, &path)?;
                for d in &p.mesh.diagnostics {
                    say!("  [{:?}] {}: {}", d.severity, d.code, d.message);
                    if d.severity >= crate::mesh::Severity::Warning {
                        let msg = format!("{}: {}", d.code, d.message);
                        report(|r| r.warnings.push(msg));
                    }
                }
                say!("  tiles {:?}, {} triangles", p.mesh.tiles, p.mesh.triangle_count());
                let mut vp = Viewport::new(&gpu, &p.scene.layout);
                vp.set_mesh(&gpu, &p.gpu_mesh);
                self.viewport = Some(vp);
                self.paint = PaintEngine::new(&gpu);
            }
            "info" => {
                let p = self.project()?;
                say!("{}", serde_json::to_string_pretty(&project_info(p))?);
            }
            "open" => {
                let dir = self.path(rest.first().context("open <dir.painter>")?);
                let t = std::time::Instant::now();
                let loaded = crate::projectfile::load(&gpu, &dir)?;
                if let Some(c) = loaded.viewport.camera {
                    self.camera = c;
                }
                let mut vp = Viewport::new(&gpu, &loaded.project.scene.layout);
                vp.set_mesh(&gpu, &loaded.project.gpu_mesh);
                self.viewport = Some(vp);
                self.project = Some(loaded.project);
                say!("  opened {} in {:.2}s", dir.display(), t.elapsed().as_secs_f32());
            }
            "ungroup" => {
                let p = self.project.as_mut().context("no project")?;
                let sel = p.selection.context("no selection")?;
                let node = ops::selected_node(&p.graph, sel).context("selection has no node")?;
                p.edit(&gpu, "ungroup", |g| crate::ui_graph::ungroup(g, node)).context("not a generator")?;
                p.validate_selection();
            }
            "node" => {
                // node <var> <type>: adds a node and names it.
                let var = rest.first().context("node <var> <type>")?.clone();
                let ty = rest.get(1).context("node <var> <type>")?.replace('-', "_");
                crate::graph::def(&ty).with_context(|| format!("unknown node type {ty}"))?;
                let p = self.project.as_mut().context("no project")?;
                let id = p.edit(&gpu, "add node", |g| g.add(&ty));
                self.vars.insert(var, id);
            }
            "link" => {
                // link <var>.<output> <var>.<input>
                let from = rest.first().context("link a.out b.in")?;
                let to = rest.get(1).context("link a.out b.in")?;
                let (fa, fo) = from.split_once('.').context("link a.out b.in")?;
                let (ta, ti) = to.split_once('.').context("link a.out b.in")?;
                let (f, t) = (self.var(fa)?, self.var(ta)?);
                let p = self.project.as_mut().context("no project")?;
                let ok = p.edit(&gpu, "connect", |g| g.connect(f, fo, t, ti));
                anyhow::ensure!(ok, "cannot connect {from} to {to}");
            }
            "node-set" => {
                let id = self.var(rest.first().context("node-set <var> key=value")?)?;
                let p = self.project.as_mut().context("no project")?;
                for kv in &rest[1..] {
                    let (k, v) = kv.split_once('=').context("key=value")?;
                    let n = p.graph.node(id).clone();
                    let def = n.def().params.iter().find(|d| d.name == k).with_context(|| format!("no parameter {k}"))?;
                    let value = match def.kind {
                        ParamKind::Float { .. } | ParamKind::Int { .. } => Value::Float(v.parse()?),
                        ParamKind::Bool => Value::Bool(v.parse()?),
                        ParamKind::Color | ParamKind::Vec3 => Value::Vec3(parse_color(v)?),
                        ParamKind::Enum(o) => Value::Text(o.iter().find(|x| x.eq_ignore_ascii_case(v)).with_context(|| format!("{k} must be one of {o:?}"))?.to_string()),
                        _ => bail!("{k} cannot be set from a script"),
                    };
                    let name = def.name;
                    p.edit(&gpu, "set", |g| g.set(id, name, value));
                }
            }
            "cache" => {
                let id = self.var(rest.first().context("cache <var> on|off")?)?;
                let on = rest.get(1).map(|s| s == "on").unwrap_or(true);
                let p = self.project.as_mut().context("no project")?;
                p.edit(&gpu, "cached", |g| g.node_mut(id).cached = on);
            }
            "layer-content" | "layer-mask" => {
                let id = self.var(rest.first().context("layer-content <var>")?)?;
                let p = self.project.as_mut().context("no project")?;
                let l = p.selection.context("no selection")?.layer();
                p.edit(&gpu, cmd, |g| {
                    if cmd == "layer-content" {
                        ops::slot_mut(g, l).unwrap().content = Some(id);
                    } else {
                        let ms = g.add("mask_stack");
                        g.node_mut(ms).mask_slots.push(crate::graph::MaskSlot {
                            name: "Graph".into(),
                            visible: true,
                            opacity: 1.0,
                            mode: BlendMode::Normal,
                            source: crate::graph::MaskSource::Node(id),
                        });
                        ops::slot_mut(g, l).unwrap().mask = Some(ms);
                    }
                });
            }
            "caches" => {
                let p = self.project.as_mut().context("no project")?;
                p.finish_background_work(&gpu);
                if let Some(e) = &p.cache_error {
                    bail!("cache error: {e}");
                }
                for e in p.caches.entries.values() {
                    say!("  cache n{} {:?} computed={} {:.1} ms", e.node, e.cache, e.signature.is_some(), e.cost_ms);
                }
            }
            "save-smart" => {
                let p = self.project.as_mut().context("no project")?;
                let l = p.selection.context("no selection")?.layer();
                if let Some(name) = rest.first() {
                    let name = name.clone();
                    p.edit(&gpu, "rename", |g| ops::slot_mut(g, l).unwrap().name = name);
                }
                let path = crate::smartmat::save(&gpu, p, l, None)?;
                say!("  saved {}", path.display());
            }
            "add-smart" => {
                let name = rest.first().context("add-smart <name or path>")?;
                let path = if name.ends_with(".psmat") {
                    self.path(name)
                } else {
                    crate::smartmat::library_dir().join(format!("{name}.psmat"))
                };
                let p = self.project.as_mut().context("no project")?;
                let above = p.selection.map(|s| s.layer());
                let l = crate::smartmat::insert(&gpu, p, &path, above)?;
                p.selection = Some(Selection::Layer(l));
            }
            "delete-smart" => {
                let name = rest.first().context("delete-smart <name>")?;
                let path = crate::smartmat::library_dir().join(format!("{name}.psmat"));
                std::fs::remove_file(&path).with_context(|| format!("removing {}", path.display()))?;
            }
            "expect-same" => {
                // expect-same <a.png> <b.png> [max channel difference, default 2]
                let a = image::open(self.path(rest.first().context("expect-same a b [tol]")?))?.to_rgba8();
                let b = image::open(self.path(rest.get(1).context("expect-same a b [tol]")?))?.to_rgba8();
                let tol: u8 = rest.get(2).map(|t| t.parse()).transpose()?.unwrap_or(2);
                anyhow::ensure!(a.dimensions() == b.dimensions(), "images differ in size");
                let max = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
                anyhow::ensure!(max <= tol, "images differ by up to {max} (tolerance {tol})");
                say!("  same (max difference {max})");
            }
            "expect-different" => {
                // expect-different <a.png> <b.png> [min channel difference, default 20]
                let a = image::open(self.path(rest.first().context("expect-different a b [min]")?))?.to_rgba8();
                let b = image::open(self.path(rest.get(1).context("expect-different a b [min]")?))?.to_rgba8();
                let min: u8 = rest.get(2).map(|t| t.parse()).transpose()?.unwrap_or(20);
                let max = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
                anyhow::ensure!(max >= min, "images are too similar (max difference {max}, expected at least {min})");
                say!("  different (max difference {max})");
            }
            "bake-buffer" => {
                let p = self.project.as_mut().context("no project")?;
                self.paint.bake(&gpu, p).map_err(|e| anyhow::anyhow!(e))?;
            }
            "clear-buffer" => self.paint.clear_buffer(&gpu),
            "buffer-transform" => {
                // buffer-transform dx dy rotation_deg scale (screen px)
                let v: Vec<f32> = rest.iter().map(|x| x.parse()).collect::<Result<_, _>>()?;
                anyhow::ensure!(v.len() == 4, "buffer-transform dx dy rotation scale");
                self.paint.transform.offset = Vec2::new(v[0], v[1]);
                self.paint.transform.rotation = v[2].to_radians();
                self.paint.transform.scale = v[3];
            }
            "expect-unbaked" => {
                let want: bool = rest.first().context("expect-unbaked true|false")?.parse()?;
                anyhow::ensure!(self.paint.unbaked == want, "unbaked is {}", self.paint.unbaked);
            }
            "environment" => {
                let path = self.path(rest.first().context("environment <file.hdr|exr>")?);
                self.viewport.as_mut().context("no project")?.set_environment(&gpu, Some(&path))?;
                self.view.hdri = true;
            }
            "exposure" => self.view.exposure = rest.first().context("exposure <ev>")?.parse()?,
            "size" => {
                let (w, h) = rest.first().context("size WxH")?.split_once('x').context("size WxH")?;
                self.size = (w.parse()?, h.parse()?);
            }
            "preview" => {
                // preview <var>[.output] | preview off: what Ctrl+Shift+click shows in the viewport.
                let arg = rest.first().context("preview <node>[.output] | off")?.clone();
                let link = if arg == "off" {
                    None
                } else {
                    let (var, out) = arg.split_once('.').map_or((arg.as_str(), None), |(v, o)| (v, Some(o)));
                    let node = self.var(var)?;
                    let p = self.project()?;
                    let output = match out {
                        Some(o) => o.to_string(),
                        None => p.graph.node(node).def().outputs.first().context("node has no outputs")?.0.to_string(),
                    };
                    p.graph.output_type(&crate::graph::Link { node, output: output.clone() }).with_context(|| format!("no output {output}"))?;
                    Some(crate::graph::Link { node, output })
                };
                let p = self.project()?;
                p.set_preview(&gpu, link);
            }
            "render" => {
                let out = self.path(rest.first().context("render <out.png>")?);
                self.render(&out)?;
                say!("  wrote {}", out.display());
                report(|r| r.files.push(norm(&out)));
            }
            _ => bail!("unknown command {cmd}"),
        }
        Ok(())
    }

    fn var(&self, name: &str) -> Result<crate::graph::NodeId> {
        self.vars.get(name).copied().with_context(|| format!("unknown node variable {name}"))
    }

    pub fn render(&mut self, out: &Path) -> Result<()> {
        let gpu = self.gpu.clone();
        let p = self.project.as_mut().context("no project")?;
        p.finish_background_work(&gpu);
        let vp = self.viewport.as_mut().unwrap();
        if let Err(e) = vp.set_material(&gpu, p.viewport_code()) {
            bail!("material shader failed to compile:\n{e}");
        }
        vp.render(
            &gpu,
            &self.camera,
            &p.mesh,
            &p.gpu_mesh,
            p.scene.bind_group.as_ref().unwrap(),
            &self.view,
            self.size.0,
            self.size.1,
        );
        self.paint.draw_overlay(&gpu, vp.output_view().unwrap(), self.size);
        let layer = p.stencils.active().map(|s| p.scene.image_layer(&s.image)).unwrap_or(-1);
        self.stencil_overlay.draw(&gpu, vp.output_view().unwrap(), self.size, &p.stencils, &p.scene.images, layer);
        let pixels = gpu.read_texture(vp.output_texture().unwrap(), 0, 4);
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        image::save_buffer(out, &pixels, self.size.0, self.size.1, image::ColorType::Rgba8)?;
        Ok(())
    }
}

pub fn print_layers(p: &Project) {
    fn walk(p: &Project, stack: crate::graph::NodeId, depth: usize) {
        for s in p.graph.node(stack).slots.iter().rev() {
            let kind = ops::layer_kind(&p.graph, s);
            say!("  {}{:?} {:?}{}", "  ".repeat(depth), kind, s.name, if s.mask.is_some() { " [mask]" } else { "" });
            if let Some(m) = s.mask {
                for ms in p.graph.node(m).mask_slots.iter().rev() {
                    say!("  {}    mask: {:?}", "  ".repeat(depth), ms.name);
                }
            }
            if kind == ops::LayerKind::Group {
                walk(p, s.content.unwrap(), depth + 1);
            }
        }
    }
    walk(p, p.graph.root, 0);
}

fn split_args(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for ch in line.chars() {
        match ch {
            '"' => quoted = !quoted,
            c if c.is_whitespace() && !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_map(m: &str) -> Result<MeshMap> {
    let n = m.to_ascii_lowercase().replace(['-', '_', ' '], "");
    Ok(match n.as_str() {
        "ao" | "ambientocclusion" => MeshMap::AmbientOcclusion,
        "curvature" => MeshMap::Curvature,
        "thickness" => MeshMap::Thickness,
        "id" => MeshMap::Id,
        "position" => MeshMap::Position,
        "worldnormal" | "normal" => MeshMap::WorldNormal,
        _ => bail!("unknown mesh map {m}"),
    })
}

fn parse_display(s: &str) -> Result<DisplayMode> {
    let n = s.to_ascii_lowercase().replace(['-', '_', ' '], "");
    if n == "material" {
        return Ok(DisplayMode::Material);
    }
    for c in Channel::ALL {
        if c.name().to_ascii_lowercase().replace(' ', "") == n {
            return Ok(DisplayMode::Channel(c.index() as u32));
        }
    }
    let m = parse_map(s)?;
    Ok(DisplayMode::MeshMap(m.index() as u32))
}

/// sRGB hex (#RRGGBB) or "r,g,b" (linear) to linear RGB.
pub fn parse_color(v: &str) -> Result<[f32; 3]> {
    if let Some(hex) = v.strip_prefix('#') {
        if hex.len() != 6 {
            bail!("colors are #RRGGBB");
        }
        let c = |i: usize| -> Result<f32> {
            let x = u8::from_str_radix(&hex[i..i + 2], 16)? as f32 / 255.0;
            Ok(if x <= 0.04045 { x / 12.92 } else { ((x + 0.055) / 1.055).powf(2.4) })
        };
        return Ok([c(0)?, c(2)?, c(4)?]);
    }
    let parts: Vec<f32> = v.split(',').map(|x| x.trim().parse()).collect::<Result<_, _>>()?;
    match parts.as_slice() {
        [r, g, b] => Ok([*r, *g, *b]),
        [x] => Ok([*x; 3]),
        _ => bail!("bad color {v}"),
    }
}

fn channel_by_key(k: &str) -> Option<Channel> {
    let n = k.to_ascii_lowercase().replace(['-', '_', ' '], "");
    Channel::ALL.into_iter().find(|c| c.name().to_ascii_lowercase().replace(' ', "") == n)
}

/// `set` on the current selection. Layer keys: `opacity`, `<channel>.enabled|mode|opacity`.
/// Fill layers also accept channel names as shorthand for their values (`base_color=#..`).
/// Everything else is a parameter of the selected node.
fn set_value(p: &mut Project, gpu: &Gpu, key: &str, v: &str) -> Result<()> {
    let sel = p.selection.context("no selection")?;
    if let Selection::Layer(l) = sel {
        if key == "opacity" {
            let x: f32 = v.parse()?;
            p.edit(gpu, "set", |g| ops::slot_mut(g, l).unwrap().opacity = x);
            return Ok(());
        }
        if let Some((ch, prop)) = key.split_once('.') {
            let c = channel_by_key(ch).with_context(|| format!("unknown channel {ch}"))?;
            match prop {
                "enabled" => {
                    let b: bool = v.parse()?;
                    p.edit(gpu, "set", |g| ops::slot_mut(g, l).unwrap().channels[c.index()].enabled = b);
                }
                "opacity" => {
                    let x: f32 = v.parse()?;
                    p.edit(gpu, "set", |g| ops::slot_mut(g, l).unwrap().channels[c.index()].opacity = x);
                }
                "mode" => {
                    let m = BlendMode::for_channel(c)
                        .iter()
                        .find(|m| m.name().eq_ignore_ascii_case(v))
                        .with_context(|| format!("blend mode {v} not available for {}", c.name()))?;
                    p.edit(gpu, "set", |g| ops::slot_mut(g, l).unwrap().channels[c.index()].mode = *m);
                }
                _ => bail!("unknown layer property {prop}"),
            }
            return Ok(());
        }
    }
    if let Selection::MaskLayer(l, i) = sel {
        if key == "opacity" || key == "mode" {
            let v = v.to_string();
            let key = key.to_string();
            p.edit(gpu, "set", |g| {
                let ms = ops::mask_slot_mut(g, l, i).unwrap();
                if key == "opacity" {
                    ms.opacity = v.parse().unwrap_or(1.0);
                } else if let Some(m) = BlendMode::MASK.iter().find(|m| m.name().eq_ignore_ascii_case(&v)) {
                    ms.mode = *m;
                }
            });
            return Ok(());
        }
    }
    let node = ops::selected_node(&p.graph, sel).context("selection has no parameters")?;
    let n = p.graph.node(node).clone();
    let key = key.replace('-', "_");
    let def = n
        .def()
        .params
        .iter()
        .find(|d| d.name == key)
        .with_context(|| format!("{} has no parameter {key}", n.def().title))?;
    let value = match def.kind {
        ParamKind::Float { .. } | ParamKind::Int { .. } => Value::Float(v.parse()?),
        ParamKind::Bool => Value::Bool(v.parse()?),
        ParamKind::Color => Value::Vec3(parse_color(v)?),
        ParamKind::Vec3 => Value::Vec3(parse_color(v)?),
        ParamKind::Enum(opts) => {
            let o = opts.iter().find(|o| o.eq_ignore_ascii_case(v)).with_context(|| format!("{key} must be one of {opts:?}"))?;
            Value::Text(o.to_string())
        }
        ParamKind::Image => {
            let path = PathBuf::from(v);
            Value::Text(p.import_image(gpu, &path)?)
        }
        ParamKind::Stops | ParamKind::Colors => bail!("{key} cannot be set from a script"),
    };
    let name = def.name;
    p.edit(gpu, "set", |g| g.set(node, name, value));
    Ok(())
}

/// Summary of a project for `painter info` and the `info` command.
pub fn project_info(p: &Project) -> serde_json::Value {
    fn layers(p: &Project, stack: crate::graph::NodeId) -> Vec<serde_json::Value> {
        p.graph
            .get(stack)
            .map(|n| n.slots.iter().rev())
            .into_iter()
            .flatten()
            .map(|s| {
                let kind = ops::layer_kind(&p.graph, s);
                let mut v = serde_json::json!({
                    "name": s.name,
                    "kind": kind.name(),
                    "visible": s.visible,
                    "opacity": s.opacity,
                    "mask": s.mask.and_then(|m| p.graph.get(m)).map(|m| m.mask_slots.iter().rev().map(|ms| ms.name.clone()).collect::<Vec<_>>()),
                });
                if kind == ops::LayerKind::Group {
                    v["layers"] = serde_json::Value::from(layers(p, s.content.unwrap_or(0)));
                }
                v
            })
            .collect()
    }
    let maps: serde_json::Map<String, serde_json::Value> = MeshMap::ALL
        .iter()
        .map(|m| (m.name().to_string(), serde_json::Value::from(p.maps.status(*m).label())))
        .collect();
    serde_json::json!({
        "name": p.name,
        "path": p.path.as_ref().map(|x| norm(x)),
        "mesh": norm(&p.original_mesh),
        "triangles": p.mesh.triangle_count(),
        "materials": p.mesh.materials,
        "tiles": p.mesh.tiles,
        "tile_resolution": p.tile_resolution(),
        "channels": Channel::ALL.iter().filter(|c| p.channels.is_enabled(**c)).map(|c| c.name()).collect::<Vec<_>>(),
        "layers": layers(p, p.graph.root),
        "mesh_maps": maps,
        "unsaved_changes": p.has_unsaved_changes(),
    })
}
