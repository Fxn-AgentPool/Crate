//! Mesh map baking. See Painter-docs/reference/mesh-maps.md.
//!
//! Position, World Normal and ID come from a UV-space raster pass; Ambient Occlusion,
//! Curvature and Thickness are ray traced on the GPU against a BVH. Padding is computed
//! once per tile as a "nearest covered texel" map that every map is gathered from.

use std::collections::VecDeque;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::bvh::Bvh;
use crate::gpu::{self, Gpu};
use crate::mesh::Mesh;
use crate::viewport::{GpuMesh, vertex_layout};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MeshMap {
    Position,
    WorldNormal,
    AmbientOcclusion,
    Curvature,
    Thickness,
    Id,
}

impl MeshMap {
    pub const ALL: [MeshMap; 6] = [
        MeshMap::Position,
        MeshMap::WorldNormal,
        MeshMap::AmbientOcclusion,
        MeshMap::Curvature,
        MeshMap::Thickness,
        MeshMap::Id,
    ];

    pub fn name(self) -> &'static str {
        match self {
            MeshMap::Position => "Position",
            MeshMap::WorldNormal => "World Normal",
            MeshMap::AmbientOcclusion => "Ambient Occlusion",
            MeshMap::Curvature => "Curvature",
            MeshMap::Thickness => "Thickness",
            MeshMap::Id => "ID",
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
        }
    }

    pub fn index(self) -> usize {
        self as usize
    }

    fn is_ray_traced(self) -> bool {
        matches!(self, MeshMap::AmbientOcclusion | MeshMap::Curvature | MeshMap::Thickness)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
        }
    }
}

impl BakeSettings {
    /// Whether a map baked with `old` would differ when baked with `self`.
    fn affects(&self, old: &BakeSettings, map: MeshMap) -> bool {
        if self.resolution != old.resolution || self.padding != old.padding {
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
    _pad: [u32; 2],
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
    _pad: u32,
    max_distance: f32,
    cos_spread: f32,
    curvature_radius: f32,
    epsilon: f32,
}

#[derive(Clone, Copy, Debug)]
enum Step {
    Raster { tile: u32 },
    Ray { map: MeshMap, tile: u32, row_start: u32 },
}

/// A bake in progress. Advanced a little each frame so the UI stays responsive.
pub struct BakeJob {
    steps: VecDeque<Step>,
    total: usize,
    settings: BakeSettings,
    maps: Vec<MeshMap>,
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
    /// Nearest covered texel per texel (padding source), `u32::MAX` when empty.
    pub source: MapTexture,
    pub status: [MapStatus; 6],
    baked_with: [Option<BakeSettings>; 6],
    pub settings: BakeSettings,
    /// Incremented whenever any map's contents change.
    pub version: u64,

    bvh_nodes: wgpu::Buffer,
    bvh_tris: wgpu::Buffer,
    bbox_min: glam::Vec3,
    bbox_size: glam::Vec3,
    diagonal: f32,

    raster_pipeline: wgpu::RenderPipeline,
    raster_layout: wgpu::BindGroupLayout,
    dilate_init: wgpu::ComputePipeline,
    dilate_step: wgpu::ComputePipeline,
    dilate_gather: wgpu::ComputePipeline,
    ray_pipeline: wgpu::ComputePipeline,
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
                targets: &[Some(POS_FORMAT.into()), Some(NRM_FORMAT.into()), Some(ID_FORMAT.into())],
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

        let size = (mesh.bbox_max - mesh.bbox_min).max(glam::Vec3::splat(1e-6));
        let mut maps = Self {
            res,
            layers,
            position: MapTexture::new(gpu, "map position", POS_FORMAT, res, layers),
            normal: MapTexture::new(gpu, "map normal", NRM_FORMAT, res, layers),
            id: MapTexture::new(gpu, "map id", ID_FORMAT, res, layers),
            ao: MapTexture::new(gpu, "map ao", SCALAR_FORMAT, res, layers),
            curvature: MapTexture::new(gpu, "map curvature", SCALAR_FORMAT, res, layers),
            thickness: MapTexture::new(gpu, "map thickness", SCALAR_FORMAT, res, layers),
            source: MapTexture::new(gpu, "map source", SRC_FORMAT, res, layers),
            status: [MapStatus::Missing; 6],
            baked_with: [None; 6],
            settings,
            version: 0,
            bvh_nodes,
            bvh_tris,
            bbox_min: mesh.bbox_min,
            bbox_size: size,
            diagonal: size.length(),
            raster_pipeline,
            raster_layout,
            dilate_init,
            dilate_step,
            dilate_gather,
            ray_pipeline,
        };
        maps.clear_scalar_maps(gpu);
        maps
    }

    /// Neutral values for maps that have not been baked: AO 1, curvature 0.5, thickness 1.
    fn clear_scalar_maps(&self, gpu: &Gpu) {
        let texels = (self.res * self.res) as usize;
        for (tex, value) in [(&self.ao, 1.0f32), (&self.curvature, 0.5), (&self.thickness, 1.0)] {
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

    pub fn status(&self, map: MeshMap) -> MapStatus {
        self.status[map.index()]
    }

    /// Re-evaluates Up to date / Out of date after the bake settings changed.
    pub fn refresh_status(&mut self) {
        for map in MeshMap::ALL {
            if let Some(old) = &self.baked_with[map.index()] {
                self.status[map.index()] = if self.settings.affects(old, map) {
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
        self.version += 1;
    }

    /// Marks a map as needing a rebake (its data no longer matches the mesh).
    pub fn mark_out_of_date(&mut self, map: MeshMap) {
        self.status[map.index()] = MapStatus::OutOfDate;
        self.baked_with[map.index()] = Some(self.settings);
    }

    pub fn mark_all_out_of_date(&mut self) {
        for s in &mut self.status {
            if *s == MapStatus::UpToDate {
                *s = MapStatus::OutOfDate;
            }
        }
    }

    /// Starts baking `maps`. Raster maps are always baked first, since every other map
    /// reads them.
    pub fn start_bake(&mut self, gpu: &Gpu, tile_resolution: u32, maps: &[MeshMap]) -> BakeJob {
        let res = if self.settings.resolution == 0 { tile_resolution } else { self.settings.resolution };
        let mut maps: Vec<MeshMap> = maps.to_vec();
        if res != self.res {
            self.reallocate(gpu, res);
            // Everything baked at the old resolution is gone.
            self.status = [MapStatus::Missing; 6];
            self.baked_with = [None; 6];
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
        }
        let rows = self.rows_per_step();
        for &map in &maps {
            if !map.is_ray_traced() {
                continue;
            }
            for tile in 0..self.layers {
                let mut row = 0;
                while row < self.res {
                    steps.push_back(Step::Ray { map, tile, row_start: row });
                    row += rows;
                }
            }
        }
        BakeJob { total: steps.len(), steps, settings: self.settings, maps }
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
        while let Some(step) = job.steps.pop_front() {
            match step {
                Step::Raster { tile } => self.raster_tile(gpu, mesh, tile, job.settings.padding),
                Step::Ray { map, tile, row_start } => self.ray_rows(gpu, map, tile, row_start),
            }
            gpu.wait_idle();
            if start.elapsed() >= budget {
                break;
            }
        }
        self.version += 1;
        if job.steps.is_empty() {
            for &m in &job.maps {
                self.status[m.index()] = MapStatus::UpToDate;
                self.baked_with[m.index()] = Some(job.settings);
            }
            true
        } else {
            false
        }
    }

    /// Bakes synchronously, for headless use.
    pub fn bake_blocking(&mut self, gpu: &Gpu, mesh: &GpuMesh, tile_resolution: u32, maps: &[MeshMap]) {
        let mut job = self.start_bake(gpu, tile_resolution, maps);
        while !self.advance(gpu, mesh, &mut job, std::time::Duration::from_secs(3600)) {}
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
                color_attachments: &[attach(&pos), attach(&nrm), attach(&id)],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.raster_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..mesh.index_count, 0, 0..1);
        }

        // Padding: nearest covered texel, grown one texel per step.
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
        let dparams = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dilate params"),
            contents: bytemuck::bytes_of(&DilateParams { res, layer, _pad: [0; 2] }),
            usage: wgpu::BufferUsages::UNIFORM,
        });
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
        for _ in 0..padding {
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("dilate step"),
                layout: &self.dilate_step.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: dparams.as_entire_binding() },
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

    fn ray_rows(&mut self, gpu: &Gpu, map: MeshMap, layer: u32, row_start: u32) {
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
            _pad: 0,
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
        let out_view = out.texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            ..Default::default()
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("ray bake"),
            layout: &self.ray_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&self.position.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.normal.view) },
                wgpu::BindGroupEntry { binding: 3, resource: self.bvh_nodes.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: self.bvh_tris.as_entire_binding() },
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
