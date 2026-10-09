# Mesh import and UDIM tiles

## Supported formats

| Format | Extensions | Notes |
|---|---|---|
| glTF 2.0 | `.gltf`, `.glb` | Recommended. First UV set (`TEXCOORD_0`) is used. |
| Wavefront OBJ | `.obj` | `vt` coordinates are used. `.mtl` material names become IDs. |

Meshes are triangulated on import. Quads and n-gons are triangulated by ear clipping.

## Import settings

| Setting | Default | Description |
|---|---|---|
| UV set | `0` | Index of the UV set used for texturing (glTF only). |
| Compute tangents | `Auto` | `Auto` uses tangents from the file if present, otherwise generates MikkTSpace tangents. `Always` regenerates. |
| Tile resolution | `2048` | `1024`, `2048` or `4096`. Applies to all tiles. |
| Scale | `1.0` | Uniform scale applied to positions. Affects world-unit brush sizes and mesh map distances. |
| Up axis | `Y` | `Y` or `Z`. |

## UDIM tile numbering

A UV coordinate `(u, v)` lies in tile:

```
udim = 1001 + floor(u) + 10 * floor(v)
```

- `u` must be in `[0, 10)`. `v` must be in `[0, 10)`.
- Tile range: `1001` – `1100`.
- A triangle whose UVs span more than one tile is reported as an import warning. Its texels are
  assigned per-texel, so it textures correctly, but bleeding across tile edges is not padded.

## Import diagnostics

| Code | Severity | Meaning |
|---|---|---|
| `UV_MISSING` | Error | The mesh has no UVs. Import is aborted. |
| `UV_OUT_OF_RANGE` | Error | Some UVs are negative or have `u ≥ 10` / `v ≥ 10`. |
| `UV_CROSSES_TILE` | Warning | A triangle spans more than one tile. |
| `UV_OVERLAP` | Warning | Two or more triangles share UV space. Painting one also paints the other. |
| `TANGENTS_GENERATED` | Info | Tangents were not present and were generated. |

## Reimport

**File → Reimport Mesh** replaces the mesh while keeping layers and paint.

- Paint is stored in UV space, so it follows the new mesh as long as UVs are unchanged.
- If the set of used tiles changes, paint in removed tiles is kept on disk but not displayed.
- Mesh maps are marked out of date and must be rebaked.
