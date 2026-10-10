//! Saving and loading `.crate` project directories. See docs/reference/project-format.md.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::camera::Camera;
use crate::channels::ChannelSettings;
use crate::export::ExportSettings;
use crate::gpu::Gpu;
use crate::graph::{Graph, Link, MaskSlot, Node, NodeId, Slot, Value};
use crate::mesh::{self, ImportSettings};
use crate::meshmaps::{BakeSettings, MapStatus, MeshMap};
use crate::paint::{PAGE, read_pages, write_pages};
use crate::project::Project;
use crate::stencil::StencilSet;

pub const FORMAT: &str = "crate-project";
/// Projects saved before the rename.
const LEGACY_FORMAT: &str = "painter-project";
pub const VERSION: u32 = 1;

/// State saved with the project that lives outside `Project`.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ViewportState {
    pub camera: Option<Camera>,
    #[serde(default)]
    pub exposure: f32,
    #[serde(default)]
    pub env_rotation: f32,
    #[serde(default)]
    pub environment: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct MeshRef {
    file: String,
    original: String,
    settings: ImportSettings,
}

#[derive(Serialize, Deserialize)]
struct MeshMapsState {
    settings: BakeSettings,
    baked: Vec<MeshMap>,
}

#[derive(Serialize, Deserialize)]
struct NodeFile {
    id: String,
    #[serde(rename = "type")]
    ty: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    label: String,
    #[serde(default)]
    params: BTreeMap<String, Value>,
    #[serde(default)]
    cached: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    exposed: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    slots: Vec<Json>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    mask_slots: Vec<MaskSlot>,
    #[serde(default)]
    pos: [f32; 2],
}

#[derive(Serialize, Deserialize)]
struct Connection {
    from: String,
    to: String,
}

#[derive(Serialize, Deserialize)]
struct GraphFile {
    nodes: Vec<NodeFile>,
    connections: Vec<Connection>,
}

#[derive(Serialize, Deserialize)]
struct ProjectFile {
    format: String,
    version: u32,
    mesh: MeshRef,
    #[serde(rename = "tileResolution")]
    tile_resolution: u32,
    tiles: Vec<u32>,
    channels: ChannelSettings,
    graph: GraphFile,
    #[serde(rename = "rootStack")]
    root_stack: String,
    viewport: ViewportState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    export: Option<ExportSettings>,
    #[serde(default)]
    stencils: StencilSet,
    #[serde(rename = "meshMaps")]
    mesh_maps: MeshMapsState,
    #[serde(default)]
    images: Vec<String>,
    /// Keys this version doesn't know about, preserved on save.
    #[serde(flatten)]
    extra: serde_json::Map<String, Json>,
}

fn id_str(id: NodeId) -> String {
    format!("n{id}")
}

fn parse_id(s: &str) -> Result<NodeId> {
    s.strip_prefix('n').unwrap_or(s).parse().with_context(|| format!("bad node id {s}"))
}

/// Slots refer to nodes by string id in the file.
fn slot_to_json(s: &Slot) -> Json {
    let mut v = serde_json::to_value(s).unwrap();
    if let Json::Object(m) = &mut v {
        m.insert("content".into(), s.content.map(|c| Json::String(id_str(c))).unwrap_or(Json::Null));
        m.insert("mask".into(), s.mask.map(|c| Json::String(id_str(c))).unwrap_or(Json::Null));
    }
    v
}

fn slot_from_json(mut v: Json) -> Result<Slot> {
    if let Json::Object(m) = &mut v {
        for k in ["content", "mask"] {
            let id = match m.get(k) {
                Some(Json::String(s)) => Json::from(parse_id(s)?),
                _ => Json::Null,
            };
            m.insert(k.into(), id);
        }
    }
    Ok(serde_json::from_value(v)?)
}

fn graph_to_file(g: &Graph) -> GraphFile {
    let mut connections = Vec::new();
    let nodes = g
        .nodes
        .values()
        .map(|n| {
            for (input, l) in &n.inputs {
                connections.push(Connection { from: format!("{}.{}", id_str(l.node), l.output), to: format!("{}.{}", id_str(n.id), input) });
            }
            let mask_slots = n.mask_slots.clone();
            NodeFile {
                id: id_str(n.id),
                ty: n.ty.clone(),
                label: n.label.clone(),
                params: n.params.clone(),
                cached: n.cached,
                exposed: n.exposed.iter().cloned().collect(),
                slots: n.slots.iter().map(slot_to_json).collect(),
                mask_slots,
                pos: n.pos,
            }
        })
        .collect();
    GraphFile { nodes, connections }
}

fn graph_from_file(f: GraphFile, root: &str) -> Result<Graph> {
    let mut g = Graph { nodes: BTreeMap::new(), next_id: 1, root: parse_id(root)? };
    for nf in f.nodes {
        let id = parse_id(&nf.id)?;
        if crate::graph::def(&nf.ty).is_none() {
            bail!("unknown node type {} ({})", nf.ty, nf.id);
        }
        let slots = nf.slots.into_iter().map(slot_from_json).collect::<Result<Vec<_>>>()?;
        g.nodes.insert(
            id,
            Node {
                id,
                ty: nf.ty,
                label: nf.label,
                params: nf.params,
                inputs: BTreeMap::new(),
                cached: nf.cached,
                exposed: nf.exposed.into_iter().collect(),
                slots,
                mask_slots: nf.mask_slots,
                pos: nf.pos,
            },
        );
        g.next_id = g.next_id.max(id + 1);
    }
    for c in f.connections {
        let (from, out) = c.from.split_once('.').context("connection from node.output")?;
        let (to, input) = c.to.split_once('.').context("connection to node.input")?;
        let (from, to) = (parse_id(from)?, parse_id(to)?);
        if let Some(n) = g.nodes.get_mut(&to) {
            n.inputs.insert(input.to_string(), Link { node: from, output: out.to_string() });
        }
    }
    if g.get(g.root).is_none() || g.output_node().is_none() {
        bail!("the graph has no root stack or material output");
    }
    Ok(g)
}

fn copy_mesh(src: &Path, dir: &Path) -> Result<String> {
    let mesh_dir = dir.join("mesh");
    std::fs::create_dir_all(&mesh_dir)?;
    let ext = src.extension().and_then(|e| e.to_str()).unwrap_or("glb").to_ascii_lowercase();
    let name = format!("source.{ext}");
    let dst = mesh_dir.join(&name);
    if src.canonicalize().ok() != dst.canonicalize().ok() {
        std::fs::copy(src, &dst).with_context(|| format!("copying {}", src.display()))?;
        // glTF and OBJ can reference sidecar files; copy those that sit next to the mesh.
        if let Some(parent) = src.parent() {
            let text = std::fs::read_to_string(src).unwrap_or_default();
            for entry in std::fs::read_dir(parent).into_iter().flatten().flatten() {
                let fname = entry.file_name().to_string_lossy().into_owned();
                let is_sidecar = [".bin", ".mtl"].iter().any(|e| fname.to_ascii_lowercase().ends_with(e));
                if is_sidecar && text.contains(&fname) {
                    let _ = std::fs::copy(entry.path(), mesh_dir.join(&fname));
                }
            }
        }
    }
    Ok(format!("mesh/{name}"))
}

/// Writes a project to `dir` (a `.crate` directory). Returns the number of paint pages written.
pub fn save(gpu: &Gpu, p: &mut Project, dir: &Path, viewport: &ViewportState, export: Option<&ExportSettings>) -> Result<usize> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    p.finish_background_work(gpu);
    let mesh_file = copy_mesh(&p.mesh.source, dir)?;

    // Images and stencils.
    let images_dir = dir.join("images");
    let stencils_dir = dir.join("stencils");
    let stencil_names: Vec<String> = p.stencils.stencils.iter().map(|s| s.image.clone()).collect();
    let mut images = Vec::new();
    for (name, src) in p.image_sources.clone() {
        let target_dir = if stencil_names.contains(&name) { &stencils_dir } else { &images_dir };
        std::fs::create_dir_all(target_dir)?;
        let dst = target_dir.join(&name);
        if src.canonicalize().ok() != dst.canonicalize().ok() {
            std::fs::copy(&src, &dst).with_context(|| format!("copying {}", src.display()))?;
        }
        p.image_sources.insert(name.clone(), dst);
        if !stencil_names.contains(&name) {
            images.push(name);
        }
    }

    // Mesh maps.
    let maps_dir = dir.join("maps");
    std::fs::create_dir_all(&maps_dir)?;
    let mut baked = Vec::new();
    for m in MeshMap::ALL {
        if p.maps.status(m) == MapStatus::Missing {
            continue;
        }
        baked.push(m);
        if p.maps.status(m) == MapStatus::UpToDate && p.saved_maps.contains(&m) {
            continue;
        }
        for (layer, udim) in p.mesh.tiles.clone().iter().enumerate() {
            let path = maps_dir.join(format!("{}.{udim}.exr", m.file_stem()));
            save_map(gpu, p, m, layer as u32, &path)?;
        }
        p.saved_maps.insert(m);
    }

    let pages = save_paint(gpu, p, &dir.join("paint"))?;

    let file = ProjectFile {
        format: FORMAT.into(),
        version: VERSION,
        mesh: MeshRef {
            file: mesh_file,
            original: p.original_mesh.display().to_string(),
            settings: p.import_settings.clone(),
        },
        tile_resolution: p.tile_resolution(),
        tiles: p.mesh.tiles.clone(),
        channels: p.channels.clone(),
        graph: graph_to_file(&p.graph),
        root_stack: id_str(p.graph.root),
        viewport: viewport.clone(),
        export: export.cloned(),
        stencils: p.stencils.clone(),
        mesh_maps: MeshMapsState { settings: p.maps.settings, baked },
        images,
        extra: p.file_extra.clone(),
    };
    let json = serde_json::to_string_pretty(&file)?;
    let tmp = dir.join("project.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, dir.join("project.json"))?;
    p.name = dir.file_stem().map(|s| s.to_string_lossy().into_owned());
    p.path = Some(dir.to_path_buf());
    p.saved_version = p.edit_version;
    Ok(pages)
}

fn save_map(gpu: &Gpu, p: &Project, m: MeshMap, layer: u32, path: &Path) -> Result<()> {
    let maps = &p.maps;
    let (tex, bpp) = match m {
        MeshMap::Position => (&maps.position.texture, 16),
        MeshMap::WorldNormal => (&maps.normal.texture, 8),
        MeshMap::Id => (&maps.id.texture, 4),
        MeshMap::AmbientOcclusion => (&maps.ao.texture, 4),
        MeshMap::Curvature => (&maps.curvature.texture, 4),
        MeshMap::Thickness => (&maps.thickness.texture, 4),
    };
    let bytes = gpu.read_texture(tex, layer, bpp);
    let res = maps.res as usize;
    let px: Vec<[f32; 4]> = match m {
        MeshMap::Position => bytemuck::cast_slice::<u8, [f32; 4]>(&bytes).to_vec(),
        MeshMap::WorldNormal => bytemuck::cast_slice::<u8, [half::f16; 4]>(&bytes).iter().map(|c| c.map(|h| h.to_f32())).collect(),
        MeshMap::Id => bytes.chunks_exact(4).map(|c| [c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0]).collect(),
        _ => bytemuck::cast_slice::<u8, f32>(&bytes).iter().map(|v| [*v, *v, *v, 1.0]).collect(),
    };
    let h = half::f16::from_f32;
    exr::prelude::write_rgba_file(path, res, res, |x, y| {
        let c = px[y * res + x];
        (h(c[0]), h(c[1]), h(c[2]), h(c[3]))
    })
    .map_err(|e| anyhow::anyhow!("writing {}: {e}", path.display()))
}

fn load_map(gpu: &Gpu, p: &Project, m: MeshMap, layer: u32, path: &Path) -> Result<()> {
    if matches!(m, MeshMap::Position | MeshMap::WorldNormal) {
        return Ok(()); // Rebaked on load; they are cheap and must match the mesh exactly.
    }
    let img = image::open(path).with_context(|| format!("reading {}", path.display()))?.to_rgba32f();
    let res = p.maps.res;
    if img.width() != res || img.height() != res {
        bail!("{} has the wrong size", path.display());
    }
    let (tex, data): (&wgpu::Texture, Vec<u8>) = match m {
        MeshMap::Id => (&p.maps.id.texture, img.pixels().flat_map(|c| c.0.map(|v| (v * 255.0 + 0.5) as u8)).collect()),
        _ => {
            let t = match m {
                MeshMap::AmbientOcclusion => &p.maps.ao.texture,
                MeshMap::Curvature => &p.maps.curvature.texture,
                _ => &p.maps.thickness.texture,
            };
            let v: Vec<f32> = img.pixels().map(|c| c.0[0]).collect();
            (t, bytemuck::cast_slice(&v).to_vec())
        }
    };
    let bpp = (data.len() as u32) / (res * res);
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: layer }, aspect: wgpu::TextureAspect::All },
        &data,
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(res * bpp), rows_per_image: Some(res) },
        wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
    );
    Ok(())
}

/// True if any coverage value in the page is non-zero.
fn page_has_paint(planes: &[&[u8]], mask: bool) -> bool {
    let cov = |plane: &[u8], comps: &[usize]| {
        bytemuck::cast_slice::<u8, [half::f16; 4]>(plane).iter().any(|t| comps.iter().any(|&c| t[c].to_f32() > 0.0))
    };
    if mask {
        cov(planes[0], &[3])
    } else {
        cov(planes[0], &[3]) || cov(planes[1], &[3]) || cov(planes[2], &[3]) || cov(planes[4], &[0, 1, 2, 3])
    }
}

fn hash(data: &[&[u8]]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for d in data {
        d.hash(&mut h);
    }
    h.finish()
}

/// Writes non-empty pages of every painted node; unchanged pages are skipped and pages
/// that became empty are deleted.
fn save_paint(gpu: &Gpu, p: &mut Project, dir: &Path) -> Result<usize> {
    let tiles = p.tile_count();
    let res = p.tile_resolution();
    let ppr = res / PAGE;
    let mut written = 0;
    let live = p.graph.subgraph(p.graph.output_node().unwrap_or(p.graph.root));
    let mut nodes: Vec<(NodeId, u32, bool)> = p.paint_slots.iter().map(|(n, s)| (*n, *s, false)).collect();
    nodes.extend(p.mask_slots.iter().map(|(n, s)| (*n, *s, true)));
    let mut keep_dirs = Vec::new();
    let mut saved = std::mem::take(&mut p.saved_pages);
    for (node, slot, mask) in nodes {
        if !live.contains(&node) || !matches!(p.graph.node(node).ty.as_str(), "paint" | "paint_mask") {
            continue;
        }
        let node_dir = dir.join(id_str(node));
        keep_dirs.push(node_dir.clone());
        let planes = crate::paint::pool_textures(p, mask);
        for (t, udim) in p.mesh.tiles.clone().iter().enumerate() {
            let pages: Vec<(u32, u32, u32)> =
                (0..ppr * ppr).map(|i| (slot * tiles + t as u32, i % ppr, i / ppr)).collect();
            let data = read_pages(gpu, &planes, &pages);
            let tile_dir = node_dir.join(udim.to_string());
            let page_bytes = (PAGE * PAGE * 8) as usize;
            for (k, &(_, px, py)) in pages.iter().enumerate() {
                let slices: Vec<&[u8]> = data.iter().map(|d| &d[k * page_bytes..(k + 1) * page_bytes]).collect();
                let file = tile_dir.join(format!("{px}_{py}.page"));
                let key = (node, *udim, px, py);
                if !page_has_paint(&slices, mask) {
                    if saved.remove(&key).is_some() || file.exists() {
                        let _ = std::fs::remove_file(&file);
                    }
                    continue;
                }
                let h = hash(&slices);
                if saved.get(&key) == Some(&h) && file.exists() {
                    continue;
                }
                std::fs::create_dir_all(&tile_dir)?;
                let raw: Vec<u8> = slices.concat();
                let compressed = compress(&raw);
                let tmp = file.with_extension("tmp");
                std::fs::write(&tmp, compressed)?;
                std::fs::rename(&tmp, &file)?;
                saved.insert(key, h);
                written += 1;
            }
        }
    }
    p.saved_pages = saved;
    // Remove paint of nodes that no longer exist.
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if !keep_dirs.contains(&e.path()) {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
    Ok(written)
}

fn load_paint(gpu: &Gpu, p: &mut Project, dir: &Path) -> Result<()> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Ok(()) };
    let tiles = p.tile_count();
    let page_bytes = (PAGE * PAGE * 8) as usize;
    for e in entries.flatten() {
        let Ok(node) = parse_id(&e.file_name().to_string_lossy()) else { continue };
        let Some(n) = p.graph.get(node) else { continue };
        let mask = n.ty == "paint_mask";
        let slot = p.paint_slot(gpu, node);
        let planes_n = if mask { 1 } else { crate::scene::PAINT_PLANES };
        for t in std::fs::read_dir(e.path())?.flatten() {
            let Ok(udim) = t.file_name().to_string_lossy().parse::<u32>() else { continue };
            let Some(layer) = p.mesh.tiles.iter().position(|x| *x == udim) else { continue };
            let mut pages = Vec::new();
            let mut data: Vec<Vec<u8>> = vec![Vec::new(); planes_n];
            for f in std::fs::read_dir(t.path())?.flatten() {
                let name = f.file_name().to_string_lossy().into_owned();
                let Some(stem) = name.strip_suffix(".page") else { continue };
                let Some((px, py)) = stem.split_once('_') else { continue };
                let (px, py): (u32, u32) = (px.parse()?, py.parse()?);
                let raw = decompress(&std::fs::read(f.path())?)?;
                if raw.len() != page_bytes * planes_n {
                    bail!("{} has the wrong size", f.path().display());
                }
                for (k, d) in data.iter_mut().enumerate() {
                    d.extend_from_slice(&raw[k * page_bytes..(k + 1) * page_bytes]);
                }
                pages.push((slot * tiles + layer as u32, px, py));
                p.saved_pages.insert((node, udim, px, py), hash(&raw.chunks(page_bytes).collect::<Vec<_>>()));
            }
            let planes = crate::paint::pool_textures(p, mask);
            write_pages(gpu, &planes, &pages, &data);
        }
    }
    Ok(())
}

pub struct Loaded {
    pub project: Project,
    pub viewport: ViewportState,
    pub export: Option<ExportSettings>,
}

pub fn load(gpu: &Gpu, dir: &Path) -> Result<Loaded> {
    let text = std::fs::read_to_string(dir.join("project.json")).with_context(|| format!("{} is not a Crate project", dir.display()))?;
    let file: ProjectFile = serde_json::from_str(&text).context("project.json is invalid")?;
    if file.format != FORMAT && file.format != LEGACY_FORMAT {
        bail!("not a Crate project (format {:?})", file.format);
    }
    if file.version > VERSION {
        bail!("this project was saved by a newer version (format version {})", file.version);
    }
    let mut settings = file.mesh.settings;
    settings.tile_resolution = file.tile_resolution;
    let mesh_path = dir.join(&file.mesh.file);
    let mesh = mesh::import(&mesh_path, &settings)?;
    if mesh.has_errors() {
        bail!("the project's mesh failed to import");
    }
    let mut p = Project::new(gpu, mesh, settings);
    p.original_mesh = PathBuf::from(&file.mesh.original);
    p.channels = file.channels;
    p.graph = graph_from_file(file.graph, &file.root_stack)?;
    p.maps.settings = file.mesh_maps.settings;
    p.file_extra = file.extra;
    for name in &file.images {
        let path = dir.join("images").join(name);
        if let Err(e) = p.import_image(gpu, &path) {
            log::warn!("{e:#}");
        }
    }
    for s in &file.stencils.stencils {
        let path = dir.join("stencils").join(&s.image);
        if let Err(e) = p.import_image(gpu, &path) {
            log::warn!("{e:#}");
        }
    }
    p.stencils = file.stencils;
    for m in file.mesh_maps.baked {
        let mut ok = true;
        for (layer, udim) in p.mesh.tiles.clone().iter().enumerate() {
            let path = dir.join("maps").join(format!("{}.{udim}.exr", m.file_stem()));
            if let Err(e) = load_map(gpu, &p, m, layer as u32, &path) {
                log::warn!("{e:#}");
                ok = false;
            }
        }
        if ok {
            p.maps.mark_baked(m);
            p.saved_maps.insert(m);
        }
    }
    load_paint(gpu, &mut p, &dir.join("paint"))?;
    p.name = dir.file_stem().map(|s| s.to_string_lossy().into_owned());
    p.path = Some(dir.to_path_buf());
    p.graph_changed(gpu);
    p.clear_history();
    p.saved_version = p.edit_version;
    Ok(Loaded { project: p, viewport: file.viewport, export: file.export })
}

/// Zstandard compression of paint pages (pure Rust, no C toolchain needed to build).
pub fn compress(data: &[u8]) -> Vec<u8> {
    ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
}

pub fn decompress(data: &[u8]) -> Result<Vec<u8>> {
    use std::io::Read;
    let mut src = data;
    let mut dec = ruzstd::decoding::StreamingDecoder::new(&mut src).map_err(|e| anyhow::anyhow!("corrupt page: {e}"))?;
    let mut out = Vec::new();
    dec.read_to_end(&mut out)?;
    Ok(out)
}

/// Adds the `.crate` extension if missing (`.painter`, from before the rename, is kept).
pub fn with_extension(path: PathBuf) -> PathBuf {
    if path.extension().is_some_and(|e| e == "crate" || e == "painter") { path } else { path.with_extension("crate") }
}
