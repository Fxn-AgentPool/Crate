# Mesh maps

Mesh maps are per-tile textures baked from the mesh itself. Generators, Mesh Map nodes and the
paint buffer read them. They are stored in `maps/` as 16-bit float EXR.

The MVP bakes from the imported mesh only. Baking from a separate high-poly mesh is not supported.

## Maps

| Map | Components | Range | Used by |
|---|---|---|---|
| Position | RGB | Object space, normalized to the bounding box: `[0, 1]³` | Paint buffer bake, Position Gradient, triplanar projection |
| World Normal | RGB | Object-space normal, encoded `n * 0.5 + 0.5` | Paint buffer bake, triplanar projection |
| Ambient Occlusion | 1 | `[0, 1]`, 1 = unoccluded | Dirt |
| Curvature | 1 | `[0, 1]`, 0.5 = flat, > 0.5 convex | Edge Wear, Curvature Mask |
| Thickness | 1 | `[0, 1]`, 1 = thick | Mesh Map node |
| ID | RGB | One color per material (glTF) or `usemtl` group (OBJ) | ID Select |

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

## Bake status

| Status | Meaning |
|---|---|
| Missing | Never baked. Nodes reading it return the neutral value. |
| Up to date | Baked with the current mesh and settings. |
| Out of date | The mesh or settings changed. Old data is still used until rebaked. |

Position and World Normal are baked automatically on import, because painting requires them.
