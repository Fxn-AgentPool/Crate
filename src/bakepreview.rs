//! Bake preview: the low poly, the high poly and the outer cage of a high-poly bake, shown in
//! the viewport (drawn by viewport.rs with shaders/bake_preview.wgsl).
//! See docs/reference/mesh-maps.md#bake-preview.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::{bail, Result};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use wgpu::util::DeviceExt;

use crate::cage::LowMesh;
use crate::gpu::Gpu;
use crate::meshmaps::BakeSettings;
use crate::mesh::ImportSettings;

/// What the preview shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    pub low: bool,
    pub high: bool,
    pub cage: bool,
}

impl Default for Parts {
    fn default() -> Self {
        Self { low: true, high: false, cage: false }
    }
}

impl Parts {
    /// High poly and cage need the high-poly file.
    pub fn needs_high(&self) -> bool {
        self.high || self.cage
    }

    /// `off`, or a comma-separated list of `low`, `high` and `cage` (script command).
    pub fn parse(s: &str) -> Result<Self> {
        if s.eq_ignore_ascii_case("off") {
            return Ok(Self::default());
        }
        let mut p = Self { low: false, high: false, cage: false };
        for part in s.split(',').map(str::trim) {
            match part.to_ascii_lowercase().as_str() {
                "low" => p.low = true,
                "high" => p.high = true,
                "cage" => p.cage = true,
                _ => bail!("unknown bake preview part {part:?} (use low, high, cage or off)"),
            }
        }
        Ok(p)
    }
}

/// Vertex of the high poly and the cage: position and normal.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct PreviewVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

pub fn vertex_layout() -> wgpu::VertexBufferLayout<'static> {
    const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
    wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<PreviewVertex>() as u64,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &ATTRS,
    }
}

/// What the loaded data depends on: the file, and the settings the auto cage is measured with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Key {
    stamp: u64,
    auto_cage: bool,
    match_by_name: bool,
}

struct Loaded {
    vertices: Vec<PreviewVertex>,
    indices: Vec<u32>,
    /// Auto cage frontal distance per low-poly vertex, in world units.
    frontal: Option<Vec<f32>>,
}

enum State {
    Idle,
    Loading(Key, JoinHandle<Result<Loaded>>),
    Ready(Key),
    Failed(Key, String),
}

/// The high poly, uploaded.
pub struct HighMesh {
    pub vertices: wgpu::Buffer,
    pub indices: wgpu::Buffer,
    pub index_count: u32,
}

/// What the preview is built from (borrowed from `MeshMaps`).
pub struct Source<'a> {
    pub high_poly: Option<&'a Path>,
    pub import: &'a ImportSettings,
    pub low: &'a Arc<LowMesh>,
    pub low_groups: &'a [String],
    pub settings: &'a BakeSettings,
    pub diagonal: f32,
}

pub struct Preview {
    pub parts: Parts,
    state: State,
    pub high: Option<HighMesh>,
    /// The cage's vertices, in the low poly's vertex order, so the low poly's index and edge
    /// buffers draw it.
    pub cage: Option<wgpu::Buffer>,
    /// Auto cage frontal distances of the loaded data.
    frontal: Option<Vec<f32>>,
    /// Bumped on every load, so the cage is rebuilt.
    generation: u64,
    /// (generation, uniform frontal distance bits) the cage was built with.
    cage_built: Option<(u64, u32)>,
}

impl Default for Preview {
    fn default() -> Self {
        Self { parts: Parts::default(), state: State::Idle, high: None, cage: None, frontal: None, generation: 0, cage_built: None }
    }
}

impl Preview {
    pub fn loading(&self) -> bool {
        matches!(self.state, State::Loading(..))
    }

    pub fn error(&self) -> Option<&str> {
        match &self.state {
            State::Failed(_, e) => Some(e),
            _ => None,
        }
    }

    /// Starts reading the high poly when the shown parts need it and it's missing or stale,
    /// picks up a finished read, and rebuilds the cage when its distances changed. Call every
    /// frame; it doesn't block.
    pub fn update(&mut self, gpu: &Gpu, src: Source) {
        let Some(path) = src.high_poly else {
            *self = Self { parts: Parts { high: false, cage: false, ..self.parts }, ..Self::default() };
            return;
        };
        if let State::Loading(key, handle) = &self.state {
            if handle.is_finished() {
                let key = *key;
                let State::Loading(_, handle) = std::mem::replace(&mut self.state, State::Idle) else { unreachable!() };
                let result = handle.join().map_err(|_| anyhow::anyhow!("loading the high poly failed")).and_then(|r| r);
                self.state = match result.and_then(|l| self.upload(gpu, l)) {
                    Ok(()) => State::Ready(key),
                    Err(e) => {
                        self.high = None;
                        self.cage = None;
                        self.frontal = None;
                        State::Failed(key, format!("Could not load the high-poly mesh for the preview: {e:#}"))
                    }
                };
            }
        }
        if !self.parts.needs_high() {
            return;
        }
        let s = src.settings;
        let key = Key {
            stamp: crate::highpoly::stamp(Some(&path.to_path_buf())),
            auto_cage: s.auto_cage,
            match_by_name: s.auto_cage && s.match_by_name,
        };
        let current = match &self.state {
            State::Idle => None,
            State::Loading(k, _) | State::Ready(k) | State::Failed(k, _) => Some(*k),
        };
        if current != Some(key) && !self.loading() {
            let (path, import, low, groups, diagonal) =
                (path.to_path_buf(), src.import.clone(), src.low.clone(), src.low_groups.to_vec(), src.diagonal);
            self.state = State::Loading(key, std::thread::spawn(move || load(&path, &import, &low, &groups, key, diagonal)));
        }
        if self.parts.cage && matches!(self.state, State::Ready(_)) {
            // The uniform distance only matters without auto cage.
            let uniform = if self.frontal.is_some() { 0.0 } else { s.frontal * src.diagonal };
            let built = (self.generation, uniform.to_bits());
            if self.cage_built != Some(built) {
                let vertices = match &self.frontal {
                    Some(f) => cage_vertices(src.low, |i| f[i]),
                    None => cage_vertices(src.low, |_| uniform),
                };
                self.cage = Some(gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("bake preview cage"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                }));
                self.cage_built = Some(built);
            }
        }
    }

    fn upload(&mut self, gpu: &Gpu, l: Loaded) -> Result<()> {
        let limit = gpu.device.limits().max_buffer_size;
        let largest = std::mem::size_of_val(l.vertices.as_slice()).max(std::mem::size_of_val(l.indices.as_slice())) as u64;
        if largest > limit {
            bail!("it needs {} MB GPU buffers; this GPU allows {} MB", largest >> 20, limit >> 20);
        }
        let buf = |label, contents: &[u8], usage| {
            gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor { label: Some(label), contents, usage })
        };
        self.high = Some(HighMesh {
            vertices: buf("bake preview high vertices", bytemuck::cast_slice(&l.vertices), wgpu::BufferUsages::VERTEX),
            indices: buf("bake preview high indices", bytemuck::cast_slice(&l.indices), wgpu::BufferUsages::INDEX),
            index_count: l.indices.len() as u32,
        });
        self.frontal = l.frontal;
        self.generation += 1;
        Ok(())
    }
}

/// Reads the high poly and, with auto cage, measures the cage (as a bake does).
fn load(path: &PathBuf, import: &ImportSettings, low: &LowMesh, low_groups: &[String], key: Key, diagonal: f32) -> Result<Loaded> {
    let mesh = crate::mesh::import_high(path, import)?;
    let frontal = key.auto_cage.then(|| {
        let (low_match, high_match) = crate::highpoly::match_parts(low_groups, &mesh.groups, key.match_by_name);
        crate::highpoly::measure_cage(&mesh, &high_match, low, &low_match, diagonal).frontal
    });
    let vertices = mesh.vertices.iter().map(|v| PreviewVertex { pos: v.pos, normal: v.normal }).collect();
    Ok(Loaded { vertices, indices: mesh.indices, frontal })
}

/// The outer cage: every low-poly vertex moved outward along its normal by `frontal(vertex)`.
pub fn cage_vertices(low: &LowMesh, frontal: impl Fn(usize) -> f32) -> Vec<PreviewVertex> {
    low.vertices
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let n = Vec3::from(v.normal).normalize_or_zero();
            PreviewVertex { pos: (Vec3::from(v.pos) + n * frontal(i)).into(), normal: n.into() }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Vertex;

    #[test]
    fn parse_parts() {
        assert_eq!(Parts::parse("off").unwrap(), Parts::default());
        assert_eq!(Parts::parse("high,Cage").unwrap(), Parts { low: false, high: true, cage: true });
        assert_eq!(Parts::parse("low").unwrap(), Parts { low: true, high: false, cage: false });
        assert!(Parts::parse("low,mid").is_err());
        assert!(!Parts::default().needs_high());
    }

    #[test]
    fn cage_moves_along_normals() {
        let v = |pos, normal| Vertex { pos, normal, tangent: [1.0, 0.0, 0.0, 1.0], uv: [0.0; 2], material: 0, group: 0 };
        let low = LowMesh { vertices: vec![v([1.0, 2.0, 3.0], [0.0, 0.0, 2.0]), v([0.0; 3], [3.0, 4.0, 0.0])], indices: vec![] };
        let c = cage_vertices(&low, |i| [0.5, 1.0][i]);
        assert_eq!(c[0].pos, [1.0, 2.0, 3.5]);
        assert_eq!(c[0].normal, [0.0, 0.0, 1.0]);
        let p = Vec3::from(c[1].pos);
        assert!((p - Vec3::new(0.6, 0.8, 0.0)).length() < 1e-6);
    }
}
