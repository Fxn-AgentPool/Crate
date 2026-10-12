//! Mesh map baking. See docs/reference/mesh-maps.md.
//!
//! Position, World Normal and ID come from a UV-space raster pass; Ambient Occlusion,
//! Curvature and Thickness are ray traced on the GPU against a BVH. Padding is computed
//! once per tile as a "nearest covered texel" map that every map is gathered from.
//!
//! With a high-poly mesh, a projection pass (shaders/highbake.wgsl) finds the high-poly point
//! for every texel, writing Normal, Height and ID, and AO, Curvature and Thickness are then
//! traced from those points against the high poly. Tiles are baked one after the other, so the
//! projected points need only one tile of memory.

use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::bvh::Bvh;
use crate::cage::LowMesh;
use crate::gpu::{self, Gpu};
use crate::highpoly::{HighCpu, HighGpu};
use crate::mesh::{ImportSettings, Mesh};
use crate::viewport::{GpuMesh, vertex_layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MeshMap {
    Position,
    WorldNormal,
    AmbientOcclusion,
    Curvature,
    Thickness,
    Id,
    /// Tangent-space normal from the high poly.
    Normal,
    /// Height from the high poly.
    Height,
}

impl MeshMap {
    pub const ALL: [MeshMap; 8] = [
        MeshMap::Position,
        MeshMap::WorldNormal,
        MeshMap::AmbientOcclusion,
        MeshMap::Curvature,
        MeshMap::Thickness,
        MeshMap::Id,
        MeshMap::Normal,
        MeshMap::Height,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MeshMap::Position => "Position",
            MeshMap::WorldNormal => "World Normal",
            MeshMap::AmbientOcclusion => "Ambient Occlusion",
            MeshMap::Curvature => "Curvature",
            MeshMap::Thickness => "Thickness",
            MeshMap::Id => "ID",
            MeshMap::Normal => "Normal",
            MeshMap::Height => "Height",
        }
    }

    pub fn file_stem(self) -> &'static str {
        match self {
            MeshMap::Position => "position",
            MeshMap::WorldNormal => "world_normal",
            MeshMap::AmbientOcclusion => "ambient_occlusion",
            MeshMap::Curvature => "curvature",
            MeshMap::Thickness => "thickness",
            MeshMap::Id => "id",
            MeshMap::Normal => "normal",
            MeshMap::Height => "height",
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    /// Value of the Mesh Map node's `map` parameter (see `graph::MAP_NAMES`).
    pub fn param_name(self) -> &'static str {
        match self {
            MeshMap::AmbientOcclusion => "AO",
            m => m.name(),
        }
    }

    fn is_ray_traced(self) -> bool {
        matches!(self, MeshMap::AmbientOcclusion | MeshMap::Curvature | MeshMap::Thickness)
    }

    /// Baked from the high poly when one is set.
    pub fn baked_from_high(self) -> bool {
        !matches!(self, MeshMap::Position | MeshMap::WorldNormal)
    }

    /// Can only be baked from a high poly.
    pub fn needs_high(self) -> bool {
        matches!(self, MeshMap::Normal | MeshMap::Height)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct BakeSettings {
    /// 0 = tile resolution.
    pub resolution: u32,
    pub samples: u32,
    /// Relative to the bounding box diagonal.
    pub max_distance: f32,
    /// Degrees.
    pub spread_angle: f32,
    pub ignore_backfaces: bool,
    /// Relative to the bounding box diagonal.
    pub curvature_radius: f32,
    /// Texels.
    pub padding: u32,
    /// High poly: measure Frontal and Rear distance per vertex instead (see cage.rs).
    pub auto_cage: bool,
    /// High poly: ray start outside the low-poly surface, relative to the bounding box diagonal.
    pub frontal: f32,
    /// High poly: how far inside the surface rays still search, relative to the diagonal.
    pub rear: f32,
    pub match_by_name: bool,
    /// Combine the baked Normal map with the material's normal (viewport and export).
    pub apply_normal: bool,
    /// `highpoly::stamp` of the high-poly file the maps were baked from (0 = none).
    pub high_poly: u64,
}

impl Default for BakeSettings {
    fn default() -> Self {
        Self {
            resolution: 0,
            samples: 64,
            max_distance: 0.1,
            spread_angle: 180.0,
            ignore_backfaces: true,
            curvature_radius: 0.005,
            padding: 8,
            auto_cage: false,
            frontal: 0.01,
            rear: 0.01,
            match_by_name: true,
            apply_normal: true,
            high_poly: 0,
        }
    }
}

impl BakeSettings {
    /// Whether a map baked with `old` would differ when baked with `self`.
    fn affects(&self, old: &BakeSettings, map: MeshMap) -> bool {
        if self.resolution != old.resolution || self.padding != old.padding {
            return true;
        }
        if map.baked_from_high()
            && (self.high_poly != old.high_poly
                || (self.high_poly != 0
                    && (self.auto_cage != old.auto_cage
                        || (!self.auto_cage && (self.frontal != old.frontal || self.rear != old.rear))
                        || self.match_by_name != old.match_by_name)))
        {
            return true;
        }
        match map {
            MeshMap::AmbientOcclusion => {
                self.samples != old.samples
                    || self.max_distance != old.max_distance
                    || self.spread_angle != old.spread_angle
                    || self.ignore_backfaces != old.ignore_backfaces
            }
            MeshMap::Thickness => self.samples != old.samples || self.max_distance != old.max_distance,
            MeshMap::Curvature => self.curvature_radius != old.curvature_radius,
            _ => false,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapStatus {
    Missing,
    UpToDate,
    OutOfDate,
}

impl MapStatus {
    pub fn label(self) -> &'static str {
        match self {
            MapStatus::Missing => "Missing",
            MapStatus::UpToDate => "Up to date",
            MapStatus::OutOfDate => "Out of date",
        }
    }
}

pub const POS_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba32Float;
pub const NRM_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const SCALAR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
pub const SRC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
/// Auto cage per texel: frontal, rear (world units).
const CAGE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;

pub struct MapTexture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
}

impl MapTexture {
    fn new(gpu: &Gpu, label: &str, format: wgpu::TextureFormat, res: u32, layers: u32) -> Self {
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: layers },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::STORAGE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        Self { texture, view }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RasterParams {
    bbox_min: [f32; 4],
    bbox_size: [f32; 4],
    tile: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DilateParams {
    res: u32,
    layer: u32,
    step: u32,
    padding: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct RayParams {
    layer: u32,
    res: u32,
    row_start: u32,
    row_count: u32,
    mode: u32,
    samples: u32,
    ignore_backfaces: u32,
    /// Layer of the surface textures read (the projected surface has a single layer).
    in_layer: u32,
    max_distance: f32,
    cos_spread: f32,
    curvature_radius: f32,
    epsilon: f32,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct ProjectParams {
    layer: u32,
    res: u32,
    row_start: u32,
    row_count: u32,
    frontal: f32,
    rear: f32,
    flags: u32,
    _pad: u32,
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Raster { tile: u32 },
    /// High-poly projection of some rows of a tile.
    Project { tile: u32, row_start: u32 },
    Ray { map: MeshMap, tile: u32, row_start: u32 },
}

// Only one exists at a time, so the size difference doesn't matter.
#[allow(clippy::large_enum_variant)]
enum HighState {
    Loading(std::thread::JoinHandle<anyhow::Result<HighCpu>>),
    Ready(HighBake),
}

/// GPU state of a high-poly bake: the mesh, and one tile of projected surface.
struct HighBake {
    mesh: HighGpu,
    /// Projected position and normal of the tile being baked (same encoding as the position
    /// and normal maps).
    hpos: wgpu::Texture,
    hnrm: wgpu::Texture,
    /// Covered and missed texels, summed over all tiles.
    counters: wgpu::Buffer,
    /// Auto cage of the tile being baked: frontal and rear distance per texel (1×1 and unused
    /// without auto cage).
    cage: wgpu::Texture,
}

/// A bake in progress. Advanced a little each frame so the UI stays responsive.
pub struct BakeJob {
    steps: VecDeque<Step>,
    total: usize,
    settings: BakeSettings,
    maps: Vec<MeshMap>,
    high: Option<HighState>,
    error: Option<String>,
}

impl BakeJob {
    pub fn progress(&self) -> f32 {
        1.0 - self.steps.len() as f32 / self.total.max(1) as f32
    }

    pub fn maps(&self) -> &[MeshMap] {
        &self.maps
    }
}

pub struct MeshMaps {
    pub res: u32,
    pub layers: u32,
    pub position: MapTexture,
    pub normal: MapTexture,
    pub id: MapTexture,
    pub ao: MapTexture,
    pub curvature: MapTexture,
    pub thickness: MapTexture,
    /// Tangent-space normal from the high poly, encoded `n * 0.5 + 0.5`; alpha 0 = missed.
    pub tnormal: MapTexture,
    pub height: MapTexture,
    /// Low-poly tangent per texel (w = sign * (part + 1)); not padded.
    tangent: MapTexture,
    /// Nearest covered texel per texel (padding source), `u32::MAX` when empty.
    pub source: MapTexture,
    pub status: [MapStatus; 8],
    baked_with: [Option<BakeSettings>; 8],
    /// Out of date whatever the settings: the mesh or the bake exclusion changed since the bake.
    stale: [bool; 8],
    pub settings: BakeSettings,
    /// The high-poly mesh to bake from.
    pub high_poly: Option<PathBuf>,
    /// Low-poly part names, for matching.
    low_groups: Vec<String>,
    /// Low-poly geometry, for measuring the auto cage.
    low: Arc<LowMesh>,
    /// The bake preview (docs/reference/mesh-maps.md#bake-preview).
    pub preview: crate::bakepreview::Preview,
    /// Percentage of texels the last high-poly bake missed.
    pub last_misses: Option<f32>,
    /// Names of the IDs in the ID map when it was baked from a high poly (its materials);
    /// empty when the ID map comes from the mesh's own materials.
    pub id_names: Vec<String>,
    /// Message about the last finished bake, for the status bar (taken once).
    pub report: Option<Result<String, String>>,
    /// Incremented whenever any map's contents change.
    pub version: u64,
    /// The triangles the UV raster draws when some are excluded from baking
    /// (docs/reference/bake-exclusion.md), and their index count.
    raster_indices: Option<(wgpu::Buffer, u32)>,

    bvh_nodes: wgpu::Buffer,
    bvh_tris: wgpu::Buffer,
    bbox_min: glam::Vec3,
    bbox_size: glam::Vec3,
    diagonal: f32,

    raster_pipeline: wgpu::RenderPipeline,
    raster_layout: wgpu::BindGroupLayout,
    cage_pipeline: wgpu::RenderPipeline,
    dilate_init: wgpu::ComputePipeline,
    dilate_step: wgpu::ComputePipeline,
    dilate_gather: wgpu::ComputePipeline,
    ray_pipeline: wgpu::ComputePipeline,
    project_pipeline: wgpu::ComputePipeline,
}

impl MeshMaps {
    pub fn new(gpu: &Gpu, mesh: &Mesh, tile_resolution: u32) -> Self {
        let d = &gpu.device;
        let settings = BakeSettings::default();
        let res = tile_resolution;
        let layers = mesh.tiles.len().max(1) as u32;

        let bvh = Bvh::build(mesh);
        log::info!("BVH: {} nodes, {} triangles", bvh.nodes.len(), bvh.tris.len());
        let bvh_nodes = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bvh nodes"),
            contents: bytemuck::cast_slice(&bvh.nodes),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let bvh_tris = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("bvh tris"),
            contents: bytemuck::cast_slice(&bvh.tris),
            usage: wgpu::BufferUsages::STORAGE,
        });

        let raster_module = gpu::shader(d, "uv raster", include_str!("shaders/uv_raster.wgsl"));
        let raster_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("uv raster"),
            entries: &[gpu::uniform_entry(0, wgpu::ShaderStages::VERTEX_FRAGMENT)],
        });
        let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("uv raster"),
            bind_group_layouts: &[Some(&raster_layout)],
            immediate_size: 0,
        });
        let raster_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("uv raster"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &raster_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &raster_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(POS_FORMAT.into()), Some(NRM_FORMAT.into()), Some(ID_FORMAT.into()), Some(NRM_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let cage_module = gpu::shader(d, "cage raster", include_str!("shaders/cage_raster.wgsl"));
        let cage_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("cage raster"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &cage_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: 16,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![0 => Float32x4],
                })],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &cage_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(CAGE_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let dilate = gpu::shader(d, "dilate", include_str!("shaders/dilate.wgsl"));
        let compute = |module: &wgpu::ShaderModule, entry: &str| {
            d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry),
                layout: None,
                module,
                entry_point: Some(entry),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let dilate_init = compute(&dilate, "init");
        let dilate_step = compute(&dilate, "step");
        let dilate_gather = compute(&dilate, "gather");
        let ray = gpu::shader(d, "ray bake", include_str!("shaders/raybake.wgsl"));
        let ray_pipeline = compute(&ray, "main");
        let project = gpu::shader(d, "high-poly projection", include_str!("shaders/highbake.wgsl"));
        let project_pipeline = compute(&project, "main");

        let size = (mesh.bbox_max - mesh.bbox_min).max(glam::Vec3::splat(1e-6));
        let maps = Self {
            res,
            layers,
            position: MapTexture::new(gpu, "map position", POS_FORMAT, res, layers),
            normal: MapTexture::new(gpu, "map normal", NRM_FORMAT, res, layers),
            id: MapTexture::new(gpu, "map id", ID_FORMAT, res, layers),
            ao: MapTexture::new(gpu, "map ao", SCALAR_FORMAT, res, layers),
            curvature: MapTexture::new(gpu, "map curvature", SCALAR_FORMAT, res, layers),
            thickness: MapTexture::new(gpu, "map thickness", SCALAR_FORMAT, res, layers),
            tnormal: MapTexture::new(gpu, "map normal (tangent)", NRM_FORMAT, res, layers),
            height: MapTexture::new(gpu, "map height", SCALAR_FORMAT, res, layers),
            tangent: MapTexture::new(gpu, "map tangent", NRM_FORMAT, res, layers),
            source: MapTexture::new(gpu, "map source", SRC_FORMAT, res, layers),
            status: [MapStatus::Missing; 8],
            baked_with: [None; 8],
            stale: [false; 8],
            settings,
            high_poly: None,
            low_groups: mesh.groups.clone(),
            low: Arc::new(LowMesh { vertices: mesh.vertices.clone(), indices: mesh.indices.clone() }),
            preview: Default::default(),
            last_misses: None,
            id_names: Vec::new(),
            report: None,
            version: 0,
            raster_indices: None,
            bvh_nodes,
            bvh_tris,
            bbox_min: mesh.bbox_min,
            bbox_size: size,
            diagonal: size.length(),
            raster_pipeline,
            raster_layout,
            cage_pipeline,
            dilate_init,
            dilate_step,
            dilate_gather,
            ray_pipeline,
            project_pipeline,
        };
        maps.clear_scalar_maps(gpu);
        maps
    }

    /// Neutral values for maps that have not been baked: AO 1, curvature 0.5, thickness 1,
    /// height 0.5, flat normal.
    fn clear_scalar_maps(&self, gpu: &Gpu) {
        let texels = (self.res * self.res) as usize;
        let h = half::f16::from_f32;
        let flat = vec![[h(0.5), h(0.5), h(1.0), h(1.0)]; texels];
        for layer in 0..self.layers {
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.tnormal.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                bytemuck::cast_slice(&flat),
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(self.res * 8), rows_per_image: Some(self.res) },
                wgpu::Extent3d { width: self.res, height: self.res, depth_or_array_layers: 1 },
            );
        }
        for (tex, value) in [(&self.ao, 1.0f32), (&self.curvature, 0.5), (&self.thickness, 1.0), (&self.height, 0.5)] {
            let data = vec![value; texels];
            for layer in 0..self.layers {
                gpu.queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &tex.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                        aspect: wgpu::TextureAspect::All,
                    },
                    bytemuck::cast_slice(&data),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(self.res * 4),
                        rows_per_image: Some(self.res),
                    },
                    wgpu::Extent3d { width: self.res, height: self.res, depth_or_array_layers: 1 },
                );
            }
        }
    }

    /// Leaves the triangles flagged in `excluded` out of the UV raster (an empty slice: none).
    pub fn set_excluded(&mut self, gpu: &Gpu, mesh: &crate::mesh::Mesh, excluded: &[bool]) {
        if !excluded.iter().any(|e| *e) {
            self.raster_indices = None;
            return;
        }
        let indices: Vec<u32> = mesh.indices.chunks(3).zip(excluded).filter(|(_, e)| !**e).flat_map(|(t, _)| t.iter().copied()).collect();
        let buffer = gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("raster indices"),
            // A buffer can't be empty.
            contents: bytemuck::cast_slice(if indices.is_empty() { &[0u32; 3] } else { &indices[..] }),
            usage: wgpu::BufferUsages::INDEX,
        });
        self.raster_indices = Some((buffer, indices.len() as u32));
    }

    /// A map's texture and its bytes per texel.
    pub fn texture(&self, map: MeshMap) -> (&wgpu::Texture, u32) {
        match map {
            MeshMap::Position => (&self.position.texture, 16),
            MeshMap::WorldNormal => (&self.normal.texture, 8),
            MeshMap::Id => (&self.id.texture, 4),
            MeshMap::AmbientOcclusion => (&self.ao.texture, 4),
            MeshMap::Curvature => (&self.curvature.texture, 4),
            MeshMap::Thickness => (&self.thickness.texture, 4),
            MeshMap::Normal => (&self.tnormal.texture, 8),
            MeshMap::Height => (&self.height.texture, 4),
        }
    }

    /// Reads one tile of a map back as stored: position in object space, World Normal encoded
    /// 0..1 (n * 0.5 + 0.5), Normal encoded 0..1, ID colors 0..1 and scalar maps as gray with alpha 1.
    pub fn read(&self, gpu: &Gpu, map: MeshMap, layer: u32) -> Vec<[f32; 4]> {
        let (tex, bpp) = self.texture(map);
        let bytes = gpu.read_texture(tex, layer, bpp);
        match map {
            MeshMap::Position => bytemuck::cast_slice::<u8, [f32; 4]>(&bytes).to_vec(),
            MeshMap::WorldNormal | MeshMap::Normal => {
                bytemuck::cast_slice::<u8, [half::f16; 4]>(&bytes).iter().map(|c| c.map(|h| h.to_f32())).collect()
            }
            MeshMap::Id => bytemuck::cast_slice::<u8, [u8; 4]>(&bytes).iter().map(|c| c.map(|v| v as f32 / 255.0)).collect(),
            _ => bytemuck::cast_slice::<u8, f32>(&bytes).iter().map(|v| [*v, *v, *v, 1.0]).collect(),
        }
    }

    pub fn status(&self, map: MeshMap) -> MapStatus {
        self.status[map.index()]
    }

    /// Re-evaluates Up to date / Out of date after the bake settings changed.
    pub fn refresh_status(&mut self) {
        for map in MeshMap::ALL {
            if let Some(old) = &self.baked_with[map.index()] {
                self.status[map.index()] = if self.stale[map.index()] || self.settings.affects(old, map) {
                    MapStatus::OutOfDate
                } else {
                    MapStatus::UpToDate
                };
            }
        }
    }

    /// Marks a map as baked with the current settings (after loading it from disk).
    pub fn mark_baked(&mut self, map: MeshMap) {
        self.status[map.index()] = MapStatus::UpToDate;
        self.baked_with[map.index()] = Some(self.settings);
        self.stale[map.index()] = false;
        self.version += 1;
    }

    /// Marks a map as needing a rebake (its data no longer matches the mesh).
    pub fn mark_out_of_date(&mut self, map: MeshMap) {
        self.status[map.index()] = MapStatus::OutOfDate;
        self.baked_with[map.index()] = Some(self.settings);
        self.stale[map.index()] = true;
    }

    /// Advances the bake preview; doesn't block (see `bakepreview::Preview::update`).
    pub fn update_preview(&mut self, gpu: &Gpu, import: &ImportSettings) {
        let src = crate::bakepreview::Source {
            high_poly: self.high_poly.as_deref(),
            import,
            low: &self.low,
            low_groups: &self.low_groups,
            settings: &self.settings,
            diagonal: self.diagonal,
        };
        self.preview.update(gpu, src);
    }

    /// Starts baking `maps`. Raster maps are always baked first, since every other map
    /// reads them.
    pub fn start_bake(&mut self, gpu: &Gpu, tile_resolution: u32, maps: &[MeshMap], import: &ImportSettings) -> BakeJob {
        let res = if self.settings.resolution == 0 { tile_resolution } else { self.settings.resolution };
        self.settings.high_poly = crate::highpoly::stamp(self.high_poly.as_ref());
        self.refresh_status();
        let mut maps: Vec<MeshMap> = maps.iter().copied().filter(|m| self.high_poly.is_some() || !m.needs_high()).collect();
        if res != self.res {
            self.reallocate(gpu, res);
            // Everything baked at the old resolution is gone.
            self.status = [MapStatus::Missing; 8];
            self.baked_with = [None; 8];
            for m in [MeshMap::Position, MeshMap::WorldNormal] {
                if !maps.contains(&m) {
                    maps.push(m);
                }
            }
        }
        let raster_needed = maps.iter().any(|m| !m.is_ray_traced())
            || self.status(MeshMap::Position) != MapStatus::UpToDate
            || self.baked_with[MeshMap::Position.index()]
                .is_none_or(|old| old.padding != self.settings.padding);
        let mut steps = VecDeque::new();
        if raster_needed {
            for tile in 0..self.layers {
                steps.push_back(Step::Raster { tile });
            }
            for m in [MeshMap::Position, MeshMap::WorldNormal] {
                if !maps.contains(&m) {
                    maps.push(m);
                }
            }
            // The raster pass writes the low poly's ID; an ID baked from the high poly is redone.
            if self.high_poly.is_some() && self.status(MeshMap::Id) != MapStatus::Missing && !maps.contains(&MeshMap::Id) {
                maps.push(MeshMap::Id);
            }
        }
        let high = match &self.high_poly {
            Some(path) if maps.iter().any(|m| m.baked_from_high()) => {
                let (path, import, groups, by_name) = (path.clone(), import.clone(), self.low_groups.clone(), self.settings.match_by_name);
                let cage = self
                    .settings
                    .auto_cage
                    .then(|| crate::highpoly::CageRequest { low: self.low.clone(), diagonal: self.diagonal });
                Some(HighState::Loading(std::thread::spawn(move || crate::highpoly::load(&path, &import, &groups, by_name, cage))))
            }
            _ => None,
        };
        let rows = self.rows_per_step();
        // One ray per texel: as many rows per step as the ray budget allows.
        let project_rows = ((1u64 << 24) / self.res.max(1) as u64).clamp(1, self.res as u64) as u32;
        for tile in 0..self.layers {
            if high.is_some() {
                let mut row = 0;
                while row < self.res {
                    steps.push_back(Step::Project { tile, row_start: row });
                    row += project_rows;
                }
            }
            for &map in &maps {
                if !map.is_ray_traced() {
                    continue;
                }
                let mut row = 0;
                while row < self.res {
                    steps.push_back(Step::Ray { map, tile, row_start: row });
                    row += rows;
                }
            }
        }
        BakeJob { total: steps.len(), steps, settings: self.settings, maps, high, error: None }
    }

    /// Waits for the high poly to load and uploads it. Ok(false) while it is still loading.
    fn high_ready(&self, gpu: &Gpu, job: &mut BakeJob) -> Result<bool, String> {
        match job.high.take() {
            Some(HighState::Loading(handle)) if !handle.is_finished() => {
                job.high = Some(HighState::Loading(handle));
                Ok(false)
            }
            Some(HighState::Loading(handle)) => {
                let cpu = handle
                    .join()
                    .map_err(|_| "loading the high poly failed".to_string())?
                    .map_err(|e| format!("Could not load the high-poly mesh: {e:#}"))?;
                let d = &gpu.device;
                let tex_sized = |label, format, size: u32, usage| {
                    d.create_texture(&wgpu::TextureDescriptor {
                        label: Some(label),
                        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        format,
                        usage: wgpu::TextureUsages::TEXTURE_BINDING | usage,
                        view_formats: &[],
                    })
                };
                let tex = |label, format| tex_sized(label, format, self.res, wgpu::TextureUsages::STORAGE_BINDING);
                let cage_size = if cpu.cage.is_some() { self.res } else { 1 };
                let counters = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("bake counters"),
                    contents: bytemuck::cast_slice(&[0u32; 2]),
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                });
                job.high = Some(HighState::Ready(HighBake {
                    mesh: cpu.upload(gpu).map_err(|e| format!("{e:#}"))?,
                    hpos: tex("projected position", POS_FORMAT),
                    hnrm: tex("projected normal", NRM_FORMAT),
                    counters,
                    cage: tex_sized("auto cage", CAGE_FORMAT, cage_size, wgpu::TextureUsages::RENDER_ATTACHMENT),
                }));
                Ok(true)
            }
            other => {
                job.high = other;
                Ok(true)
            }
        }
    }

    fn rows_per_step(&self) -> u32 {
        // Roughly 16M rays per submission keeps each well below the OS GPU timeout.
        let per_row = (self.res as u64 * self.settings.samples.max(1) as u64).max(1);
        ((1u64 << 24) / per_row).clamp(1, self.res as u64) as u32
    }

    fn reallocate(&mut self, gpu: &Gpu, res: u32) {
        let layers = self.layers;
        self.res = res;
        self.position = MapTexture::new(gpu, "map position", POS_FORMAT, res, layers);
        self.normal = MapTexture::new(gpu, "map normal", NRM_FORMAT, res, layers);
        self.id = MapTexture::new(gpu, "map id", ID_FORMAT, res, layers);
        self.ao = MapTexture::new(gpu, "map ao", SCALAR_FORMAT, res, layers);
        self.curvature = MapTexture::new(gpu, "map curvature", SCALAR_FORMAT, res, layers);
        self.thickness = MapTexture::new(gpu, "map thickness", SCALAR_FORMAT, res, layers);
        self.tnormal = MapTexture::new(gpu, "map normal (tangent)", NRM_FORMAT, res, layers);
        self.height = MapTexture::new(gpu, "map height", SCALAR_FORMAT, res, layers);
        self.tangent = MapTexture::new(gpu, "map tangent", NRM_FORMAT, res, layers);
        self.source = MapTexture::new(gpu, "map source", SRC_FORMAT, res, layers);
        self.clear_scalar_maps(gpu);
        self.version += 1;
    }

    /// Runs bake steps for up to `budget`. Returns true when the job is finished.
    pub fn advance(
        &mut self,
        gpu: &Gpu,
        mesh: &GpuMesh,
        job: &mut BakeJob,
        budget: std::time::Duration,
    ) -> bool {
        let start = std::time::Instant::now();
        while let Some(&step) = job.steps.front() {
            if matches!(step, Step::Project { .. }) {
                match self.high_ready(gpu, job) {
                    Ok(true) => {}
                    Ok(false) => break,
                    Err(e) => {
                        job.error = Some(e);
                        job.steps.clear();
                        break;
                    }
                }
            }
            job.steps.pop_front();
            let high = match &job.high {
                Some(HighState::Ready(h)) => Some(h),
                _ => None,
            };
            match step {
                Step::Raster { tile } => self.raster_tile(gpu, mesh, tile, job.settings.padding),
                Step::Project { tile, row_start } => {
                    let high = high.expect("high poly loaded");
                    if row_start == 0 {
                        self.raster_cage(gpu, mesh, high, tile);
                    }
                    self.project_rows(gpu, &job.maps, high, tile, row_start)
                }
                Step::Ray { map, tile, row_start } => self.ray_rows(gpu, map, tile, row_start, high),
            }
            gpu.wait_idle();
            if start.elapsed() >= budget {
                break;
            }
        }
        self.version += 1;
        if !job.steps.is_empty() {
            return false;
        }
        if let Some(e) = job.error.take() {
            // The raster maps were baked; the rest keep their old data and status.
            for m in [MeshMap::Position, MeshMap::WorldNormal] {
                if job.maps.contains(&m) {
                    self.status[m.index()] = MapStatus::UpToDate;
                    self.baked_with[m.index()] = Some(job.settings);
                    self.stale[m.index()] = false;
                }
            }
            self.report = Some(Err(e));
            return true;
        }
        for &m in &job.maps {
            self.status[m.index()] = MapStatus::UpToDate;
            self.baked_with[m.index()] = Some(job.settings);
            self.stale[m.index()] = false;
        }
        if job.maps.contains(&MeshMap::Id) {
            self.id_names = match &job.high {
                Some(HighState::Ready(h)) => h.mesh.materials.clone(),
                _ => Vec::new(),
            };
        }
        if let Some(HighState::Ready(h)) = &job.high {
            let c = gpu.read_buffer(&h.counters);
            let c: &[u32] = bytemuck::cast_slice(&c);
            let pct = if c[0] > 0 { c[1] as f32 * 100.0 / c[0] as f32 } else { 0.0 };
            self.last_misses = Some(pct);
            let mut msg = format!("High-poly bake: {pct:.1}% of texels missed");
            if let Some(c) = &h.mesh.cage {
                let (f, r) = (c.max_frontal / self.diagonal, c.max_rear / self.diagonal);
                msg += &format!(" (auto cage: up to {f:.3} frontal, {r:.3} rear)");
            }
            self.report = Some(Ok(msg));
        }
        true
    }

    /// Bakes synchronously, for headless use.
    pub fn bake_blocking(&mut self, gpu: &Gpu, mesh: &GpuMesh, tile_resolution: u32, maps: &[MeshMap]) {
        // Only used for the raster maps; a high poly is baked through `start_bake`.
        let mut job = self.start_bake(gpu, tile_resolution, maps, &ImportSettings::default());
        while !self.advance(gpu, mesh, &mut job, std::time::Duration::from_secs(3600)) {
            // Waiting for the high poly to load.
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    fn tile_offset(&self, mesh: &GpuMesh, layer: u32) -> (f32, f32) {
        let udim = mesh.tiles[layer as usize] - 1001;
        ((udim % 10) as f32, (udim / 10) as f32)
    }

    fn raster_tile(&mut self, gpu: &Gpu, mesh: &GpuMesh, layer: u32, padding: u32) {
        let d = &gpu.device;
        let res = self.res;
        let target = |label, format| {
            gpu::render_target(d, label, res, res, format, wgpu::TextureUsages::empty())
                .create_view(&Default::default())
        };
        let pos = target("raster pos", POS_FORMAT);
        let nrm = target("raster nrm", NRM_FORMAT);
        let id = target("raster id", ID_FORMAT);
        let tan_tex = gpu::render_target(d, "raster tangent", res, res, NRM_FORMAT, wgpu::TextureUsages::COPY_SRC);
        let tan = tan_tex.create_view(&Default::default());
        let (tu, tv) = self.tile_offset(mesh, layer);
        let params = RasterParams {
            bbox_min: self.bbox_min.extend(0.0).into(),
            bbox_size: self.bbox_size.extend(0.0).into(),
            tile: [tu, tv, 0.0, 0.0],
        };
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("raster params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("raster"),
            layout: &self.raster_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let attach = |view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })
            };
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("uv raster"),
                color_attachments: &[attach(&pos), attach(&nrm), attach(&id), attach(&tan)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.raster_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            // Excluded triangles write no texels; rays still meet them (the BVH has every one).
            match &self.raster_indices {
                Some((buffer, count)) => {
                    if *count > 0 {
                        pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..*count, 0, 0..1);
                    }
                }
                None => {
                    pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.index_count, 0, 0..1);
                }
            }
        }
        enc.copy_texture_to_texture(
            tan_tex.as_image_copy(),
            wgpu::TexelCopyTextureInfo {
                texture: &self.tangent.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x: 0, y: 0, z: layer },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
        );

        // Nearest covered texel for every texel, by jump flooding (docs/reference/mesh-maps.md#padding).
        let src_tex = |label| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: SRC_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default())
        };
        let src = [src_tex("src a"), src_tex("src b")];
        let dparams_step = |step: u32| {
            d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("dilate params"),
                contents: bytemuck::bytes_of(&DilateParams { res, layer, step, padding }),
                usage: wgpu::BufferUsages::UNIFORM,
            })
        };
        let dparams = dparams_step(0);
        let groups = res.div_ceil(8);
        let layer_view = |tex: &MapTexture| {
            tex.texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        {
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("dilate init"),
                layout: &self.dilate_init.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: dparams.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: tex_res(&pos) },
                    wgpu::BindGroupEntry { binding: 2, resource: tex_res(&src[0]) },
                ],
            });
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.dilate_init);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups, groups, 1);
        }
        let mut cur = 0;
        // Halving offsets, then one more 1-texel step to fix the few misses of plain jump flooding.
        let mut steps = Vec::new();
        let mut k = res.next_power_of_two() / 2;
        while k >= 1 {
            steps.push(k);
            k /= 2;
        }
        steps.push(1);
        for k in steps {
            let sp = dparams_step(k);
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("dilate step"),
                layout: &self.dilate_step.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: sp.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: tex_res(&src[1 - cur]) },
                    wgpu::BindGroupEntry { binding: 3, resource: tex_res(&src[cur]) },
                ],
            });
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.dilate_step);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups, groups, 1);
            cur = 1 - cur;
        }
        {
            let (out_pos, out_nrm, out_id, out_src) = (
                layer_view(&self.position),
                layer_view(&self.normal),
                layer_view(&self.id),
                layer_view(&self.source),
            );
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("dilate gather"),
                layout: &self.dilate_gather.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: dparams.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: tex_res(&pos) },
                    wgpu::BindGroupEntry { binding: 3, resource: tex_res(&src[cur]) },
                    wgpu::BindGroupEntry { binding: 4, resource: tex_res(&nrm) },
                    wgpu::BindGroupEntry { binding: 5, resource: tex_res(&id) },
                    wgpu::BindGroupEntry { binding: 6, resource: tex_res(&out_pos) },
                    wgpu::BindGroupEntry { binding: 7, resource: tex_res(&out_nrm) },
                    wgpu::BindGroupEntry { binding: 8, resource: tex_res(&out_id) },
                    wgpu::BindGroupEntry { binding: 9, resource: tex_res(&out_src) },
                ],
            });
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.dilate_gather);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(groups, groups, 1);
        }
        gpu.queue.submit([enc.finish()]);
    }

    /// Rasterizes the auto cage of one tile in UV space, like `raster_tile` does the low poly.
    fn raster_cage(&self, gpu: &Gpu, mesh: &GpuMesh, high: &HighBake, layer: u32) {
        let Some(cage) = &high.mesh.cage else { return };
        let d = &gpu.device;
        let (tu, tv) = self.tile_offset(mesh, layer);
        let params = RasterParams {
            bbox_min: self.bbox_min.extend(0.0).into(),
            bbox_size: self.bbox_size.extend(0.0).into(),
            tile: [tu, tv, 0.0, 0.0],
        };
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("cage raster params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("cage raster"),
            layout: &self.raster_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let view = high.cage.create_view(&Default::default());
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("cage raster"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.cage_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, cage.vertices.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
    }

    /// High-poly projection of `rows` of one tile: Normal, Height, ID (as requested) and the
    /// projected surface that `ray_rows` traces from.
    fn project_rows(&mut self, gpu: &Gpu, maps: &[MeshMap], high: &HighBake, layer: u32, row_start: u32) {
        let d = &gpu.device;
        let s = &self.settings;
        let rows = ((1u64 << 24) / self.res.max(1) as u64).clamp(1, self.res as u64) as u32;
        let rows = rows.min(self.res - row_start);
        let flags = maps.contains(&MeshMap::Normal) as u32
            | (maps.contains(&MeshMap::Height) as u32) << 1
            | (maps.contains(&MeshMap::Id) as u32) << 2;
        // With auto cage, the per-texel cage replaces the distances, and their maximum scales
        // Height.
        let (frontal, rear, flags) = match &high.mesh.cage {
            Some(c) => (c.max_frontal, c.max_rear, flags | 8),
            None => (s.frontal * self.diagonal, s.rear * self.diagonal, flags),
        };
        let params = ProjectParams { layer, res: self.res, row_start, row_count: rows, frontal, rear, flags, _pad: 0 };
        let cage = high.cage.create_view(&Default::default());
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("project params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let array = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let (hpos, hnrm) = (array(&high.hpos), array(&high.hnrm));
        let (tnrm, height, id) = (array(&self.tnormal.texture), array(&self.height.texture), array(&self.id.texture));
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("high-poly projection"),
            layout: &self.project_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tex_res(&self.position.view) },
                wgpu::BindGroupEntry { binding: 2, resource: tex_res(&self.normal.view) },
                wgpu::BindGroupEntry { binding: 3, resource: tex_res(&self.tangent.view) },
                wgpu::BindGroupEntry { binding: 4, resource: tex_res(&self.source.view) },
                wgpu::BindGroupEntry { binding: 5, resource: high.mesh.nodes.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 6, resource: high.mesh.tris.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 7, resource: high.mesh.data.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 8, resource: high.mesh.low_match.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 9, resource: tex_res(&hpos) },
                wgpu::BindGroupEntry { binding: 10, resource: tex_res(&hnrm) },
                wgpu::BindGroupEntry { binding: 11, resource: tex_res(&tnrm) },
                wgpu::BindGroupEntry { binding: 12, resource: tex_res(&height) },
                wgpu::BindGroupEntry { binding: 13, resource: tex_res(&id) },
                wgpu::BindGroupEntry { binding: 14, resource: high.counters.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 15, resource: tex_res(&cage) },
            ],
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.project_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(self.res.div_ceil(8), rows.div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
    }

    fn ray_rows(&mut self, gpu: &Gpu, map: MeshMap, layer: u32, row_start: u32, high: Option<&HighBake>) {
        let d = &gpu.device;
        let s = &self.settings;
        let rows = self.rows_per_step().min(self.res - row_start);
        let (mode, out) = match map {
            MeshMap::AmbientOcclusion => (0, &self.ao),
            MeshMap::Curvature => (1, &self.curvature),
            _ => (2, &self.thickness),
        };
        let params = RayParams {
            layer,
            res: self.res,
            row_start,
            row_count: rows,
            mode,
            samples: if map == MeshMap::Curvature { s.samples.clamp(16, 64) } else { s.samples.max(1) },
            ignore_backfaces: s.ignore_backfaces as u32,
            in_layer: if high.is_some() { 0 } else { layer },
            max_distance: s.max_distance * self.diagonal,
            cos_spread: (s.spread_angle.clamp(1.0, 180.0) * 0.5).to_radians().cos(),
            // The probe sits two radii above the surface.
            curvature_radius: 2.0 * s.curvature_radius * self.diagonal,
            epsilon: self.diagonal * 2e-5,
        };
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ray params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let array = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let out_view = array(&out.texture);
        // With a high poly, trace from the projected surface against the high poly.
        let (pos, nrm, nodes, tris) = match high {
            Some(h) => (array(&h.hpos), array(&h.hnrm), &h.mesh.nodes, &h.mesh.tris),
            None => (array(&self.position.texture), array(&self.normal.texture), &self.bvh_nodes, &self.bvh_tris),
        };
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray bake"),
            layout: &self.ray_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tex_res(&pos) },
                wgpu::BindGroupEntry { binding: 2, resource: tex_res(&nrm) },
                wgpu::BindGroupEntry { binding: 3, resource: nodes.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: tris.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&out_view) },
            ],
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.ray_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(self.res.div_ceil(8), rows.div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
    }
}

fn tex_res(v: &wgpu::TextureView) -> wgpu::BindingResource<'_> {
    wgpu::BindingResource::TextureView(v)
}

/// Saves one tile of a map as a 16-bit PNG (the `save-map` script command), encoded as in
/// docs/reference/mesh-maps.md#maps: normals `n * 0.5 + 0.5`, position normalized to the
/// bounding box, scalar maps as gray.
pub fn save_png(gpu: &Gpu, maps: &MeshMaps, map: MeshMap, layer: u32, path: &std::path::Path) -> anyhow::Result<()> {
    let px = maps.read(gpu, map, layer);
    let res = maps.res;
    let q = |v: f32| (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let rgb: Vec<[f32; 3]> = match map {
        MeshMap::Position => px
            .iter()
            .map(|p| {
                let v = (glam::Vec3::new(p[0], p[1], p[2]) - maps.bbox_min) / maps.bbox_size;
                [v.x, v.y, v.z]
            })
            .collect(),
        MeshMap::WorldNormal | MeshMap::Normal | MeshMap::Id => px.iter().map(|c| [c[0], c[1], c[2]]).collect(),
        _ => {
            let v: Vec<u16> = px.iter().map(|c| q(c[0])).collect();
            image::ImageBuffer::<image::Luma<u16>, _>::from_raw(res, res, v).unwrap().save(path)?;
            return Ok(());
        }
    };
    let v: Vec<u16> = rgb.iter().flat_map(|c| c.map(q)).collect();
    image::ImageBuffer::<image::Rgb<u16>, _>::from_raw(res, res, v).unwrap().save(path)?;
    Ok(())
}
