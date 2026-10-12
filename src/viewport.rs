//! Viewport renderer: mesh shaded with the compiled material, then converted for the screen
//! by the view transform. See docs/reference/color-management.md.

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
const AGX_FIT_WGSL: &str = include_str!("shaders/agx_fit.wgsl");
const VIEW_TRANSFORM_WGSL: &str = include_str!("shaders/view_transform.wgsl");
/// Baked ACES output transforms (tools/view_transforms.py bake-aces).
const LUT_ACES13: &[u8] = include_bytes!("luts/aces13.lut.zst");
const LUT_ACES20: &[u8] = include_bytes!("luts/aces20.lut.zst");

/// How scene-linear values become display values. See docs/reference/color-management.md.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ViewTransform {
    Standard,
    #[default]
    AgX,
    Aces13,
    Aces20,
    PbrNeutral,
    Reinhard,
}

impl ViewTransform {
    pub const ALL: [ViewTransform; 6] =
        [Self::Standard, Self::AgX, Self::Aces13, Self::Aces20, Self::PbrNeutral, Self::Reinhard];

    pub fn name(self) -> &'static str {
        match self {
            Self::Standard => "Standard",
            Self::AgX => "AgX",
            Self::Aces13 => "ACES 1.3",
            Self::Aces20 => "ACES 2.0",
            Self::PbrNeutral => "Khronos PBR Neutral",
            Self::Reinhard => "Reinhard",
        }
    }

    /// Name in scripts and project files.
    pub fn key(self) -> &'static str {
        match self {
            Self::Standard => "standard",
            Self::AgX => "agx",
            Self::Aces13 => "aces-1.3",
            Self::Aces20 => "aces-2.0",
            Self::PbrNeutral => "pbr-neutral",
            Self::Reinhard => "reinhard",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|t| t.key().eq_ignore_ascii_case(key))
    }

    /// VT_* in view_transform.wgsl.
    fn shader_id(self) -> u32 {
        self as u32
    }
}
pub const SURFACE_WGSL: &str = include_str!("shaders/surface.wgsl");

/// What the viewport shows. See docs/reference/channels.md "Viewport channel display".
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
    pub view_transform: ViewTransform,
    pub env_rotation: f32,
    pub height_scale: f32,
    /// An HDRI environment is loaded and used for lighting.
    pub hdri: bool,
    /// Darken environment lighting with the baked AO map (docs/reference/viewport-lighting.md).
    pub ambient_occlusion: bool,
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            display: DisplayMode::Material,
            show_tiles: false,
            wireframe: false,
            exposure: 0.0,
            view_transform: ViewTransform::AgX,
            env_rotation: 0.0,
            height_scale: 0.01,
            hdri: false,
            ambient_occlusion: true,
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
    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3, 1 => Float32x3, 2 => Float32x4, 3 => Float32x2, 4 => Uint32, 5 => Uint32
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

/// Index buffers drawn over the mesh: Geo Fill's outlines (vertex pairs) and highlight
/// (triangles). See docs/reference/brushes-tools-stencils.md#outlines-and-highlight.
#[derive(Default)]
pub struct Overlay<'a> {
    pub borders: Option<(&'a wgpu::Buffer, u32)>,
    pub highlight: Option<(&'a wgpu::Buffer, u32)>,
}

pub struct Viewport {
    frame_buffer: wgpu::Buffer,
    tonemap_buffer: wgpu::Buffer,
    frame_layout: wgpu::BindGroupLayout,
    tonemap_layout: wgpu::BindGroupLayout,
    tonemap_pipeline: wgpu::RenderPipeline,
    mesh_pipeline: Option<wgpu::RenderPipeline>,
    line_pipeline: Option<wgpu::RenderPipeline>,
    /// Geo Fill outlines and highlight.
    border_pipeline: Option<wgpu::RenderPipeline>,
    highlight_pipeline: Option<wgpu::RenderPipeline>,
    /// UV view: the space between islands (docs/reference/2d-paint.md#display).
    gaps_pipeline: Option<wgpu::RenderPipeline>,
    frame_bind: Option<wgpu::BindGroup>,
    targets: Option<Targets>,
    /// Generated material code currently compiled into `mesh_pipeline`.
    material_code: String,
    /// Scene resources layout (group 1).
    scene_layout: wgpu::BindGroupLayout,
    env_view: wgpu::TextureView,
    /// Diffuse irradiance of the environment (cosine-convolved, divided by pi), equirectangular.
    irradiance_view: wgpu::TextureView,
    env_sampler: wgpu::Sampler,
    tile_map: Option<wgpu::Buffer>,
    lut_aces13: wgpu::TextureView,
    lut_aces20: wgpu::TextureView,
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
                gpu::texture_entry(4, frag, wgpu::TextureViewDimension::D2, true),
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
                gpu::texture_entry(3, frag, wgpu::TextureViewDimension::D3, false),
                gpu::texture_entry(4, frag, wgpu::TextureViewDimension::D3, false),
            ],
        });
        let source = format!("{AGX_FIT_WGSL}
{VIEW_TRANSFORM_WGSL}
{TONEMAP_WGSL}");
        let module = gpu::shader(d, "tonemap", &source);
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
            border_pipeline: None,
            highlight_pipeline: None,
            gaps_pipeline: None,
            frame_bind: None,
            targets: None,
            material_code: String::new(),
            scene_layout: scene_layout.clone(),
            env_view: env_texture(gpu, 1, 1, &[vec![0u16; 4]]).create_view(&Default::default()),
            irradiance_view: env_texture(gpu, 1, 1, &[vec![0u16; 4]]).create_view(&Default::default()),
            lut_aces13: lut_texture(gpu, "ACES 1.3", LUT_ACES13).create_view(&Default::default()),
            lut_aces20: lut_texture(gpu, "ACES 2.0", LUT_ACES20).create_view(&Default::default()),
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
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.irradiance_view) },
            ],
        }));
    }

    /// Loads an equirectangular HDR/EXR environment (None restores the studio lighting).
    pub fn set_environment(&mut self, gpu: &Gpu, path: Option<&std::path::Path>) -> anyhow::Result<()> {
        let to_f16 = |v: &[f32]| v.iter().map(|v| half::f16::from_f32(v.min(65000.0)).to_bits()).collect::<Vec<u16>>();
        let mut irradiance = env_texture(gpu, 1, 1, &[vec![0u16; 4]]);
        let tex = match path {
            None => env_texture(gpu, 1, 1, &[vec![0u16; 4]]),
            Some(p) => {
                use anyhow::Context;
                let img = image::open(p).with_context(|| format!("Could not read {}", p.display()))?.to_rgba32f();
                // Keep the base level at most 2048 wide; build the mip chain by box filtering.
                // (Not image::imageops::resize: it clamps float pixels to 1, clipping every light.)
                let mut cur = (img.width(), img.height(), img.into_raw());
                while cur.0 > 2048 {
                    cur = halve(cur.0, cur.1, &cur.2);
                }
                let mut mips = Vec::new();
                loop {
                    mips.push((cur.0, cur.1, to_f16(&cur.2)));
                    // Irradiance is smooth, so a small source suffices; box filtering keeps small, bright lights' energy.
                    if cur.0 <= IRRADIANCE_SOURCE_WIDTH && irradiance.width() == 1 {
                        let (w, h, data) = irradiance_map(cur.0, cur.1, &cur.2);
                        irradiance = env_texture(gpu, w, h, &[to_f16(&data)]);
                    }
                    if cur.0 <= 4 || cur.1 <= 2 {
                        break;
                    }
                    cur = halve(cur.0, cur.1, &cur.2);
                }
                let (w, h, _) = mips[0];
                let levels = SPECULAR_LEVELS.min(mips.len() as u32);
                let data: Vec<Vec<u16>> = mips.into_iter().map(|m| m.2).collect();
                prefilter_environment(gpu, &env_texture(gpu, w, h, &data), &self.env_sampler, levels)
            }
        };
        self.env_view = tex.create_view(&Default::default());
        self.irradiance_view = irradiance.create_view(&Default::default());
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
        // Drawn over the mesh with a small depth offset, hidden where the mesh covers them.
        let overlay = |label: &str, topology, fs: &str| {
            d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_line"),
                    compilation_options: Default::default(),
                    buffers: &[Some(vertex_layout())],
                },
                primitive: wgpu::PrimitiveState { topology, cull_mode: None, ..Default::default() },
                depth_stencil: depth(false, wgpu::CompareFunction::LessEqual),
                multisample: wgpu::MultisampleState { count: MSAA, ..Default::default() },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: HDR_FORMAT,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let line_pipeline = overlay("viewport wireframe", wgpu::PrimitiveTopology::LineList, "fs_line");
        let border_pipeline = overlay("geo fill outlines", wgpu::PrimitiveTopology::LineList, "fs_border");
        let highlight_pipeline = overlay("geo fill highlight", wgpu::PrimitiveTopology::TriangleList, "fs_highlight");
        let gaps_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("viewport between islands"),
            layout: Some(&layout),
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_gaps"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: depth(true, wgpu::CompareFunction::Less),
            multisample: wgpu::MultisampleState { count: MSAA, ..Default::default() },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_gaps"),
                compilation_options: Default::default(),
                targets: &[Some(HDR_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        if let Some(err) = pollster::block_on(scope.pop()) {
            return Err(err.to_string());
        }
        self.mesh_pipeline = Some(mesh_pipeline);
        self.line_pipeline = Some(line_pipeline);
        self.border_pipeline = Some(border_pipeline);
        self.highlight_pipeline = Some(highlight_pipeline);
        self.gaps_pipeline = Some(gaps_pipeline);
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
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&self.lut_aces13) },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&self.lut_aces20) },
            ],
        });
        self.targets = Some(Targets { width, height, hdr_msaa, hdr, depth, output, output_view, tonemap_bind });
    }

    pub fn view_proj(camera: &Camera, mesh: &Mesh, width: u32, height: u32) -> Mat4 {
        camera.proj(width as f32 / height.max(1) as f32, mesh.radius().max(1e-3)) * camera.view()
    }

    #[allow(clippy::too_many_arguments)]
    /// Renders `view`, with `overlay` over the mesh: the 3D camera, or the mesh laid out at its UVs (docs/reference/uv-view.md).
    pub fn render(
        &mut self,
        gpu: &Gpu,
        view_of: &crate::camera::View,
        mesh: &Mesh,
        gpu_mesh: &GpuMesh,
        scene_bind: &wgpu::BindGroup,
        view: &ViewSettings,
        overlay: &Overlay,
        width: u32,
        height: u32,
    ) {
        let (width, height) = (width.max(1), height.max(1));
        self.ensure_targets(gpu, width, height);
        if self.mesh_pipeline.is_none() {
            self.set_material(gpu, DEFAULT_MATERIAL_WGSL).expect("default material compiles");
        }
        let uv = matches!(view_of, crate::camera::View::Uv(_));
        let (view_proj, eye, key) = match view_of {
            crate::camera::View::Persp(camera) => (
                Self::view_proj(camera, mesh, width, height),
                camera.eye().extend(1.0),
                (camera.right() * 0.5 + camera.up() * 0.9 - camera.forward() * 0.6).normalize(),
            ),
            crate::camera::View::Uv(v) => (v.view_proj(glam::Vec2::new(width as f32, height as f32)), glam::Vec4::ZERO, Vec3::new(0.3, 0.9, 0.3).normalize()),
        };
        let size = mesh.bbox_max - mesh.bbox_min;
        let (mode, index) = match view.display {
            DisplayMode::Material => (0, 0),
            DisplayMode::Channel(c) => (1, c),
            DisplayMode::MeshMap(m) => (2, m),
        };
        let uniform = FrameUniform {
            view_proj: view_proj.to_cols_array_2d(),
            eye: eye.into(),
            bbox_min: mesh.bbox_min.extend(0.0).into(),
            bbox_size: size.max(Vec3::splat(1e-6)).extend(0.0).into(),
            viewport: [width as f32, height as f32, view.exposure, view.env_rotation],
            // The tile grid toggle is the 3D view's; the UV view draws its own grid.
            mode: [
                mode,
                index,
                (view.show_tiles && !uv) as u32 | (view.hdri as u32) << 1 | (view.ambient_occlusion as u32) << 2 | (uv as u32) << 3,
                view.height_scale.to_bits(),
            ],
            light: [key.x, key.y, key.z, 0.35],
        };
        gpu.queue.write_buffer(&self.frame_buffer, 0, bytemuck::bytes_of(&uniform));
        let tonemap = TonemapUniform {
            flags: [(view.display == DisplayMode::Material) as u32, view.view_transform.shader_id(), 0, 0],
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
                // Then whatever the mesh left uncovered: the depth test skips the islands, so
                // the material is only evaluated between them.
                if matches!(view_of, crate::camera::View::Uv(v) if v.gaps) {
                    pass.set_pipeline(self.gaps_pipeline.as_ref().unwrap());
                    pass.draw(0..3, 0..1);
                }
                // The UV view always shows its edges.
                if view.wireframe || uv {
                    pass.set_pipeline(self.line_pipeline.as_ref().unwrap());
                    pass.set_index_buffer(gpu_mesh.edge_buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..gpu_mesh.edge_count, 0, 0..1);
                }
                if let Some((buffer, count)) = overlay.highlight {
                    pass.set_pipeline(self.highlight_pipeline.as_ref().unwrap());
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..count, 0, 0..1);
                }
                if let Some((buffer, count)) = overlay.borders {
                    pass.set_pipeline(self.border_pipeline.as_ref().unwrap());
                    pass.set_index_buffer(buffer.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..count, 0, 0..1);
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

/// Unpacks a baked view transform LUT: (size N, N³ RGBA halves, red fastest). The format is
/// described in tools/view_transforms.py.
fn unpack_lut(compressed: &[u8]) -> anyhow::Result<(u32, Vec<u16>)> {
    use std::io::Read;
    let mut src = compressed;
    let mut dec = ruzstd::decoding::StreamingDecoder::new(&mut src).map_err(|e| anyhow::anyhow!("{e}"))?;
    let mut data = Vec::new();
    dec.read_to_end(&mut data)?;
    anyhow::ensure!(data.len() >= 8 && &data[0..4] == b"CLUT", "not a view transform LUT");
    let n = u32::from_le_bytes(data[4..8].try_into()?);
    let texels = (n as usize).pow(3);
    anyhow::ensure!(data.len() == 8 + texels * 6, "LUT size doesn't match its header");
    let one = half::f16::from_f32(1.0).to_bits();
    let rgba = data[8..]
        .as_chunks::<6>().0.iter()
        .flat_map(|c| [u16::from_le_bytes([c[0], c[1]]), u16::from_le_bytes([c[2], c[3]]), u16::from_le_bytes([c[4], c[5]]), one])
        .collect();
    Ok((n, rgba))
}

/// A 3D texture from a baked LUT embedded in the binary.
fn lut_texture(gpu: &Gpu, label: &str, compressed: &[u8]) -> wgpu::Texture {
    let (n, rgba) = unpack_lut(compressed).unwrap_or_else(|e| panic!("embedded {label} LUT: {e}"));
    let size = wgpu::Extent3d { width: n, height: n, depth_or_array_layers: n };
    let tex = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D3,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    gpu.queue.write_texture(
        wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
        bytemuck::cast_slice(&rgba),
        wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(n * 8), rows_per_image: Some(n) },
        size,
    );
    tex
}

/// Halves an RGBA float image with a 2x2 box filter, keeping values above 1. An odd last
/// row or column is folded into its neighbor.
fn halve(w: u32, h: u32, src: &[f32]) -> (u32, u32, Vec<f32>) {
    let (ow, oh) = ((w / 2).max(1), (h / 2).max(1));
    let mut out = vec![0.0f32; (ow * oh * 4) as usize];
    for y in 0..h {
        let oy = (y / 2).min(oh - 1);
        for x in 0..w {
            let ox = (x / 2).min(ow - 1);
            let (si, oi) = (((y * w + x) * 4) as usize, ((oy * ow + ox) * 4) as usize);
            for c in 0..4 {
                out[oi + c] += src[si + c];
            }
        }
    }
    // Each output texel's share of source texels (more at a folded odd edge).
    for oy in 0..oh {
        let ny = if oy == oh - 1 { h - 2 * oy } else { 2 }.min(h);
        for ox in 0..ow {
            let nx = if ox == ow - 1 { w - 2 * ox } else { 2 }.min(w);
            let oi = ((oy * ow + ox) * 4) as usize;
            for v in &mut out[oi..oi + 4] {
                *v /= (nx * ny) as f32;
            }
        }
    }
    (ow, oh, out)
}

const IRRADIANCE_SOURCE_WIDTH: u32 = 128;
const IRRADIANCE_WIDTH: u32 = 32;

/// Direction of an equirectangular texel center, matching `equirect_uv` in viewport.wgsl.
fn equirect_dir(x: u32, y: u32, w: u32, h: u32) -> [f32; 3] {
    let phi = ((x as f32 + 0.5) / w as f32 - 0.5) * std::f32::consts::TAU;
    let theta = (y as f32 + 0.5) / h as f32 * std::f32::consts::PI;
    [phi.sin() * theta.sin(), theta.cos(), -phi.cos() * theta.sin()]
}

/// Cosine-convolves an equirectangular RGBA environment into a small irradiance map holding
/// E(n) / pi, so Lambertian diffuse is `albedo * map(n)`.
fn irradiance_map(sw: u32, sh: u32, src: &[f32]) -> (u32, u32, Vec<f32>) {
    // Each source texel as a direction weighted by its solid angle.
    let texel = std::f32::consts::TAU / sw as f32 * std::f32::consts::PI / sh as f32;
    let source: Vec<([f32; 3], [f32; 3])> = (0..sh)
        .flat_map(|y| (0..sw).map(move |x| (x, y)))
        .map(|(x, y)| {
            let d = equirect_dir(x, y, sw, sh);
            let i = ((y * sw + x) * 4) as usize;
            let weight = texel * (1.0 - d[1] * d[1]).max(0.0).sqrt() / std::f32::consts::PI;
            (d, [src[i] * weight, src[i + 1] * weight, src[i + 2] * weight])
        })
        .collect();
    let (w, h) = (IRRADIANCE_WIDTH, IRRADIANCE_WIDTH / 2);
    let mut out = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let n = equirect_dir(x, y, w, h);
            let mut e = [0.0f32; 3];
            for (d, l) in &source {
                let c = n[0] * d[0] + n[1] * d[1] + n[2] * d[2];
                if c > 0.0 {
                    e[0] += l[0] * c;
                    e[1] += l[1] * c;
                    e[2] += l[2] * c;
                }
            }
            out.extend_from_slice(&[e[0], e[1], e[2], 1.0]);
        }
    }
    (w, h, out)
}

/// Mip levels of the specular environment; level i holds GGX roughness i / (levels - 1).
const SPECULAR_LEVELS: u32 = 7;
const SPECULAR_SAMPLES: u32 = 512;

/// Convolves a box-filtered environment mip chain with the GGX lobe into a new texture whose
/// mip level i is prefiltered for roughness i / (levels - 1), read by `env_sample` in viewport.wgsl.
fn prefilter_environment(gpu: &Gpu, source: &wgpu::Texture, sampler: &wgpu::Sampler, levels: u32) -> wgpu::Texture {
    let d = &gpu.device;
    let (w, h) = (source.width(), source.height());
    let tex = d.create_texture(&wgpu::TextureDescriptor {
        label: Some("specular environment"),
        size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
        mip_level_count: levels,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::STORAGE_BINDING,
        view_formats: &[],
    });
    let module = gpu::shader(d, "environment prefilter", include_str!("shaders/env_prefilter.wgsl"));
    let pipeline = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some("environment prefilter"),
        layout: None,
        module: &module,
        entry_point: Some("prefilter"),
        compilation_options: Default::default(),
        cache: None,
    });
    let source_view = source.create_view(&Default::default());
    // One submit per level keeps each GPU batch short (the largest is ~270M samples).
    for level in 0..levels {
        let (lw, lh) = ((w >> level).max(1), (h >> level).max(1));
        let roughness = if levels > 1 { level as f32 / (levels - 1) as f32 } else { 0.0 };
        let mut params = Vec::with_capacity(16);
        for word in [lw, lh, roughness.to_bits(), SPECULAR_SAMPLES] {
            params.extend_from_slice(&word.to_le_bytes());
        }
        let buffer = d.create_buffer(&wgpu::BufferDescriptor {
            label: Some("environment prefilter"),
            size: params.len() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&buffer, 0, &params);
        let target = tex.create_view(&wgpu::TextureViewDescriptor {
            base_mip_level: level,
            mip_level_count: Some(1),
            ..Default::default()
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("environment prefilter"),
            layout: &pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&source_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(sampler) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&target) },
            ],
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(lw.div_ceil(8), lh.div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
    }
    tex
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_luts_unpack() {
        for lut in [LUT_ACES13, LUT_ACES20] {
            let (n, rgba) = unpack_lut(lut).unwrap();
            assert_eq!(n, 65);
            assert_eq!(rgba.len(), 65usize.pow(3) * 4);
            // Black maps to black, and the brightest corner to (nearly) white.
            let px = |i: usize| [0, 1, 2].map(|k| half::f16::from_bits(rgba[i * 4 + k]).to_f32());
            assert!(px(0).iter().all(|v| *v < 0.01), "{:?}", px(0));
            assert!(px(65usize.pow(3) - 1).iter().all(|v| *v > 0.9), "{:?}", px(65usize.pow(3) - 1));
        }
    }

    #[test]
    fn halving_keeps_bright_values() {
        // 3x2: a bright texel must survive (no clamping), and the odd column folds in.
        let src: Vec<f32> = [50.0, 1.0, 3.0, 1.0, 1.0, 3.0].iter().flat_map(|v| [*v, *v, *v, 1.0]).collect();
        let (w, h, out) = halve(3, 2, &src);
        assert_eq!((w, h), (1, 1));
        assert!((out[0] - 59.0 / 6.0).abs() < 1e-4, "{out:?}");
        assert_eq!(out[3], 1.0);
    }

    #[test]
    fn irradiance_of_uniform_environment_is_its_radiance() {
        // A uniform environment of radiance L gives E = pi * L in every direction.
        let (w, h) = (64, 32);
        let src: Vec<f32> = (0..w * h).flat_map(|_| [2.0, 1.0, 0.5, 1.0]).collect();
        let (_, _, out) = irradiance_map(w, h, &src);
        for px in out.as_chunks::<4>().0 {
            for (got, want) in px[..3].iter().zip([2.0, 1.0, 0.5]) {
                assert!((got - want).abs() < want * 0.02, "{px:?}");
            }
        }
    }

    #[test]
    fn view_transform_keys_round_trip() {
        for t in ViewTransform::ALL {
            assert_eq!(ViewTransform::from_key(t.key()), Some(t));
        }
        assert_eq!(ViewTransform::default(), ViewTransform::AgX);
        assert_eq!(ViewTransform::from_key("filmic"), None);
    }
}
