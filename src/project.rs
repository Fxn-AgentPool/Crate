//! The open document: mesh, mesh maps, layers and paint. Shared by the GUI and headless mode.

use std::time::Duration;

use crate::channels::ChannelSettings;
use crate::compile;
use crate::gpu::Gpu;
use crate::graph::Graph;
use crate::mesh::{ImportSettings, Mesh};
use crate::meshmaps::{BakeJob, MeshMap, MeshMaps};
use crate::ops::Selection;
use crate::scene::SceneResources;
use crate::viewport::GpuMesh;

/// One undoable step.
pub struct UndoStep {
    pub label: String,
    pub graph: Graph,
    pub selection: Option<Selection>,
    /// Page contents to restore for paint steps.
    pub paint: Option<crate::paint::PaintPages>,
    /// Bake exclusion to restore, for steps that changed it.
    pub bake: Option<crate::bakemask::BakeMask>,
}

/// A paint node's curve slot.
pub struct CurveSlot {
    pub slot: u32,
    pub mask: bool,
    /// Signature of what the slot was last drawn from.
    signature: u64,
    /// Each curve's path on the surface, as last drawn.
    pub paths: Vec<Vec<crate::curve::PathPoint>>,
}

pub const UNDO_LIMIT: usize = 64;
pub const UNDO_BYTES: usize = 2 << 30;

pub struct Project {
    /// Project name (the .crate directory's stem) once saved.
    pub name: Option<String>,
    /// The .crate directory, once saved.
    pub path: Option<std::path::PathBuf>,
    /// The mesh file the user imported (the project keeps its own copy in mesh/). Its name is
    /// what {mesh} means in export file names.
    pub original_mesh: std::path::PathBuf,
    /// Unknown project.json keys, preserved on save.
    pub file_extra: serde_json::Map<String, serde_json::Value>,
    /// Hash of every paint page as last written to disk: (node, udim, px, py).
    pub saved_pages: std::collections::HashMap<(crate::graph::NodeId, u32, u32, u32), u64>,
    /// Mesh maps whose files on disk are current.
    pub saved_maps: std::collections::HashSet<MeshMap>,
    pub caches: crate::cache::Caches,
    /// Incremented whenever paint changes; part of cache signatures.
    pub paint_epoch: u64,
    /// Next free slot in the material paint pools (paint nodes and material caches).
    next_paint_slot: u32,
    /// Error from the last cache update, if any.
    pub cache_error: Option<String>,
    /// Incremented by every edit; compared with `saved_version` for unsaved changes.
    pub edit_version: u64,
    pub saved_version: u64,
    pub mesh: Mesh,
    pub gpu_mesh: GpuMesh,
    pub import_settings: ImportSettings,
    pub channels: ChannelSettings,
    pub maps: MeshMaps,
    pub scene: SceneResources,
    pub bake_job: Option<BakeJob>,
    pub graph: Graph,
    pub selection: Option<Selection>,
    /// Additional layers selected with Ctrl+click (for grouping).
    pub multi: Vec<crate::ops::LayerRef>,
    /// Library + generated material code, inserted into every material shader.
    pub material_code: String,
    pub material_errors: Vec<String>,
    /// Incremented whenever `material_code` changes.
    pub material_version: u64,
    /// Node Editor preview (not saved, not part of undo).
    pub preview: Option<crate::graph::Link>,
    /// Material shader with the preview, when there is one.
    pub preview_code: Option<String>,
    /// Pool slot of every Paint / Paint Mask node that has been painted. Kept outside the
    /// graph so undoing a graph edit never detaches paint data.
    pub paint_slots: std::collections::BTreeMap<crate::graph::NodeId, u32>,
    /// Same for Paint Mask nodes (mask pool).
    pub mask_slots: std::collections::BTreeMap<crate::graph::NodeId, u32>,
    /// Pool slots the curves of paint nodes are drawn into (docs/reference/curves.md), with what
    /// they were drawn from. Not saved: drawn again from the graph.
    pub curve_slots: std::collections::BTreeMap<crate::graph::NodeId, CurveSlot>,
    /// The curve being edited with the Curve tool (UI state; not saved, not part of undo).
    pub curve_sel: Option<crate::curve::CurveSelection>,
    /// Triangles excluded from mesh map baking, and the bake sources
    /// (docs/reference/bake-exclusion.md).
    pub bake_mask: crate::bakemask::BakeMask,
    /// The Bake Exclude tool's selection, as triangles (UI state; not saved, not part of undo).
    pub bake_sel: std::collections::BTreeSet<u32>,
    /// Incremented when `bake_mask` or `bake_sel` changes, for the viewport display.
    pub bake_version: u64,
    curve_gpu: crate::curve::CurveGpu,
    /// Geometry queries on the mesh, built on first use.
    geo: std::cell::OnceCell<crate::geo::Geo>,
    /// Geo Fill outlines per Fill by kind, as GPU edge lists, built on first use.
    borders: [std::cell::OnceCell<(wgpu::Buffer, u32)>; 5],
    /// Incremented when the mesh is replaced.
    mesh_version: u64,
    pub stencils: crate::stencil::StencilSet,
    /// Original pixel size of imported images, by name.
    pub image_sizes: std::collections::BTreeMap<String, (u32, u32)>,
    /// Source files of imported images, by name.
    pub image_sources: std::collections::BTreeMap<String, std::path::PathBuf>,
    undo: Vec<UndoStep>,
    redo: Vec<UndoStep>,
    pub undo_limit: usize,
    pub undo_bytes: usize,
    /// What the last reimport did with the paint.
    pub last_transfer: Option<crate::transfer::Report>,
    /// Paint was transferred to new UVs: the next save deletes pages of tiles no longer used.
    pub prune_tiles: bool,
}

impl Project {
    pub fn new(gpu: &Gpu, mesh: Mesh, import_settings: ImportSettings) -> Self {
        let gpu_mesh = GpuMesh::new(gpu, &mesh);
        let mut maps = MeshMaps::new(gpu, &mesh, import_settings.tile_resolution);
        // Painting needs Position and World Normal, so they are baked on import.
        maps.bake_blocking(
            gpu,
            &gpu_mesh,
            import_settings.tile_resolution,
            &[MeshMap::Position, MeshMap::WorldNormal],
        );
        let scene = SceneResources::new(gpu);
        let mut p = Self {
            name: None,
            path: None,
            original_mesh: mesh.source.clone(),
            file_extra: Default::default(),
            saved_pages: Default::default(),
            saved_maps: Default::default(),
            edit_version: 0,
            saved_version: 0,
            caches: Default::default(),
            paint_epoch: 0,
            next_paint_slot: 0,
            cache_error: None,
            mesh,
            gpu_mesh,
            import_settings,
            channels: ChannelSettings::default(),
            maps,
            scene,
            bake_job: None,
            graph: Graph::new(),
            selection: None,
            multi: Vec::new(),
            material_code: String::new(),
            preview: None,
            preview_code: None,
            material_errors: Vec::new(),
            material_version: 0,
            paint_slots: Default::default(),
            mask_slots: Default::default(),
            curve_slots: Default::default(),
            curve_sel: None,
            bake_mask: Default::default(),
            bake_sel: Default::default(),
            bake_version: 0,
            curve_gpu: crate::curve::CurveGpu::new(gpu),
            geo: Default::default(),
            borders: Default::default(),
            mesh_version: 0,
            image_sources: Default::default(),
            stencils: Default::default(),
            image_sizes: Default::default(),
            undo: Vec::new(),
            redo: Vec::new(),
            undo_limit: UNDO_LIMIT,
            undo_bytes: UNDO_BYTES,
            last_transfer: None,
            prune_tiles: false,
        };
        p.graph_changed(gpu);
        p
    }

    pub fn tile_resolution(&self) -> u32 {
        self.import_settings.tile_resolution
    }

    pub fn tile_count(&self) -> u32 {
        self.mesh.tiles.len() as u32
    }

    pub fn start_bake(&mut self, gpu: &Gpu, maps: &[MeshMap]) {
        let res = self.tile_resolution();
        let job = self.maps.start_bake(gpu, res, maps, &self.import_settings);
        for m in job.maps() {
            self.saved_maps.remove(m);
        }
        self.edit_version += 1;
        self.bake_job = Some(job);
    }

    /// Advances background work. Returns true if anything visible changed.
    pub fn tick(&mut self, gpu: &Gpu, budget: Duration) -> bool {
        let mut changed = false;
        if let Some(job) = &mut self.bake_job {
            if self.maps.advance(gpu, &self.gpu_mesh, job, budget) {
                self.bake_job = None;
            }
            changed = true;
        }
        if changed {
            self.update_scene(gpu);
        }
        if !self.caches.entries.is_empty() || self.graph.nodes.values().any(|n| n.cached || n.def().non_local) {
            self.update_caches(gpu);
        }
        changed
    }

    pub fn update_preview(&mut self, gpu: &Gpu) {
        self.maps.update_preview(gpu, &self.import_settings);
    }

    pub fn finish_background_work(&mut self, gpu: &Gpu) {
        while self.bake_job.is_some() {
            self.tick(gpu, Duration::from_secs(3600));
        }
        self.update_caches(gpu);
    }

    pub fn update_scene(&mut self, gpu: &Gpu) {
        let size = self.mesh.bbox_max - self.mesh.bbox_min;
        self.scene.update(gpu, &self.maps, self.mesh.bbox_min, size);
    }

    /// Recompiles the material after any change to the graph or channel settings.
    pub fn graph_changed(&mut self, gpu: &Gpu) {
        self.validate_curve_selection();
        self.update_curves(gpu);
        self.compile_material(gpu);
        self.edit_version += 1;
        self.update_scene(gpu);
    }

    fn compile_material(&mut self, gpu: &Gpu) {
        let scene = &self.scene;
        let (slots, masks) = (&self.paint_slots, &self.mask_slots);
        let caches = &self.caches;
        let image_layer = |n: &str| scene.image_layer(n);
        let paint_slot = |id| slots.get(&id).or(masks.get(&id)).copied();
        let curve_slot = |id| self.curve_slot(id);
        let cache = |id| caches.lookup(id);
        let env = compile::Env { image_layer: &image_layer, paint_slot: &paint_slot, curve_slot: &curve_slot, cache: &cache, preview: None };
        let compiled = compile::compile(&self.graph, &self.channels, &env);
        let code = format!("{}\n{}", compile::LIB_WGSL, compiled.code);
        // The preview only changes what the viewport shows; exports always use `material_code`.
        let preview = self.preview.as_ref().filter(|l| self.graph.get(l.node).is_some());
        let preview_code = preview.map(|l| {
            let env = compile::Env { image_layer: &image_layer, paint_slot: &paint_slot, curve_slot: &curve_slot, cache: &cache, preview: Some(l) };
            format!("{}\n{}", compile::LIB_WGSL, compile::compile(&self.graph, &self.channels, &env).code)
        });
        self.scene.write_params(gpu, &compiled.params);
        if code != self.material_code || preview_code != self.preview_code {
            self.material_code = code;
            self.preview_code = preview_code;
            self.material_version += 1;
        }
        self.material_errors = compiled.errors;
    }

    /// Shows a node output in the viewport instead of the material (None: the material).
    pub fn set_preview(&mut self, gpu: &Gpu, link: Option<crate::graph::Link>) {
        if self.preview != link {
            self.preview = link;
            self.compile_material(gpu);
        }
    }

    /// The material shader the viewport shows.
    pub fn viewport_code(&self) -> &str {
        self.preview_code.as_deref().unwrap_or(&self.material_code)
    }

    pub fn has_unsaved_changes(&self) -> bool {
        self.edit_version != self.saved_version
    }

    pub fn clear_history(&mut self) {
        self.undo.clear();
        self.redo.clear();
    }

    /// Records the current graph so the next change can be undone.
    pub fn checkpoint(&mut self, label: &str) {
        self.push_undo(label, None);
    }

    /// Applies an undoable edit to the graph.
    pub fn edit<R>(&mut self, gpu: &Gpu, label: &str, f: impl FnOnce(&mut Graph) -> R) -> R {
        self.checkpoint(label);
        let r = f(&mut self.graph);
        // Layer indices may have shifted; the Ctrl+click selection is no longer meaningful.
        self.multi.clear();
        self.graph_changed(gpu);
        r
    }

    /// Records a paint step: `pages` hold the contents before the stroke.
    pub fn push_paint_undo(&mut self, label: &str, pages: crate::paint::PaintPages) {
        self.edit_version += 1;
        self.push_undo(label, Some(pages));
    }

    /// Pushes an undo step, then drops the oldest steps beyond `undo_limit` steps or
    /// `undo_bytes` of paint pages.
    fn push_undo(&mut self, label: &str, paint: Option<crate::paint::PaintPages>) {
        self.push_step(UndoStep { label: label.into(), graph: self.graph.clone(), selection: self.selection, paint, bake: None });
    }

    fn push_step(&mut self, step: UndoStep) {
        self.undo.push(step);
        let size = |s: &UndoStep| s.paint.as_ref().map_or(0, |p| p.data.iter().map(Vec::len).sum::<usize>());
        let mut bytes: usize = self.undo.iter().map(size).sum();
        while self.undo.len() > self.undo_limit || (bytes > self.undo_bytes && self.undo.len() > 1) {
            bytes -= size(&self.undo.remove(0));
        }
        self.redo.clear();
    }

    /// Swaps a step with the current state, returning the step that reverts it.
    fn apply_step(&mut self, gpu: &Gpu, step: UndoStep) -> UndoStep {
        if step.paint.is_some() {
            self.paint_epoch += 1;
        }
        let paint = step.paint.map(|saved| {
            let planes = crate::paint::pool_textures(self, saved.mask);
            let current = crate::paint::read_pages(gpu, &planes, &saved.pages);
            crate::paint::write_pages(gpu, &planes, &saved.pages, &saved.data);
            crate::paint::PaintPages { mask: saved.mask, pages: saved.pages, data: current }
        });
        let bake = step.bake.map(|saved| std::mem::replace(&mut self.bake_mask, saved));
        let restore_bake = bake.is_some();
        let reverse = UndoStep {
            label: step.label,
            graph: std::mem::replace(&mut self.graph, step.graph),
            selection: self.selection,
            paint,
            bake,
        };
        self.selection = step.selection;
        self.validate_selection();
        if restore_bake {
            // Rebaking Position and World Normal can't run inside another bake.
            if self.bake_job.is_some() {
                self.finish_background_work(gpu);
            }
            self.apply_bake_mask(gpu);
        }
        self.graph_changed(gpu);
        reverse
    }

    pub fn undo(&mut self, gpu: &Gpu) -> Option<String> {
        let step = self.undo.pop()?;
        let label = step.label.clone();
        let reverse = self.apply_step(gpu, step);
        self.redo.push(reverse);
        Some(label)
    }

    pub fn redo(&mut self, gpu: &Gpu) -> Option<String> {
        let step = self.redo.pop()?;
        let label = step.label.clone();
        let reverse = self.apply_step(gpu, step);
        self.undo.push(reverse);
        Some(label)
    }

    /// Pool slot of a Paint / Paint Mask node, allocated (and the pool grown) on first use.
    pub fn paint_slot(&mut self, gpu: &Gpu, node: crate::graph::NodeId) -> u32 {
        let mask = self.graph.get(node).is_some_and(|n| crate::graph::is_paint_mask(&n.ty));
        let map = if mask { &self.mask_slots } else { &self.paint_slots };
        if let Some(&s) = map.get(&node) {
            return s;
        }
        let (res, tiles) = (self.tile_resolution(), self.tile_count());
        let slot;
        if mask {
            slot = self.mask_slot_count();
            self.mask_slots.insert(node, slot);
            self.scene.ensure_mask_slots(gpu, slot + 1, res, tiles);
        } else {
            slot = self.alloc_material_slot(gpu);
            self.paint_slots.insert(node, slot);
        }
        self.graph_changed(gpu);
        slot
    }

    /// Loads an image file into the image pool. Returns the image's name.
    pub fn import_image(&mut self, gpu: &Gpu, path: &std::path::Path) -> anyhow::Result<String> {
        use anyhow::Context;
        let img = image::open(path).with_context(|| format!("Could not read {}", path.display()))?;
        let res = crate::scene::IMAGE_RES;
        let size = (img.width(), img.height());
        let rgba = img.resize_exact(res, res, image::imageops::FilterType::Triangle).to_rgba8();
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "image".into());
        self.image_sizes.insert(name.clone(), size);
        self.scene.set_image(gpu, &name, &rgba);
        self.image_sources.insert(name.clone(), path.to_path_buf());
        self.graph_changed(gpu);
        Ok(name)
    }

    /// Loads an image as a new active stencil.
    pub fn add_stencil(&mut self, gpu: &Gpu, path: &std::path::Path) -> anyhow::Result<()> {
        let name = self.import_image(gpu, path)?;
        let (w, h) = self.image_sizes.get(&name).copied().unwrap_or((1, 1));
        self.stencils
            .add(crate::stencil::Stencil::new(name, w as f32 / h.max(1) as f32))
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Replaces the mesh, keeping layers and paint. If the UVs changed, paint is transferred to
    /// the new UVs by matching the surface, unless `transfer` is off; otherwise it stays where it
    /// is in UV space (paint of tiles that no longer exist is dropped from memory). Mesh maps are
    /// marked out of date and the undo history is cleared. See
    /// docs/reference/mesh-and-udims.md#reimport.
    pub fn reimport(&mut self, gpu: &Gpu, path: &std::path::Path, transfer: bool) -> anyhow::Result<crate::transfer::Report> {
        let mesh = crate::mesh::import(path, &self.import_settings)?;
        if mesh.has_errors() {
            anyhow::bail!("the mesh has import errors; see the Import Log");
        }
        let old_tiles = self.mesh.tiles.clone();
        let new_tiles = mesh.tiles.clone();
        let res = self.tile_resolution();
        // The new surface, which transfer reads and painting needs anyway.
        let gpu_mesh = crate::viewport::GpuMesh::new(gpu, &mesh);
        let mut maps = MeshMaps::new(gpu, &mesh, res);
        maps.settings = self.maps.settings;
        maps.high_poly = self.maps.high_poly.clone();
        // Bake exclusion is kept for the same triangles and UVs (docs/reference/bake-exclusion.md#saving-and-reimport).
        let keep_mask = crate::transfer::same_uvs(&self.mesh, &mesh);
        if keep_mask {
            maps.set_excluded(gpu, &mesh, &self.bake_mask.excluded);
        }
        maps.bake_blocking(gpu, &gpu_mesh, res, &[MeshMap::Position, MeshMap::WorldNormal]);
        let transfer = transfer && !crate::transfer::same_uvs(&self.mesh, &mesh);
        let mut report = crate::transfer::Report::default();
        if transfer {
            let nt = new_tiles.len() as u32;
            let fresh = |slots: u32| crate::scene::Pool::new(gpu, "paint plane", crate::scene::PAINT_FORMAT, res, (slots * nt).max(1).next_power_of_two());
            let slots = if self.scene.paint[0].res == res { self.next_paint_slot } else { 0 };
            let mslots = if self.scene.mask.res == res { self.mask_slot_count() } else { 0 };
            let paint: Vec<crate::scene::Pool> = (0..self.scene.paint.len()).map(|_| fresh(slots)).collect();
            let mask_pool = fresh(mslots);
            let (pos, _) = maps.texture(MeshMap::Position);
            let (nrm, _) = maps.texture(MeshMap::WorldNormal);
            let planes = [
                crate::transfer::Planes { src: self.scene.paint.iter().collect(), dst: paint.iter().collect(), slots },
                crate::transfer::Planes { src: vec![&self.scene.mask], dst: vec![&mask_pool], slots: mslots },
            ];
            report = crate::transfer::run(gpu, &self.mesh, &mesh, res, pos, nrm, &planes);
            // 2D paint stays in UV space: copied tile by tile instead (docs/reference/2d-paint.md#reimport).
            let two_d: Vec<(u32, bool)> = self
                .paint_slots
                .iter()
                .map(|(n, s)| (*n, *s, false))
                .chain(self.mask_slots.iter().map(|(n, s)| (*n, *s, true)))
                .filter(|(n, _, _)| self.graph.get(*n).is_some_and(|n| crate::graph::is_2d(&n.ty)))
                .map(|(_, s, m)| (s, m))
                .collect();
            let (ot, nt) = (old_tiles.len() as u32, new_tiles.len() as u32);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            for &(slot, mask) in &two_d {
                if slot >= if mask { mslots } else { slots } {
                    continue;
                }
                let pairs: Vec<(&crate::scene::Pool, &crate::scene::Pool)> =
                    if mask { vec![(&self.scene.mask, &mask_pool)] } else { self.scene.paint.iter().zip(paint.iter()).collect() };
                for (src, dst) in pairs {
                    for (ni, udim) in new_tiles.iter().enumerate() {
                        // A tile the old mesh didn't use starts empty, not with transferred paint.
                        let Some(oi) = old_tiles.iter().position(|u| u == udim) else {
                            enc.clear_texture(
                                &dst.texture,
                                &wgpu::ImageSubresourceRange { base_array_layer: slot * nt + ni as u32, array_layer_count: Some(1), ..Default::default() },
                            );
                            continue;
                        };
                        enc.copy_texture_to_texture(
                            wgpu::TexelCopyTextureInfo { texture: &src.texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: slot * ot + oi as u32 }, aspect: wgpu::TextureAspect::All },
                            wgpu::TexelCopyTextureInfo { texture: &dst.texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: slot * nt + ni as u32 }, aspect: wgpu::TextureAspect::All },
                            wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
                        );
                    }
                }
            }
            gpu.queue.submit([enc.finish()]);
            report.kept_2d = two_d.len();
            if slots > 0 {
                self.scene.paint = paint;
            }
            if mslots > 0 {
                self.scene.mask = mask_pool;
            }
            self.scene.mark_dirty();
            self.caches = Default::default();
            self.prune_tiles = true;
        } else if old_tiles != new_tiles {
            // Remap pool layers when the tile set changed.
            let (ot, nt) = (old_tiles.len() as u32, new_tiles.len() as u32);
            let remap = |pools: &mut Vec<&mut crate::scene::Pool>, slots: u32| {
                for pool in pools.iter_mut() {
                    let fresh = crate::scene::Pool::new(gpu, "paint plane", crate::scene::PAINT_FORMAT, res, (slots * nt).max(1).next_power_of_two());
                    let mut enc = gpu.device.create_command_encoder(&Default::default());
                    for slot in 0..slots {
                        for (ni, udim) in new_tiles.iter().enumerate() {
                            if let Some(oi) = old_tiles.iter().position(|u| u == udim) {
                                enc.copy_texture_to_texture(
                                    wgpu::TexelCopyTextureInfo { texture: &pool.texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: slot * ot + oi as u32 }, aspect: wgpu::TextureAspect::All },
                                    wgpu::TexelCopyTextureInfo { texture: &fresh.texture, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: slot * nt + ni as u32 }, aspect: wgpu::TextureAspect::All },
                                    wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
                                );
                            }
                        }
                    }
                    gpu.queue.submit([enc.finish()]);
                    **pool = fresh;
                }
            };
            let slots = self.next_paint_slot;
            if slots > 0 && self.scene.paint[0].res == res {
                let mut planes: Vec<&mut crate::scene::Pool> = self.scene.paint.iter_mut().collect();
                remap(&mut planes, slots);
            }
            let mslots = self.mask_slot_count();
            if mslots > 0 && self.scene.mask.res == res {
                let mut m = vec![&mut self.scene.mask];
                remap(&mut m, mslots);
            }
            self.scene.mark_dirty();
            self.caches = Default::default();
        }
        let statuses: Vec<_> = crate::meshmaps::MeshMap::ALL.iter().map(|m| self.maps.status(*m)).collect();
        self.original_mesh = path.to_path_buf();
        self.gpu_mesh = gpu_mesh;
        self.maps = maps;
        self.mesh = mesh;
        // Other maps keep their old data in name only: they must be rebaked.
        for (m, st) in crate::meshmaps::MeshMap::ALL.iter().zip(statuses) {
            if !matches!(m, MeshMap::Position | MeshMap::WorldNormal) && st != crate::meshmaps::MapStatus::Missing {
                self.maps.mark_out_of_date(*m);
            }
        }
        self.saved_maps.clear();
        self.paint_epoch += 1;
        // Curves are drawn again on the new surface (docs/reference/curves.md#saving-and-copying).
        self.geo = Default::default();
        self.borders = Default::default();
        self.mesh_version += 1;
        self.bake_sel.clear();
        if !keep_mask && self.bake_mask != Default::default() {
            self.bake_mask = Default::default();
            report.bake_cleared = true;
        }
        self.bake_version += 1;
        // Undo steps hold texels of the old pools.
        self.clear_history();
        self.edit_version += 1;
        self.last_transfer = Some(report);
        self.graph_changed(gpu);
        Ok(report)
    }

    /// Slots in use in the mask pool: Paint Mask nodes and their curves.
    fn mask_slot_count(&self) -> u32 {
        (self.mask_slots.len() + self.curve_slots.values().filter(|c| c.mask).count()) as u32
    }

    /// Incremented when the mesh is replaced.
    pub fn mesh_version(&self) -> u64 {
        self.mesh_version
    }

    /// Geometry queries on the mesh, built on first use.
    pub fn geo(&self) -> &crate::geo::Geo {
        self.geo.get_or_init(|| crate::geo::Geo::new(&self.mesh))
    }

    /// Changes the bake exclusion, as one undo step (docs/reference/bake-exclusion.md).
    pub fn set_bake_mask(&mut self, gpu: &Gpu, label: &str, mask: crate::bakemask::BakeMask) -> Result<(), String> {
        if self.bake_job.is_some() {
            return Err("Wait for the mesh map bake to finish.".into());
        }
        if mask == self.bake_mask {
            return Ok(());
        }
        let previous = std::mem::replace(&mut self.bake_mask, mask);
        self.push_step(UndoStep { label: label.into(), graph: self.graph.clone(), selection: self.selection, paint: None, bake: Some(previous) });
        self.apply_bake_mask(gpu);
        self.graph_changed(gpu);
        Ok(())
    }

    /// Bakes Position and World Normal again without the excluded triangles; the other baked
    /// maps become out of date.
    pub fn apply_bake_mask(&mut self, gpu: &Gpu) {
        self.maps.set_excluded(gpu, &self.mesh, &self.bake_mask.excluded);
        self.maps.bake_blocking(gpu, &self.gpu_mesh, self.tile_resolution(), &[MeshMap::Position, MeshMap::WorldNormal]);
        // The other maps were baked with the old exclusion.
        for m in MeshMap::ALL {
            if !matches!(m, MeshMap::Position | MeshMap::WorldNormal) && self.maps.status(m) == crate::meshmaps::MapStatus::UpToDate {
                self.maps.mark_out_of_date(m);
            }
        }
        self.saved_maps.remove(&MeshMap::Position);
        self.saved_maps.remove(&MeshMap::WorldNormal);
        self.bake_version += 1;
        self.paint_epoch += 1;
        self.update_scene(gpu);
    }

    /// Geo Fill's outlines for `by`: vertex index pairs and their count
    /// (docs/reference/brushes-tools-stencils.md#outlines-and-highlight).
    pub fn borders(&self, gpu: &Gpu, by: crate::geo::FillBy) -> (&wgpu::Buffer, u32) {
        use wgpu::util::DeviceExt;
        let i = crate::geo::FillBy::ALL.iter().position(|b| *b == by).unwrap();
        let (buffer, count) = self.borders[i].get_or_init(|| {
            let edges = self.geo().borders(&self.mesh, by);
            let buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("geo fill outlines"),
                // A buffer can't be empty.
                contents: bytemuck::cast_slice(if edges.is_empty() { &[0u32, 0] } else { &edges[..] }),
                usage: wgpu::BufferUsages::INDEX,
            });
            (buffer, edges.len() as u32)
        });
        (buffer, *count)
    }

    /// The slot holding a node's curves, if it has any.
    pub fn curve_slot(&self, node: crate::graph::NodeId) -> Option<u32> {
        let has = self.graph.get(node).is_some_and(|n| !n.curves.is_empty());
        self.curve_slots.get(&node).filter(|_| has).map(|c| c.slot)
    }

    /// Draws the curves of every paint node whose curves, mesh or textures changed since they
    /// were last drawn.
    fn update_curves(&mut self, gpu: &Gpu) {
        use std::hash::{Hash, Hasher};
        let nodes: Vec<(crate::graph::NodeId, bool, u64)> = self
            .graph
            .nodes
            .values()
            .filter(|n| !n.curves.is_empty() && (crate::graph::is_paint(&n.ty) || crate::graph::is_paint_mask(&n.ty)))
            .map(|n| {
                let mut h = std::collections::hash_map::DefaultHasher::new();
                serde_json::to_string(&n.curves).unwrap_or_default().hash(&mut h);
                for c in &n.curves {
                    self.scene.image_layer(&c.alpha).hash(&mut h);
                }
                (self.mesh_version, self.maps.version, self.tile_resolution(), self.tile_count()).hash(&mut h);
                crate::channels::Channel::ALL.map(|c| self.channels.is_enabled(c)).hash(&mut h);
                (n.id, crate::graph::is_paint_mask(&n.ty), h.finish())
            })
            .collect();
        let mut drawn = false;
        for (node, mask, signature) in nodes {
            if self.curve_slots.get(&node).is_some_and(|c| c.signature == signature && c.mask == mask) {
                continue;
            }
            let slot = match self.curve_slots.get(&node) {
                Some(c) if c.mask == mask => c.slot,
                _ => {
                    let (res, tiles) = (self.tile_resolution(), self.tile_count());
                    let slot = if mask {
                        let s = self.mask_slot_count();
                        self.scene.ensure_mask_slots(gpu, s + 1, res, tiles);
                        s
                    } else {
                        self.alloc_material_slot(gpu)
                    };
                    self.curve_slots.insert(node, CurveSlot { slot, mask, signature: 0, paths: Vec::new() });
                    self.update_scene(gpu);
                    slot
                }
            };
            let curves = self.graph.node(node).curves.clone();
            let paths = self.curve_gpu.render(gpu, self, &curves, slot, mask);
            let entry = self.curve_slots.get_mut(&node).unwrap();
            entry.signature = signature;
            entry.paths = paths;
            drawn = true;
        }
        if drawn {
            self.paint_epoch += 1;
        }
    }

    /// Drops the curve selection when its node or curve no longer exists.
    pub fn validate_curve_selection(&mut self) {
        if let Some(s) = self.curve_sel {
            let target = self.selection.and_then(|sel| crate::ops::paint_target(&self.graph, sel));
            let ok = target == Some(s.node) && s.curve.is_none_or(|c| self.graph.get(s.node).is_some_and(|n| c < n.curves.len()));
            if !ok {
                self.curve_sel = None;
            }
        }
    }

    /// Reserves a slot in the material paint pools (paint nodes and material caches).
    pub fn alloc_material_slot(&mut self, gpu: &Gpu) -> u32 {
        let slot = self.next_paint_slot;
        self.next_paint_slot += 1;
        let (res, tiles) = (self.tile_resolution(), self.tile_count());
        self.scene.ensure_paint_slots(gpu, slot + 1, res, tiles);
        self.update_scene(gpu);
        slot
    }

    /// Recomputes stale caches; recompiles the material when cache availability changed.
    pub fn update_caches(&mut self, gpu: &Gpu) -> bool {
        match crate::cache::update(gpu, self) {
            Ok(changed) => {
                self.cache_error = None;
                self.update_scene(gpu);
                if changed {
                    self.graph_changed(gpu);
                }
                true
            }
            Err(e) => {
                self.cache_error = Some(format!("{e:#}"));
                false
            }
        }
    }

    /// Copies paint data from duplicated Paint / Paint Mask nodes.
    pub fn copy_paint(&mut self, gpu: &Gpu, pairs: &[(crate::graph::NodeId, crate::graph::NodeId)]) {
        let tiles = self.tile_count();
        let res = self.tile_resolution();
        for &(old, new) in pairs {
            let (from, mask) = match (self.paint_slots.get(&old), self.mask_slots.get(&old)) {
                (Some(&s), _) => (s, false),
                (_, Some(&s)) => (s, true),
                _ => continue,
            };
            let to = self.paint_slot(gpu, new);
            let mut enc = gpu.device.create_command_encoder(&Default::default());
            for t in crate::paint::pool_textures(self, mask) {
                enc.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: from * tiles }, aspect: wgpu::TextureAspect::All },
                    wgpu::TexelCopyTextureInfo { texture: t, mip_level: 0, origin: wgpu::Origin3d { x: 0, y: 0, z: to * tiles }, aspect: wgpu::TextureAspect::All },
                    wgpu::Extent3d { width: res, height: res, depth_or_array_layers: tiles },
                );
            }
            gpu.queue.submit([enc.finish()]);
        }
    }

    pub fn validate_selection(&mut self) {
        self.multi.clear();
        if let Some(sel) = self.selection {
            let l = sel.layer();
            let ok = match sel {
                Selection::Layer(l) => crate::ops::slot(&self.graph, l).is_some(),
                Selection::MaskLayer(l, i) => crate::ops::mask_slot(&self.graph, l, i).is_some(),
            };
            if !ok {
                self.selection = crate::ops::slot(&self.graph, l).map(|_| Selection::Layer(l));
            }
        }
    }
}
