//! Bounding volume hierarchy for the GPU ray tracer used by mesh map baking.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::mesh::Mesh;

/// GPU node layout. Interior nodes: `count == 0`, children at `index` and `index + 1`.
/// Leaves: triangles `index .. index + count`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Node {
    pub min: [f32; 3],
    pub index: u32,
    pub max: [f32; 3],
    pub count: u32,
}

/// Triangle with its vertices inlined, so traversal needs no index indirection.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable)]
pub struct Tri {
    pub v0: [f32; 4],
    pub v1: [f32; 4],
    pub v2: [f32; 4],
}

pub struct Bvh {
    pub nodes: Vec<Node>,
    pub tris: Vec<Tri>,
    /// Source triangle index of each entry in `tris`.
    pub order: Vec<u32>,
}

const LEAF_SIZE: usize = 4;
const BINS: usize = 12;

#[derive(Clone, Copy)]
struct Aabb {
    min: Vec3,
    max: Vec3,
}

impl Aabb {
    const EMPTY: Aabb = Aabb { min: Vec3::splat(f32::MAX), max: Vec3::splat(f32::MIN) };
    fn grow(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }
    fn merge(&mut self, o: &Aabb) {
        self.min = self.min.min(o.min);
        self.max = self.max.max(o.max);
    }
    fn area(&self) -> f32 {
        let e = (self.max - self.min).max(Vec3::ZERO);
        e.x * e.y + e.y * e.z + e.z * e.x
    }
}

impl Bvh {
    pub fn build(mesh: &Mesh) -> Self {
        let tri_count = mesh.indices.len() / 3;
        let pos = |t: usize, k: usize| Vec3::from(mesh.vertices[mesh.indices[t * 3 + k] as usize].pos);
        let mut boxes = Vec::with_capacity(tri_count);
        let mut centroids = Vec::with_capacity(tri_count);
        for t in 0..tri_count {
            let mut b = Aabb::EMPTY;
            for k in 0..3 {
                b.grow(pos(t, k));
            }
            boxes.push(b);
            centroids.push((b.min + b.max) * 0.5);
        }
        let mut order: Vec<u32> = (0..tri_count as u32).collect();
        let mut nodes = vec![Node::default()];
        // Iterative build: (node index, start, end).
        let mut stack = vec![(0usize, 0usize, tri_count)];
        while let Some((ni, start, end)) = stack.pop() {
            let mut bounds = Aabb::EMPTY;
            let mut cbounds = Aabb::EMPTY;
            for &t in &order[start..end] {
                bounds.merge(&boxes[t as usize]);
                cbounds.grow(centroids[t as usize]);
            }
            nodes[ni].min = bounds.min.into();
            nodes[ni].max = bounds.max.into();
            let n = end - start;
            let split = if n <= LEAF_SIZE { None } else { Self::sah_split(&order[start..end], &boxes, &centroids, &cbounds, &bounds) };
            match split {
                None => {
                    nodes[ni].index = start as u32;
                    nodes[ni].count = n as u32;
                }
                Some((axis, pos)) => {
                    let slice = &mut order[start..end];
                    let mut mid = itertools_partition(slice, |&t| centroids[t as usize][axis] < pos);
                    if mid == 0 || mid == n {
                        // Degenerate split (identical centroids): split in the middle.
                        slice.sort_by(|a, b| centroids[*a as usize][axis].total_cmp(&centroids[*b as usize][axis]));
                        mid = n / 2;
                    }
                    let left = nodes.len();
                    nodes.push(Node::default());
                    nodes.push(Node::default());
                    nodes[ni].index = left as u32;
                    nodes[ni].count = 0;
                    stack.push((left, start, start + mid));
                    stack.push((left + 1, start + mid, end));
                }
            }
        }
        let tris = order
            .iter()
            .map(|&t| {
                let t = t as usize;
                Tri {
                    v0: pos(t, 0).extend(0.0).into(),
                    v1: pos(t, 1).extend(0.0).into(),
                    v2: pos(t, 2).extend(0.0).into(),
                }
            })
            .collect();
        Self { nodes, tris, order }
    }

    /// Binned SAH. Returns (axis, split position) or None if a leaf is cheaper.
    fn sah_split(
        tris: &[u32],
        boxes: &[Aabb],
        centroids: &[Vec3],
        cbounds: &Aabb,
        bounds: &Aabb,
    ) -> Option<(usize, f32)> {
        let leaf_cost = tris.len() as f32;
        let mut best: Option<(f32, usize, f32)> = None;
        for axis in 0..3 {
            let lo = cbounds.min[axis];
            let hi = cbounds.max[axis];
            if hi - lo < 1e-12 {
                continue;
            }
            let scale = BINS as f32 / (hi - lo);
            let mut bin_box = [Aabb::EMPTY; BINS];
            let mut bin_n = [0usize; BINS];
            for &t in tris {
                let b = (((centroids[t as usize][axis] - lo) * scale) as usize).min(BINS - 1);
                bin_box[b].merge(&boxes[t as usize]);
                bin_n[b] += 1;
            }
            // Sweep from the right to get suffix costs.
            let mut right_area = [0.0f32; BINS];
            let mut right_n = [0usize; BINS];
            let mut acc = Aabb::EMPTY;
            let mut cnt = 0;
            for i in (1..BINS).rev() {
                acc.merge(&bin_box[i]);
                cnt += bin_n[i];
                right_area[i] = acc.area();
                right_n[i] = cnt;
            }
            let mut acc = Aabb::EMPTY;
            let mut cnt = 0;
            for i in 0..BINS - 1 {
                acc.merge(&bin_box[i]);
                cnt += bin_n[i];
                if cnt == 0 || right_n[i + 1] == 0 {
                    continue;
                }
                let cost = 0.5
                    + (acc.area() * cnt as f32 + right_area[i + 1] * right_n[i + 1] as f32)
                        / bounds.area().max(1e-20);
                if best.is_none_or(|b| cost < b.0) {
                    best = Some((cost, axis, lo + (i + 1) as f32 / scale));
                }
            }
        }
        match best {
            Some((cost, axis, pos)) if cost < leaf_cost || tris.len() > 32 => Some((axis, pos)),
            None if tris.len() > LEAF_SIZE * 4 => {
                // All centroids coincide; force a median split on the longest axis.
                let e = cbounds.max - cbounds.min;
                let axis = if e.x >= e.y && e.x >= e.z { 0 } else if e.y >= e.z { 1 } else { 2 };
                Some((axis, cbounds.min[axis]))
            }
            _ => None,
        }
    }
}

/// In-place partition; returns the number of elements satisfying `pred`.
fn itertools_partition<T>(slice: &mut [T], pred: impl Fn(&T) -> bool) -> usize {
    let mut i = 0;
    for j in 0..slice.len() {
        if pred(&slice[j]) {
            slice.swap(i, j);
            i += 1;
        }
    }
    i
}
