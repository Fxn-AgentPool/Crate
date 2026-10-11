//! Bake preview: the low poly, the high poly and the outer cage of a high-poly bake, loaded for
//! the preview pane of the Mesh Maps window (drawn by bakeview.rs).
//! See docs/reference/mesh-maps.md#bake-preview.

use std::path::Path;
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

/// One mesh of the preview.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Part {
    pub show: bool,
    /// 0 to 1.
    pub opacity: f32,
}

/// What the preview shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Parts {
    pub low: Part,
    pub high: Part,
    pub cage: Part,
}

impl Default for Parts {
    fn default() -> Self {
        let on = |opacity| Part { show: true, opacity };
        Self { low: on(0.5), high: on(0.5), cage: on(0.3) }
    }
}

impl Parts {
    /// High poly and cage need the high-poly file.
    pub fn needs_high(&self) -> bool {
        self.high.show || self.cage.show
    }

    /// Applies `key=value` (script command): keys `low`, `high`, `cage`; values `on`, `off` or
    /// an opacity, which also turns the part on.
    pub fn set(&mut self, arg: &str) -> Result<()> {
        let Some((key, value)) = arg.split_once('=') else { bail!("expected key=value, got {arg:?}") };
        let part = match key {
            "low" => &mut self.low,
            "high" => &mut self.high,
            "cage" => &mut self.cage,
            _ => bail!("unknown bake preview part {key:?} (use low, high or cage)"),
        };
        match value {
            "on" => part.show = true,
            "off" => part.show = false,
            v => {
                let o: f32 = v.parse().map_err(|_| anyhow::anyhow!("expected on, off or an opacity, got {v:?}"))?;
                if !(0.0..=1.0).contains(&o) {
                    bail!("opacity {o} is outside 0 to 1");
                }
                *part = Part { show: true, opacity: o };
            }
        }
        Ok(())
    }
}

/// Vertex of the preview meshes: position and normal.
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
    /// The preview pane is shown.
    pub open: bool,
    pub parts: Parts,
    state: State,
    /// The low poly, in the preview's vertex format; drawn with the low poly's index buffer.
    pub low: Option<wgpu::Buffer>,
    pub high: Option<HighMesh>,
    /// The cage's vertices, in the low poly's vertex order, so the low poly's index buffer draws
    /// it.
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
        Self {
            open: false,
            parts: Parts::default(),
            state: State::Idle,
            low: None,
            high: None,
            cage: None,
            frontal: None,
            generation: 0,
            cage_built: None,
        }
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

    /// While the pane is open: starts reading the high poly when the shown parts need it and
    /// it's missing or stale, picks up a finished read, and rebuilds the cage when its distances
    /// changed. Call every frame; it doesn't block.
    pub fn update(&mut self, gpu: &Gpu, src: Source) {
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
        if !self.open {
            return;
        }
        if self.low.is_none() {
            let vertices = cage_vertices(src.low, |_| 0.0);
            self.low = Some(gpu.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("bake preview low vertices"),
                contents: bytemuck::cast_slice(&vertices),
                usage: wgpu::BufferUsages::VERTEX,
            }));
        }
        let Some(path) = src.high_poly else {
            // Nothing to show of a high poly that was removed.
            if !matches!(self.state, State::Idle) {
                self.state = State::Idle;
                (self.high, self.cage, self.frontal, self.cage_built) = (None, None, None, None);
            }
            return;
        };
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
        if self.parts.cage.show && matches!(self.state, State::Ready(_)) {
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
fn load(path: &Path, import: &ImportSettings, low: &LowMesh, low_groups: &[String], key: Key, diagonal: f32) -> Result<Loaded> {
    let mesh = crate::mesh::import_high(path, import)?;
    let frontal = key.auto_cage.then(|| {
        let (low_match, high_match) = crate::highpoly::match_parts(low_groups, &mesh.groups, key.match_by_name);
        crate::highpoly::measure_cage(&mesh, &high_match, low, &low_match, diagonal).frontal
    });
    let vertices = mesh.vertices.iter().map(|v| PreviewVertex { pos: v.pos, normal: v.normal }).collect();
    Ok(Loaded { vertices, indices: mesh.indices, frontal })
}

/// The low poly with every vertex moved outward along its normal by `frontal(vertex)`: the
/// outer cage, or with 0 the low poly itself.
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
    fn set_parts() {
        let mut p = Parts::default();
        assert!(p.low.show && p.high.show && p.cage.show);
        assert_eq!((p.low.opacity, p.high.opacity, p.cage.opacity), (0.5, 0.5, 0.3));
        p.set("low=off").unwrap();
        p.set("high=off").unwrap();
        assert!(!p.low.show && p.low.opacity == 0.5);
        p.set("cage=0.8").unwrap();
        assert_eq!(p.cage, Part { show: true, opacity: 0.8 });
        assert!(p.needs_high());
        p.set("cage=off").unwrap();
        assert!(!p.needs_high());
        p.set("low=on").unwrap();
        assert!(p.low.show);
        assert!(p.set("cage=1.5").is_err());
        assert!(p.set("mid=on").is_err());
        assert!(p.set("low").is_err());
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
