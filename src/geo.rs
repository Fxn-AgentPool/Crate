//! Geometry queries on the CPU: rays from the camera, the nearest surface point, and the mesh
//! elements Geo Fill fills. See docs/reference/brushes-tools-stencils.md#geo-fill and
//! docs/reference/curves.md#shape.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::bvh::{self, Bvh};
use crate::camera::Camera;
use crate::mesh::Mesh;

/// What a Geo Fill click fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize)]
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

/// How Geo Fill picks elements.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectMode {
    Click,
    Brush,
    Box,
}

impl SelectMode {
    pub const ALL: [SelectMode; 3] = [SelectMode::Click, SelectMode::Brush, SelectMode::Box];

    pub fn name(self) -> &'static str {
        match self {
            SelectMode::Click => "Click",
            SelectMode::Brush => "Brush",
            SelectMode::Box => "Box",
        }
    }
}

/// A screen region of a Brush stroke or Box, in pixels.
#[derive(Clone, Copy, Debug)]
pub enum Region {
    Circle { center: Vec2, radius: f32 },
    Rect { min: Vec2, max: Vec2 },
}

/// At most this many rays for one region in the 3D view.
const MAX_SAMPLES: f32 = 16384.0;

/// One element: the kind and its id (face, island, part, material index or UDIM number).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
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
    /// Stacked UV islands, found on first use.
    stacks: std::cell::OnceCell<Stacks>,
}

/// Groups of UV islands that share UV space (docs/reference/bake-exclusion.md#stacks).
#[derive(Debug, Default)]
pub struct Stacks {
    /// Stack of each stacked island; islands not in a stack are absent.
    pub of_island: HashMap<u32, u32>,
    pub count: usize,
}

impl Stacks {
    /// A stack's display color (well-separated hues), in linear RGB.
    pub fn color(stack: u32) -> [f32; 3] {
        let hue = (stack as f32 * 0.618034 + 0.05).fract();
        let k = [1.0, 2.0 / 3.0, 1.0 / 3.0];
        let c = k.map(|k: f32| (((hue + k).fract() * 6.0 - 3.0).abs() - 1.0).clamp(0.0, 1.0));
        // Toward the middle of the range, so they read on light and dark materials.
        c.map(|v| 0.15 + 0.85 * v)
    }
}

/// Cells per UV unit when measuring how islands overlap.
const STACK_GRID: f32 = 512.0;

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
        Self { bvh, facing, islands: islands(mesh), tiles, stacks: Default::default() }
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

    /// The UV island of a triangle.
    pub fn island(&self, tri: u32) -> u32 {
        self.islands[tri as usize]
    }

    /// Stacked UV islands: two islands are stacked when at least half of the smaller one's cells
    /// (on a grid of 512 per UV unit) are also the other's; stacks are the groups they join.
    pub fn stacks(&self, mesh: &Mesh) -> &Stacks {
        self.stacks.get_or_init(|| {
            let mut cells: HashMap<(i32, i32), Vec<u32>> = HashMap::new();
            let mut area: HashMap<u32, u32> = HashMap::new();
            let uv_of = |i: u32| Vec2::from(mesh.vertices[i as usize].uv) * STACK_GRID;
            for (t, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                let island = self.islands[t];
                let p = tri.map(uv_of);
                let den = (p[1].y - p[2].y) * (p[0].x - p[2].x) + (p[2].x - p[1].x) * (p[0].y - p[2].y);
                if den.abs() < 1e-12 {
                    continue;
                }
                let (lo, hi) = (p[0].min(p[1]).min(p[2]).floor(), p[0].max(p[1]).max(p[2]).ceil());
                if (hi - lo).x * (hi - lo).y > 4.0e6 {
                    continue;
                }
                for y in lo.y as i32..hi.y as i32 {
                    for x in lo.x as i32..hi.x as i32 {
                        let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                        let a = ((p[1].y - p[2].y) * (c.x - p[2].x) + (p[2].x - p[1].x) * (c.y - p[2].y)) / den;
                        let b = ((p[2].y - p[0].y) * (c.x - p[2].x) + (p[0].x - p[2].x) * (c.y - p[2].y)) / den;
                        if a >= 0.0 && b >= 0.0 && a + b <= 1.0 {
                            let here = cells.entry((x, y)).or_default();
                            if !here.contains(&island) {
                                here.push(island);
                                *area.entry(island).or_default() += 1;
                            }
                        }
                    }
                }
            }
            let mut shared: HashMap<(u32, u32), u32> = HashMap::new();
            for islands in cells.values().filter(|v| v.len() > 1) {
                for (i, &a) in islands.iter().enumerate() {
                    for &b in &islands[i + 1..] {
                        *shared.entry((a.min(b), a.max(b))).or_default() += 1;
                    }
                }
            }
            // Join stacked pairs.
            let mut parent: HashMap<u32, u32> = HashMap::new();
            fn find(parent: &mut HashMap<u32, u32>, x: u32) -> u32 {
                let p = *parent.entry(x).or_insert(x);
                if p == x {
                    return x;
                }
                let r = find(parent, p);
                parent.insert(x, r);
                r
            }
            for (&(a, b), &n) in &shared {
                if n * 2 >= area[&a].min(area[&b]) {
                    let (ra, rb) = (find(&mut parent, a), find(&mut parent, b));
                    parent.insert(ra.max(rb), ra.min(rb));
                }
            }
            // Number stacks in order of their first triangle (islands are numbered that way).
            let mut members: Vec<u32> = parent.keys().copied().collect();
            members.sort();
            let mut ids: HashMap<u32, u32> = HashMap::new();
            let mut of_island = HashMap::new();
            for island in members {
                let root = find(&mut parent, island);
                let n = ids.len() as u32;
                of_island.insert(island, *ids.entry(root).or_insert(n));
            }
            Stacks { count: ids.len(), of_island }
        })
    }

    /// The edges of `island` along its border (other islands or open edges), as vertex pairs.
    pub fn island_outline(&self, mesh: &Mesh, island: u32) -> Vec<u32> {
        let corner = |t: usize, k: usize| mesh.indices[t * 3 + k % 3];
        let key = |i: u32| {
            let v = &mesh.vertices[i as usize];
            (v.pos.map(f32::to_bits), v.uv.map(f32::to_bits))
        };
        let edge = |t: usize, k: usize| {
            let (a, b) = (key(corner(t, k)), key(corner(t, k + 1)));
            if a < b { (a, b) } else { (b, a) }
        };
        let tris: Vec<usize> = (0..mesh.indices.len() / 3).filter(|&t| self.islands[t] == island).collect();
        let mut count: HashMap<_, u32> = HashMap::new();
        for &t in &tris {
            for k in 0..3 {
                *count.entry(edge(t, k)).or_default() += 1;
            }
        }
        tris.iter()
            .flat_map(|&t| (0..3).map(move |k| (t, k)))
            .filter(|&(t, k)| count[&edge(t, k)] == 1)
            .flat_map(|(t, k)| [corner(t, k), corner(t, k + 1)])
            .collect()
    }

    /// The element of kind `by` that triangle `tri` belongs to.
    pub fn element(&self, mesh: &Mesh, by: FillBy, tri: u32) -> Element {
        let v = &mesh.vertices[mesh.indices[tri as usize * 3] as usize];
        let id = match by {
            FillBy::Face => tri,
            FillBy::Island => self.islands[tri as usize],
            FillBy::Part => v.group,
            FillBy::Material => v.material,
            FillBy::Tile => self.tiles[tri as usize],
        };
        Element { by, id }
    }

    /// The triangles of an element, in mesh order.
    #[cfg(test)]
    pub fn triangles(&self, mesh: &Mesh, e: Element) -> Vec<u32> {
        self.triangles_of(mesh, &std::iter::once(e).collect())
    }

    /// The triangles of any of `elements`, in mesh order.
    pub fn triangles_of(&self, mesh: &Mesh, elements: &std::collections::BTreeSet<Element>) -> Vec<u32> {
        let Some(by) = elements.first().map(|e| e.by) else { return Vec::new() };
        (0..(mesh.indices.len() / 3) as u32).filter(|&t| elements.contains(&self.element(mesh, by, t))).collect()
    }

    /// Outlines (docs/reference/brushes-tools-stencils.md#outlines-and-highlight): the edges of
    /// every triangle that borders another element of kind `by`, or no other triangle, as vertex
    /// index pairs. A border is listed once per triangle along it, so the UV view outlines both
    /// sides of a seam.
    pub fn borders(&self, mesh: &Mesh, by: FillBy) -> Vec<u32> {
        let corner = |t: usize, k: usize| mesh.indices[t * 3 + k % 3];
        let key = |i: u32| mesh.vertices[i as usize].pos.map(f32::to_bits);
        let edge = |t: usize, k: usize| {
            let (a, b) = (key(corner(t, k)), key(corner(t, k + 1)));
            if a < b { (a, b) } else { (b, a) }
        };
        let tris = mesh.indices.len() / 3;
        // The elements on each edge, by position.
        let mut sides: HashMap<_, (u32, u32, bool)> = HashMap::new();
        for t in 0..tris {
            let id = self.element(mesh, by, t as u32).id;
            for k in 0..3 {
                sides
                    .entry(edge(t, k))
                    .and_modify(|(first, count, mixed)| {
                        *count += 1;
                        *mixed |= *first != id;
                    })
                    .or_insert((id, 1, false));
            }
        }
        let mut out = Vec::new();
        for t in 0..tris {
            for k in 0..3 {
                let (_, count, mixed) = sides[&edge(t, k)];
                if count == 1 || mixed || by == FillBy::Face {
                    out.extend([corner(t, k), corner(t, k + 1)]);
                }
            }
        }
        out
    }

    /// What the pointer label says about an element of `count` triangles.
    pub fn label(&self, mesh: &Mesh, e: Element, count: usize) -> String {
        let name = |names: &[String]| names.get(e.id as usize).cloned().unwrap_or_else(|| format!("#{}", e.id));
        match e.by {
            FillBy::Face => "Face".into(),
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

/// The elements of kind `by` in a screen region of a view (docs/reference/brushes-tools-stencils.md#geo-fill):
/// in 3D, the visible surface sampled every 4 pixels; in the UV view, every triangle (and, for
/// UDIM tile, every used tile) that overlaps it.
pub fn elements_in(p: &crate::project::Project, view: &crate::camera::View, screen: (u32, u32), region: Region, by: FillBy) -> std::collections::BTreeSet<Element> {
    let mesh = &p.mesh;
    let geo = p.geo();
    let mut out = std::collections::BTreeSet::new();
    match view {
        crate::camera::View::Persp(camera) => {
            let (lo, hi) = match region {
                Region::Circle { center, radius } => (center - radius, center + radius),
                Region::Rect { min, max } => (min, max),
            };
            let area = (hi - lo).max(Vec2::ONE);
            let step = 4f32.max((area.x * area.y / MAX_SAMPLES).sqrt());
            let inside = |q: Vec2| match region {
                Region::Circle { center, radius } => q.distance(center) <= radius,
                Region::Rect { .. } => true,
            };
            // Sample centers, and the region's center so a small circle still picks something.
            let mid = (lo + hi) * 0.5;
            let (nx, ny) = (((hi.x - lo.x) / step).floor() as i32, ((hi.y - lo.y) / step).floor() as i32);
            let points = (-nx / 2..=nx / 2).flat_map(|i| (-ny / 2..=ny / 2).map(move |j| mid + Vec2::new(i as f32, j as f32) * step));
            for q in points.filter(|q| inside(*q)) {
                let (o, d) = camera_ray(camera, screen, q);
                if let Some(hit) = geo.raycast(mesh, o, d) {
                    out.insert(geo.element(mesh, by, hit.tri));
                }
            }
        }
        crate::camera::View::Uv(v) => {
            let size = Vec2::new(screen.0 as f32, screen.1 as f32);
            let shape = match region {
                Region::Circle { center, radius } => UvShape::Circle(v.to_uv(center, size), radius / v.scale(size)),
                Region::Rect { min, max } => {
                    let (a, b) = (v.to_uv(min, size), v.to_uv(max, size));
                    UvShape::Rect(a.min(b), a.max(b))
                }
            };
            let uv_of = |i: u32| Vec2::from(mesh.vertices[i as usize].uv);
            for (t, tri) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                if shape.overlaps(tri.map(uv_of)) {
                    out.insert(geo.element(mesh, by, t as u32));
                }
            }
            if by == FillBy::Tile {
                for &udim in &mesh.tiles {
                    let k = udim - 1001;
                    let o = Vec2::new((k % 10) as f32, (k / 10) as f32);
                    let square = [o, o + Vec2::X, o + Vec2::ONE];
                    if shape.overlaps(square) || shape.overlaps([o, o + Vec2::ONE, o + Vec2::Y]) {
                        out.insert(Element { by, id: udim });
                    }
                }
            }
        }
    }
    out
}

/// A Brush circle or Box in UV space.
enum UvShape {
    Circle(Vec2, f32),
    Rect(Vec2, Vec2),
}

impl UvShape {
    fn overlaps(&self, t: [Vec2; 3]) -> bool {
        let inside_tri = |q: Vec2| {
            let cross = |p: Vec2, r: Vec2, s: Vec2| (r - p).perp_dot(s - p);
            let (d0, d1, d2) = (cross(t[0], t[1], q), cross(t[1], t[2], q), cross(t[2], t[0], q));
            (d0 >= 0.0 && d1 >= 0.0 && d2 >= 0.0) || (d0 <= 0.0 && d1 <= 0.0 && d2 <= 0.0)
        };
        let edges = [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])];
        match *self {
            UvShape::Circle(c, r) => {
                inside_tri(c)
                    || edges.iter().any(|&(a, b)| {
                        let ab = b - a;
                        let k = ((c - a).dot(ab) / ab.length_squared().max(1e-20)).clamp(0.0, 1.0);
                        (a + ab * k).distance(c) <= r
                    })
            }
            UvShape::Rect(lo, hi) => {
                let (tlo, thi) = (t[0].min(t[1]).min(t[2]), t[0].max(t[1]).max(t[2]));
                if thi.x < lo.x || thi.y < lo.y || tlo.x > hi.x || tlo.y > hi.y {
                    return false;
                }
                let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
                let in_rect = |q: Vec2| q.cmpge(lo).all() && q.cmple(hi).all();
                if t.iter().any(|q| in_rect(*q)) || corners.iter().any(|q| inside_tri(*q)) {
                    return true;
                }
                // Otherwise they overlap only if an edge of the triangle crosses one of the rectangle.
                let crosses = |a: Vec2, b: Vec2, c: Vec2, d: Vec2| {
                    let side = |p: Vec2, q: Vec2, r: Vec2| (q - p).perp_dot(r - p);
                    side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
                };
                edges.iter().any(|&(a, b)| (0..4).any(|k| crosses(a, b, corners[k], corners[(k + 1) % 4])))
            }
        }
    }
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
        assert_eq!(g.label(&m, g.element(&m, FillBy::Face, 0), 1), "Face");
        assert_eq!(g.triangles(&m, g.element(&m, FillBy::Face, 1)), vec![1]);
        assert_eq!(g.element(&m, FillBy::Tile, 3).id, 1001);
        assert_eq!(g.triangles(&m, g.element(&m, FillBy::Material, 0)).len(), 4);
    }

    #[test]
    fn stacks() {
        // Two copies of a quad on the same UV square, and a third quad elsewhere: one stack.
        let v = |x: f32, y: f32, u: f32, w: f32| Vertex { pos: [x, y, 0.0], uv: [u, w], ..Default::default() };
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        for (x0, u0) in [(0.0, 0.1), (2.0, 0.1), (4.0, 0.6)] {
            let b = vertices.len() as u32;
            vertices.extend([v(x0, 0.0, u0, 0.1), v(x0 + 1.0, 0.0, u0 + 0.2, 0.1), v(x0 + 1.0, 1.0, u0 + 0.2, 0.3), v(x0, 1.0, u0, 0.3)]);
            indices.extend([b, b + 1, b + 2, b, b + 2, b + 3]);
        }
        let m = Mesh {
            source: Default::default(),
            vertices,
            indices,
            materials: vec!["M".into()],
            groups: vec!["G".into()],
            tiles: vec![1001],
            bbox_min: Vec3::ZERO,
            bbox_max: Vec3::new(5.0, 1.0, 0.0),
            diagnostics: Vec::new(),
        };
        let g = Geo::new(&m);
        let s = g.stacks(&m);
        assert_eq!(s.count, 1);
        assert_eq!(s.of_island.get(&g.island(0)), Some(&0));
        assert_eq!(s.of_island.get(&g.island(2)), Some(&0));
        assert_eq!(s.of_island.get(&g.island(4)), None);
        assert_eq!(g.island_outline(&m, g.island(0)).len(), 8);
        // Make the second copy the source: the first is excluded, the lone quad untouched.
        let mut mask = crate::bakemask::BakeMask::default();
        assert_eq!(mask.make_sources(&m, &g, &[3].into()), 1);
        assert_eq!(mask.excluded, vec![true, true, false, false, false, false]);
        assert_eq!(mask.sources, [3].into());
        // Excluding the source's island drops the source.
        mask.set(&m, &g, &[2], true);
        assert!(mask.sources.is_empty());
        assert_eq!(mask.make_sources(&m, &g, &[5].into()), 0);
    }

    #[test]
    fn uv_overlaps() {
        let t = [Vec2::ZERO, Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0)];
        assert!(UvShape::Circle(Vec2::new(0.2, 0.2), 0.01).overlaps(t));
        assert!(UvShape::Circle(Vec2::new(0.6, 0.6), 0.2).overlaps(t));
        assert!(!UvShape::Circle(Vec2::new(0.8, 0.8), 0.2).overlaps(t));
        // A rectangle inside the triangle, around it, crossing an edge only, and beside it.
        assert!(UvShape::Rect(Vec2::splat(0.1), Vec2::splat(0.2)).overlaps(t));
        assert!(UvShape::Rect(Vec2::splat(-1.0), Vec2::splat(2.0)).overlaps(t));
        assert!(UvShape::Rect(Vec2::new(0.4, -0.5), Vec2::new(0.45, 1.5)).overlaps(t));
        assert!(!UvShape::Rect(Vec2::new(0.6, 0.6), Vec2::new(0.9, 0.9)).overlaps(t));
    }

    #[test]
    fn borders() {
        let m = two_quads();
        let g = Geo::new(&m);
        let edges = |by| {
            let b = g.borders(&m, by);
            let mut e: Vec<(u32, u32)> = b.chunks(2).map(|c| (c[0].min(c[1]), c[0].max(c[1]))).collect();
            e.sort();
            e
        };
        // Face: every edge of every triangle.
        assert_eq!(edges(FillBy::Face).len(), 12);
        // Island and part: the outline of each quad, the shared x = 1 edge once per side;
        // not the quads' diagonals.
        let island = edges(FillBy::Island);
        assert_eq!(island.len(), 8);
        assert!(island.contains(&(1, 2)) && island.contains(&(4, 7)));
        assert!(!island.contains(&(0, 2)));
        assert_eq!(edges(FillBy::Part), island);
        // One material: only the open edges, x = 1 shared inside it is no border.
        let material = edges(FillBy::Material);
        assert_eq!(material.len(), 6);
        assert!(!material.contains(&(1, 2)));
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
