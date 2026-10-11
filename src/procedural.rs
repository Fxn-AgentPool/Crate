//! Procedural texture presets, listed first in the Library's Textures category: a Procedural
//! node with some settings chosen, and its thumbnail. See docs/reference/windows-and-panels.md
//! "Library".

use wgpu::util::DeviceExt;

use crate::gpu::{self, Gpu};
use crate::graph;

pub struct Preset {
    pub name: &'static str,
    /// The node type the preset adds.
    pub ty: &'static str,
    /// Choice parameters set on the node; the others keep their defaults.
    pub params: &'static [(&'static str, &'static str)],
}

pub const PRESETS: [Preset; 7] = [
    Preset { name: "Perlin Noise", ty: "noise", params: &[("type", "Perlin")] },
    Preset { name: "Simplex Noise", ty: "noise", params: &[("type", "Simplex")] },
    Preset { name: "Value Noise", ty: "noise", params: &[("type", "Value")] },
    Preset { name: "Cellular F1", ty: "cellular", params: &[("output", "F1")] },
    Preset { name: "Cellular F2", ty: "cellular", params: &[("output", "F2")] },
    Preset { name: "Cellular F2-F1", ty: "cellular", params: &[("output", "F2-F1")] },
    Preset { name: "Gradient", ty: "gradient", params: &[] },
];

impl Preset {
    /// The thumbnail shader's inputs (shaders/procedural_thumb.wgsl): node, choice index, and
    /// scale (the nodes' default).
    fn shader_info(&self) -> [f32; 4] {
        let index = |options: &[&str], key: &str| {
            self.params.iter().find(|(k, _)| *k == key).and_then(|(_, v)| options.iter().position(|o| o == v)).unwrap_or(0) as f32
        };
        match self.ty {
            "noise" => [0.0, index(graph::NOISE_TYPES, "type"), 4.0, 0.0],
            "cellular" => [1.0, index(graph::CELL_OUTPUTS, "output"), 4.0, 0.0],
            _ => [2.0, 0.0, 0.0, 0.0],
        }
    }
}

/// Renders every preset's thumbnail, `size` pixels square, in PRESETS order.
pub fn thumbnails(gpu: &Gpu, size: u32) -> Vec<egui::ColorImage> {
    let d = &gpu.device;
    let source = format!("{}\n{}", include_str!("shaders/noise.wgsl"), include_str!("shaders/procedural_thumb.wgsl"));
    let module = gpu::shader(d, "procedural thumbnail", &source);
    let pipeline = d.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("procedural thumbnail"),
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
    let target = gpu::render_target(d, "procedural thumbnail", size, size, wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureUsages::COPY_SRC);
    let view = target.create_view(&Default::default());
    PRESETS
        .iter()
        .map(|preset| {
            let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("procedural thumbnail params"),
                contents: bytemuck::cast_slice(&preset.shader_info()),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("procedural thumbnail"),
                layout: &pipeline.get_bind_group_layout(0),
                entries: &[wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() }],
            });
            let mut enc = d.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("procedural thumbnail"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(&pipeline);
                pass.set_bind_group(0, &bind, &[]);
                pass.draw(0..3, 0..1);
            }
            gpu.queue.submit([enc.finish()]);
            let bytes = gpu.read_texture(&target, 0, 4);
            egui::ColorImage::from_rgba_unmultiplied([size as usize, size as usize], &bytes)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every preset names a node type and choice parameters that exist, with allowed values.
    #[test]
    fn presets_match_the_nodes() {
        for preset in &PRESETS {
            let def = graph::def(preset.ty).unwrap_or_else(|| panic!("{}: no node {}", preset.name, preset.ty));
            for (key, value) in preset.params {
                let param = def.params.iter().find(|p| p.name == *key).unwrap_or_else(|| panic!("{}: no parameter {key}", preset.name));
                let graph::ParamKind::Enum(options) = param.kind else { panic!("{}: {key} is not a choice", preset.name) };
                assert!(options.contains(value), "{}: {value} is not a {key}", preset.name);
            }
        }
    }
}
