//! Layer thumbnails rendered from paint pools (first UDIM tile).

use std::collections::HashMap;

use wgpu::util::DeviceExt;

use crate::gpu::{self, Gpu};
use crate::graph::NodeId;
use crate::project::Project;

const SIZE: u32 = 64;

pub struct Thumbnails {
    pipeline: wgpu::RenderPipeline,
    sampler: wgpu::Sampler,
    target: wgpu::Texture,
    /// node -> (paint epoch it was rendered at, time rendered, texture)
    cache: HashMap<NodeId, (u64, f64, egui::TextureHandle)>,
}

impl Thumbnails {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let module = gpu::shader(d, "thumbnail", include_str!("shaders/thumb.wgsl"));
        let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("thumbnail"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs_main"), compilation_options: Default::default(), buffers: &[] },
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::TextureFormat::Rgba8Unorm.into())],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("thumbnail"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let target = gpu::render_target(d, "thumbnail", SIZE, SIZE, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_SRC);
        Self { pipeline, sampler, target, cache: HashMap::new() }
    }

    /// Forgets every thumbnail (another project was opened).
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Drops thumbnails of nodes that no longer exist.
    pub fn prune(&mut self, p: &Project) {
        self.cache.retain(|n, _| p.graph.get(*n).is_some());
    }

    /// Thumbnail of a Paint / Paint Mask node, or None if it has never been painted.
    pub fn get(&mut self, ctx: &egui::Context, gpu: &Gpu, p: &Project, node: NodeId) -> Option<egui::TextureId> {
        let mask = p.graph.get(node).is_some_and(|n| crate::graph::is_paint_mask(&n.ty));
        let slot = if mask { p.mask_slots.get(&node) } else { p.paint_slots.get(&node) }.copied()?;
        let now = ctx.input(|i| i.time);
        let entry = self.cache.get(&node);
        let stale = entry.is_none_or(|(e, _, _)| *e != p.paint_epoch);
        // Refresh each thumbnail at most four times a second while painting, and make sure a
        // throttled refresh still happens once painting stops.
        let due = entry.is_none_or(|(_, t, _)| now - *t >= 0.25);
        if stale && !due {
            ctx.request_repaint_after(std::time::Duration::from_millis(260));
        }
        if stale && due {
            let pool = if mask { &p.scene.mask } else { &p.scene.paint[0] };
            let image = self.render(gpu, pool, slot * p.tile_count(), p.tile_count(), mask);
            match self.cache.get_mut(&node) {
                Some((epoch, time, handle)) => {
                    handle.set(image, egui::TextureOptions::LINEAR);
                    *epoch = p.paint_epoch;
                    *time = now;
                }
                None => {
                    let handle = ctx.load_texture(format!("thumb {node}"), image, egui::TextureOptions::LINEAR);
                    self.cache.insert(node, (p.paint_epoch, now, handle));
                }
            }
        }
        self.cache.get(&node).map(|(_, _, h)| h.id())
    }

    /// Renders up to four tiles in a grid (one tile fills the thumbnail).
    fn render(&self, gpu: &Gpu, pool: &crate::scene::Pool, first_layer: u32, tiles: u32, mask: bool) -> egui::ColorImage {
        let d = &gpu.device;
        let n = tiles.clamp(1, 4);
        let cols = if n == 1 { 1 } else { 2 };
        let rows = n.div_ceil(cols);
        let cell = SIZE / cols.max(rows);
        let binds: Vec<wgpu::BindGroup> = (0..n)
            .map(|t| {
                let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("thumbnail params"),
                    contents: bytemuck::cast_slice(&[(first_layer + t) as f32, mask as u8 as f32, cell as f32, 0.0]),
                    usage: wgpu::BufferUsages::UNIFORM,
                });
                d.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("thumbnail"),
                    layout: &self.pipeline.get_bind_group_layout(0),
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pool.view) },
                        wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                    ],
                })
            })
            .collect();
        let view = self.target.create_view(&Default::default());
        let mut enc = d.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("thumbnail"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.1, g: 0.1, b: 0.11, a: 1.0 }), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.pipeline);
            // Tiles in UDIM order: left to right, bottom row first.
            let y0 = (SIZE - rows * cell) / 2;
            for (t, bind) in binds.iter().enumerate() {
                let (cx, cy) = (t as u32 % cols, rows - 1 - t as u32 / cols);
                pass.set_viewport((cx * cell) as f32, (y0 + cy * cell) as f32, cell as f32, cell as f32, 0.0, 1.0);
                pass.set_bind_group(0, bind, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        gpu.queue.submit([enc.finish()]);
        let bytes = gpu.read_texture(&self.target, 0, 4);
        egui::ColorImage::from_rgba_unmultiplied([SIZE as usize, SIZE as usize], &bytes)
    }
}
