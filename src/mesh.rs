//! Mesh import (glTF 2.0, OBJ), UDIM tile detection and import diagnostics.
//! See docs/reference/mesh-and-udims.md.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use bytemuck::{Pod, Zeroable};
use glam::{Mat4, Vec2, Vec3, Vec4};

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, Pod, Zeroable, PartialEq)]
pub struct Vertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 4],
    /// UDIM convention: v points up, tile 1001 is [0,1)².
    pub uv: [f32; 2],
    pub material: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum TangentMode {
    Auto,
    Always,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum UpAxis {
    Y,
    Z,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct ImportSettings {
    pub uv_set: u32,
    pub compute_tangents: TangentMode,
    pub tile_resolution: u32,
    pub scale: f32,
    pub up_axis: UpAxis,
}

impl Default for ImportSettings {
    fn default() -> Self {
        Self {
            uv_set: 0,
            compute_tangents: TangentMode::Auto,
            tile_resolution: 2048,
            scale: 1.0,
            up_axis: UpAxis::Y,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub code: &'static str,
    pub severity: Severity,
    pub message: String,
}

pub struct Mesh {
    pub source: PathBuf,
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    /// Material names; `Vertex::material` indexes this. Used for the ID map.
    pub materials: Vec<String>,
    /// Sorted UDIM numbers in use.
    pub tiles: Vec<u32>,
    pub bbox_min: Vec3,
    pub bbox_max: Vec3,
    pub diagnostics: Vec<Diagnostic>,
}

impl Mesh {
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn center(&self) -> Vec3 {
        (self.bbox_min + self.bbox_max) * 0.5
    }

    pub fn radius(&self) -> f32 {
        (self.bbox_max - self.bbox_min).length() * 0.5
    }

    pub fn tile_index(&self, udim: u32) -> Option<usize> {
        self.tiles.iter().position(|&t| t == udim)
    }

    pub fn has_errors(&self) -> bool {
        self.diagnostics.iter().any(|d| d.severity == Severity::Error)
    }
}

pub const MAX_TRIANGLES: usize = 10_000_000;

pub fn udim_of(u: f32, v: f32) -> u32 {
    (1001.0 + u.floor() + 10.0 * v.floor()) as u32
}

/// Loads, triangulates and validates a mesh. Errors in the returned diagnostics mean the
/// import must be aborted; I/O and parse failures are returned as `Err`.
pub fn import(path: &Path, settings: &ImportSettings) -> Result<Mesh> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut raw = match ext.as_str() {
        "gltf" | "glb" => load_gltf(path, settings.uv_set)?,
        "obj" => load_obj(path)?,
        _ => bail!("Unsupported mesh format: .{ext}. Use .gltf, .glb or .obj."),
    };
    if raw.indices.len() / 3 > MAX_TRIANGLES {
        bail!(
            "The mesh has {} triangles; the limit is {}.",
            raw.indices.len() / 3,
            MAX_TRIANGLES
        );
    }

    let mut diagnostics = Vec::new();
    if !raw.has_uvs {
        diagnostics.push(Diagnostic {
            code: "UV_MISSING",
            severity: Severity::Error,
            message: "The mesh has no UVs. Import is aborted.".into(),
        });
    }

    // Axis conversion and scale.
    for v in &mut raw.vertices {
        let mut p = Vec3::from(v.pos);
        let mut n = Vec3::from(v.normal);
        let mut t = Vec4::from(v.tangent);
        if settings.up_axis == UpAxis::Z {
            p = Vec3::new(p.x, p.z, -p.y);
            n = Vec3::new(n.x, n.z, -n.y);
            t = Vec4::new(t.x, t.z, -t.y, t.w);
        }
        v.pos = (p * settings.scale).into();
        v.normal = n.normalize_or_zero().into();
        v.tangent = t.into();
    }

    if !raw.has_normals {
        compute_normals(&mut raw.vertices, &raw.indices);
    }

    let regenerate = settings.compute_tangents == TangentMode::Always || !raw.has_tangents;
    if raw.has_uvs && regenerate {
        let (v, i) = generate_tangents(&raw.vertices, &raw.indices);
        raw.vertices = v;
        raw.indices = i;
        if !raw.has_tangents {
            diagnostics.push(Diagnostic {
                code: "TANGENTS_GENERATED",
                severity: Severity::Info,
                message: "Tangents were not present and were generated (MikkTSpace).".into(),
            });
        }
    }

    let mut bbox_min = Vec3::splat(f32::MAX);
    let mut bbox_max = Vec3::splat(f32::MIN);
    for v in &raw.vertices {
        bbox_min = bbox_min.min(Vec3::from(v.pos));
        bbox_max = bbox_max.max(Vec3::from(v.pos));
    }
    if raw.vertices.is_empty() {
        bail!("The mesh contains no triangles.");
    }

    let mut tiles = Vec::new();
    if raw.has_uvs {
        tiles = check_uvs(&raw.vertices, &raw.indices, &mut diagnostics);
    }

    Ok(Mesh {
        source: path.to_path_buf(),
        vertices: raw.vertices,
        indices: raw.indices,
        materials: raw.materials,
        tiles,
        bbox_min,
        bbox_max,
        diagnostics,
    })
}

struct RawMesh {
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    materials: Vec<String>,
    has_uvs: bool,
    has_normals: bool,
    has_tangents: bool,
}

fn load_gltf(path: &Path, uv_set: u32) -> Result<RawMesh> {
    let (doc, buffers, _images) =
        gltf::import(path).with_context(|| format!("Could not read {}", path.display()))?;

    let mut materials: Vec<String> = doc
        .materials()
        .enumerate()
        .map(|(i, m)| m.name().map(str::to_owned).unwrap_or(format!("Material {i}")))
        .collect();
    let default_material = materials.len() as u32;
    let mut uses_default = false;

    let mut out = RawMesh {
        vertices: Vec::new(),
        indices: Vec::new(),
        materials: Vec::new(),
        has_uvs: true,
        has_normals: true,
        has_tangents: true,
    };
    let mut any_primitive = false;

    let scene = doc
        .default_scene()
        .or_else(|| doc.scenes().next())
        .context("The glTF file contains no scene.")?;
    let mut stack: Vec<(gltf::Node, Mat4)> =
        scene.nodes().map(|n| (n, Mat4::IDENTITY)).collect();
    while let Some((node, parent)) = stack.pop() {
        let world = parent * Mat4::from_cols_array_2d(&node.transform().matrix());
        let normal_mat = world.inverse().transpose();
        for child in node.children() {
            stack.push((child, world));
        }
        let Some(mesh) = node.mesh() else { continue };
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                continue;
            }
            any_primitive = true;
            let reader = prim.reader(|b| Some(&buffers[b.index()]));
            let Some(positions) = reader.read_positions() else { continue };
            let positions: Vec<[f32; 3]> = positions.collect();
            let normals: Option<Vec<[f32; 3]>> = reader.read_normals().map(|n| n.collect());
            let tangents: Option<Vec<[f32; 4]>> = reader.read_tangents().map(|t| t.collect());
            let uvs: Option<Vec<[f32; 2]>> =
                reader.read_tex_coords(uv_set).map(|t| t.into_f32().collect());
            out.has_normals &= normals.is_some();
            out.has_tangents &= tangents.is_some();
            out.has_uvs &= uvs.is_some();
            let material = match prim.material().index() {
                Some(i) => i as u32,
                None => {
                    uses_default = true;
                    default_material
                }
            };

            let base = out.vertices.len() as u32;
            for i in 0..positions.len() {
                let p = world.transform_point3(Vec3::from(positions[i]));
                let n = normals
                    .as_ref()
                    .map(|n| normal_mat.transform_vector3(Vec3::from(n[i])).normalize_or_zero())
                    .unwrap_or(Vec3::ZERO);
                let t = tangents
                    .as_ref()
                    .map(|t| {
                        let d = world.transform_vector3(Vec3::new(t[i][0], t[i][1], t[i][2]));
                        d.normalize_or_zero().extend(t[i][3])
                    })
                    .unwrap_or(Vec4::new(1.0, 0.0, 0.0, 1.0));
                // glTF UVs have v pointing down; UDIM convention has v pointing up.
                let uv = uvs.as_ref().map(|u| [u[i][0], 1.0 - u[i][1]]).unwrap_or([0.0; 2]);
                out.vertices.push(Vertex {
                    pos: p.into(),
                    normal: n.into(),
                    tangent: t.into(),
                    uv,
                    material,
                });
            }
            let flip = world.determinant() < 0.0;
            let idx: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            for tri in idx.chunks_exact(3) {
                if flip {
                    out.indices.extend([base + tri[0], base + tri[2], base + tri[1]]);
                } else {
                    out.indices.extend([base + tri[0], base + tri[1], base + tri[2]]);
                }
            }
        }
    }
    if !any_primitive {
        bail!("The glTF file contains no triangle meshes.");
    }
    if uses_default {
        materials.push("Default".into());
    }
    out.materials = materials;
    Ok(out)
}

fn load_obj(path: &Path) -> Result<RawMesh> {
    let (models, materials) = tobj::load_obj(
        path,
        &tobj::LoadOptions {
            triangulate: true,
            single_index: true,
            ..Default::default()
        },
    )
    .with_context(|| format!("Could not read {}", path.display()))?;
    // Material names come from `usemtl`; a missing .mtl file is not an error.
    let mut names: Vec<String> = materials
        .map(|m| m.into_iter().map(|m| m.name).collect())
        .unwrap_or_default();

    let mut out = RawMesh {
        vertices: Vec::new(),
        indices: Vec::new(),
        materials: Vec::new(),
        has_uvs: true,
        has_normals: true,
        has_tangents: false,
    };
    let mut default_material = None;
    for model in &models {
        let m = &model.mesh;
        out.has_uvs &= !m.texcoords.is_empty();
        out.has_normals &= !m.normals.is_empty();
        let material = match m.material_id {
            Some(i) if i < names.len() => i as u32,
            _ => *default_material.get_or_insert_with(|| {
                names.push("Default".into());
                names.len() as u32 - 1
            }),
        };
        let base = out.vertices.len() as u32;
        for i in 0..m.positions.len() / 3 {
            out.vertices.push(Vertex {
                pos: [m.positions[3 * i], m.positions[3 * i + 1], m.positions[3 * i + 2]],
                normal: if m.normals.is_empty() {
                    [0.0; 3]
                } else {
                    [m.normals[3 * i], m.normals[3 * i + 1], m.normals[3 * i + 2]]
                },
                tangent: [1.0, 0.0, 0.0, 1.0],
                uv: if m.texcoords.is_empty() {
                    [0.0; 2]
                } else {
                    [m.texcoords[2 * i], m.texcoords[2 * i + 1]]
                },
                material,
            });
        }
        out.indices.extend(m.indices.iter().map(|i| base + i));
    }
    if out.vertices.is_empty() {
        bail!("The OBJ file contains no faces.");
    }
    out.materials = names;
    Ok(out)
}

fn compute_normals(vertices: &mut [Vertex], indices: &[u32]) {
    // Area-weighted, accumulated over vertices sharing a position so that unwelded
    // OBJ/glTF vertices still produce smooth normals.
    let key = |p: [f32; 3]| (p[0].to_bits(), p[1].to_bits(), p[2].to_bits());
    let mut acc: HashMap<(u32, u32, u32), Vec3> = HashMap::new();
    for tri in indices.chunks_exact(3) {
        let [a, b, c] = [0, 1, 2].map(|k| Vec3::from(vertices[tri[k] as usize].pos));
        let n = (b - a).cross(c - a);
        for k in 0..3 {
            *acc.entry(key(vertices[tri[k] as usize].pos)).or_default() += n;
        }
    }
    for v in vertices {
        v.normal = acc
            .get(&key(v.pos))
            .copied()
            .unwrap_or(Vec3::Y)
            .normalize_or(Vec3::Y)
            .into();
    }
}

/// MikkTSpace works per face corner. Generate on an unwelded copy, then weld identical
/// vertices back together.
fn generate_tangents(vertices: &[Vertex], indices: &[u32]) -> (Vec<Vertex>, Vec<u32>) {
    struct Corners {
        v: Vec<Vertex>,
    }
    impl mikktspace::Geometry for Corners {
        fn num_faces(&self) -> usize {
            self.v.len() / 3
        }
        fn num_vertices_of_face(&self, _face: usize) -> usize {
            3
        }
        fn position(&self, face: usize, vert: usize) -> [f32; 3] {
            self.v[face * 3 + vert].pos
        }
        fn normal(&self, face: usize, vert: usize) -> [f32; 3] {
            self.v[face * 3 + vert].normal
        }
        fn tex_coord(&self, face: usize, vert: usize) -> [f32; 2] {
            self.v[face * 3 + vert].uv
        }
        fn set_tangent_encoded(&mut self, tangent: [f32; 4], face: usize, vert: usize) {
            self.v[face * 3 + vert].tangent = tangent;
        }
    }
    let mut corners = Corners {
        v: indices.iter().map(|&i| vertices[i as usize]).collect(),
    };
    if !mikktspace::generate_tangents(&mut corners) {
        return (vertices.to_vec(), indices.to_vec());
    }
    let mut map: HashMap<Vec<u8>, u32> = HashMap::new();
    let mut out_v = Vec::with_capacity(vertices.len());
    let mut out_i = Vec::with_capacity(indices.len());
    for v in corners.v {
        let key = bytemuck::bytes_of(&v).to_vec();
        let idx = *map.entry(key).or_insert_with(|| {
            out_v.push(v);
            out_v.len() as u32 - 1
        });
        out_i.push(idx);
    }
    (out_v, out_i)
}

/// Determines the tiles in use and reports UV_OUT_OF_RANGE, UV_CROSSES_TILE and UV_OVERLAP.
fn check_uvs(vertices: &[Vertex], indices: &[u32], diags: &mut Vec<Diagnostic>) -> Vec<u32> {
    const EPS: f32 = 1e-5;
    let mut out_of_range = 0usize;
    let mut crossing = 0usize;
    let mut tiles = std::collections::BTreeSet::new();
    for tri in indices.chunks_exact(3) {
        let uv = [0, 1, 2].map(|k| Vec2::from(vertices[tri[k] as usize].uv));
        let min = uv[0].min(uv[1]).min(uv[2]);
        let max = uv[0].max(uv[1]).max(uv[2]);
        if min.x < -EPS || min.y < -EPS || max.x > 10.0 + EPS || max.y > 10.0 + EPS {
            out_of_range += 1;
            continue;
        }
        let lo = (min + EPS).floor();
        let hi = (max - EPS).floor().max(lo);
        if lo != hi {
            crossing += 1;
        }
        for ty in lo.y as i32..=hi.y as i32 {
            for tx in lo.x as i32..=hi.x as i32 {
                if (0..10).contains(&tx) && (0..10).contains(&ty) {
                    tiles.insert(1001 + tx as u32 + 10 * ty as u32);
                }
            }
        }
    }
    if out_of_range > 0 {
        diags.push(Diagnostic {
            code: "UV_OUT_OF_RANGE",
            severity: Severity::Error,
            message: format!(
                "{out_of_range} triangle(s) have UVs that are negative or beyond u = 10 / v = 10."
            ),
        });
    }
    if crossing > 0 {
        diags.push(Diagnostic {
            code: "UV_CROSSES_TILE",
            severity: Severity::Warning,
            message: format!("{crossing} triangle(s) span more than one UDIM tile."),
        });
    }
    let overlapping = overlap_texels(vertices, indices);
    if overlapping.0 > 0 && overlapping.0 * 1000 > overlapping.1 {
        diags.push(Diagnostic {
            code: "UV_OVERLAP",
            severity: Severity::Warning,
            message: format!(
                "Triangles share UV space ({:.1} % of covered area). Painting one also paints the other.",
                100.0 * overlapping.0 as f64 / overlapping.1.max(1) as f64
            ),
        });
    }
    tiles.into_iter().collect()
}

/// Coarse UV-space rasterization. Returns (overlapping cells, covered cells).
fn overlap_texels(vertices: &[Vertex], indices: &[u32]) -> (usize, usize) {
    const RES: f32 = 256.0;
    let mut cells: HashMap<(i32, i32), u8> = HashMap::new();
    for tri in indices.chunks_exact(3) {
        let p = [0, 1, 2].map(|k| Vec2::from(vertices[tri[k] as usize].uv) * RES);
        let den = (p[1].y - p[2].y) * (p[0].x - p[2].x) + (p[2].x - p[1].x) * (p[0].y - p[2].y);
        if den.abs() < 1e-9 {
            continue;
        }
        let min = p[0].min(p[1]).min(p[2]).floor();
        let max = p[0].max(p[1]).max(p[2]).ceil();
        if (max - min).x * (max - min).y > 1.0e6 {
            continue;
        }
        for y in min.y as i32..max.y as i32 {
            for x in min.x as i32..max.x as i32 {
                let c = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                let a = ((p[1].y - p[2].y) * (c.x - p[2].x) + (p[2].x - p[1].x) * (c.y - p[2].y)) / den;
                let b = ((p[2].y - p[0].y) * (c.x - p[2].x) + (p[0].x - p[2].x) * (c.y - p[2].y)) / den;
                let g = 1.0 - a - b;
                if a > 1e-4 && b > 1e-4 && g > 1e-4 {
                    let e = cells.entry((x, y)).or_insert(0);
                    *e = e.saturating_add(1);
                }
            }
        }
    }
    let over = cells.values().filter(|&&c| c > 1).count();
    (over, cells.len())
}

/// Unique triangle edges, for wireframe display.
pub fn edge_list(indices: &[u32], vertices: &[Vertex]) -> Vec<u32> {
    let key = |i: u32| {
        let p = vertices[i as usize].pos;
        (p[0].to_bits(), p[1].to_bits(), p[2].to_bits())
    };
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for tri in indices.chunks_exact(3) {
        for (a, b) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let (ka, kb) = (key(a), key(b));
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            if seen.insert(k) {
                out.extend([a, b]);
            }
        }
    }
    out
}
