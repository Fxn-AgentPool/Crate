//! Auto cage: per-vertex Frontal and Rear distances for high-poly baking, measured from where
//! the high poly lies relative to the low poly.
//! See docs/reference/mesh-maps.md#auto-cage.

use std::collections::HashMap;

use glam::Vec3;

use crate::bvh::{self, Bvh};
use crate::mesh::{Mesh, Vertex};

/// At most this many high-poly samples are matched.
pub const MAX_SAMPLES: usize = 4_000_000;
/// Samples farther than this from every low-poly triangle they may hit are ignored, and no
/// distance exceeds it. Relative to the bounding box diagonal.
pub const MAX_DISTANCE: f32 = 0.1;
/// Added to every distance, relative to the bounding box diagonal.
pub const MARGIN: f32 = 0.002;
/// Every measured distance is multiplied by this.
pub const SCALE: f32 = 1.1;
/// Lower bound of the cosine between the interpolated and the face normal.
const MIN_COS: f32 = 0.5;

/// The low poly's geometry, kept for measuring the cage.
pub struct LowMesh {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// Cage distances per low-poly vertex, in world units.
#[derive(Debug, Clone)]
pub struct Cage {
    pub frontal: Vec<f32>,
    pub rear: Vec<f32>,
    pub max_frontal: f32,
    pub max_rear: f32,
}

/// Samples of the high poly: every vertex and triangle center, with the match id of its part.
/// Above `MAX_SAMPLES`, an evenly spaced subset.
pub fn samples(high: &Mesh, high_match: &[u32]) -> Vec<(Vec3, u32)> {
    let id = |v: &Vertex| high_match.get(v.group as usize).copied().unwrap_or(0);
    let nv = high.vertices.len();
    let total = nv + high.indices.len() / 3;
    let sample = |i: usize| {
        if i < nv {
            let v = &high.vertices[i];
            (Vec3::from(v.pos), id(v))
        } else {
            let t = (i - nv) * 3;
            let v = |k: usize| &high.vertices[high.indices[t + k] as usize];
            ((Vec3::from(v(0).pos) + Vec3::from(v(1).pos) + Vec3::from(v(2).pos)) / 3.0, id(v(0)))
        }
    };
    if total <= MAX_SAMPLES {
        (0..total).map(sample).collect()
    } else {
        (0..MAX_SAMPLES).map(|k| sample((k as u64 * total as u64 / MAX_SAMPLES as u64) as usize)).collect()
    }
}

/// Measures the cage. `low_match` is the match id per low-poly part (see
/// `highpoly::match_parts`), `diagonal` the low poly's bounding box diagonal.
pub fn compute(low: &LowMesh, low_match: &[u32], samples: &[(Vec3, u32)], diagonal: f32) -> Cage {
    let vtx = |t: usize, k: usize| &low.vertices[low.indices[t * 3 + k] as usize];
    let mesh = Mesh {
        source: Default::default(),
        vertices: low.vertices.clone(),
        indices: low.indices.clone(),
        materials: Vec::new(),
        groups: Vec::new(),
        tiles: Vec::new(),
        bbox_min: Vec3::ZERO,
        bbox_max: Vec3::ZERO,
        diagnostics: Vec::new(),
    };
    let bvh = Bvh::build(&mesh);
    drop(mesh);
    let tri_match: Vec<u32> = bvh
        .order
        .iter()
        .map(|&t| low_match.get(vtx(t as usize, 0).group as usize).copied().unwrap_or(0))
        .collect();
    let max_d = MAX_DISTANCE * diagonal;

    // (low triangle, signed distance) per matched sample, in parallel.
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(32);
    let chunk = samples.len().div_ceil(threads).max(1);
    let found: Vec<Vec<(u32, f32)>> = std::thread::scope(|s| {
        let handles: Vec<_> = samples
            .chunks(chunk)
            .map(|part| {
                let (bvh, tri_match) = (&bvh, &tri_match);
                s.spawn(move || {
                    let mut out = Vec::with_capacity(part.len());
                    for &(p, id) in part {
                        let Some((i, q, bary)) = closest(bvh, tri_match, p, id, max_d) else { continue };
                        let t = bvh.order[i as usize] as usize;
                        let [a, b, c] = [0, 1, 2].map(|k| vtx(t, k));
                        let (pa, pb, pc) = (Vec3::from(a.pos), Vec3::from(b.pos), Vec3::from(c.pos));
                        let n = (Vec3::from(a.normal) * bary.x + Vec3::from(b.normal) * bary.y + Vec3::from(c.normal) * bary.z)
                            .normalize_or_zero();
                        let mut f = (pb - pa).cross(pc - pa).normalize_or_zero();
                        // Outside is where the rays come from: the interpolated normal's side,
                        // whatever the winding.
                        if f == Vec3::ZERO {
                            f = n;
                        } else if f.dot(n) < 0.0 {
                            f = -f;
                        }
                        let cos = n.dot(f).max(MIN_COS);
                        let v = p - q;
                        let d = v.length() / cos;
                        out.push((t as u32, if v.dot(f) >= 0.0 { d } else { -d }));
                    }
                    out
                })
            })
            .collect();
        handles.into_iter().map(|h| h.join().expect("cage thread")).collect()
    });

    // Vertices at the same position share their distances.
    let mut weld: HashMap<[u32; 3], u32> = HashMap::new();
    let welded: Vec<u32> = low
        .vertices
        .iter()
        .map(|v| {
            let n = weld.len() as u32;
            *weld.entry(v.pos.map(f32::to_bits)).or_insert(n)
        })
        .collect();
    let mut need = vec![[0.0f32; 2]; weld.len()];
    for (t, d) in found.into_iter().flatten() {
        let side = if d >= 0.0 { 0 } else { 1 };
        for k in 0..3 {
            let w = welded[low.indices[t as usize * 3 + k] as usize] as usize;
            need[w][side] = need[w][side].max(d.abs());
        }
    }
    let finish = |d: f32| (SCALE * d + MARGIN * diagonal).min(max_d);
    let frontal: Vec<f32> = welded.iter().map(|&w| finish(need[w as usize][0])).collect();
    let rear: Vec<f32> = welded.iter().map(|&w| finish(need[w as usize][1])).collect();
    Cage {
        max_frontal: frontal.iter().copied().fold(0.0, f32::max),
        max_rear: rear.iter().copied().fold(0.0, f32::max),
        frontal,
        rear,
    }
}

/// Closest point to `p` on a triangle that a sample of match id `id` may hit, within `max_d`.
/// Returns the triangle's BVH index, the point and its barycentric coordinates.
fn closest(bvh: &Bvh, tri_match: &[u32], p: Vec3, id: u32, max_d: f32) -> Option<(u32, Vec3, Vec3)> {
    let mut best: Option<(u32, Vec3, Vec3)> = None;
    let mut best_d2 = max_d * max_d;
    let box_d2 = |n: &bvh::Node| {
        let q = p.clamp(Vec3::from(n.min), Vec3::from(n.max));
        (q - p).length_squared()
    };
    let mut stack = vec![0u32];
    while let Some(ni) = stack.pop() {
        let n = &bvh.nodes[ni as usize];
        if box_d2(n) > best_d2 {
            continue;
        }
        if n.count == 0 {
            // Visit the nearer child first (pushed last).
            let (a, b) = (n.index, n.index + 1);
            if box_d2(&bvh.nodes[a as usize]) < box_d2(&bvh.nodes[b as usize]) {
                stack.extend([b, a]);
            } else {
                stack.extend([a, b]);
            }
            continue;
        }
        for i in n.index..n.index + n.count {
            let lo = tri_match[i as usize];
            if lo != 0 && id != 0 && lo != id {
                continue;
            }
            let t = &bvh.tris[i as usize];
            let (a, b, c) = (Vec3::from_slice(&t.v0[..3]), Vec3::from_slice(&t.v1[..3]), Vec3::from_slice(&t.v2[..3]));
            let (q, bary) = closest_on_triangle(p, a, b, c);
            let d2 = (q - p).length_squared();
            if d2 <= best_d2 {
                best_d2 = d2;
                best = Some((i, q, bary));
            }
        }
    }
    best
}

/// Closest point on triangle `abc` to `p`, and its barycentric coordinates (Ericson, Real-Time
/// Collision Detection, 5.1.5).
fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> (Vec3, Vec3) {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return (a, Vec3::X);
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return (b, Vec3::Y);
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return (a + ab * v, Vec3::new(1.0 - v, v, 0.0));
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return (c, Vec3::Z);
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return (a + ac * w, Vec3::new(1.0 - w, 0.0, w));
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return (b + (c - b) * w, Vec3::new(0.0, 1.0 - w, w));
    }
    let denom = va + vb + vc;
    if denom.abs() < 1e-30 {
        return (a, Vec3::X);
    }
    let (v, w) = (vb / denom, vc / denom);
    (a + ab * v + ac * w, Vec3::new(1.0 - v - w, v, w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vert(pos: [f32; 3], normal: [f32; 3], group: u32) -> Vertex {
        Vertex { pos, normal, tangent: [1.0, 0.0, 0.0, 1.0], uv: [0.0; 2], material: 0, group }
    }

    /// A unit quad in the XY plane facing +Z, as two triangles with unshared vertices on the
    /// diagonal (like a UV seam), so welding is exercised.
    fn quad(group: u32, z: f32) -> LowMesh {
        let n = [0.0, 0.0, 1.0];
        let p = [[0.0, 0.0, z], [1.0, 0.0, z], [1.0, 1.0, z], [0.0, 0.0, z], [1.0, 1.0, z], [0.0, 1.0, z]];
        LowMesh { vertices: p.iter().map(|&p| vert(p, n, group)).collect(), indices: (0..6).collect() }
    }

    #[test]
    fn closest_point() {
        let (a, b, c) = (Vec3::ZERO, Vec3::X, Vec3::Y);
        let (q, bary) = closest_on_triangle(Vec3::new(0.25, 0.25, 3.0), a, b, c);
        assert!((q - Vec3::new(0.25, 0.25, 0.0)).length() < 1e-6);
        assert!((bary - Vec3::new(0.5, 0.25, 0.25)).length() < 1e-6);
        let (q, _) = closest_on_triangle(Vec3::new(2.0, -1.0, 0.0), a, b, c);
        assert_eq!(q, b);
        let (q, _) = closest_on_triangle(Vec3::new(0.5, -1.0, 0.0), a, b, c);
        assert!((q - Vec3::new(0.5, 0.0, 0.0)).length() < 1e-6);
    }

    #[test]
    fn measures_frontal_and_rear() {
        let low = quad(0, 0.0);
        let diag = 10.0;
        // A point 0.05 above the first triangle, one 0.02 below the second.
        let samples = [(Vec3::new(0.7, 0.2, 0.05), 0), (Vec3::new(0.2, 0.7, -0.02), 0)];
        let cage = compute(&low, &[0], &samples, diag);
        let f = |d: f32| (SCALE * d + MARGIN * diag).min(MAX_DISTANCE * diag);
        // Vertex 1 (1,0) only belongs to the first triangle; vertex 5 (0,1) only to the second.
        assert!((cage.frontal[1] - f(0.05)).abs() < 1e-5, "{:?}", cage);
        assert!((cage.rear[1] - f(0.0)).abs() < 1e-5);
        assert!((cage.rear[5] - f(0.02)).abs() < 1e-5);
        assert!((cage.frontal[5] - f(0.0)).abs() < 1e-5);
        // Welded diagonal vertices take both triangles' needs.
        for v in [0, 2, 3, 4] {
            assert!((cage.frontal[v] - f(0.05)).abs() < 1e-5);
            assert!((cage.rear[v] - f(0.02)).abs() < 1e-5);
        }
        assert!((cage.max_frontal - f(0.05)).abs() < 1e-5);
    }

    #[test]
    fn leaning_normal_and_limits() {
        // Normals leaning 60° from the face normal: the distance doubles (cos clamped at 0.5).
        let mut low = quad(0, 0.0);
        let lean = [0.0, (60f32).to_radians().sin(), (60f32).to_radians().cos()];
        for v in &mut low.vertices {
            v.normal = lean;
        }
        let diag = 10.0;
        let cage = compute(&low, &[0], &[(Vec3::new(0.7, 0.2, 0.1), 0)], diag);
        assert!((cage.frontal[1] - (SCALE * 0.2 + MARGIN * diag)).abs() < 1e-4, "{:?}", cage.frontal);
        // A sample farther than the limit is ignored; one within it is capped by it.
        let cage = compute(&low, &[0], &[(Vec3::new(0.5, 0.2, 1.5), 0)], diag);
        assert!(cage.frontal.iter().all(|&d| (d - MARGIN * diag).abs() < 1e-6));
        let cage = compute(&low, &[0], &[(Vec3::new(0.7, 0.2, 0.9), 0)], diag);
        assert!((cage.frontal[1] - MAX_DISTANCE * diag).abs() < 1e-6);
    }

    #[test]
    fn winding_does_not_flip_sides() {
        // Reversed winding, same +Z normals: a point above is still frontal.
        let mut low = quad(0, 0.0);
        for t in low.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
        let diag = 10.0;
        let cage = compute(&low, &[0], &[(Vec3::new(0.7, 0.2, 0.05), 0)], diag);
        assert!((cage.frontal[1] - (SCALE * 0.05 + MARGIN * diag)).abs() < 1e-5, "{:?}", cage);
        assert!((cage.rear[1] - MARGIN * diag).abs() < 1e-6);
    }

    #[test]
    fn respects_matching() {
        // Two stacked quads: part 0 (match 1) at z=0, part 1 (match 2) at z=0.5. A sample of
        // match 1 just below the upper quad must measure against the lower one.
        let mut low = quad(0, 0.0);
        let upper = quad(1, 0.5);
        low.vertices.extend(upper.vertices);
        low.indices.extend(upper.indices.iter().map(|i| i + 6));
        let diag = 10.0;
        let cage = compute(&low, &[1, 2], &[(Vec3::new(0.7, 0.2, 0.45), 1)], diag);
        let f = SCALE * 0.45 + MARGIN * diag;
        assert!((cage.frontal[1] - f).abs() < 1e-4, "{:?}", cage.frontal);
        assert!((cage.rear[7] - MARGIN * diag).abs() < 1e-6, "{:?}", cage.rear);
        // An unmatched sample (0) takes the nearest part: the upper quad, from below.
        let cage = compute(&low, &[1, 2], &[(Vec3::new(0.7, 0.2, 0.45), 0)], diag);
        assert!((cage.rear[7] - (SCALE * 0.05 + MARGIN * diag)).abs() < 1e-4, "{:?}", cage.rear);
    }
}
