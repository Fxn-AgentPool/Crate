//! Editable curves: brush strokes along smooth paths on the surface, stored on Paint and Paint Mask
//! nodes and drawn into a texture slot of their own, which the material reads over the node's
//! freehand paint. See docs/reference/curves.md and
//! docs/explanation/about-curves-and-geometry-fills.md.

use bytemuck::{Pod, Zeroable};
use glam::Vec3;
use serde::{Deserialize, Serialize};
use wgpu::util::DeviceExt;

use crate::channels::Channel;
use crate::geo::Geo;
use crate::gpu::{self, Gpu};
use crate::mesh::Mesh;
use crate::paint::{Brush, BrushValues, SizeSpace};

pub const MAX_CURVES: usize = 256;
pub const MAX_POINTS: usize = 1000;
pub const MAX_DABS: usize = 16384;
/// Dabs per chunk: the shader skips a chunk whose box misses the texel.
const CHUNK: usize = 32;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CurvePoint {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    pub name: String,
    pub points: Vec<CurvePoint>,
    #[serde(default)]
    pub closed: bool,
    /// Width of a dab, in world units.
    pub size: f32,
    pub hardness: f32,
    pub flow: f32,
    pub opacity: f32,
    /// Percent of size.
    pub spacing: f32,
    /// Image name in the image pool ("" = round tip).
    #[serde(default)]
    pub alpha: String,
    /// Degrees, from the path's direction.
    #[serde(default)]
    pub angle: f32,
    #[serde(default)]
    pub jitter_size: f32,
    #[serde(default)]
    pub jitter_angle: f32,
    #[serde(default)]
    pub jitter_position: f32,
    #[serde(default)]
    pub jitter_flow: f32,
    pub values: BrushValues,
}

impl Curve {
    /// A curve with the brush's settings and values. `size` is its width in world units.
    pub fn from_brush(name: String, brush: &Brush, values: &BrushValues, size: f32) -> Self {
        let mut c = Self {
            name,
            points: Vec::new(),
            closed: false,
            size,
            hardness: 0.0,
            flow: 0.0,
            opacity: 0.0,
            spacing: 0.0,
            alpha: String::new(),
            angle: 0.0,
            jitter_size: 0.0,
            jitter_angle: 0.0,
            jitter_position: 0.0,
            jitter_flow: 0.0,
            values: values.clone(),
        };
        c.copy_brush(brush, values, size);
        c
    }

    /// Copy brush: every setting and value from the brush (docs/reference/curves.md).
    pub fn copy_brush(&mut self, b: &Brush, values: &BrushValues, size: f32) {
        self.size = size;
        self.hardness = b.hardness;
        self.flow = b.flow;
        self.opacity = b.opacity;
        self.spacing = b.spacing;
        self.alpha = b.alpha.clone();
        self.angle = b.angle;
        self.jitter_size = b.jitter_size;
        self.jitter_angle = b.jitter_angle;
        self.jitter_position = b.jitter_position;
        self.jitter_flow = b.jitter_flow;
        self.values = values.clone();
    }

    /// Whether the path closes: on, with enough points to make a loop.
    pub fn loops(&self) -> bool {
        self.closed && self.points.len() >= 3
    }
}

/// The brush's width in world units for a curve: World sizes as World brushes measure them,
/// Screen sizes at `world_per_px` (the depth of the curve's first point).
pub fn brush_world_size(brush: &Brush, world_per_px: Option<f32>) -> f32 {
    match (brush.size_space, world_per_px) {
        (SizeSpace::World, _) => brush.size * 0.01,
        (SizeSpace::Screen, Some(k)) => brush.size * k,
        (SizeSpace::Screen, None) => brush.size * 0.01,
    }
}

/// "Curve N", N one more than the highest number among `curves`' names.
pub fn next_name(curves: &[Curve]) -> String {
    let n = curves.iter().filter_map(|c| c.name.strip_prefix("Curve ")?.trim().parse::<u32>().ok()).max().unwrap_or(0);
    format!("Curve {}", n + 1)
}

/// The curve the Curve tool works on (docs/reference/curves.md#editing).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CurveSelection {
    /// The paint node holding the curves.
    pub node: crate::graph::NodeId,
    /// The selected curve.
    pub curve: Option<usize>,
    /// The selected curve is being drawn: clicks add points to it.
    pub drawing: bool,
}

/// A point of the path on the surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PathPoint {
    pub pos: Vec3,
    pub normal: Vec3,
}

/// Centripetal Catmull–Rom between p1 and p2 (Barry–Goldman), at t in 0..1.
fn catmull_rom(p0: Vec3, p1: Vec3, p2: Vec3, p3: Vec3, t: f32) -> Vec3 {
    let knot = |a: Vec3, b: Vec3| a.distance(b).sqrt().max(1e-6);
    let t1 = knot(p0, p1);
    let t2 = t1 + knot(p1, p2);
    let t3 = t2 + knot(p2, p3);
    let t = t1 + (t2 - t1) * t;
    let lerp = |a: Vec3, b: Vec3, ta: f32, tb: f32| a * ((tb - t) / (tb - ta)) + b * ((t - ta) / (tb - ta));
    let a1 = lerp(p0, p1, 0.0, t1);
    let a2 = lerp(p1, p2, t1, t2);
    let a3 = lerp(p2, p3, t2, t3);
    let b1 = lerp(a1, a2, 0.0, t2);
    let b2 = lerp(a2, a3, t1, t3);
    lerp(b1, b2, t1, t2)
}

/// The curve's path, pulled onto the surface: samples about a fifth of the size apart, each moved
/// to the nearest surface point facing the way the curve does there (docs/reference/curves.md#shape).
pub fn path(curve: &Curve, mesh: &Mesh, geo: &Geo) -> Vec<PathPoint> {
    let pts: Vec<Vec3> = curve.points.iter().map(|p| Vec3::from(p.pos)).collect();
    let nrm: Vec<Vec3> = curve.points.iter().map(|p| Vec3::from(p.normal)).collect();
    let n = pts.len();
    let snap = |p: Vec3, facing: Vec3| match geo.closest_facing(mesh, p, facing) {
        Some(h) => PathPoint { pos: h.pos, normal: h.normal },
        None => PathPoint { pos: p, normal: facing.normalize_or(Vec3::Y) },
    };
    if n == 0 {
        return Vec::new();
    }
    if n == 1 {
        return vec![snap(pts[0], nrm[0])];
    }
    let closed = curve.loops();
    let segments = if closed { n } else { n - 1 };
    let at = |i: isize| -> Vec3 {
        if closed {
            pts[i.rem_euclid(n as isize) as usize]
        } else if i < 0 {
            // Mirrored ends: the end segments set the end directions.
            pts[0] * 2.0 - pts[1]
        } else if i as usize >= n {
            pts[n - 1] * 2.0 - pts[n - 2]
        } else {
            pts[i as usize]
        }
    };
    let chord: f32 = (0..segments).map(|i| at(i as isize).distance(at(i as isize + 1))).sum();
    // At most 50 000 samples, however long the curve.
    let step = (curve.size * 0.2).max(chord / 50_000.0).max(1e-6);
    let mut out: Vec<PathPoint> = Vec::new();
    // Neighbors that landed far apart (on two sides of an edge the spline cut through): add
    // surface points between them, from their midpoints, until the gaps close.
    fn refine(out: &mut Vec<PathPoint>, a: PathPoint, b: PathPoint, step: f32, depth: u32, snap: &dyn Fn(Vec3, Vec3) -> PathPoint) {
        if depth == 0 || a.pos.distance(b.pos) <= step * 2.0 {
            return;
        }
        let m = snap((a.pos + b.pos) * 0.5, (a.normal + b.normal).normalize_or(a.normal));
        if m.pos.distance_squared(a.pos) < 1e-14 || m.pos.distance_squared(b.pos) < 1e-14 {
            return;
        }
        refine(out, a, m, step, depth - 1, snap);
        out.push(m);
        refine(out, m, b, step, depth - 1, snap);
    }
    for i in 0..segments {
        let i = i as isize;
        let (p0, p1, p2, p3) = (at(i - 1), at(i), at(i + 1), at(i + 2));
        let (n1, n2) = (nrm[i as usize % n], nrm[(i as usize + 1) % n]);
        let steps = ((p1.distance(p2) / step).ceil() as usize).max(1);
        let last = i as usize == segments - 1;
        for k in 0..steps + last as usize {
            let t = k as f32 / steps as f32;
            let p = catmull_rom(p0, p1, p2, p3, t);
            let facing = n1.lerp(n2, t).normalize_or(n1);
            let s = snap(p, facing);
            if let Some(&q) = out.last() {
                if q.pos.distance_squared(s.pos) <= 1e-14 {
                    continue;
                }
                refine(&mut out, q, s, step, 12, &snap);
            }
            out.push(s);
        }
    }
    out
}

#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct Dab {
    /// xyz: center, w: radius.
    pub pos: [f32; 4],
    /// xyz: surface normal, w: alpha (flow and jitter).
    pub normal: [f32; 4],
    /// xyz: direction of the path, w: tip angle (radians).
    pub tangent: [f32; 4],
}

fn rand(rng: &mut u64) -> f32 {
    *rng ^= *rng << 13;
    *rng ^= *rng >> 7;
    *rng ^= *rng << 17;
    (*rng >> 40) as f32 / (1u64 << 24) as f32
}

/// Dabs along the path every Spacing percent of Size, at most `MAX_DABS` (spacing widens to fit).
pub fn dabs(curve: &Curve, path: &[PathPoint]) -> Vec<Dab> {
    let radius = curve.size.max(1e-6) * 0.5;
    let mut rng = curve.name.bytes().fold(0x9E3779B97F4A7C15u64, |h, b| (h ^ b as u64).wrapping_mul(0x100000001B3)) | 1;
    let mut make = |p: PathPoint, dir: Vec3| {
        let n = p.normal;
        let t = (dir - n * dir.dot(n)).normalize_or(n.any_orthonormal_vector());
        let b = n.cross(t);
        let r = radius * (1.0 + (rand(&mut rng) - 0.5) * 2.0 * curve.jitter_size).max(0.05);
        let angle = curve.angle.to_radians() + (rand(&mut rng) - 0.5) * std::f32::consts::TAU * curve.jitter_angle;
        let j = (rand(&mut rng) - 0.5, rand(&mut rng) - 0.5);
        let pos = p.pos + (t * j.0 + b * j.1) * 2.0 * curve.jitter_position * radius * 2.0;
        let alpha = curve.flow * (1.0 - rand(&mut rng) * curve.jitter_flow);
        Dab { pos: pos.extend(r).into(), normal: n.extend(alpha).into(), tangent: t.extend(angle).into() }
    };
    match path {
        [] => return Vec::new(),
        [p] => {
            let dir = p.normal.any_orthonormal_vector();
            return vec![make(*p, dir)];
        }
        _ => {}
    }
    let length: f32 = path.windows(2).map(|w| w[0].pos.distance(w[1].pos)).sum();
    let spacing = (curve.size * curve.spacing.clamp(1.0, 200.0) / 100.0).max(length / (MAX_DABS - 1) as f32).max(1e-6);
    let mut out = Vec::new();
    let mut next = 0.0f32;
    let mut walked = 0.0f32;
    for w in path.windows(2) {
        let (a, b) = (w[0], w[1]);
        let len = a.pos.distance(b.pos);
        if len <= 0.0 {
            continue;
        }
        let dir = (b.pos - a.pos) / len;
        while next <= walked + len && out.len() < MAX_DABS {
            let t = (next - walked) / len;
            let p = PathPoint { pos: a.pos.lerp(b.pos, t), normal: a.normal.lerp(b.normal, t).normalize_or(a.normal) };
            out.push(make(p, dir));
            next += spacing;
        }
        walked += len;
    }
    out
}

// ------------------------------------------------------------------ GPU

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Chunk {
    /// xyz: box min, w: first dab (as bits).
    lo: [f32; 4],
    /// xyz: box max, w: dab count (as bits).
    hi: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CurveInfo {
    values: [[f32; 4]; 7],
    /// x: opacity, y: hardness, z: mask value, w: alpha image layer (-1 round).
    params: [f32; 4],
    /// x: channel mask, y: first chunk, z: chunk count.
    range: [u32; 4],
    lo: [f32; 4],
    hi: [f32; 4],
}

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct Params {
    /// x: tile layer in the mesh maps, y: output layer (slot * tiles + tile), z: 1 = mask target,
    /// w: curve count.
    dest: [u32; 4],
}

/// The curve pipeline: draws all of a node's curves into its curve slot, tile by tile.
pub struct CurveGpu {
    pipeline: wgpu::ComputePipeline,
    sampler: wgpu::Sampler,
    dummy_out: Vec<wgpu::Texture>,
    dummy_images: wgpu::Texture,
}

impl CurveGpu {
    pub fn new(gpu: &Gpu) -> Self {
        let d = &gpu.device;
        let module = gpu::shader(d, "curves", include_str!("shaders/curves.wgsl"));
        let pipeline = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("curves"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let sampler = d.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("curve tip"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let tex = |label, format, usage| {
            d.create_texture(&wgpu::TextureDescriptor {
                label: Some(label),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let dummy_out = (0..4).map(|_| tex("curve dummy out", crate::scene::PAINT_FORMAT, wgpu::TextureUsages::STORAGE_BINDING)).collect();
        let dummy_images = tex("curve dummy images", crate::scene::IMAGE_FORMAT, wgpu::TextureUsages::TEXTURE_BINDING);
        Self { pipeline, sampler, dummy_out, dummy_images }
    }

    /// Draws `curves` into pool slot `slot` (all tiles), replacing what was there. Returns each
    /// curve's path, for display.
    pub fn render(&self, gpu: &Gpu, p: &crate::project::Project, curves: &[Curve], slot: u32, mask: bool) -> Vec<Vec<PathPoint>> {
        let geo = p.geo();
        let mut infos = Vec::new();
        let mut chunks = Vec::new();
        let mut all_dabs: Vec<Dab> = Vec::new();
        let paths: Vec<Vec<PathPoint>> = curves.iter().map(|c| path(c, &p.mesh, geo)).collect();
        for (c, path) in curves.iter().zip(&paths) {
            let dabs = dabs(c, path);
            if dabs.is_empty() {
                continue;
            }
            let first_chunk = chunks.len() as u32;
            let (mut clo, mut chi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
            for group in dabs.chunks(CHUNK) {
                let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
                for d in group {
                    // A dab reaches √3 radii: a corner of an alpha tip, one radius off its plane.
                    let (c, r) = (Vec3::from_slice(&d.pos[..3]), d.pos[3] * 1.75);
                    lo = lo.min(c - r);
                    hi = hi.max(c + r);
                }
                let start = all_dabs.len() as u32;
                all_dabs.extend_from_slice(group);
                chunks.push(Chunk { lo: lo.extend(f32::from_bits(start)).into(), hi: hi.extend(f32::from_bits(group.len() as u32)).into() });
                clo = clo.min(lo);
                chi = chi.max(hi);
            }
            let mut mask_bits = 0u32;
            let mut values = [[0f32; 4]; 7];
            for ch in Channel::ALL {
                if c.values.enabled[ch.index()] && p.channels.is_enabled(ch) {
                    mask_bits |= 1 << ch.index();
                }
                let v = c.values.values[ch.index()];
                values[ch.index()] = match ch {
                    Channel::Normal => {
                        let n = Vec3::new(v[0] * 2.0 - 1.0, v[1] * 2.0 - 1.0, v[2] * 2.0 - 1.0).normalize_or(Vec3::Z);
                        [n.x, n.y, n.z, 0.0]
                    }
                    _ => [v[0], v[1], v[2], 0.0],
                };
            }
            infos.push(CurveInfo {
                values,
                params: [c.opacity.clamp(0.0, 1.0), c.hardness, c.values.mask_value, p.scene.image_layer(&c.alpha) as f32],
                range: [mask_bits, first_chunk, chunks.len() as u32 - first_chunk, 0],
                lo: clo.extend(0.0).into(),
                hi: chi.extend(0.0).into(),
            });
        }
        let d = &gpu.device;
        let storage = |label, bytes: &[u8]| {
            d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                // Bindings can't be empty.
                contents: if bytes.is_empty() { &[0u8; 256] } else { bytes },
                usage: wgpu::BufferUsages::STORAGE,
            })
        };
        let curve_buf = storage("curves", bytemuck::cast_slice(&infos));
        let chunk_buf = storage("curve chunks", bytemuck::cast_slice(&chunks));
        let dab_buf = storage("curve dabs", bytemuck::cast_slice(&all_dabs));
        let tiles = p.tile_count();
        let res = p.tile_resolution();
        let pools = crate::paint::pool_textures(p, mask);
        let arr = |t: &wgpu::Texture| t.create_view(&wgpu::TextureViewDescriptor { dimension: Some(wgpu::TextureViewDimension::D2Array), ..Default::default() });
        let outs: Vec<wgpu::TextureView> = (0..crate::scene::PAINT_PLANES).map(|i| if i < pools.len() { arr(pools[i]) } else { arr(&self.dummy_out[i - 1]) }).collect();
        let images = if p.scene.images.res > 1 { p.scene.images.view.clone() } else { arr(&self.dummy_images) };
        let mut enc = d.create_command_encoder(&Default::default());
        for tile in 0..tiles {
            let params = Params { dest: [tile, slot * tiles + tile, mask as u32, infos.len() as u32] };
            let ubuf = d.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("curve params"),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let mut entries = vec![
                wgpu::BindGroupEntry { binding: 0, resource: ubuf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: curve_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: chunk_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: dab_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: wgpu::BindingResource::TextureView(&p.maps.position.view) },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(&p.maps.normal.view) },
                wgpu::BindGroupEntry { binding: 6, resource: wgpu::BindingResource::TextureView(&images) },
                wgpu::BindGroupEntry { binding: 7, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ];
            for (i, v) in outs.iter().enumerate() {
                entries.push(wgpu::BindGroupEntry { binding: 8 + i as u32, resource: wgpu::BindingResource::TextureView(v) });
            }
            let bind = d.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("curves"),
                layout: &self.pipeline.get_bind_group_layout(0),
                entries: &entries,
            });
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipeline);
            pass.set_bind_group(0, &bind, &[]);
            pass.dispatch_workgroups(res.div_ceil(8), res.div_ceil(8), 1);
        }
        gpu.queue.submit([enc.finish()]);
        paths
    }
}

// ------------------------------------------------------------------ editing
// Edits of the selected layer's curves, shared by the Curve tool and scripts
// (docs/reference/curves.md#drawing and #editing). Each is one undo step.

use crate::project::Project;

/// The paint node the selected layer or mask layer paints into.
pub fn target(p: &Project) -> Result<crate::graph::NodeId, String> {
    let sel = p.selection.ok_or("No layer selected.")?;
    crate::ops::paint_target(&p.graph, sel).ok_or_else(|| "Select a paint layer or paint mask to draw curves.".into())
}

/// The surface point under screen pixel `px` of the 3D view.
pub fn surface_point(p: &Project, camera: &crate::camera::Camera, screen: (u32, u32), px: glam::Vec2) -> Option<CurvePoint> {
    let (o, d) = crate::geo::camera_ray(camera, screen, px);
    let hit = p.geo().raycast(&p.mesh, o, d)?;
    Some(CurvePoint { pos: hit.pos.into(), normal: hit.normal.into() })
}

/// World size of one screen pixel at `pos` (for a Screen-size brush).
pub fn world_per_px(camera: &crate::camera::Camera, screen: (u32, u32), pos: Vec3) -> f32 {
    let depth = (pos - camera.eye()).dot(camera.forward()).max(1e-6);
    2.0 * depth * (camera.fov_y * 0.5).tan() / screen.1.max(1) as f32
}

/// The selected curve of the target node.
pub fn selected(p: &Project) -> Option<(crate::graph::NodeId, usize)> {
    let s = p.curve_sel?;
    Some((s.node, s.curve?))
}

fn curves_mut(p: &mut Project, node: crate::graph::NodeId) -> &mut Vec<Curve> {
    &mut p.graph.node_mut(node).curves
}

/// Starts a curve at `first` with the brush's settings, selected and being drawn.
pub fn start(gpu: &Gpu, p: &mut Project, first: CurvePoint, brush: &Brush, values: &BrushValues, size: f32) -> Result<usize, String> {
    let node = target(p)?;
    if p.graph.node(node).curves.len() >= MAX_CURVES {
        return Err(format!("A layer holds at most {MAX_CURVES} curves."));
    }
    let mut c = Curve::from_brush(next_name(&p.graph.node(node).curves), brush, values, size);
    c.points.push(first);
    let index = p.edit(gpu, "Add curve", |g| {
        g.node_mut(node).curves.push(c);
        g.node(node).curves.len() - 1
    });
    p.curve_sel = Some(CurveSelection { node, curve: Some(index), drawing: true });
    Ok(index)
}

/// Inserts a point before point `at` of the selected curve (`at` = point count appends).
pub fn insert_point(gpu: &Gpu, p: &mut Project, at: usize, point: CurvePoint) -> Result<(), String> {
    let (node, i) = selected(p).ok_or("No curve selected.")?;
    let n = p.graph.node(node).curves[i].points.len();
    if n >= MAX_POINTS {
        return Err(format!("A curve has at most {MAX_POINTS} points."));
    }
    if at > n {
        return Err(format!("The curve has {n} points."));
    }
    p.edit(gpu, if at == n { "Add curve point" } else { "Insert curve point" }, |g| g.node_mut(node).curves[i].points.insert(at, point));
    Ok(())
}

/// Moves point `at` of the selected curve. No undo step: the caller makes one per drag.
pub fn move_point(gpu: &Gpu, p: &mut Project, at: usize, point: CurvePoint) -> Result<(), String> {
    let (node, i) = selected(p).ok_or("No curve selected.")?;
    let pts = &mut curves_mut(p, node)[i].points;
    *pts.get_mut(at).ok_or("No such point.")? = point;
    p.graph_changed(gpu);
    Ok(())
}

/// Deletes point `at` of the selected curve; one of its last two deletes the curve.
pub fn delete_point(gpu: &Gpu, p: &mut Project, at: usize) -> Result<(), String> {
    let (node, i) = selected(p).ok_or("No curve selected.")?;
    let n = p.graph.node(node).curves[i].points.len();
    if at >= n {
        return Err(format!("The curve has {n} points."));
    }
    if n <= 2 {
        return delete(gpu, p);
    }
    p.edit(gpu, "Delete curve point", |g| {
        g.node_mut(node).curves[i].points.remove(at);
    });
    Ok(())
}

/// Removes the last point of the curve being drawn; the last one deletes the curve.
pub fn remove_last_point(gpu: &Gpu, p: &mut Project) -> Result<(), String> {
    let (node, i) = selected(p).ok_or("No curve selected.")?;
    let n = p.graph.node(node).curves[i].points.len();
    if n <= 1 {
        return delete(gpu, p);
    }
    p.edit(gpu, "Delete curve point", |g| {
        g.node_mut(node).curves[i].points.pop();
    });
    Ok(())
}

/// Ends drawing. A curve of a single point is deleted.
pub fn finish(gpu: &Gpu, p: &mut Project) {
    let Some(s) = p.curve_sel.filter(|s| s.drawing) else { return };
    p.curve_sel = Some(CurveSelection { drawing: false, ..s });
    let single = s.curve.and_then(|i| p.graph.get(s.node)?.curves.get(i)).is_some_and(|c| c.points.len() < 2);
    if single {
        // Part of drawing it: undoing "Add curve" already removes it.
        let i = s.curve.unwrap();
        p.graph.node_mut(s.node).curves.remove(i);
        p.curve_sel = Some(CurveSelection { node: s.node, curve: None, drawing: false });
        p.graph_changed(gpu);
    }
}

/// Closes the curve being drawn (3 or more points) and finishes it.
pub fn close(gpu: &Gpu, p: &mut Project) -> Result<(), String> {
    let (node, i) = selected(p).ok_or("No curve selected.")?;
    if p.graph.node(node).curves[i].points.len() < 3 {
        return Err("A closed curve needs 3 or more points.".into());
    }
    p.edit(gpu, "Close curve", |g| g.node_mut(node).curves[i].closed = true);
    finish(gpu, p);
    Ok(())
}

/// Deletes the selected curve.
pub fn delete(gpu: &Gpu, p: &mut Project) -> Result<(), String> {
    let (node, i) = selected(p).ok_or("No curve selected.")?;
    p.edit(gpu, "Delete curve", |g| {
        g.node_mut(node).curves.remove(i);
    });
    p.curve_sel = Some(CurveSelection { node, curve: None, drawing: false });
    Ok(())
}

/// Moves the selected curve up (drawn later) or down.
pub fn reorder(gpu: &Gpu, p: &mut Project, up: bool) -> Result<(), String> {
    let (node, i) = selected(p).ok_or("No curve selected.")?;
    let n = p.graph.node(node).curves.len();
    let j = if up { i + 1 } else { i.wrapping_sub(1) };
    if j >= n {
        return Err(if up { "The curve is already at the top." } else { "The curve is already at the bottom." }.into());
    }
    p.edit(gpu, "Move curve", |g| g.node_mut(node).curves.swap(i, j));
    p.curve_sel = Some(CurveSelection { node, curve: Some(j), drawing: false });
    Ok(())
}

/// Selects curve `index` of the target (None deselects).
pub fn select(p: &mut Project, index: Option<usize>) -> Result<(), String> {
    let node = target(p)?;
    if let Some(i) = index {
        let n = p.graph.node(node).curves.len();
        if i >= n {
            return Err(format!("The layer has {n} curves."));
        }
    }
    p.curve_sel = Some(CurveSelection { node, curve: index, drawing: false });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn curve(points: &[[f32; 3]]) -> Curve {
        let mut c = Curve::from_brush("Curve 1".into(), &Brush::default(), &BrushValues::default(), 0.1);
        c.points = points.iter().map(|p| CurvePoint { pos: *p, normal: [0.0, 0.0, 1.0] }).collect();
        c
    }

    #[test]
    fn spline_passes_through_points() {
        let (p0, p1, p2, p3) = (Vec3::ZERO, Vec3::X, Vec3::new(2.0, 1.0, 0.0), Vec3::new(3.0, 1.0, 0.0));
        assert!(catmull_rom(p0, p1, p2, p3, 0.0).distance(p1) < 1e-5);
        assert!(catmull_rom(p0, p1, p2, p3, 1.0).distance(p2) < 1e-5);
        // Collinear points stay on their line.
        let m = catmull_rom(Vec3::ZERO, Vec3::X, Vec3::X * 2.0, Vec3::X * 3.0, 0.5);
        assert!(m.distance(Vec3::X * 1.5) < 1e-5);
    }

    #[test]
    fn dab_spacing_and_cap() {
        let c = curve(&[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]]);
        let path = vec![
            PathPoint { pos: Vec3::ZERO, normal: Vec3::Z },
            PathPoint { pos: Vec3::X, normal: Vec3::Z },
        ];
        // Size 0.1, spacing 10 %: a dab every 0.01 over a length of 1.
        let d = dabs(&c, &path);
        assert!((100..=101).contains(&d.len()), "{}", d.len());
        assert!((d[1].pos[0] - 0.01).abs() < 1e-5);
        // Tips point along the path.
        assert!((Vec3::from_slice(&d[0].tangent[..3]) - Vec3::X).length() < 1e-5);
        let mut long = c.clone();
        long.size = 1e-5;
        assert_eq!(dabs(&long, &path).len(), MAX_DABS);
    }

    #[test]
    fn names() {
        let mut a = curve(&[]);
        assert_eq!(next_name(&[]), "Curve 1");
        a.name = "Curve 7".into();
        let mut b = a.clone();
        b.name = "Seam".into();
        assert_eq!(next_name(&[a, b]), "Curve 8");
    }
}
