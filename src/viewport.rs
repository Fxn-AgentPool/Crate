//! Viewport renderer: mesh shaded with the compiled material, tone mapped to 8-bit.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec3};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::gpu::{self, Gpu};
use crate::mesh::{Mesh, Vertex};

pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const OUTPUT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
pub const MSAA: u32 = 4;

const VIEWPORT_WGSL: &str = include_str!("shaders/viewport.wgsl");
const TONEMAP_WGSL: &str = include_str!("shaders/tonemap.wgsl");
pub const SURFACE_WGSL: &str = include_str!("shaders/surface.wgsl");

/// What the viewport shows. See Painter-docs/reference/channels.md "Viewport channel display".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DisplayMode {
    Material,
    /// Index into `crate::channels::Channel::ALL`.
    Channel(u32),
    /// Index into the mesh map list.
    MeshMap(u32),
}

#[derive(Clone, Copy, Debug)]
pub struct ViewSettings {
    pub display: DisplayMode,
    pub show_tiles: bool,
    pub wireframe: bool,
    pub exposure: f32,
    pub env_rotation: f32,
    pub height_scale: f32,
    /// An HDRI environment is loaded and used for lighting.
    pub hdri: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            display: DisplayMode::Material,
            show_tiles: false,
            wireframe: false,
            exposure: 0.0,
            env_rotation: 0.0,
            height_scale: 0.01,
            hdri: false,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct FrameUniform {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    bbox_min: [f32; 4],
    bbox_size: [f32; 4],
    viewport: [f32; 4],
    mode: [u32; 4],
    light: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct TonemapUniform {
    flags: [u32; 4],
    background_top: [f32; 4],
    background_bottom: [f32; 4],
}

pub struct GpuMesh {
    pub tiles: Vec<u32>,
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub edge_buffer: wgpu::Buffer,
    pub edge_count: u32,
    pub tile_map: wgpu::Buffer,
}

impl GpuMesh {
    pub fn new(gpu: &Gpu, mesh: &Mesh) -> Self {
        let d = &gpu.device;
        let edges = crate::mesh::edge_list(&mesh.indices, &mesh.vertices);
        let mut map = [u32::MAX; 100];
        for (layer, &udim) in mesh.tiles.iter().enumerate() {
            map[(udim - 1001) as usize] = layer as u32;
        }
        Self {
            tiles: mesh.tiles.clone(),
            vertex_buffer: d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("vertices"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE,
            }),
            index_buffer: d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::STORAGE,
            }),
            index_count: mesh.indices.len() as u32,
            edge_buffer: d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("edges"),
                contents: bytemuck::cast_slice(&edges),
                usage: wgpu::BufferUsages::INDEX,
            }),
            edge_count: edges.len() as u32,
            tile_map: d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("tile map"),
                contents: bytemuck::cast_slice(&map),
                usage: wgpu::BufferUsages::UNIFORM,
            }),
        }
    }
}

pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2, 4 => Uint32
    ];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<Vertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    }
}

struct Targets {
    width: u32,
    height: u32,
    hdr_msaa: wgpu::TextureView,
    hdr: wgpu::TextureView,
    depth: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    tonemap_bind: wgpu::BindGroup,
}

pub struct Viewport {
    frame_buffer: wgpu::Buffer,
    tonemap_buffer: wgpu::Buffer,
    frame_layout: wgpu::BindGroupLayout,
    tonemap_layout: wgpu::BindGroupLayout,
    tonemap_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: Option<wgpu::RenderPipeline>,
    line_pipeline: Option<wgpu::RenderPipeline>,
    frame_bind: Option<wgpu::BindGroup>,
    targets: Option<Targets>,
    /// Generated material code currently compiled into `mesh_pipeline`.
    material_code: String,
    /// Scene resources layout (group 1).
    scene_layout: wgpu::BindGroupLayout,
    env_view: wgpu::TextureView,
    env_sampler: wgpu::Sampler,
    tile_map: Option<wgpu::Buffer>,
}

pub const DEFAULT_MATERIAL_WGSL: &str = r#"
fn eval_material(s: Surface) -> Material {
    var m: Material;
    m.base_color = vec3f(0.5);
    m.roughness = 0.5;
    m.metallic = 0.0;
    m.height = 0.0;
    m.normal = vec3f(0.0, 0.0, 1.0);
    m.emissive = vec3f(0.0);
    m.opacity = 1.0;
    return m;
}
"#;

impl Viewport {
    pub fn new(gpu: &Gpu, scene_layout: &wgpu::BindGroupLayout) -> Self {
        let d = &gpu.device;
        let frag = wgpu::ShaderStages::FRAGMENT;
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let frame_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("frame"),
            entries: &[
                gpu::uniform_entry(0, vf),
                gpu::uniform_entry(1, vf),
                gpu::texture_entry(2, frag, wgpu::TextureViewDimension::D2, true),
                gpu::sampler_entry(3, frag),
            ],
        });
        let tonemap_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap"),
            entries: &[
                gpu::texture_entry(0, frag, wgpu::TextureViewDimension::D2, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: frag,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: true,
                    },
                    count: None,
                },
                gpu::uniform_entry(2, frag),
            ],
        });
        let module = gpu::shader(d, "tonemap", TONEMAP_WGSL);
        let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tonemap"),
            bind_group_layouts: &[Some(&tonemap_layout)],
            immediate_size: 0,
        });
        let tonemap_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tonemap"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(OUTPUT_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        Self {
            frame_buffer: d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("frame uniform"),
                size: std::mem::size_of::<FrameUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            tonemap_buffer: d.create_buffer(&wgpu::BufferDescriptor {
                label: Some("tonemap uniform"),
                size: std::mem::size_of::<TonemapUniform>() as u64,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }),
            frame_layout,
            tonemap_layout,
            tonemap_pipeline,
            mesh_pipeline: None,
            line_pipeline: None,
            frame_bind: None,
            targets: None,
            material_code: String::new(),
            scene_layout: scene_layout.clone(),
            env_view: env_texture(gpu, 1, 1, &[vec![0u16; 4]]).create_view(&Default::default()),
            env_sampler: d.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("environment"),
                address_mode_u: wgpu::AddressMode::Repeat,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: wgpu::FilterMode::Linear,
                min_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            tile_map: None,
        }
    }

    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.output_view)
    }

    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.output)
    }

    pub fn frame_layout(&self) -> &wgpu::BindGroupLayout {
        &self.frame_layout
    }

    /// Binds a mesh's tile map. Must be called when the mesh changes.
    pub fn set_mesh(&mut self, gpu: &Gpu, mesh: &GpuMesh) {
        self.tile_map = Some(mesh.tile_map.clone());
        self.rebind(gpu);
    }

    fn rebind(&mut self, gpu: &Gpu) {
        let Some(tile_map) = &self.tile_map else { return };
        self.frame_bind = Some(gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("frame"),
            layout: &self.frame_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.frame_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: tile_map.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.env_view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::Sampler(&self.env_sampler) },
            ],
        }));
    }

    /// Loads an equirectangular HDR/EXR environment (None restores the studio lighting).
    pub fn set_environment(&mut self, gpu: &Gpu, path: Option<&std::path::Path>) -> anyhow::Result<()> {
        let tex = match path {
            None => env_texture(gpu, 1, 1, &[vec![0u16; 4]]),
            Some(p) => {
                use anyhow::Context;
                let img = image::open(p).with_context(|| format!("Could not read {}", p.display()))?.to_rgba32f();
                // Keep the base level at most 2048 wide; build the mip chain by box filtering.
                let mut cur = if img.width() > 2048 {
                    image::imageops::resize(&img, 2048, 1024, image::imageops::FilterType::Triangle)
                } else {
                    img
                };
                let mut mips = Vec::new();
                loop {
                    mips.push((cur.width(), cur.height(), cur.as_raw().iter().map(|v| half::f16::from_f32(v.min(65000.0)).to_bits()).collect::<Vec<u16>>()));
                    if cur.width() <= 4 || cur.height() <= 2 {
                        break;
                    }
                    cur = image::imageops::resize(&cur, cur.width() / 2, cur.height() / 2, image::imageops::FilterType::Triangle);
                }
                let (w, h, _) = mips[0];
                let data: Vec<Vec<u16>> = mips.into_iter().map(|m| m.2).collect();
                env_texture(gpu, w, h, &data)
            }
        };
        self.env_view = tex.create_view(&Default::default());
        self.rebind(gpu);
        Ok(())
    }

    /// Recompiles the mesh pipeline if the generated material code changed.
    /// Returns the shader compilation error, if any; the previous pipeline stays in use.
    pub fn set_material(&mut self, gpu: &Gpu, code: &str) -> Result<bool, String> {
        if self.mesh_pipeline.is_some() && code == self.material_code {
            return Ok(false);
        }
        let source = VIEWPORT_WGSL
            .replace("//{{SURFACE}}", SURFACE_WGSL)
            .replace("//{{SCENE}}", crate::scene::WGSL)
            .replace("//{{MATERIAL}}", code);
        let d = &gpu.device;
        let scope = d.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = gpu::shader(d, "viewport", &source);
        let layout = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("viewport"),
            bind_group_layouts: &[Some(&self.frame_layout), Some(&self.scene_layout)],
            immediate_size: 0,
        });
        let depth = |write: bool, compare| {
            Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(write),
                depth_compare: Some(compare),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        let mesh_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport mesh"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: depth(true, wgpu::CompareFunction::Less),
            multisample: wgpu::MultisampleState { count: MSAA, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let line_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport wireframe"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_line"),
                compilation_options: Default::default(),
                buffers: &[Some(vertex_layout())],
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::LineList,
                ..Default::default()
            },
            depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
            multisample: wgpu::MultisampleState { count: MSAA, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_line"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(err.to_string());
        }
        self.mesh_pipeline = Some(mesh_pipeline);
        self.line_pipeline = Some(line_pipeline);
        self.material_code = code.to_owned();
        Ok(true)
    }

    fn ensure_targets(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if let Some(t) = &self.targets {
            if t.width == width && t.height == height {
                return;
            }
        }
        let d = &gpu.device;
        let hdr = gpu::render_target(d, "viewport hdr", width, height, HDR_FORMAT, wgpu::TextureUsages::empty());
        let msaa = |label, format| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: MSAA,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let hdr_msaa = msaa("viewport hdr msaa", HDR_FORMAT).create_view(&Default::default());
        let depth = msaa("viewport depth", DEPTH_FORMAT);
        let output = gpu::render_target(d, "viewport output", width, height, OUTPUT_FORMAT, wgpu::TextureUsages::COPY_SRC);
        let hdr = hdr.create_view(&Default::default());
        let depth = depth.create_view(&Default::default());
        let output_view = output.create_view(&Default::default());
        let tonemap_bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("tonemap"),
            layout: &self.tonemap_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&hdr) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&depth) },
                wgpu::BindGroupEntry { binding: 2, resource: self.tonemap_buffer.as_entire_binding() },
            ],
        });
        self.targets = Some(Targets { width, height, hdr_msaa, hdr, depth, output, output_view, tonemap_bind });
    }

    pub fn view_proj(camera: &Camera, mesh: &Mesh, width: u32, height: u32) -> Mat4 {
        camera.proj(width as f32 / height.max(1) as f32, mesh.radius().max(1e-3)) * camera.view()
    }

    #[allow(clippy::too_many_arguments)]
    pub fn render(
        &mut self,
        gpu: &Gpu,
        camera: &Camera,
        mesh: &Mesh,
        gpu_mesh: &GpuMesh,
        scene_bind: &wgpu::BindGroup,
        view: &ViewSettings,
        width: u32,
        height: u32,
    ) {
        let (width, height) = (width.max(1), height.max(1));
        self.ensure_targets(gpu, width, height);
        if self.mesh_pipeline.is_none() {
            self.set_material(gpu, DEFAULT_MATERIAL_WGSL).expect("default material compiles");
        }
        let view_proj = Self::view_proj(camera, mesh, width, height);
        let size = mesh.bbox_max - mesh.bbox_min;
        let (mode, index) = match view.display {
            DisplayMode::Material => (0, 0),
            DisplayMode::Channel(c) => (1, c),
            DisplayMode::MeshMap(m) => (2, m),
        };
        let key = (camera.right() * 0.5 + camera.up() * 0.9 - camera.forward() * 0.6).normalize();
        let uniform = FrameUniform {
            view_proj: view_proj.to_cols_array_2d(),
            eye: camera.eye().extend(1.0).into(),
            bbox_min: mesh.bbox_min.extend(0.0).into(),
            bbox_size: size.max(Vec3::splat(1e-6)).extend(0.0).into(),
            viewport: [width as f32, height as f32, view.exposure, view.env_rotation],
            mode: [mode, index, view.show_tiles as u32 | (view.hdri as u32) << 1, view.height_scale.to_bits()],
            light: [key.x, key.y, key.z, 0.35],
        };
        gpu.queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
        let tonemap = TonemapUniform {
            flags: [(view.display == DisplayMode::Material) as u32, 0, 0, 0],
            background_top: [0.30, 0.31, 0.33, 1.0],
            background_bottom: [0.16, 0.165, 0.175, 1.0],
        };
        gpu.queue.write_buffer(&self.tonemap_buffer, 0, bytemuck::bytes_of(&tonemap));

        let targets = self.targets.as_ref().unwrap();
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("viewport"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.hdr_msaa,
                    depth_slice: None,
                    resolve_target: Some(&targets.hdr),
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &targets.depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let Some(frame_bind) = &self.frame_bind {
                pass.set_bind_group(0, frame_bind, &[]);
                pass.set_bind_group(1, scene_bind, &[]);
                pass.set_vertex_buffer(0, gpu_mesh.vertex_buffer.slice(..));
                pass.set_pipeline(self.mesh_pipeline.as_ref().unwrap());
                pass.set_index_buffer(gpu_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..gpu_mesh.index_count, 0, 0..1);
                if view.wireframe {
                    pass.set_pipeline(self.line_pipeline.as_ref().unwrap());
                    pass.set_index_buffer(gpu_mesh.edge_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..gpu_mesh.edge_count, 0, 0..1);
                }
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("tonemap"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &targets.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.tonemap_pipeline);
            pass.set_bind_group(0, &targets.tonemap_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
    }
}

/// Rgba16Float texture with the given mip levels (u16 halves, RGBA).
fn env_texture(gpu: &Gpu, w: u32, h: u32, mips: &[Vec<u16>]) -> wgpu::Texture {
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("environment"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: mips.len() as u32,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    for (level, data) in mips.iter().enumerate() {
        let (mw, mh) = ((w >> level).max(1), (h >> level).max(1));
        gpu.queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: level as u32, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            bytemuck::cast_slice(data),
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(mw * 8), rows_per_image: Some(mh) },
            wgpu::Extent3d { width: mw, height: mh, depth_or_array_layers: 1 },
        );
    }
    tex
}
