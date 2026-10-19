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

Each diagnostic lists where the problem is, by part (glTF node or mesh name, OBJ object name),
up to 20 lines:

| Code | Detail per line |
|---|---|
| `UV_MISSING`, `TANGENTS_GENERATED` | The parts that lack them; nothing when every part does. |
| `UV_OUT_OF_RANGE` | Part, triangle count, and the UV range of its offending triangles. |
| `UV_CROSSES_TILE` | Part, triangle count, and the tiles its crossing triangles span. |
| `UV_OVERLAP` | The two parts that share UV space (or one part with itself), their share of the covered area, and the tiles. |

## Reimport

**File → Reimport Mesh** replaces the mesh while keeping layers and paint.

- If every UV is unchanged (same triangles, same UVs to within 0.000001), paint texels are kept
  as they are.
- Otherwise the paint of every Paint and Paint Mask layer is **transferred** to the new UVs:
  - For each texel the new UVs cover, Crate takes its point on the new surface (from the
    Position and World Normal maps, which a reimport bakes first) and looks along the surface
    normal, in front and behind, for the nearest point of the old mesh within 1 % of the new
    mesh's bounding-box diagonal.
  - The texel gets the old paint at that point's UV (the nearest old texel, all channels and
    coverage). Where nothing is found, the texel is left unpainted.
  - Paint in tiles the new mesh no longer uses is transferred like the rest, and their pages are
    deleted from the project on the next save.
  - While it runs, the old and the new paint are both on the GPU: paint takes twice its usual
    GPU memory (see [Limits](limits.md)).
  - [2D paint](2d-paint.md#reimport) isn't transferred; its texels stay where they are in UV
    space.
  - [Curves](curves.md#saving-and-copying) aren't transferred either: they are drawn again on
    the new surface, from their points.
  - When done, the status bar (or the command line) reports **Paint transferred to the new UVs**
    and, if any texels found no old surface, the share of the new UV area they make up, as
    **N % of the UV area found no old surface**.
- With `--no-transfer` ([command line](command-line.md#reimport), [script](script-commands.md)),
  paint texels are kept where they are in UV space even when UVs changed. Paint in tiles the new
  mesh no longer uses is then kept on disk but not displayed.
- Mesh maps are marked out of date and must be rebaked.
- [Bake exclusion](bake-exclusion.md#saving-and-reimport) is kept if the triangles and UVs are
  unchanged, and cleared otherwise.
- The undo history is cleared.

See [About keeping paint when UVs change](../explanation/about-paint-transfer.md) for why paint
is transferred rather than repainted, and what that costs.
