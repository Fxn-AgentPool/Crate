//! Stencils: images pinned to the viewport, shown over the mesh and sampled by the Paint
//! Through tool. See Painter-docs/reference/brushes-tools-stencils.md "Stencils".

use glam::Vec2;
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

use crate::channels::Channel;
use crate::gpu::{self, Gpu};
use crate::paint::StencilSample;

pub const MAX_STENCILS: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Stencil {
    /// Image name in the image pool.
    pub image: String,
    /// Width / height of the source image.
    pub aspect: f32,
    pub offset: Vec2,
    pub rotation: f32,
    pub scale: f32,
    pub flip: bool,
    pub opacity: f32,
    pub visible: bool,
    pub targets: [bool; 7],
    pub use_alpha: bool,
    pub tile: bool,
}

impl Stencil {
    pub fn new(image: String, aspect: f32) -> Self {
        Self {
            image,
            aspect: aspect.max(1e-3),
            offset: Vec2::ZERO,
            rotation: 0.0,
            scale: 1.0,
            flip: false,
            opacity: 0.5,
            visible: true,
            targets: Channel::ALL.map(|c| c == Channel::BaseColor),
            use_alpha: true,
            tile: false,
        }
    }

    pub fn reset_transform(&mut self) {
        self.offset = Vec2::ZERO;
        self.rotation = 0.0;
        self.scale = 1.0;
        self.flip = false;
    }

    /// Size in screen pixels before scaling: 80 % of the viewport height.
    fn base_size(&self, screen: Vec2) -> Vec2 {
        let h = screen.y * 0.8;
        Vec2::new(h * self.aspect, h)
    }

    /// Rows of the affine mapping screen pixels to stencil uv.
    pub fn uv_rows(&self, screen: Vec2) -> ([f32; 4], [f32; 4]) {
        let center = screen * 0.5 + self.offset;
        let size = self.base_size(screen) * self.scale.max(1e-4);
        let (s, c) = (-self.rotation).sin_cos();
        // local = R(-rot) (q - center); uv = local / size + 0.5
        // Flipping mirrors u around the stencil's center.
        let fx = if self.flip { -1.0 } else { 1.0 };
        let a = [c / size.x * fx, -s / size.x * fx];
        let b = [s / size.y, c / size.y];
        let tx = -(a[0] * center.x + a[1] * center.y) + 0.5;
        let ty = -(b[0] * center.x + b[1] * center.y) + 0.5;
        ([a[0], a[1], tx, 0.0], [b[0], b[1], ty, 0.0])
    }

    pub fn target_mask(&self) -> u32 {
        self.targets.iter().enumerate().fold(0, |m, (i, &on)| if on { m | 1 << i } else { m })
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct StencilSet {
    pub stencils: Vec<Stencil>,
    pub active: Option<usize>,
}

impl StencilSet {
    pub fn active(&self) -> Option<&Stencil> {
        self.stencils.get(self.active?)
    }

    pub fn active_mut(&mut self) -> Option<&mut Stencil> {
        self.stencils.get_mut(self.active?)
    }

    pub fn add(&mut self, s: Stencil) -> Result<(), String> {
        if self.stencils.len() >= MAX_STENCILS {
            return Err(format!("At most {MAX_STENCILS} stencils can be loaded."));
        }
        self.stencils.push(s);
        self.active = Some(self.stencils.len() - 1);
        Ok(())
    }

    pub fn remove(&mut self, i: usize) {
        if i < self.stencils.len() {
            self.stencils.remove(i);
        }
        self.active = if self.stencils.is_empty() { None } else { Some(self.active.unwrap_or(0).min(self.stencils.len() - 1)) };
    }

    /// Alt+S: next stencil becomes active.
    pub fn next(&mut self) {
        if !self.stencils.is_empty() {
            self.active = Some(self.active.map(|a| (a + 1) % self.stencils.len()).unwrap_or(0));
        }
    }

    /// What Paint Through samples, for the given screen size.
    pub fn sample(&self, image_layer: impl Fn(&str) -> i32, screen: (u32, u32)) -> Option<StencilSample> {
        let s = self.active()?;
        let (row0, row1) = s.uv_rows(Vec2::new(screen.0 as f32, screen.1 as f32));
        Some(StencilSample {
            layer: image_layer(&s.image),
            row0,
            row1,
            target_mask: s.target_mask(),
            use_alpha: s.use_alpha,
            tile: s.tile,
        })
    }
}

/// Draws the active stencil over the viewport.
pub struct StencilOverlay {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
}

impl StencilOverlay {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let module = gpu::shader(d, "stencil overlay", include_str!("shaders/stencil.wgsl"));
        let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stencil overlay"),
            layout: None,
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
            label: Some("stencil"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self { pipeline, sampler }
    }

    pub fn draw(&self, gpu: &Gpu, out: &wgpu::TextureView, screen: (u32, u32), set: &StencilSet, images: &crate::scene::Pool, layer: i32) {
        let Some(s) = set.active() else { return };
        if !s.visible || layer < 0 {
            return;
        }
        let (r0, r1) = s.uv_rows(Vec2::new(screen.0 as f32, screen.1 as f32));
        let data: [[f32; 4]; 3] = [r0, r1, [s.opacity, layer as f32, s.tile as u8 as f32, 0.0]];
        let d = &gpu.device;
        let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("stencil params"),
            contents: bytemuck::cast_slice(&data),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("stencil"),
            layout: &self.pipeline.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&images.view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("stencil overlay"),
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
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
    }
}
