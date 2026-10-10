# Mesh maps

Mesh maps are per-tile textures baked from the mesh. Generators, Mesh Map nodes, the paint
buffer and the material read them. They are stored in `maps/` as 16-bit float EXR.

Maps are baked from the imported (low-poly) mesh, or, for the maps marked below, from a
separate [high-poly mesh](#high-poly-mesh) when one is set.

## Maps

| Map | Components | Range | From the high poly | Used by |
|---|---|---|---|---|
| Position | RGB | Object space, normalized to the bounding box: `[0, 1]³` | no | Paint buffer bake, Position Gradient, triplanar projection |
| World Normal | RGB | Object-space normal, encoded `n * 0.5 + 0.5` | no | Paint buffer bake, triplanar projection |
| Ambient Occlusion | 1 | `[0, 1]`, 1 = unoccluded | yes | Dirt |
| Curvature | 1 | `[0, 1]`, 0.5 = flat, > 0.5 convex | yes | Edge Wear, Curvature Mask |
| Thickness | 1 | `[0, 1]`, 1 = thick | yes | Mesh Map node |
| ID | RGB | One color per material (glTF) or `usemtl` group (OBJ) | yes | ID Select |
| Normal | RGB | Tangent-space normal, encoded `n * 0.5 + 0.5`; `(0.5, 0.5, 1)` = flat | only | The material (see [Normal map](#normal-map)), Mesh Map node |
| Height | 1 | `[0, 1]`, 0.5 = on the low-poly surface, 1 = Frontal distance above it, 0 = Rear distance below (see [How texels are baked](#how-texels-are-baked)) | only | Mesh Map node |

Position and World Normal always describe the low-poly surface, because that's the surface
painting projects onto. **Normal** and **Height** need a high-poly mesh; without one they stay
**Missing**.

## Bake settings

**Mesh Maps → Bake settings**:

| Setting | Applies to | Default | Description |
|---|---|---|---|
| Resolution | all | tile resolution | Can be lower than tile resolution to save memory and time. |
| Samples | AO, Thickness | 64 | Rays per texel. |
| Max distance | AO, Thickness | 0.1 | Ray length, relative to the bounding box diagonal. |
| Spread angle | AO | 180° | Cone angle of the AO rays. |
| Ignore backfaces | AO | on | Rays hitting backfaces don't count as occlusion. |
| Curvature radius | Curvature | 0.005 | Sampling radius, relative to the bounding box diagonal. |
| Padding | all | 8 texels | Dilation into empty UV space. |

## High-poly mesh

**Mesh Maps → High poly**:

| Setting | Default | Description |
|---|---|---|
| High poly mesh | none | A `.glb`, `.gltf` or `.obj` file. **Browse…** sets it, **Clear** removes it. It needs no UVs, and is imported with the low-poly mesh's scale and up axis. Up to 10 million triangles. |
| Auto cage | off | Measures Frontal and Rear distance per low-poly vertex from the high poly, instead of using the two settings below (see [Auto cage](#auto-cage)). |
| Frontal distance | 0.01 | How far outside the low-poly surface rays start, relative to the bounding box diagonal of the low-poly mesh. Disabled while Auto cage is on. |
| Rear distance | 0.01 | How far inside the low-poly surface rays still find the high poly, relative to the bounding box diagonal. Disabled while Auto cage is on. |
| Match by name | on | Restricts each low-poly part to the high-poly part with the same name (see below). |
| Apply Normal map | on | Combines the baked Normal map with the material's normal (see below). |

The project stores the high-poly file's path, not the file. It's read while baking and while
the [bake preview](#bake-preview) shows it; the baked maps are saved in the project.

### How texels are baked

For every texel, a ray starts at the low-poly surface point moved **Frontal distance** outward
along the low-poly normal (interpolated across the triangle), and travels inward for **Frontal
distance + Rear distance**. The first front-facing high-poly triangle it meets is the hit.
Back-facing triangles are skipped. With **Auto cage** on, each texel's Frontal and Rear distance
are the cage's, interpolated across the triangle from its vertices.

| Map | Value at a hit |
|---|---|
| Normal | The high poly's interpolated vertex normal at the hit, in the low poly's tangent space. |
| Height | `0.5 + 0.5 × d / max(Frontal, Rear)`, clamped to `[0, 1]`, where `d` is the hit's signed distance from the low-poly surface (positive outside). With Auto cage on, Frontal and Rear are the cage's largest distances, so the scale is the same across the whole map. |
| ID | The color of the hit triangle's material. |
| AO, Curvature, Thickness | Computed at the hit point with the high poly's normal, against the high-poly mesh. |

A texel whose ray finds nothing is a **miss**: Normal is flat, Height 0.5, ID is the low poly's,
and AO, Curvature and Thickness are computed from the low-poly surface against the high poly.

### Auto cage

With **Auto cage** on, Crate measures the cage from the two meshes before projecting:

1. **Samples.** Every high-poly vertex and triangle center is a sample. Above 4 million, an
   evenly spaced subset of 4 million is used.
2. **Matching.** Each sample is matched to the closest point of the low-poly triangles its part
   may hit (see [Match by name](#match-by-name)). Samples farther than 0.1 × the bounding box
   diagonal from all of them are ignored.
3. **Distance.** The sample's distance from that point, divided by `max(cos θ, 0.5)`, where θ
   is the angle between the low poly's interpolated normal there and the triangle's face
   normal, is how far a ray must reach. It counts as frontal when the sample lies on
   the outer side of the triangle's plane, as rear otherwise.
4. **Vertices.** Each low-poly vertex takes the largest frontal and the largest rear distance of
   the samples matched to triangles that use it. Vertices at the same position share their
   values, so the cage has no gaps at hard edges and UV seams.
5. **Margin.** Each distance becomes `1.1 × distance + 0.002 × diagonal`, limited to
   `0.1 × diagonal`. A vertex with no samples gets `0.002 × diagonal`.

The frontal and rear distances of vertices are independent: a low poly that lies inside its high
poly gets a frontal cage and a thin rear one.

### Misses

After a bake, the status bar and the Mesh Maps window show the share of missed texels, for
example **High-poly bake: 0.4% of texels missed**. With Auto cage on, the status bar also shows
the cage's largest distances relative to the bounding box diagonal, for example
**High-poly bake: 0.1% of texels missed (auto cage: up to 0.014 frontal, 0.006 rear)**. While the viewport shows the Normal map
(`Shift+C`, or the script command `view normal-map`), missed texels are drawn red.

### Match by name

Part names come from glTF node names (or the mesh name if the node has none) and OBJ object
names (`o`, or `g` when there are no objects). A part whose name ends in `_low` matches the part
with the same name ending in `_high`, ignoring case: `Bolt_low` matches `bolt_HIGH`.

| Low-poly part | Its rays hit |
|---|---|
| Has a matching high-poly part | Only that high-poly part. |
| No matching high-poly part | Every high-poly part. |

High-poly parts that match no low-poly part can be hit by every unmatched low-poly part. With
**Match by name** off, every ray can hit every part.

### Normal map

The tangent space is the low poly's: MikkTSpace tangents, orthonormalized per pixel, with the
bitangent pointing towards +V (OpenGL convention, "Y+"). This is the same basis the viewport
uses to apply normal maps, and that Blender, Unreal Engine and Unity use with MikkTSpace
tangents. Exports convert to DirectX convention when the export's normal format asks for it.

With **Apply Normal map** on, the viewport and exports use the baked Normal map as the base of
the material's Normal channel: the channel's value is applied on top of it as detail, with
reoriented normal mapping (RNM), so layers that paint or fill normals add their detail to the
baked surface. A Missing Normal map is flat and changes nothing.

### Bake preview

**Mesh Maps → High poly → Show preview** extends the Mesh Maps window with a preview pane: its
own view of the meshes a high-poly bake uses, so you can check the cage before baking. The
viewport is not affected. With the pane, the window can be resized by its edges (980 × 620 points
at first); the settings scroll in a column on the left and the view fills the rest.

| Part | Color | Default opacity | Shows |
|---|---|---|---|
| Low poly | blue | 0.5 | The low-poly mesh. |
| High poly | white | 0.5 | The high-poly mesh. |
| Cage | red | 0.3 | The outer cage: every low-poly vertex moved outward along its normal by **Frontal distance**, or with **Auto cage** on, by the auto cage's frontal distance. |

Each part has a checkbox (all on by default) and an opacity from 0 to 1. All parts are
see-through and drawn without wireframes:

- Of each part, only the surface nearest to the camera is drawn, so a part's back side and
  insides don't show.
- Where parts overlap, they're layered by distance at each pixel: a nearer part covers the
  parts behind it by its opacity, whatever the order of the list.
- Surfaces are lit from the camera. The opacity applies where a surface faces the camera; seen
  at a grazing angle, it is up to twice as opaque, so outlines stay visible. At opacity 1 a part
  is opaque.

**Navigation**, with the pointer over the pane:

| Input | Action |
|---|---|
| Drag | Orbit |
| Middle-drag, or `Shift`+drag | Pan |
| Scroll | Zoom |
| Double-click | Frame the mesh |

The pane's camera starts at the viewport's view each time the pane is shown, and then moves
independently of it.

- **High poly** and **Cage** need a high-poly mesh and are disabled without one.
- While the pane shows **High poly** or **Cage**, the high-poly file is read in the background;
  meanwhile the pane shows **Loading preview…**. With **Auto cage** on, the cage is measured as
  for a bake. The file is read again when it changes, or when **Auto cage** or (with Auto cage
  on) **Match by name** changes. Changes to **Frontal distance** show immediately.
- High-poly detail that pokes out of the red cage lies outside it. The rays start on the cage,
  so they miss that detail (see [Misses](#misses)).
- At hard edges, where a low-poly vertex has several normals, the cage has gaps, just as the
  rays do.
- The rear side (**Rear distance**) is not shown.
- If the high poly can't be read, or needs larger GPU buffers than the GPU allows, the pane
  shows the error, and **High poly** and **Cage** show nothing.
- The pane, checkboxes and opacities are not saved. Opening a project or reimporting its mesh
  resets them.

## Bake status

| Status | Meaning |
|---|---|
| Missing | Never baked. Nodes reading it return the neutral value. |
| Up to date | Baked with the current mesh, high poly and settings. |
| Out of date | The mesh, the high poly or a setting it depends on changed. Old data is still used until rebaked. |

Changing the high-poly mesh, Auto cage, Match by name, or (with Auto cage off) Frontal distance
or Rear distance makes Normal, Height, AO, Curvature, Thickness and ID out of date.

Position and World Normal are baked automatically on import, because painting requires them.

## In the Library

**Library → Mesh Maps** shows every map with a thumbnail of its first UDIM tile and its status:
**not baked** for Missing, an orange name for Out of date. Double-clicking a map adds it as a mask
layer of the selected layer; dragging it onto the Node Editor adds a Mesh Map node. See
[Windows and panels](windows-and-panels.md#library).
