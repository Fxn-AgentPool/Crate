//! Geometry queries on the CPU: rays from the camera, the nearest surface point, and the mesh
//! elements Geo Fill fills. See docs/reference/brushes-tools-stencils.md#geo-fill and
//! docs/reference/curves.md#shape.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::bvh::{self, Bvh};
use crate::camera::Camera;
use crate::mesh::Mesh;

/// What a Geo Fill click fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FillBy {
    Face,
    Island,
    Part,
    Material,
    Tile,
}

impl FillBy {
    pub const ALL: [FillBy; 5] = [FillBy::Face, FillBy::Island, FillBy::Part, FillBy::Material, FillBy::Tile];

    pub fn name(self) -> &'static str {
        match self {
            FillBy::Face => "Face",
            FillBy::Island => "UV island",
            FillBy::Part => "Part",
            FillBy::Material => "Material",
            FillBy::Tile => "UDIM tile",
        }
    }
}

/// One element: the kind and its id (face, island, part, material index or UDIM number).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Element {
    pub by: FillBy,
    pub id: u32,
}

/// A point on the surface.
#[derive(Clone, Copy, Debug)]
pub struct Hit {
    /// Source triangle index.
    pub tri: u32,
    pub pos: Vec3,
    /// Interpolated vertex normal.
    pub normal: Vec3,
}

pub struct Geo {
    bvh: Bvh,
    /// Average of the corner normals per triangle, in BVH order: which way the triangle faces,
    /// whatever its winding.
    facing: Vec<Vec3>,
    /// UV island per source triangle.
    islands: Vec<u32>,
    /// UDIM tile per source triangle (of its UV centroid).
    tiles: Vec<u32>,
}

impl Geo {
    pub fn new(mesh: &Mesh) -> Self {
        let bvh = Bvh::build(mesh);
        let corner = |t: usize, k: usize| &mesh.vertices[mesh.indices[t * 3 + k] as usize];
        let facing = bvh
            .order
            .iter()
            .map(|&t| (0..3).map(|k| Vec3::from(corner(t as usize, k).normal)).sum::<Vec3>().normalize_or_zero())
            .collect();
        let tris = mesh.indices.len() / 3;
        let tiles = (0..tris)
            .map(|t| {
                let c = (0..3).map(|k| Vec2::from(corner(t, k).uv)).sum::<Vec2>() / 3.0;
                let (u, v) = (c.x.floor().clamp(0.0, 9.0) as u32, c.y.floor().clamp(0.0, 9.0) as u32);
                1001 + u + 10 * v
            })
            .collect();
        Self { bvh, facing, islands: islands(mesh), tiles }
    }

    /// The frontmost triangle along a ray (either side of it).
    pub fn raycast(&self, mesh: &Mesh, origin: Vec3, dir: Vec3) -> Option<Hit> {
        let inv = dir.recip();
        let mut best: Option<(f32, u32, Vec3)> = None;
        let mut stack = vec![0u32];
        while let Some(ni) = stack.pop() {
            let n = &self.bvh.nodes[ni as usize];
            let Some(near) = slab(origin, inv, n) else { continue };
            if best.is_some_and(|(t, _, _)| near > t) {
                continue;
            }
            if n.count == 0 {
                stack.extend([n.index, n.index + 1]);
                continue;
            }
            for i in n.index..n.index + n.count {
                let t = &self.bvh.tris[i as usize];
                let [a, b, c] = [t.v0, t.v1, t.v2].map(|v| Vec3::from_slice(&v[..3]));
                if let Some((d, bary)) = intersect(origin, dir, a, b, c) {
                    if best.is_none_or(|(bt, _, _)| d < bt) {
                        best = Some((d, i, bary));
                    }
                }
            }
        }
        let (d, i, bary) = best?;
        let tri = self.bvh.order[i as usize];
        Some(Hit { tri, pos: origin + dir * d, normal: normal_at(mesh, tri, bary) })
    }

    /// The nearest surface point to `p` on a triangle facing within 90° of `facing`.
    pub fn closest_facing(&self, mesh: &Mesh, p: Vec3, facing: Vec3) -> Option<Hit> {
        let mut best: Option<(u32, Vec3, Vec3)> = None;
        let mut best_d2 = f32::MAX;
        let box_d2 = |n: &bvh::Node| (p.clamp(Vec3::from(n.min), Vec3::from(n.max)) - p).length_squared();
        let mut stack = vec![0u32];
        while let Some(ni) = stack.pop() {
            let n = &self.bvh.nodes[ni as usize];
            if box_d2(n) > best_d2 {
                continue;
            }
            if n.count == 0 {
                let (a, b) = (n.index, n.index + 1);
                if box_d2(&self.bvh.nodes[a as usize]) < box_d2(&self.bvh.nodes[b as usize]) {
                    stack.extend([b, a]);
                } else {
                    stack.extend([a, b]);
                }
                continue;
            }
            for i in n.index..n.index + n.count {
                if self.facing[i as usize].dot(facing) <= 0.0 {
                    continue;
                }
                let t = &self.bvh.tris[i as usize];
                let [a, b, c] = [t.v0, t.v1, t.v2].map(|v| Vec3::from_slice(&v[..3]));
                let (q, bary) = crate::cage::closest_on_triangle(p, a, b, c);
                let d2 = (q - p).length_squared();
                if d2 < best_d2 {
                    best_d2 = d2;
                    best = Some((i, q, bary));
                }
            }
        }
        let (i, pos, bary) = best?;
        let tri = self.bvh.order[i as usize];
        Some(Hit { tri, pos, normal: normal_at(mesh, tri, bary) })
    }

    /// The element of kind `by` that triangle `tri` belongs to.
    pub fn element(&self, mesh: &Mesh, by: FillBy, tri: u32) -> Element {
        let v = &mesh.vertices[mesh.indices[tri as usize * 3] as usize];
        let id = match by {
            FillBy::Face => mesh.faces.get(tri as usize).copied().unwrap_or(tri),
            FillBy::Island => self.islands[tri as usize],
            FillBy::Part => v.group,
            FillBy::Material => v.material,
            FillBy::Tile => self.tiles[tri as usize],
        };
        Element { by, id }
    }

    /// The triangles of an element, in mesh order.
    pub fn triangles(&self, mesh: &Mesh, e: Element) -> Vec<u32> {
        (0..(mesh.indices.len() / 3) as u32).filter(|&t| self.element(mesh, e.by, t) == e).collect()
    }

    /// What the pointer label says about an element of `count` triangles.
    pub fn label(&self, mesh: &Mesh, e: Element, count: usize) -> String {
        let name = |names: &[String]| names.get(e.id as usize).cloned().unwrap_or_else(|| format!("#{}", e.id));
        match e.by {
            // Fanned polygons: n sides make n - 2 triangles.
            FillBy::Face => format!("Face · {} sides", count + 2),
            FillBy::Island => format!("UV island · {} triangle{}", thousands(count), if count == 1 { "" } else { "s" }),
            FillBy::Part => format!("Part: {}", name(&mesh.groups)),
            FillBy::Material => format!("Material: {}", name(&mesh.materials)),
            FillBy::Tile => format!("Tile {}", e.id),
        }
    }
}

/// The element of kind `by` under screen pixel `px` of a view (docs/reference/brushes-tools-stencils.md#geo-fill).
pub fn element_at(p: &crate::project::Project, view: &crate::camera::View, screen: (u32, u32), px: Vec2, by: FillBy) -> Option<Element> {
    let mesh = &p.mesh;
    let tri = match view {
        crate::camera::View::Persp(camera) => {
            let (o, d) = camera_ray(camera, screen, px);
            p.geo().raycast(mesh, o, d)?.tri
        }
        crate::camera::View::Uv(v) => {
            let uv = v.to_uv(px, Vec2::new(screen.0 as f32, screen.1 as f32));
            if by == FillBy::Tile {
                // The tile under the pointer, if the mesh uses it.
                if uv.x < 0.0 || uv.y < 0.0 || uv.x >= 10.0 || uv.y >= 10.0 {
                    return None;
                }
                let udim = 1001 + uv.x as u32 + 10 * uv.y as u32;
                return mesh.tiles.contains(&udim).then_some(Element { by, id: udim });
            }
            uv_triangle(mesh, uv)?
        }
    };
    Some(p.geo().element(mesh, by, tri))
}

/// The first triangle whose UVs contain `uv` (UDIM coordinates).
pub fn uv_triangle(mesh: &Mesh, uv: Vec2) -> Option<u32> {
    let uv_of = |i: u32| Vec2::from(mesh.vertices[i as usize].uv);
    mesh.indices.as_chunks::<3>().0.iter().position(|t| {
        let [a, b, c] = t.map(uv_of);
        if uv.x < a.x.min(b.x).min(c.x) || uv.x > a.x.max(b.x).max(c.x) || uv.y < a.y.min(b.y).min(c.y) || uv.y > a.y.max(b.y).max(c.y) {
            return false;
        }
        let cross = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
        let (d0, d1, d2) = (cross(a, b, uv), cross(b, c, uv), cross(c, a, uv));
        (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
    }).map(|t| t as u32)
}

/// The ray through screen pixel `px` of a `screen`-sized 3D view.
pub fn camera_ray(camera: &Camera, screen: (u32, u32), px: Vec2) -> (Vec3, Vec3) {
    let ndc = Vec2::new(px.x / screen.0.max(1) as f32 * 2.0 - 1.0, 1.0 - px.y / screen.1.max(1) as f32 * 2.0);
    let aspect = screen.0 as f32 / screen.1.max(1) as f32;
    let t = (camera.fov_y * 0.5).tan();
    let dir = (camera.forward() + camera.right() * ndc.x * t * aspect + camera.up() * ndc.y * t).normalize();
    (camera.eye(), dir)
}

fn normal_at(mesh: &Mesh, tri: u32, bary: Vec3) -> Vec3 {
    let n = |k: usize| Vec3::from(mesh.vertices[mesh.indices[tri as usize * 3 + k] as usize].normal);
    (n(0) * bary.x + n(1) * bary.y + n(2) * bary.z).normalize_or(Vec3::Y)
}

/// Entry distance of a ray into a node's box, if it hits.
fn slab(origin: Vec3, inv: Vec3, n: &bvh::Node) -> Option<f32> {
    let t0 = (Vec3::from(n.min) - origin) * inv;
    let t1 = (Vec3::from(n.max) - origin) * inv;
    let near = t0.min(t1).max_element().max(0.0);
    let far = t0.max(t1).min_element();
    (near <= far).then_some(near)
}

/// Möller–Trumbore, both sides. Returns the distance and barycentric coordinates.
fn intersect(o: Vec3, d: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Option<(f32, Vec3)> {
    let (e1, e2) = (b - a, c - a);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-12 {
        return None;
    }
    let inv = 1.0 / det;
    let s = o - a;
    let u = s.dot(p) * inv;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = s.cross(e1);
    let v = d.dot(q) * inv;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    let t = e2.dot(q) * inv;
    (t > 0.0).then_some((t, Vec3::new(1.0 - u - v, u, v)))
}

/// UV island per triangle: triangles joined by edges whose two ends have the same position and
/// UV in both.
fn islands(mesh: &Mesh) -> Vec<u32> {
    let tris = mesh.indices.len() / 3;
    let mut parent: Vec<u32> = (0..tris as u32).collect();
    fn find(parent: &mut [u32], mut x: u32) -> u32 {
        while parent[x as usize] != x {
            parent[x as usize] = parent[parent[x as usize] as usize];
            x = parent[x as usize];
        }
        x
    }
    let key = |i: u32| {
        let v = &mesh.vertices[i as usize];
        (v.pos.map(f32::to_bits), v.uv.map(f32::to_bits))
    };
    let mut edges: HashMap<_, u32> = HashMap::new();
    for t in 0..tris {
        for k in 0..3 {
            let (a, b) = (key(mesh.indices[t * 3 + k]), key(mesh.indices[t * 3 + (k + 1) % 3]));
            let e = if a < b { (a, b) } else { (b, a) };
            match edges.get(&e) {
                Some(&o) => {
                    let (ra, rb) = (find(&mut parent, o), find(&mut parent, t as u32));
                    parent[ra as usize] = rb;
                }
                None => {
                    edges.insert(e, t as u32);
                }
            }
        }
    }
    // Number islands in order of their first triangle.
    let mut ids: HashMap<u32, u32> = HashMap::new();
    (0..tris as u32)
        .map(|t| {
            let r = find(&mut parent, t);
            let n = ids.len() as u32;
            *ids.entry(r).or_insert(n)
        })
        .collect()
}

fn thousands(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh::Vertex;

    /// Two unit quads side by side (each two triangles), sharing an edge in 3D. The second quad's
    /// UVs are separate, so they are two islands.
    fn two_quads() -> Mesh {
        let v = |p: [f32; 3], uv: [f32; 2], group: u32| Vertex { pos: p, normal: [0.0, 0.0, 1.0], uv, group, ..Default::default() };
        let vertices = vec![
            v([0.0, 0.0, 0.0], [0.1, 0.1], 0),
            v([1.0, 0.0, 0.0], [0.4, 0.1], 0),
            v([1.0, 1.0, 0.0], [0.4, 0.4], 0),
            v([0.0, 1.0, 0.0], [0.1, 0.4], 0),
            v([1.0, 0.0, 0.0], [0.6, 0.1], 1),
            v([2.0, 0.0, 0.0], [0.9, 0.1], 1),
            v([2.0, 1.0, 0.0], [0.9, 0.4], 1),
            v([1.0, 1.0, 0.0], [0.6, 0.4], 1),
        ];
        let indices = vec![0, 1, 2, 0, 2, 3, 4, 5, 6, 4, 6, 7];
        Mesh {
            source: Default::default(),
            faces: vec![0, 0, 1, 1],
            vertices,
            indices,
            materials: vec!["Mat".into()],
            groups: vec!["Left".into(), "Right".into()],
            tiles: vec![1001],
            bbox_min: Vec3::ZERO,
            bbox_max: Vec3::new(2.0, 1.0, 0.0),
            diagnostics: Vec::new(),
        }
    }

    #[test]
    fn elements() {
        let m = two_quads();
        let g = Geo::new(&m);
        let island = |t| g.element(&m, FillBy::Island, t);
        assert_eq!(island(0), island(1));
        assert_ne!(island(1), island(2));
        assert_eq!(g.triangles(&m, island(3)), vec![2, 3]);
        assert_eq!(g.label(&m, g.element(&m, FillBy::Part, 2), 2), "Part: Right");
        assert_eq!(g.label(&m, g.element(&m, FillBy::Face, 0), 2), "Face · 4 sides");
        assert_eq!(g.element(&m, FillBy::Tile, 3).id, 1001);
        assert_eq!(g.triangles(&m, g.element(&m, FillBy::Material, 0)).len(), 4);
    }

    #[test]
    fn rays_and_nearest_points() {
        let m = two_quads();
        let g = Geo::new(&m);
        let hit = g.raycast(&m, Vec3::new(1.5, 0.25, 5.0), Vec3::NEG_Z).unwrap();
        assert!(hit.tri == 2 || hit.tri == 3);
        assert!((hit.pos - Vec3::new(1.5, 0.25, 0.0)).length() < 1e-5);
        assert!(g.raycast(&m, Vec3::new(3.0, 0.5, 5.0), Vec3::NEG_Z).is_none());
        let near = g.closest_facing(&m, Vec3::new(0.5, 0.5, 2.0), Vec3::Z).unwrap();
        assert!((near.pos - Vec3::new(0.5, 0.5, 0.0)).length() < 1e-5);
        // Nothing faces away from the quads.
        assert!(g.closest_facing(&m, Vec3::new(0.5, 0.5, 2.0), Vec3::NEG_Z).is_none());
        assert_eq!(uv_triangle(&m, Vec2::new(0.8, 0.15)), Some(2));
        assert_eq!(uv_triangle(&m, Vec2::new(0.5, 0.5)), None);
    }
}
