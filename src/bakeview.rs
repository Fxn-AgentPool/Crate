//! Bake preview renderer: draws the low poly, high poly and cage of `bakepreview::Preview` for the
//! Mesh Maps window's preview pane (shaders/bakeview.wgsl).
//! See docs/reference/mesh-maps.md#bake-preview.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::bakepreview::Preview;
use crate::camera::Camera;
use crate::gpu::{self, Gpu};
use crate::viewport::{GpuMesh, OUTPUT_FORMAT};

/// Samples per pixel along each axis, when the supersampled targets fit the GPU's texture size
/// limit (1 otherwise).
const SUPERSAMPLE: u32 = 2;
const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const DIST_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

/// Linear colors of the low poly, high poly and cage.
const LOW_COLOR: [f32; 3] = [0.08, 0.3, 1.0];
const HIGH_COLOR: [f32; 3] = [0.9, 0.9, 0.9];
const CAGE_COLOR: [f32; 3] = [1.0, 0.08, 0.05];

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct PartUniform {
    view_proj: [[f32; 4]; 4],
    eye: [f32; 4],
    color: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CompositeUniform {
    params: [u32; 4],
    background_top: [f32; 4],
    background_bottom: [f32; 4],
}

struct Targets {
    width: u32,
    height: u32,
    supersample: u32,
    /// One layer per part, at the supersampled size.
    color_layers: [wgpu::TextureView; 3],
    dist_layers: [wgpu::TextureView; 3],
    depth: wgpu::TextureView,
    output: wgpu::Texture,
    output_view: wgpu::TextureView,
    composite_bind: wgpu::BindGroup,
}

pub struct BakeView {
    part_pipeline: wgpu::RenderPipeline,
    composite_pipeline: wgpu::RenderPipeline,
    composite_layout: wgpu::BindGroupLayout,
    part_buffers: [wgpu::Buffer; 3],
    part_binds: [wgpu::BindGroup; 3],
    composite_buffer: wgpu::Buffer,
    targets: Option<Targets>,
}

impl BakeView {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let module = gpu::shader(d, "bake view", include_str!("shaders/bakeview.wgsl"));
        let vf = wgpu::ShaderStages::VERTEX_FRAGMENT;
        let frag = wgpu::ShaderStages::FRAGMENT;
        let part_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bake view part"),
            entries: &[gpu::uniform_entry(0, vf)],
        });
        let composite_layout = d.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bake view composite"),
            entries: &[
                gpu::texture_entry(0, frag, wgpu::TextureViewDimension::D2Array, false),
                gpu::texture_entry(1, frag, wgpu::TextureViewDimension::D2Array, false),
                gpu::uniform_entry(2, frag),
            ],
        });
        let layout = |label, bgl: &wgpu::BindGroupLayout| {
            d.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: &[Some(bgl)],
                immediate_size: 0,
            })
        };
        let part_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bake view part"),
            layout: Some(&layout("bake view part", &part_layout)),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_part"),
                compilation_options: Default::default(),
                buffers: &[Some(crate::bakepreview::vertex_layout())],
            },
            primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: Default::default(),
                bias: Default::default(),
            }),
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_part"),
                compilation_options: Default::default(),
                targets: &[Some(COLOR_FORMAT.into()), Some(DIST_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let composite_pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("bake view composite"),
            layout: Some(&layout("bake view composite", &composite_layout)),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs_full"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_composite"),
                compilation_options: Default::default(),
                targets: &[Some(OUTPUT_FORMAT.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let uniform = |label, size| {
            d.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let part_buffers = [0, 1, 2].map(|_| uniform("bake view part", std::mem::size_of::<PartUniform>() as u64));
        let part_binds = [0, 1, 2].map(|k| {
            d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("bake view part"),
                layout: &part_layout,
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: part_buffers[k].as_entire_binding() }],
            })
        });
        Self {
            part_pipeline,
            composite_pipeline,
            composite_layout,
            part_buffers,
            part_binds,
            composite_buffer: uniform("bake view composite", std::mem::size_of::<CompositeUniform>() as u64),
            targets: None,
        }
    }

    pub fn output_view(&self) -> Option<&wgpu::TextureView> {
        self.targets.as_ref().map(|t| &t.output_view)
    }

    pub fn output_texture(&self) -> Option<&wgpu::Texture> {
        self.targets.as_ref().map(|t| &t.output)
    }

    fn ensure_targets(&mut self, gpu: &Gpu, width: u32, height: u32) {
        if self.targets.as_ref().is_some_and(|t| t.width == width && t.height == height) {
            return;
        }
        let d = &gpu.device;
        let max = d.limits().max_texture_dimension_2d;
        let supersample = if width.max(height) * SUPERSAMPLE <= max { SUPERSAMPLE } else { 1 };
        let (sw, sh) = (width * supersample, height * supersample);
        let texture = |label, format, layers| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: sw, height: sh, depth_or_array_layers: layers },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        };
        let layer_views = |t: &wgpu::Texture| {
            [0, 1, 2].map(|k| {
                t.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::D2),
                    base_array_layer: k,
                    array_layer_count: Some(1),
                    ..Default::default()
                })
            })
        };
        let array_view = |t: &wgpu::Texture| {
            t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() })
        };
        let colors = texture("bake view colors", COLOR_FORMAT, 3);
        let dists = texture("bake view distances", DIST_FORMAT, 3);
        let depth = texture("bake view depth", DEPTH_FORMAT, 1).create_view(&Default::default());
        let output = gpu::render_target(d, "bake view output", width, height, OUTPUT_FORMAT, wgpu::TextureUsages::COPY_SRC);
        let output_view = output.create_view(&Default::default());
        let composite_bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bake view composite"),
            layout: &self.composite_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&array_view(&colors)) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&array_view(&dists)) },
                wgpu::BindGroupEntry { binding: 2, resource: self.composite_buffer.as_entire_binding() },
            ],
        });
        self.targets = Some(Targets {
            width,
            height,
            supersample,
            color_layers: layer_views(&colors),
            dist_layers: layer_views(&dists),
            depth,
            output,
            output_view,
            composite_bind,
        });
    }

    /// Renders the preview's parts as seen by `camera`. `radius` is the low poly's bounding
    /// radius (for the clip planes).
    #[allow(clippy::too_many_arguments)]
    pub fn render(&mut self, gpu: &Gpu, camera: &Camera, radius: f32, preview: &Preview, low: &GpuMesh, width: u32, height: u32) {
        let (width, height) = (width.max(1), height.max(1));
        self.ensure_targets(gpu, width, height);
        let view_proj = camera.proj(width as f32 / height as f32, radius.max(1e-3)) * camera.view();
        let p = &preview.parts;
        let parts = [(p.low, LOW_COLOR), (p.high, HIGH_COLOR), (p.cage, CAGE_COLOR)];
        for (k, (part, color)) in parts.iter().enumerate() {
            let u = PartUniform {
                view_proj: view_proj.to_cols_array_2d(),
                eye: camera.eye().extend(1.0).into(),
                color: Vec3::from(*color).extend(part.opacity.clamp(0.0, 1.0)).into(),
            };
            gpu.queue.write_buffer(&self.part_buffers[k], 0, bytemuck::bytes_of(&u));
        }
        let supersample = self.targets.as_ref().map_or(1, |t| t.supersample);
        let comp = CompositeUniform {
            params: [supersample, 0, 0, 0],
            // The viewport's background, in linear values.
            background_top: [0.073, 0.078, 0.089, 1.0],
            background_bottom: [0.022, 0.023, 0.026, 1.0],
        };
        gpu.queue.write_buffer(&self.composite_buffer, 0, bytemuck::bytes_of(&comp));

        // What each part draws: vertex buffer, index buffer and count, if it's shown and loaded.
        let low_draw = preview.low.as_ref().map(|v| (v, &low.index_buffer, low.index_count));
        let high_draw = preview.high.as_ref().map(|h| (&h.vertices, &h.indices, h.index_count));
        let cage_draw = preview.cage.as_ref().map(|v| (v, &low.index_buffer, low.index_count));
        let draws = [(p.low.show, low_draw), (p.high.show, high_draw), (p.cage.show, cage_draw)];

        let t = self.targets.as_ref().unwrap();
        let mut enc = gpu.device.create_command_encoder(&Default::default());
        for (k, (show, draw)) in draws.into_iter().enumerate() {
            let clear = |view| {
                Some(wgpu::RenderPassColorAttachment {
                    view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })
            };
            // Every layer is cleared, so a hidden part leaves nothing behind.
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bake view part"),
                color_attachments: &[clear(&t.color_layers[k]), clear(&t.dist_layers[k])],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &t.depth,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Discard }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            if let (true, Some((vertices, indices, count))) = (show, draw) {
                pass.set_pipeline(&self.part_pipeline);
                pass.set_bind_group(0, &self.part_binds[k], &[]);
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..count, 0, 0..1);
            }
        }
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("bake view composite"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &t.output_view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.composite_pipeline);
            pass.set_bind_group(0, &t.composite_bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.queue.submit([enc.finish()]);
    }
}
