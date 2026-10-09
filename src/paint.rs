//! Paint buffer, brush strokes and projection baking.
//! See Painter-docs/reference/paint-buffer.md, brushes-tools-stencils.md and
//! Painter-docs/explanation/about-the-paint-buffer.md.
//!
//! Strokes draw dabs into a screen-space buffer. Baking projects the buffer through the
//! bake camera into the target's tiles. Before a bake the target slot is copied to a
//! scratch "base", and the bake writes base + buffer into the target. Live mode re-bakes
//! the whole stroke every frame from that base, so a stroke painted live and the same
//! stroke baked manually produce identical texels.

use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

use crate::camera::Camera;
use crate::channels::Channel;
use crate::gpu::{self, Gpu};
use crate::graph::NodeId;
use crate::ops;
use crate::project::Project;
use crate::scene::{PAINT_FORMAT, PAINT_PLANES};
use crate::viewport::vertex_layout;

pub const PAGE: u32 = 128;
const MAX_DABS: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tool {
    Paint,
    Erase,
    PaintThrough,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BakeMode {
    Live,
    Manual,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OnCameraMove {
    Bake,
    Clear,
    LockCamera,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SizeSpace {
    Screen,
    World,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Brush {
    pub size: f32,
    pub size_space: SizeSpace,
    pub hardness: f32,
    pub flow: f32,
    pub opacity: f32,
    /// Percent of size.
    pub spacing: f32,
    /// Image name in the image pool ("" = round tip).
    pub alpha: String,
    /// Degrees.
    pub angle: f32,
    pub follow_stroke: bool,
    pub jitter_size: f32,
    pub jitter_angle: f32,
    pub jitter_position: f32,
    pub jitter_flow: f32,
    pub pressure_size: bool,
    pub pressure_flow: bool,
    pub pressure_opacity: bool,
}

impl Default for Brush {
    fn default() -> Self {
        Self {
            size: 50.0,
            size_space: SizeSpace::Screen,
            hardness: 0.8,
            flow: 1.0,
            opacity: 1.0,
            spacing: 10.0,
            alpha: String::new(),
            angle: 0.0,
            follow_stroke: false,
            jitter_size: 0.0,
            jitter_angle: 0.0,
            jitter_position: 0.0,
            jitter_flow: 0.0,
            pressure_size: true,
            pressure_flow: false,
            pressure_opacity: false,
        }
    }
}

/// Per-channel values the brush paints. Colors are linear.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct BrushValues {
    pub enabled: [bool; 7],
    pub values: [[f32; 3]; 7],
    /// Value painted into masks.
    pub mask_value: f32,
}

impl Default for BrushValues {
    fn default() -> Self {
        let mut values = Channel::ALL.map(Channel::default_value);
        values[Channel::BaseColor.index()] = [0.8, 0.8, 0.8];
        Self {
            enabled: Channel::ALL.map(|c| matches!(c, Channel::BaseColor | Channel::Roughness | Channel::Metallic)),
            values,
            mask_value: 1.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct ProjectionSettings {
    pub backface_culling: bool,
    /// Degrees.
    pub falloff_start: f32,
    pub falloff_end: f32,
    pub depth_tolerance: f32,
    pub project_through: bool,
}

impl Default for ProjectionSettings {
    fn default() -> Self {
        Self { backface_culling: true, falloff_start: 60.0, falloff_end: 85.0, depth_tolerance: 0.001, project_through: false }
    }
}

/// 2D transform of the buffer's contents relative to the viewport, about its center.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Transform2 {
    pub offset: Vec2,
    pub rotation: f32,
    pub scale: f32,
}

impl Default for Transform2 {
    fn default() -> Self {
        Self { offset: Vec2::ZERO, rotation: 0.0, scale: 1.0 }
    }
}

impl Transform2 {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Maps a point of the untransformed layer to the screen.
    pub fn apply(&self, p: Vec2, center: Vec2) -> Vec2 {
        let (s, c) = self.rotation.sin_cos();
        let d = (p - center) * self.scale;
        center + self.offset + Vec2::new(c * d.x - s * d.y, s * d.x + c * d.y)
    }

    /// Rows of the 2x3 affine mapping screen points back to the untransformed layer.
    pub fn inverse_rows(&self, center: Vec2) -> ([f32; 4], [f32; 4]) {
        let (s, c) = self.rotation.sin_cos();
        let k = 1.0 / self.scale.max(1e-4);
        // p = center + R^-1 (q - center - offset) / scale
        let r00 = c * k;
        let r01 = s * k;
        let r10 = -s * k;
        let r11 = c * k;
        let t = -(center + self.offset);
        let tx = r00 * t.x + r01 * t.y + center.x;
        let ty = r10 * t.x + r11 * t.y + center.y;
        ([r00, r01, tx, 0.0], [r10, r11, ty, 0.0])
    }
}

/// What a dab samples in Paint Through mode.
#[derive(Clone, Copy, Debug)]
pub struct StencilSample {
    pub layer: i32,
    /// Screen px -> stencil uv.
    pub row0: [f32; 4],
    pub row1: [f32; 4],
    pub target_mask: u32,
    pub use_alpha: bool,
    pub tile: bool,
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Dab {
    geom: [f32; 4],
    params: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct DabParams {
    rect: [u32; 4],
    mode: [i32; 4],
    opts: [f32; 4],
    values: [[f32; 4]; 7],
    stencil_row0: [f32; 4],
    stencil_row1: [f32; 4],
    stencil_info: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BakeParams {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    screen: [f32; 4],
    inv_row0: [f32; 4],
    inv_row1: [f32; 4],
    proj: [f32; 4],
    target: [u32; 4],
}

/// The camera a buffer belongs to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BakeCamera {
    pub camera: Camera,
    pub view_proj: Mat4,
    pub screen: (u32, u32),
}

/// Paint target: a Paint node (material pool) or Paint Mask node (mask pool).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Target {
    pub node: NodeId,
    pub mask: bool,
    pub slot: u32,
}

/// Saved page contents for undo: one entry per (pool layer, page x, page y), with the data
/// of every plane.
#[derive(Clone)]
pub struct PaintPages {
    pub mask: bool,
    pub pages: Vec<(u32, u32, u32)>,
    pub data: Vec<Vec<u8>>,
}

struct Stroke {
    target: Target,
    last: Vec2,
    /// Distance travelled since the last dab, in screen px.
    carry: f32,
    pending: Vec<Dab>,
    rng: u64,
    pressure_opacity: f32,
}

struct Buffers {
    screen: (u32, u32),
    size: (u32, u32),
    planes: Vec<wgpu::Texture>,
    erase: wgpu::Texture,
    stroke: wgpu::Texture,
    depth_color: wgpu::Texture,
    depth_ds: wgpu::TextureView,
    /// Linear depth readback for world-size brushes and picking.
    depth_cpu: Option<Vec<f32>>,
}

pub struct PaintEngine {
    pub tool: Tool,
    pub brush: Brush,
    pub values: BrushValues,
    pub mode: BakeMode,
    pub on_camera_move: OnCameraMove,
    pub projection: ProjectionSettings,
    /// 0.5, 1 or 2.
    pub buffer_scale: f32,
    pub show_buffer: bool,
    pub transform: Transform2,
    /// Manual mode: the buffer holds paint that has not been baked.
    pub unbaked: bool,
    pub bake_camera: Option<BakeCamera>,
    stroke: Option<Stroke>,
    buffers: Option<Buffers>,
    scratch: Option<(Vec<wgpu::Texture>, u32, u32)>,
    dirty: Option<(wgpu::Buffer, u32)>,
    dab_pipeline: wgpu::ComputePipeline,
    bake_pipeline: wgpu::ComputePipeline,
    depth_pipeline: wgpu::RenderPipeline,
    depth_layout: wgpu::BindGroupLayout,
    overlay_pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    dummy_out: Vec<wgpu::Texture>,
    dummy_alpha: wgpu::Texture,
}

impl PaintEngine {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let compute = |src: &str, label: &str| {
            let module = gpu::shader(d, label, src);
            d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let dab_pipeline = compute(include_str!("shaders/dab.wgsl"), "dab");
        let bake_pipeline = compute(include_str!("shaders/paintbake.wgsl"), "paint bake");

        let depth_module = gpu::shader(d, "view depth", include_str!("shaders/viewdepth.wgsl"));
        let depth_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("view depth"),
            entries: &[gpu::uniform_entry(0, wgpu::ShaderStages::VERTEX)],
        });
        let pl = d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("view depth"),
            bind_group_layouts: &[Some(&depth_layout)],
            immediate_size: 0,
        });
        let mut vl = vertex_layout();
        vl.attributes = &vl.attributes[..1];
        let depth_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("view depth"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &depth_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[Some(vl)],
            },
            primitive: Default::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &depth_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::R32Float.into())],
            }),
            multiview_mask: None,
            cache: None,
        });

        let overlay_module = gpu::shader(d, "paint overlay", include_str!("shaders/overlay.wgsl"));
        let overlay_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("paint overlay"),
            layout: None,
            vertex: wgpu::VertexState {
                module: &overlay_module,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &overlay_module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: crate::viewport::OUTPUT_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });

        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("paint sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let dummy_out = (0..4).map(|_| array_texture(gpu, "dummy out", 1, 1)).collect();
        let dummy_alpha = d.create_texture(&wgpu::TextureDescriptor {
            label: Some("dummy alpha"),
            size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: crate::scene::IMAGE_FORMAT,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        Self {
            tool: Tool::Paint,
            brush: Brush::default(),
            values: BrushValues::default(),
            mode: BakeMode::Live,
            on_camera_move: OnCameraMove::Bake,
            projection: ProjectionSettings::default(),
            buffer_scale: 1.0,
            show_buffer: true,
            transform: Transform2::default(),
            unbaked: false,
            bake_camera: None,
            stroke: None,
            buffers: None,
            scratch: None,
            dirty: None,
            dab_pipeline,
            bake_pipeline,
            depth_pipeline,
            depth_layout,
            overlay_pipeline,
            sampler,
            dummy_out,
            dummy_alpha,
        }
    }

    pub fn stroking(&self) -> bool {
        self.stroke.is_some()
    }

    fn buffer_size(&self, screen: (u32, u32)) -> (u32, u32) {
        (
            ((screen.0 as f32 * self.buffer_scale).round() as u32).max(1),
            ((screen.1 as f32 * self.buffer_scale).round() as u32).max(1),
        )
    }

    fn ensure_buffers(&mut self, gpu: &Gpu, screen: (u32, u32)) {
        let size = self.buffer_size(screen);
        if self.buffers.as_ref().is_some_and(|b| b.size == size && b.screen == screen) {
            return;
        }
        let d = &gpu.device;
        let plane = |label| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: PAINT_FORMAT,
                usage: wgpu::TextureUsages::TEXTURE_BINDING
                    | wgpu::TextureUsages::STORAGE_BINDING
                    | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        };
        let depth_color = gpu::render_target(
            d,
            "bake depth",
            size.0,
            size.1,
            wgpu::TextureFormat::R32Float,
            wgpu::TextureUsages::COPY_SRC,
        );
        let depth_ds = gpu::render_target(
            d,
            "bake depth ds",
            size.0,
            size.1,
            wgpu::TextureFormat::Depth32Float,
            wgpu::TextureUsages::empty(),
        )
        .create_view(&Default::default());
        self.buffers = Some(Buffers {
            screen,
            size,
            planes: (0..PAINT_PLANES).map(|_| plane("paint buffer")).collect(),
            erase: plane("paint buffer erase"),
            stroke: plane("paint stroke"),
            depth_color,
            depth_ds,
            depth_cpu: None,
        });
        // A resized buffer has lost its contents.
        self.unbaked = false;
        self.bake_camera = None;
    }

    fn clear_textures(&self, gpu: &Gpu, textures: &[&wgpu::Texture]) {
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        for t in textures {
            enc.clear_texture(t, &Default::default());
        }
        gpu.queue.submit([enc.finish()]);
    }

    pub fn clear_buffer(&mut self, gpu: &Gpu) {
        if let Some(b) = &self.buffers {
            let mut all: Vec<&wgpu::Texture> = b.planes.iter().collect();
            all.push(&b.erase);
            all.push(&b.stroke);
            self.clear_textures(gpu, &all);
        }
        self.unbaked = false;
        self.transform = Transform2::default();
    }

    /// Renders the bake camera's depth into the buffer-sized depth texture.
    fn render_depth(&mut self, gpu: &Gpu, p: &Project, bc: &BakeCamera) {
        let b = self.buffers.as_mut().unwrap();
        let d = &gpu.device;
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("depth params"),
            contents: bytemuck::cast_slice(&bc.view_proj.to_cols_array()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("view depth"),
            layout: &self.depth_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
        });
        let view = b.depth_color.create_view(&Default::default());
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("view depth"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 1e30, g: 0.0, b: 0.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &b.depth_ds,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.depth_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_vertex_buffer(0, p.gpu_mesh.vertex_buffer.slice(..));
            pass.set_index_buffer(p.gpu_mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
            pass.draw_indexed(0..p.gpu_mesh.index_count, 0, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
        b.depth_cpu = None;
    }

    /// Linear depth at a screen point (for world-size brushes and picking).
    pub fn depth_at(&mut self, gpu: &Gpu, screen_px: Vec2) -> Option<f32> {
        let b = self.buffers.as_mut()?;
        if b.depth_cpu.is_none() {
            let bytes = gpu.read_texture(&b.depth_color, 0, 4);
            b.depth_cpu = Some(bytemuck::cast_slice(&bytes).to_vec());
        }
        let k = b.size.0 as f32 / b.screen.0.max(1) as f32;
        let x = (screen_px.x * k) as i64;
        let y = (screen_px.y * k) as i64;
        if x < 0 || y < 0 || x >= b.size.0 as i64 || y >= b.size.1 as i64 {
            return None;
        }
        let v = b.depth_cpu.as_ref()?[(y as u32 * b.size.0 + x as u32) as usize];
        (v < 1e29).then_some(v)
    }

    /// Sets the bake camera, rendering a new depth pass when it changed.
    fn set_bake_camera(&mut self, gpu: &Gpu, p: &Project, camera: &Camera, screen: (u32, u32)) {
        let view_proj = crate::viewport::Viewport::view_proj(camera, &p.mesh, screen.0, screen.1);
        let bc = BakeCamera { camera: *camera, view_proj, screen };
        if self.bake_camera != Some(bc) {
            self.render_depth(gpu, p, &bc);
            self.bake_camera = Some(bc);
        }
    }

    /// World position under a screen point, for Shift+F. Uses the bake depth pass, so it
    /// is unavailable while Manual-mode paint is unbaked.
    pub fn pick(&mut self, gpu: &Gpu, p: &Project, camera: &Camera, screen: (u32, u32), pos: Vec2) -> Option<Vec3> {
        if self.unbaked || self.stroke.is_some() {
            return None;
        }
        self.ensure_buffers(gpu, screen);
        self.set_bake_camera(gpu, p, camera, screen);
        let depth = self.depth_at(gpu, pos)?;
        let ndc = Vec2::new(pos.x / screen.0 as f32 * 2.0 - 1.0, 1.0 - pos.y / screen.1 as f32 * 2.0);
        let aspect = screen.0 as f32 / screen.1.max(1) as f32;
        let t = (camera.fov_y * 0.5).tan();
        let dir = (camera.forward() + camera.right() * ndc.x * t * aspect + camera.up() * ndc.y * t).normalize();
        // depth is view-space distance along the forward axis.
        Some(camera.eye() + dir * (depth / dir.dot(camera.forward())))
    }

    /// Allocates a pool slot for the target node on first use.
    pub fn target(&self, gpu: &Gpu, p: &mut Project) -> Result<Target, String> {
        let sel = p.selection.ok_or("No layer selected.")?;
        let node = ops::paint_target(&p.graph, sel).ok_or("Select a paint layer or paint mask to paint.")?;
        let mask = p.graph.node(node).ty == "paint_mask";
        let slot = p.paint_slot(gpu, node);
        Ok(Target { node, mask, slot })
    }

    /// Starts a stroke at `pos` (screen pixels). In Manual mode with unbaked paint the
    /// buffer keeps its camera.
    pub fn begin_stroke(
        &mut self,
        gpu: &Gpu,
        p: &mut Project,
        camera: &Camera,
        screen: (u32, u32),
        pos: Vec2,
        pressure: f32,
    ) -> Result<(), String> {
        let target = self.target(gpu, p)?;
        self.ensure_buffers(gpu, screen);
        if !(self.mode == BakeMode::Manual && self.unbaked) {
            self.set_bake_camera(gpu, p, camera, screen);
        }
        let b = self.buffers.as_ref().unwrap();
        self.clear_textures(gpu, &[&b.stroke]);
        if self.mode == BakeMode::Live {
            self.prepare_bake(gpu, p, target);
        }
        let pressure_opacity = if self.brush.pressure_opacity { pressure } else { 1.0 };
        self.stroke = Some(Stroke {
            target,
            last: pos,
            carry: 0.0,
            pending: Vec::new(),
            rng: 0x9E3779B97F4A7C15 ^ (pos.x.to_bits() as u64) << 32 ^ pos.y.to_bits() as u64,
            pressure_opacity,
        });
        self.push_dab(gpu, pos, pressure, 0.0);
        Ok(())
    }

    fn rand(rng: &mut u64) -> f32 {
        *rng ^= *rng << 13;
        *rng ^= *rng >> 7;
        *rng ^= *rng << 17;
        (*rng >> 40) as f32 / (1u64 << 24) as f32
    }

    /// Brush radius in screen pixels at a point.
    fn radius_px(&mut self, gpu: &Gpu, pos: Vec2, pressure: f32) -> f32 {
        let mut size = self.brush.size;
        if self.brush.pressure_size {
            size *= pressure.clamp(0.05, 1.0);
        }
        if self.brush.size_space == SizeSpace::World {
            // Size is in world units; convert using the depth under the dab.
            let (fov, h) = self
                .bake_camera
                .map(|c| (c.camera.fov_y, c.screen.1 as f32))
                .unwrap_or((40f32.to_radians(), 720.0));
            if let Some(depth) = self.depth_at(gpu, pos) {
                let world_per_px = 2.0 * depth * (fov * 0.5).tan() / h;
                size = size * 0.01 / world_per_px.max(1e-6);
            }
        }
        (size * 0.5).max(0.5)
    }

    fn push_dab(&mut self, gpu: &Gpu, pos: Vec2, pressure: f32, direction: f32) {
        let radius = self.radius_px(gpu, pos, pressure);
        let scale = self.buffer_scale;
        let brush = self.brush.clone();
        let Some(stroke) = self.stroke.as_mut() else { return };
        let mut r = radius * (1.0 + (Self::rand(&mut stroke.rng) - 0.5) * 2.0 * brush.jitter_size).max(0.05);
        let mut angle = brush.angle.to_radians();
        if brush.follow_stroke {
            angle += direction;
        }
        angle += (Self::rand(&mut stroke.rng) - 0.5) * std::f32::consts::TAU * brush.jitter_angle;
        let jitter = Vec2::new(Self::rand(&mut stroke.rng) - 0.5, Self::rand(&mut stroke.rng) - 0.5)
            * 2.0
            * brush.jitter_position
            * radius
            * 2.0;
        let mut alpha = brush.flow * (1.0 - Self::rand(&mut stroke.rng) * brush.jitter_flow);
        if brush.pressure_flow {
            alpha *= pressure;
        }
        if brush.pressure_opacity {
            alpha *= pressure;
        }
        r *= scale;
        let p = (pos + jitter) * scale;
        stroke.pending.push(Dab { geom: [p.x, p.y, r, angle], params: [alpha, brush.hardness, 1.0, 0.0] });
    }

    /// Extends the stroke to `pos`, placing dabs at the brush spacing.
    pub fn stroke_to(&mut self, gpu: &Gpu, pos: Vec2, pressure: f32) {
        let Some((last, carry)) = self.stroke.as_ref().map(|s| (s.last, s.carry)) else { return };
        let spacing = (self.radius_px(gpu, pos, pressure) * 2.0 * self.brush.spacing / 100.0).max(0.75);
        let delta = pos - last;
        let len = delta.length();
        if len < 1e-3 {
            return;
        }
        let dir = delta / len;
        let angle = dir.y.atan2(dir.x);
        let mut t = spacing - carry;
        while t <= len {
            self.push_dab(gpu, last + dir * t, pressure, angle);
            t += spacing;
        }
        let stroke = self.stroke.as_mut().unwrap();
        stroke.carry = len - (t - spacing);
        stroke.last = pos;
    }

    /// Draws pending dabs; in Live mode also re-bakes the stroke. Call once per frame.
    pub fn flush(&mut self, gpu: &Gpu, p: &mut Project, stencil: Option<StencilSample>) {
        let Some(stroke) = self.stroke.as_mut() else { return };
        if stroke.pending.is_empty() {
            return;
        }
        let dabs = std::mem::take(&mut stroke.pending);
        let target = stroke.target;
        for chunk in dabs.chunks(MAX_DABS) {
            self.draw_dabs(gpu, p, chunk, target, stencil);
        }
        if self.mode == BakeMode::Live {
            self.run_bake(gpu, p, target);
            p.paint_epoch += 1;
        } else {
            self.unbaked = true;
        }
    }

    fn draw_dabs(&self, gpu: &Gpu, p: &Project, dabs: &[Dab], target: Target, stencil: Option<StencilSample>) {
        let b = self.buffers.as_ref().unwrap();
        let mut lo = Vec2::splat(f32::MAX);
        let mut hi = Vec2::splat(f32::MIN);
        for d in dabs {
            let c = Vec2::new(d.geom[0], d.geom[1]);
            let r = d.geom[2] * 1.5 + 2.0;
            lo = lo.min(c - r);
            hi = hi.max(c + r);
        }
        let lo = lo.max(Vec2::ZERO);
        let hi = hi.min(Vec2::new(b.size.0 as f32, b.size.1 as f32));
        if hi.x <= lo.x || hi.y <= lo.y {
            return;
        }
        let rect = [lo.x as u32, lo.y as u32, (hi.x - lo.x).ceil() as u32 + 1, (hi.y - lo.y).ceil() as u32 + 1];
        let mut mask = 0u32;
        let mut values = [[0f32; 4]; 7];
        for c in Channel::ALL {
            if self.values.enabled[c.index()] && p.channels.is_enabled(c) {
                mask |= 1 << c.index();
            }
            let v = self.values.values[c.index()];
            values[c.index()] = match c {
                Channel::Normal => {
                    let n = Vec3::new(v[0] * 2.0 - 1.0, v[1] * 2.0 - 1.0, v[2] * 2.0 - 1.0).normalize_or(Vec3::Z);
                    [n.x, n.y, n.z, 0.0]
                }
                _ => [v[0], v[1], v[2], 0.0],
            };
        }
        let alpha_layer = p.scene.image_layer(&self.brush.alpha);
        let tool = match self.tool {
            Tool::Paint => 0,
            Tool::Erase => 1,
            Tool::PaintThrough => 2,
        };
        let st = stencil.unwrap_or(StencilSample {
            layer: -1,
            row0: [0.0; 4],
            row1: [0.0; 4],
            target_mask: 0,
            use_alpha: true,
            tile: false,
        });
        let mut row0 = st.row0;
        row0[3] = st.tile as u8 as f32;
        let params = DabParams {
            rect,
            mode: [tool, dabs.len() as i32, alpha_layer, mask as i32],
            opts: [
                self.brush.opacity * self.stroke.as_ref().map(|s| s.pressure_opacity).unwrap_or(1.0),
                target.mask as u8 as f32,
                self.values.mask_value,
                st.use_alpha as u8 as f32,
            ],
            values,
            stencil_row0: row0,
            stencil_row1: st.row1,
            stencil_info: [1.0 / self.buffer_scale, st.layer as f32, st.target_mask as f32, 0.0],
        };
        let d = &gpu.device;
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dab params"),
            contents: bytemuck::bytes_of(&params),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let dbuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("dabs"),
            contents: bytemuck::cast_slice(dabs),
            usage: wgpu::BufferUsages::STORAGE,
        });
        let views: Vec<wgpu::TextureView> =
            b.planes.iter().chain([&b.erase, &b.stroke]).map(|t| t.create_view(&Default::default())).collect();
        let images = if p.scene.images.res > 1 {
            p.scene.images.view.clone()
        } else {
            self.dummy_alpha.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let mut entries = vec![
            wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: dbuf.as_entire_binding() },
        ];
        for (i, v) in views.iter().enumerate() {
            entries.push(wgpu::BindGroupEntry { binding: 2 + i as u32, resource: wgpu::BindingResource::TextureView(v) });
        }
        entries.push(wgpu::BindGroupEntry { binding: 9, resource: wgpu::BindingResource::TextureView(&images) });
        entries.push(wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::Sampler(&self.sampler) });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("dab"),
            layout: &self.dab_pipeline.get_bind_group_layout(0),
            entries: &entries,
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.dab_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(rect[2].div_ceil(8), rect[3].div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
    }

    /// Copies the target slot into the scratch base and clears the dirty page mask.
    fn prepare_bake(&mut self, gpu: &Gpu, p: &Project, target: Target) {
        let tiles = p.tile_count();
        let res = p.tile_resolution();
        let planes = if target.mask { 1 } else { PAINT_PLANES };
        if !self.scratch.as_ref().is_some_and(|(t, r, l)| *r == res && *l >= tiles && t.len() >= planes) {
            let tex = (0..PAINT_PLANES).map(|_| array_texture(gpu, "stroke base", res, tiles)).collect();
            self.scratch = Some((tex, res, tiles));
        }
        let ppr = res / PAGE;
        let bits = tiles * ppr * ppr;
        let words = bits.div_ceil(32).max(1);
        if !self.dirty.as_ref().is_some_and(|(_, w)| *w >= words) {
            let buf = gpu.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("dirty pages"),
                size: words as u64 * 4,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.dirty = Some((buf, words));
        }
        let (dbuf, _) = self.dirty.as_ref().unwrap();
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        enc.clear_buffer(dbuf, 0, None);
        let pools = pool_textures(p, target.mask);
        let (scratch, _, _) = self.scratch.as_ref().unwrap();
        for (i, pool) in pools.iter().enumerate() {
            enc.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: pool,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: 0, z: target.slot * tiles },
                    aspect: wgpu::TextureAspect::All,
                },
                scratch[i].as_image_copy(),
                wgpu::Extent3d { width: res, height: res, depth_or_array_layers: tiles },
            );
        }
        gpu.queue.submit([enc.finish()]);
    }

    /// Writes base + buffer into the target for every tile.
    fn run_bake(&self, gpu: &Gpu, p: &Project, target: Target) {
        let Some(bc) = self.bake_camera else { return };
        let b = self.buffers.as_ref().unwrap();
        let d = &gpu.device;
        let tiles = p.tile_count();
        let res = p.tile_resolution();
        let eye = bc.camera.eye();
        let screen = Vec2::new(bc.screen.0 as f32, bc.screen.1 as f32);
        let (inv0, inv1) = self.transform.inverse_rows(screen * 0.5);
        let pr = &self.projection;
        let (scratch, _, _) = self.scratch.as_ref().unwrap();
        let pools = pool_textures(p, target.mask);
        let tv = |t: &wgpu::Texture| t.create_view(&Default::default());
        let arr = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2Array),
                ..Default::default()
            })
        };
        let buf_views: Vec<_> = b.planes.iter().chain([&b.erase]).map(tv).collect();
        let base_views: Vec<_> = (0..PAINT_PLANES).map(|i| arr(&scratch[i])).collect();
        let out_views: Vec<_> = (0..PAINT_PLANES)
            .map(|i| if i < pools.len() { arr(pools[i]) } else { arr(&self.dummy_out[i - 1]) })
            .collect();
        let depth_view = tv(&b.depth_color);
        let mut enc = d.create_command_encoder(&Default::default());
        for tile in 0..tiles {
            let params = BakeParams {
                view_proj: bc.view_proj.to_cols_array_2d(),
                eye: eye.extend(2.0 * (bc.camera.fov_y * 0.5).tan() / b.size.1 as f32).into(),
                screen: [screen.x, screen.y, self.buffer_scale, pr.depth_tolerance],
                inv_row0: inv0,
                inv_row1: inv1,
                proj: [
                    pr.falloff_start.to_radians().cos(),
                    pr.falloff_end.to_radians().cos(),
                    pr.backface_culling as u8 as f32,
                    pr.project_through as u8 as f32,
                ],
                target: [tile, target.slot * tiles + tile, target.mask as u32, res / PAGE],
            };
            let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bake params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let mut entries = vec![
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&p.maps.position.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&p.maps.normal.view) },
                wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&depth_view) },
            ];
            for (i, v) in buf_views.iter().enumerate() {
                entries.push(wgpu::BindGroupEntry { binding: 4 + i as u32, resource: wgpu::BindingResource::TextureView(v) });
            }
            entries.push(wgpu::BindGroupEntry { binding: 10, resource: wgpu::BindingResource::Sampler(&self.sampler) });
            for (i, v) in base_views.iter().enumerate() {
                entries.push(wgpu::BindGroupEntry { binding: 11 + i as u32, resource: wgpu::BindingResource::TextureView(v) });
            }
            for (i, v) in out_views.iter().enumerate() {
                entries.push(wgpu::BindGroupEntry { binding: 16 + i as u32, resource: wgpu::BindingResource::TextureView(v) });
            }
            entries.push(wgpu::BindGroupEntry { binding: 21, resource: self.dirty.as_ref().unwrap().0.as_entire_binding() });
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("paint bake"),
                layout: &self.bake_pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.bake_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(res.div_ceil(8), res.div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
    }

    /// Finishes the stroke. Live mode records undo and clears the buffer.
    pub fn end_stroke(&mut self, gpu: &Gpu, p: &mut Project, stencil: Option<StencilSample>) {
        self.flush(gpu, p, stencil);
        let Some(stroke) = self.stroke.take() else { return };
        if self.mode == BakeMode::Live {
            self.record_undo(gpu, p, stroke.target, "Paint stroke");
            self.clear_buffer(gpu);
        }
    }

    /// Manual mode: bakes the buffer into the selected target.
    pub fn bake(&mut self, gpu: &Gpu, p: &mut Project) -> Result<(), String> {
        if !self.unbaked {
            return Ok(());
        }
        let target = self.target(gpu, p)?;
        self.prepare_bake(gpu, p, target);
        self.run_bake(gpu, p, target);
        p.paint_epoch += 1;
        self.record_undo(gpu, p, target, "Bake");
        self.clear_buffer(gpu);
        Ok(())
    }

    /// Called when the camera is about to move. Returns false if the camera must stay.
    pub fn camera_moving(&mut self, gpu: &Gpu, p: &mut Project) -> bool {
        if self.mode != BakeMode::Manual || !self.unbaked {
            return true;
        }
        match self.on_camera_move {
            OnCameraMove::Bake => {
                let _ = self.bake(gpu, p);
                true
            }
            OnCameraMove::Clear => {
                self.clear_buffer(gpu);
                true
            }
            OnCameraMove::LockCamera => false,
        }
    }

    pub fn set_mode(&mut self, gpu: &Gpu, p: &mut Project, mode: BakeMode) {
        if self.mode == BakeMode::Manual && mode == BakeMode::Live && self.unbaked {
            let _ = self.bake(gpu, p);
        }
        self.mode = mode;
    }

    /// Reads the dirty page mask and saves the pre-bake contents of those pages.
    fn record_undo(&mut self, gpu: &Gpu, p: &mut Project, target: Target, label: &str) {
        let (dbuf, words) = self.dirty.as_ref().unwrap();
        let bytes = read_buffer(gpu, dbuf, *words as u64 * 4);
        let mask: &[u32] = bytemuck::cast_slice(&bytes);
        let res = p.tile_resolution();
        let tiles = p.tile_count();
        let ppr = res / PAGE;
        let mut pages = Vec::new();
        for bit in 0..tiles * ppr * ppr {
            if mask[(bit / 32) as usize] & (1 << (bit % 32)) != 0 {
                let tile = bit / (ppr * ppr);
                let page = bit % (ppr * ppr);
                pages.push((tile, page % ppr, page / ppr));
            }
        }
        if pages.is_empty() {
            return;
        }
        let (scratch, _, _) = self.scratch.as_ref().unwrap();
        let planes: Vec<&wgpu::Texture> = scratch.iter().take(if target.mask { 1 } else { PAINT_PLANES }).collect();
        let data = read_pages(gpu, &planes, &pages);
        // Store with pool layers, as undo writes back into the pool.
        let pages = pages.into_iter().map(|(t, x, y)| (target.slot * tiles + t, x, y)).collect();
        p.push_paint_undo(label, PaintPages { mask: target.mask, pages, data });
    }

    /// Draws unbaked Manual-mode paint over the viewport output.
    pub fn draw_overlay(&self, gpu: &Gpu, out: &wgpu::TextureView, screen: (u32, u32)) {
        if !(self.mode == BakeMode::Manual && self.unbaked && self.show_buffer) {
            return;
        }
        let Some(b) = &self.buffers else { return };
        if b.screen != screen {
            return;
        }
        let d = &gpu.device;
        let center = Vec2::new(screen.0 as f32, screen.1 as f32) * 0.5;
        let (r0, r1) = self.transform.inverse_rows(center);
        let data: [[f32; 4]; 3] = [r0, r1, [screen.0 as f32, screen.1 as f32, 0.0, 0.0]];
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("overlay params"),
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let v0 = b.planes[0].create_view(&Default::default());
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("overlay"),
            layout: &self.overlay_pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&v0) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("paint overlay"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: out,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.overlay_pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
    }
}

fn array_texture(gpu: &Gpu, label: &str, res: u32, layers: u32) -> wgpu::Texture {
    gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: layers.max(1) },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: PAINT_FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::STORAGE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Pool textures a target lives in: five planes, or the mask pool.
pub fn pool_textures(p: &Project, mask: bool) -> Vec<&wgpu::Texture> {
    if mask { vec![&p.scene.mask.texture] } else { p.scene.paint.iter().map(|pl| &pl.texture).collect() }
}

pub fn read_buffer(gpu: &Gpu, src: &wgpu::Buffer, size: u64) -> Vec<u8> {
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("readback"),
        size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(src, 0, &staging, 0, size);
    gpu.queue.submit([enc.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.wait_idle();
    let out = slice.get_mapped_range().expect("mapped").to_vec();
    staging.unmap();
    out
}

const PAGE_BYTES: u64 = (PAGE * PAGE * 8) as u64;

/// Reads `pages` (layer, px, py) of every plane. Returns one Vec per plane, pages concatenated.
pub fn read_pages(gpu: &Gpu, planes: &[&wgpu::Texture], pages: &[(u32, u32, u32)]) -> Vec<Vec<u8>> {
    let total = PAGE_BYTES * pages.len() as u64;
    let staging = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("page readback"),
        size: total * planes.len() as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = gpu.device.create_command_encoder(&Default::default());
    for (pi, plane) in planes.iter().enumerate() {
        for (k, &(layer, px, py)) in pages.iter().enumerate() {
            enc.copy_texture_to_buffer(
                wgpu::TexelCopyTextureInfo {
                    texture: plane,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: px * PAGE, y: py * PAGE, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                wgpu::TexelCopyBufferInfo {
                    buffer: &staging,
                    layout: wgpu::TexelCopyBufferLayout {
                        offset: pi as u64 * total + k as u64 * PAGE_BYTES,
                        bytes_per_row: Some(PAGE * 8),
                        rows_per_image: Some(PAGE),
                    },
                },
                wgpu::Extent3d { width: PAGE, height: PAGE, depth_or_array_layers: 1 },
            );
        }
    }
    gpu.queue.submit([enc.finish()]);
    let slice = staging.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    gpu.wait_idle();
    let data = slice.get_mapped_range().expect("mapped");
    let out = (0..planes.len()).map(|pi| data[(pi as u64 * total) as usize..((pi as u64 + 1) * total) as usize].to_vec()).collect();
    drop(data);
    staging.unmap();
    out
}

pub fn write_pages(gpu: &Gpu, planes: &[&wgpu::Texture], pages: &[(u32, u32, u32)], data: &[Vec<u8>]) {
    for (pi, plane) in planes.iter().enumerate() {
        for (k, &(layer, px, py)) in pages.iter().enumerate() {
            let start = k * PAGE_BYTES as usize;
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: plane,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: px * PAGE, y: py * PAGE, z: layer },
                    aspect: wgpu::TextureAspect::All,
                },
                &data[pi][start..start + PAGE_BYTES as usize],
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PAGE * 8), rows_per_image: Some(PAGE) },
                wgpu::Extent3d { width: PAGE, height: PAGE, depth_or_array_layers: 1 },
            );
        }
    }
}
