//! Moving paint to new UVs on reimport. See docs/reference/mesh-and-udims.md#reimport and
//! docs/explanation/about-paint-transfer.md.
//!
//! For every texel of the new UVs, shaders/paint_transfer.wgsl `locate` finds the nearest point
//! of the old mesh along the new surface normal and records which old texel holds the paint
//! there; `gather` then copies every paint plane of every slot through that map.

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::bvh::Bvh;
use crate::gpu::Gpu;
use crate::mesh::Mesh;
use crate::scene::Pool;

/// How far from the new surface old paint is looked for, relative to the bounding-box diagonal.
pub const MAX_DISTANCE: f32 = 0.01;
/// UVs closer than this are the same.
const UV_EPSILON: f32 = 1e-6;
/// Rows traced per submission, so one dispatch never runs long enough to trip a GPU timeout.
const ROWS: u32 = 256;

/// Whether `new` has the same triangles with the same UVs as `old`: then paint texels are
/// already where they belong and nothing is transferred.
pub fn same_uvs(old: &Mesh, new: &Mesh) -> bool {
    old.indices.len() == new.indices.len()
        && old.indices.iter().zip(&new.indices).all(|(a, b)| {
            let (a, b) = (old.vertices[*a as usize].uv, new.vertices[*b as usize].uv);
            (a[0] - b[0]).abs() <= UV_EPSILON && (a[1] - b[1]).abs() <= UV_EPSILON
        })
}

/// What a transfer did, for the report.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize)]
pub struct Report {
    pub transferred: bool,
    /// Percent of the new UV area that found no old surface.
    pub missed: f32,
    /// 2D paint layers and effects kept in UV space (docs/reference/2d-paint.md#reimport).
    #[serde(skip)]
    pub kept_2d: usize,
    /// The bake exclusion was cleared, as the triangles or UVs changed
    /// (docs/reference/bake-exclusion.md#saving-and-reimport).
    #[serde(skip)]
    pub bake_cleared: bool,
}

impl Report {
    /// The status line, e.g. "Paint transferred to the new UVs (2.5% of the UV area found no
    /// old surface)".
    pub fn message(&self) -> Option<String> {
        if !self.transferred {
            return self.bake_cleared.then(|| "Bake exclusion cleared".to_string());
        }
        let mut msg = if self.missed > 0.0 {
            format!("Paint transferred to the new UVs ({} of the UV area found no old surface)", percent(self.missed))
        } else {
            "Paint transferred to the new UVs".into()
        };
        if self.kept_2d > 0 {
            let s = if self.kept_2d == 1 { "" } else { "s" };
            msg += &format!("; {} 2D paint layer{s} kept in UV space", self.kept_2d);
        }
        if self.bake_cleared {
            msg += "; bake exclusion cleared";
        }
        Some(msg)
    }
}

/// A percentage with one decimal, but never rounded down to 0 when there is something.
fn percent(p: f32) -> String {
    if p > 0.0 && p < 0.05 { "<0.1%".into() } else { format!("{p:.1}%") }
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct LocateParams {
    layer: u32,
    res: u32,
    row_start: u32,
    row_count: u32,
    max_dist: f32,
    _pad: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct GatherParams {
    res: u32,
    src_base: u32,
    dst_layer: u32,
    _pad: u32,
}

/// Paint pools to move: each source pool with its fresh destination (laid out for the new
/// tiles), and how many slots it holds.
pub struct Planes<'a> {
    pub src: Vec<&'a Pool>,
    pub dst: Vec<&'a Pool>,
    pub slots: u32,
}

/// Transfers every plane of `pools` from `old`'s UVs to `new`'s. `pos` and `nrm` are the new
/// mesh's Position and World Normal maps (one layer per new tile).
pub fn run(gpu: &Gpu, old: &Mesh, new: &Mesh, res: u32, pos: &wgpu::Texture, nrm: &wgpu::Texture, pools: &[Planes]) -> Report {
    let d = &gpu.device;
    let source = include_str!("shaders/paint_transfer.wgsl");
    let (locate_src, gather_src) = source.split_once("// ---- gather (compiled as its own module)").expect("gather section");
    let locate_mod = crate::gpu::shader(d, "paint transfer locate", locate_src);
    let gather_mod = crate::gpu::shader(d, "paint transfer gather", gather_src);
    let pipeline = |module, entry| {
        d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some(entry),
            layout: None,
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            cache: None,
        })
    };
    let locate = pipeline(&locate_mod, "locate");
    let gather = pipeline(&gather_mod, "gather");

    // The old mesh to trace against, with its UVs in BVH order.
    let bvh = Bvh::build(old);
    let mut tri_uv = Vec::with_capacity(bvh.order.len() * 2);
    for &t in &bvh.order {
        let uv = |k: usize| old.vertices[old.indices[t as usize * 3 + k] as usize].uv;
        let (a, b, c) = (uv(0), uv(1), uv(2));
        tri_uv.push([a[0], a[1], b[0], b[1]]);
        tri_uv.push([c[0], c[1], 0.0, 0.0]);
    }
    let mut tile_layer = vec![-1i32; 100];
    for (i, udim) in old.tiles.iter().enumerate() {
        if let Some(k) = udim.checked_sub(1001).filter(|k| *k < 100) {
            tile_layer[k as usize] = i as i32;
        }
    }
    let storage = |label, contents: &[u8]| {
        d.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage: wgpu::BufferUsages::STORAGE })
    };
    // Storage buffers can't be empty.
    let (no_node, no_tri, no_uv) = ([crate::bvh::Node::default()], [crate::bvh::Tri::default()], [[0.0f32; 4]]);
    let nodes = storage("transfer bvh nodes", bytemuck::cast_slice(if bvh.nodes.is_empty() { &no_node[..] } else { &bvh.nodes }));
    let tris = storage("transfer bvh tris", bytemuck::cast_slice(if bvh.tris.is_empty() { &no_tri[..] } else { &bvh.tris }));
    let tri_uv = storage("transfer tri uvs", bytemuck::cast_slice(if tri_uv.is_empty() { &no_uv[..] } else { &tri_uv }));
    let tile_layer = storage("transfer tile layers", bytemuck::cast_slice(&tile_layer));
    let counts = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("transfer counts"),
        contents: bytemuck::cast_slice(&[0u32; 2]),
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
    });
    let hits = d.create_texture(&wgpu::TextureDescriptor {
        label: Some("transfer map"),
        size: wgpu::Extent3d { width: res, height: res, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba32Float,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let hits_view = hits.create_view(&Default::default());
    let array = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
    let (pos_view, nrm_view) = (array(pos), array(nrm));
    let diagonal = (new.bbox_max - new.bbox_min).length().max(1e-6);
    let (old_tiles, new_tiles) = (old.tiles.len() as u32, new.tiles.len() as u32);

    for tile in 0..new_tiles {
        // Locate, in row bands.
        for row_start in (0..res).step_by(ROWS as usize) {
            let rows = ROWS.min(res - row_start);
            let params = LocateParams { layer: tile, res, row_start, row_count: rows, max_dist: MAX_DISTANCE * diagonal, _pad: [0; 3] };
            let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("transfer locate params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("transfer locate"),
                layout: &locate.get_bind_group_layout(0),
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&pos_view) },
                    wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&nrm_view) },
                    wgpu::BindGroupEntry { binding: 3, resource: nodes.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 4, resource: tris.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 5, resource: tri_uv.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 6, resource: tile_layer.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::TextureView(&hits_view) },
                    wgpu::BindGroupEntry { binding: 8, resource: counts.as_entire_binding() },
                ],
            });
            let mut enc = d.create_command_encoder(&Default::default());
            {
                let mut pass = enc.begin_compute_pass(&Default::default());
                pass.set_pipeline(&locate);
                pass.set_bind_group(0, &bind, &[]);
                pass.dispatch_workgroups(res.div_ceil(8), rows.div_ceil(8), 1);
            }
            gpu.queue.submit([enc.finish()]);
            let _ = d.poll(wgpu::PollType::wait_indefinitely());
        }
        // Gather every plane of every slot.
        let mut enc = d.create_command_encoder(&Default::default());
        for planes in pools {
            for (src, dst) in planes.src.iter().zip(&planes.dst) {
                let dst_view = array(&dst.texture);
                for slot in 0..planes.slots {
                    let params = GatherParams { res, src_base: slot * old_tiles, dst_layer: slot * new_tiles + tile, _pad: 0 };
                    let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("transfer gather params"),
                        contents: bytemuck::bytes_of(&params),
                        usage: wgpu::BufferUsages::UNIFORM,
                    });
                    let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                        label: Some("transfer gather"),
                        layout: &gather.get_bind_group_layout(0),
                        entries: &[
                            wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                            wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&hits_view) },
                            wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&src.view) },
                            wgpu::BindGroupEntry { binding: 3, resource: wgpu::BindingResource::TextureView(&dst_view) },
                        ],
                    });
                    let mut pass = enc.begin_compute_pass(&Default::default());
                    pass.set_pipeline(&gather);
                    pass.set_bind_group(0, &bind, &[]);
                    pass.dispatch_workgroups(res.div_ceil(8), res.div_ceil(8), 1);
                }
            }
        }
        gpu.queue.submit([enc.finish()]);
        let _ = d.poll(wgpu::PollType::wait_indefinitely());
    }
    let counts = read_counts(gpu, &counts);
    Report { transferred: true, missed: if counts[0] == 0 { 0.0 } else { counts[1] as f32 * 100.0 / counts[0] as f32 }, kept_2d: 0, bake_cleared: false }
}

fn read_counts(gpu: &Gpu, buf: &wgpu::Buffer) -> [u32; 2] {
    let d = &gpu.device;
    let read = d.create_buffer(&wgpu::BufferDescriptor {
        label: Some("transfer counts readback"),
        size: 8,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = d.create_command_encoder(&Default::default());
    enc.copy_buffer_to_buffer(buf, 0, &read, 0, 8);
    gpu.queue.submit([enc.finish()]);
    let slice = read.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = d.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().expect("mapped readback buffer");
    let v: &[u32] = bytemuck::cast_slice(&data);
    [v[0], v[1]]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_message() {
        assert_eq!(Report::default().message(), None);
        assert_eq!(Report { transferred: true, missed: 0.0, kept_2d: 0, bake_cleared: false }.message().unwrap(), "Paint transferred to the new UVs");
        assert_eq!(
            Report { transferred: true, missed: 0.0, kept_2d: 2, bake_cleared: false }.message().unwrap(),
            "Paint transferred to the new UVs; 2 2D paint layers kept in UV space"
        );
        assert_eq!(
            Report { transferred: true, missed: 0.0, kept_2d: 1, bake_cleared: false }.message().unwrap(),
            "Paint transferred to the new UVs; 1 2D paint layer kept in UV space"
        );
        assert_eq!(
            Report { transferred: true, missed: 2.46, kept_2d: 0, bake_cleared: false }.message().unwrap(),
            "Paint transferred to the new UVs (2.5% of the UV area found no old surface)"
        );
        assert_eq!(percent(0.01), "<0.1%");
    }
}
