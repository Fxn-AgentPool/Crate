//! Smart materials: saved Group / Graph layers. See docs/how-to/create-a-smart-material.md
//! and docs/reference/project-format.md "Smart material files".

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::gpu::Gpu;
use crate::graph::{MaskSource, Node, NodeId, Slot};
use crate::ops::{self, LayerRef};
use crate::paint::{PAGE, read_pages, write_pages};
use crate::project::Project;

pub const FORMAT: &str = "painter-smart-material";

#[derive(Serialize, Deserialize)]
struct MaterialFile {
    format: String,
    version: u32,
    name: String,
    /// The layer slot (content and mask refer to node ids in `nodes`).
    slot: Slot,
    nodes: Vec<Node>,
    /// "node:param" of every exposed parameter.
    exposed: Vec<String>,
    /// UDIMs of the mesh the paint was made on.
    tiles: Vec<u32>,
}

pub fn library_dir() -> PathBuf {
    crate::prefs::dir().join("library").join("smart-materials")
}

pub struct LibraryItem {
    pub name: String,
    pub path: PathBuf,
    pub thumbnail: Option<egui::ColorImage>,
}

pub fn list() -> Vec<LibraryItem> {
    let mut items = Vec::new();
    for e in std::fs::read_dir(library_dir()).into_iter().flatten().flatten() {
        let path = e.path();
        if path.extension().is_some_and(|x| x == "psmat") {
            let name = path.file_stem().unwrap_or_default().to_string_lossy().into_owned();
            let thumbnail = read_entry(&path, "thumbnail.png").ok().and_then(|bytes| {
                let img = image::load_from_memory(&bytes).ok()?.to_rgba8();
                Some(egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw()))
            });
            items.push(LibraryItem { name, path, thumbnail });
        }
    }
    items.sort_by(|a, b| a.name.cmp(&b.name));
    items
}

fn read_entry(path: &Path, name: &str) -> Result<Vec<u8>> {
    let mut z = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let mut f = z.by_name(name)?;
    let mut out = Vec::new();
    f.read_to_end(&mut out)?;
    Ok(out)
}

fn page_bytes() -> usize {
    (PAGE * PAGE * 8) as usize
}

/// Saves layer `l` to the library. `thumbnail` is a 256² RGBA8 image.
pub fn save(gpu: &Gpu, p: &Project, l: LayerRef, thumbnail: Option<(u32, Vec<u8>)>) -> Result<PathBuf> {
    let slot = ops::slot(&p.graph, l).context("no such layer")?.clone();
    let mut nodes = std::collections::BTreeSet::new();
    for root in [slot.content, slot.mask].into_iter().flatten() {
        nodes.extend(p.graph.subgraph(root));
    }
    let nodes: Vec<Node> = nodes.iter().map(|id| p.graph.node(*id).clone()).collect();
    let exposed = nodes.iter().flat_map(|n| n.exposed.iter().map(move |e| format!("n{}:{e}", n.id))).collect();
    let name = slot.smart_material.clone().unwrap_or_else(|| slot.name.clone());
    let file = MaterialFile {
        format: FORMAT.into(),
        version: 1,
        name: name.clone(),
        slot: slot.clone(),
        nodes: nodes.clone(),
        exposed,
        tiles: p.mesh.tiles.clone(),
    };
    let dir = library_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.psmat", name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_")));
    let mut z = zip::ZipWriter::new(std::fs::File::create(&path)?);
    let opts = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    z.start_file("material.json", opts)?;
    z.write_all(serde_json::to_string_pretty(&file)?.as_bytes())?;

    // Paint pages of Paint / Paint Mask nodes.
    let tiles = p.tile_count();
    let ppr = p.tile_resolution() / PAGE;
    for n in &nodes {
        let (slot_i, mask) = match (p.paint_slots.get(&n.id), p.mask_slots.get(&n.id)) {
            (Some(s), _) if n.ty == "paint" => (*s, false),
            (_, Some(s)) if n.ty == "paint_mask" => (*s, true),
            _ => continue,
        };
        let planes = crate::paint::pool_textures(p, mask);
        for (t, udim) in p.mesh.tiles.iter().enumerate() {
            let pages: Vec<(u32, u32, u32)> = (0..ppr * ppr).map(|i| (slot_i * tiles + t as u32, i % ppr, i / ppr)).collect();
            let data = read_pages(gpu, &planes, &pages);
            for (k, &(_, px, py)) in pages.iter().enumerate() {
                let slices: Vec<&[u8]> = data.iter().map(|d| &d[k * page_bytes()..(k + 1) * page_bytes()]).collect();
                let covered = slices.iter().any(|s| bytemuck::cast_slice::<u8, half::f16>(s).iter().any(|h| h.to_f32() != 0.0));
                if !covered {
                    continue;
                }
                z.start_file(format!("paint/n{}/{udim}/{px}_{py}.page", n.id), opts)?;
                z.write_all(&crate::projectfile::compress(&slices.concat()))?;
            }
        }
    }
    // Images used by the subgraph.
    for n in &nodes {
        for (_, v) in &n.params {
            let name = v.as_str();
            if let Some(src) = p.image_sources.get(name) {
                if let Ok(bytes) = std::fs::read(src) {
                    z.start_file(format!("images/{name}"), opts)?;
                    z.write_all(&bytes)?;
                }
            }
        }
    }
    if let Some((res, rgba)) = thumbnail {
        let mut png = Vec::new();
        let img = image::RgbaImage::from_raw(res, res, rgba).context("thumbnail")?;
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)?;
        z.start_file("thumbnail.png", opts)?;
        z.write_all(&png)?;
    }
    z.finish()?;
    Ok(path)
}

/// Inserts a smart material as a new layer above `above`. Returns the new layer.
pub fn insert(gpu: &Gpu, p: &mut Project, path: &Path, above: Option<LayerRef>) -> Result<LayerRef> {
    let file: MaterialFile = serde_json::from_slice(&read_entry(path, "material.json")?).context("invalid smart material")?;
    if file.format != FORMAT {
        bail!("not a smart material");
    }
    // Images first, so parameters resolve.
    let mut z = zip::ZipArchive::new(std::fs::File::open(path)?)?;
    let tmp = std::env::temp_dir().join("painter-smart-material");
    std::fs::create_dir_all(&tmp)?;
    let names: Vec<String> = z.file_names().filter_map(|n| n.ok()).map(|n| n.into_owned()).collect();
    for name in names.iter().filter(|n| n.starts_with("images/")) {
        let mut bytes = Vec::new();
        z.by_name(name)?.read_to_end(&mut bytes)?;
        let file_name = name.trim_start_matches("images/");
        let dst = tmp.join(file_name);
        std::fs::write(&dst, bytes)?;
        p.import_image(gpu, &dst)?;
    }
    // Copy nodes in with fresh ids.
    p.checkpoint("Add smart material");
    let mut map: BTreeMap<NodeId, NodeId> = BTreeMap::new();
    for n in &file.nodes {
        map.insert(n.id, p.graph.next_id);
        p.graph.next_id += 1;
    }
    let remap = |id: NodeId| *map.get(&id).unwrap_or(&id);
    for n in &file.nodes {
        let mut n = n.clone();
        n.id = remap(n.id);
        for l in n.inputs.values_mut() {
            l.node = remap(l.node);
        }
        for s in &mut n.slots {
            s.content = s.content.map(remap);
            s.mask = s.mask.map(remap);
        }
        for m in &mut n.mask_slots {
            if let MaskSource::Node(c) = &mut m.source {
                *c = remap(*c);
            }
        }
        p.graph.nodes.insert(n.id, n);
    }
    let mut slot = file.slot.clone();
    slot.content = slot.content.map(remap);
    slot.mask = slot.mask.map(remap);
    slot.smart_material = Some(file.name.clone());
    slot.name = file.name.clone();
    let content = slot.content.context("smart material has no content")?;
    let l = ops::add_layer_with(&mut p.graph, above, &file.name, content);
    if let Some(s) = ops::slot_mut(&mut p.graph, l) {
        let name = s.name.clone();
        *s = slot;
        s.name = name;
    }
    // Paint pages, for tiles this mesh also has.
    for name in names.iter().filter(|n| n.starts_with("paint/") && n.ends_with(".page")) {
        let parts: Vec<&str> = name.trim_start_matches("paint/").split('/').collect();
        let [node, udim, page] = parts.as_slice() else { continue };
        let Ok(old) = node.trim_start_matches('n').parse::<NodeId>() else { continue };
        let Ok(udim) = udim.parse::<u32>() else { continue };
        let Some(layer) = p.mesh.tiles.iter().position(|t| *t == udim) else { continue };
        let Some((px, py)) = page.trim_end_matches(".page").split_once('_') else { continue };
        let (px, py): (u32, u32) = (px.parse()?, py.parse()?);
        let new = remap(old);
        let mask = p.graph.get(new).is_some_and(|n| n.ty == "paint_mask");
        let slot_i = p.paint_slot(gpu, new);
        let mut bytes = Vec::new();
        z.by_name(name)?.read_to_end(&mut bytes)?;
        let raw = crate::projectfile::decompress(&bytes)?;
        let planes_n = if mask { 1 } else { crate::scene::PAINT_PLANES };
        if raw.len() != planes_n * page_bytes() {
            continue;
        }
        let data: Vec<Vec<u8>> = raw.chunks(page_bytes()).map(|c| c.to_vec()).collect();
        let planes = crate::paint::pool_textures(p, mask);
        write_pages(gpu, &planes, &[(slot_i * p.tile_count() + layer as u32, px, py)], &data);
    }
    p.paint_epoch += 1;
    p.graph_changed(gpu);
    Ok(l)
}
