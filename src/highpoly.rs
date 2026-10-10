//! High-poly meshes for baking: loading on a background thread, part matching by name, and the
//! GPU data the projection pass (shaders/highbake.wgsl) traces against.
//! See docs/reference/mesh-maps.md#high-poly-mesh.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::bvh::{self, Bvh};
use crate::cage::LowMesh;
use crate::gpu::Gpu;
use crate::mesh::{ImportSettings, Mesh};

/// Per-triangle shading data, in BVH order. `n0[3]` holds the material index and `n1[3]` the
/// match id, as bit patterns.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct TriData {
    pub n0: [f32; 4],
    pub n1: [f32; 4],
    pub n2: [f32; 4],
}

/// A loaded high poly, before upload.
pub struct HighCpu {
    pub nodes: Vec<bvh::Node>,
    pub tris: Vec<bvh::Tri>,
    pub data: Vec<TriData>,
    /// Match id per low-poly part.
    pub low_match: Vec<u32>,
    /// Material names; the ID map's colors follow this order.
    pub materials: Vec<String>,
    /// The auto cage, if requested.
    pub cage: Option<Cage>,
}

pub struct HighGpu {
    pub nodes: wgpu::Buffer,
    pub tris: wgpu::Buffer,
    pub data: wgpu::Buffer,
    pub low_match: wgpu::Buffer,
    pub materials: Vec<String>,
    /// Auto cage: per low-poly vertex `[u, v, frontal, rear]`, drawn with the low poly's index
    /// buffer (shaders/cage_raster.wgsl).
    pub cage: Option<GpuCage>,
}

pub struct GpuCage {
    pub vertices: wgpu::Buffer,
    /// Largest distances, in world units.
    pub max_frontal: f32,
    pub max_rear: f32,
}

/// The auto cage of a bake: per-vertex distances, with the UVs to rasterize them.
pub struct Cage {
    pub vertices: Vec<[f32; 4]>,
    pub max_frontal: f32,
    pub max_rear: f32,
}

impl HighCpu {
    pub fn upload(self, gpu: &Gpu) -> Result<HighGpu> {
        // Fail with a message rather than a GPU validation error on very large meshes.
        let limit = gpu.device.limits().max_storage_buffer_binding_size as u64;
        let largest = (std::mem::size_of_val(self.tris.as_slice()).max(std::mem::size_of_val(self.data.as_slice())))
            .max(std::mem::size_of_val(self.nodes.as_slice())) as u64;
        if largest > limit {
            anyhow::bail!(
                "the high-poly mesh needs {} MB GPU buffers; this GPU allows {} MB. Decimate it and bake again",
                largest >> 20,
                limit >> 20
            );
        }
        let buf = |label, contents: &[u8]| {
            gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents,
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        // Storage buffers can't be empty.
        let low_match = if self.low_match.is_empty() { vec![0] } else { self.low_match };
        let cage = self.cage.map(|c| GpuCage {
            vertices: gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("cage vertices"),
                contents: bytemuck::cast_slice(&c.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }),
            max_frontal: c.max_frontal,
            max_rear: c.max_rear,
        });
        Ok(HighGpu {
            cage,
            nodes: buf("high bvh nodes", bytemuck::cast_slice(&self.nodes)),
            tris: buf("high bvh tris", bytemuck::cast_slice(&self.tris)),
            data: buf("high tri data", bytemuck::cast_slice(&self.data)),
            low_match: buf("low match", bytemuck::cast_slice(&low_match)),
            materials: self.materials,
        })
    }
}

/// What the auto cage is measured against: the low poly and its bounding box diagonal.
pub struct CageRequest {
    pub low: Arc<LowMesh>,
    pub diagonal: f32,
}

/// Imports the high poly with the low poly's import settings and prepares it for tracing, and
/// measures the auto cage when `cage` is given.
pub fn load(
    path: &Path,
    import: &ImportSettings,
    low_groups: &[String],
    match_by_name: bool,
    cage: Option<CageRequest>,
) -> Result<HighCpu> {
    let mesh = crate::mesh::import_high(path, import)?;
    let (low_match, high_match) = match_parts(low_groups, &mesh.groups, match_by_name);
    let cage = cage.map(|req| {
        let c = measure_cage(&mesh, &high_match, &req.low, &low_match, req.diagonal);
        Cage {
            vertices: req.low.vertices.iter().zip(c.frontal.iter().zip(&c.rear)).map(|(v, (&f, &r))| [v.uv[0], v.uv[1], f, r]).collect(),
            max_frontal: c.max_frontal,
            max_rear: c.max_rear,
        }
    });
    let bvh = Bvh::build(&mesh);
    log::info!("high poly: {} triangles, {} BVH nodes", bvh.tris.len(), bvh.nodes.len());
    let data = bvh
        .order
        .iter()
        .map(|&t| {
            let v = |k: usize| mesh.vertices[mesh.indices[t as usize * 3 + k] as usize];
            let (a, b, c) = (v(0), v(1), v(2));
            let id = high_match.get(a.group as usize).copied().unwrap_or(0);
            TriData {
                n0: [a.normal[0], a.normal[1], a.normal[2], f32::from_bits(a.material)],
                n1: [b.normal[0], b.normal[1], b.normal[2], f32::from_bits(id)],
                n2: [c.normal[0], c.normal[1], c.normal[2], 0.0],
            }
        })
        .collect();
    Ok(HighCpu { nodes: bvh.nodes, tris: bvh.tris, data, low_match, materials: mesh.materials, cage })
}

/// Measures the auto cage of `low` from the high-poly mesh (see cage.rs).
pub fn measure_cage(high: &Mesh, high_match: &[u32], low: &LowMesh, low_match: &[u32], diagonal: f32) -> crate::cage::Cage {
    let t = std::time::Instant::now();
    let samples = crate::cage::samples(high, high_match);
    let c = crate::cage::compute(low, low_match, &samples, diagonal);
    log::info!(
        "auto cage: {} samples in {:.2}s, up to {:.4} frontal, {:.4} rear",
        samples.len(),
        t.elapsed().as_secs_f32(),
        c.max_frontal / diagonal,
        c.max_rear / diagonal
    );
    c
}

/// Pairs `<name>_low` parts with `<name>_high` parts (ignoring case). Returns a match id per
/// low part and per high part; 0 means "unmatched": it hits, or is hit by, every part.
pub fn match_parts(low: &[String], high: &[String], enabled: bool) -> (Vec<u32>, Vec<u32>) {
    let mut lm = vec![0u32; low.len()];
    let mut hm = vec![0u32; high.len()];
    if !enabled {
        return (lm, hm);
    }
    let base = |name: &str, suffix: &str| name.to_ascii_lowercase().strip_suffix(suffix).map(str::to_owned);
    let mut keys: Vec<String> = Vec::new();
    for (i, l) in low.iter().enumerate() {
        let Some(k) = base(l, "_low") else { continue };
        let highs: Vec<usize> = (0..high.len()).filter(|&j| base(&high[j], "_high").as_deref() == Some(&k)).collect();
        if highs.is_empty() {
            continue;
        }
        let id = match keys.iter().position(|x| *x == k) {
            Some(p) => p as u32 + 1,
            None => {
                keys.push(k);
                keys.len() as u32
            }
        };
        lm[i] = id;
        for j in highs {
            hm[j] = id;
        }
    }
    (lm, hm)
}

/// Identifies a high-poly file's contents (path, size, modification time), so changing the file
/// makes the maps baked from it out of date. 0 = no high poly.
pub fn stamp(path: Option<&PathBuf>) -> u64 {
    use std::hash::{Hash, Hasher};
    let Some(p) = path else { return 0 };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    if let Ok(m) = std::fs::metadata(p) {
        m.len().hash(&mut h);
        if let Ok(t) = m.modified() {
            t.hash(&mut h);
        }
    }
    h.finish() | 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn matching() {
        let (lm, hm) = match_parts(&s(&["Bolt_low", "Plate_low", "Strap"]), &s(&["bolt_HIGH", "Plate_high", "Plate_high", "Extra_high"]), true);
        assert_eq!(lm, vec![1, 2, 0]);
        assert_eq!(hm, vec![1, 2, 2, 0]);
        let (lm, hm) = match_parts(&s(&["Bolt_low"]), &s(&["Bolt_high"]), false);
        assert_eq!((lm, hm), (vec![0], vec![0]));
        // Two low parts with the same name share the id.
        let (lm, _) = match_parts(&s(&["A_low", "a_LOW"]), &s(&["a_high"]), true);
        assert_eq!(lm, vec![1, 1]);
    }
}
