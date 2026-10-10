# How to bake from a high-poly mesh

This guide shows you how to bake the detail of a high-poly sculpt into normal, height, AO,
curvature, thickness and ID maps for your low-poly mesh.

## Before you start

- Export the high poly as `.glb`, `.gltf` or `.obj`, in the same place and scale as the low
  poly. It doesn't need UVs.
- If the asset has several parts, name them in pairs ending in `_low` and `_high` (`Lid_low`,
  `Lid_high`), so parts don't pick up each other's detail.
- To bake from an FBX file, convert it to glTF first, for example by importing it into Blender
  and choosing **File → Export → glTF 2.0**.

## Bake

1. Open **Window → Mesh Maps**.
2. Under **High poly**, click **Browse…** and choose the high-poly file.
3. Check the maps to bake, including **Normal** and **Height**.
4. Click **Bake selected**.

The status bar reports how many texels missed the high poly. With **Apply Normal map** on (the
default), the viewport shows the low poly with the high poly's detail as soon as the Normal map
is baked.

From the command line, set the high poly when creating the project:

```
crate new lowpoly.glb -o Asset.crate --high-poly highpoly.glb --bake all
```

Add `--auto-cage` to bake with [Auto cage](../reference/mesh-maps.md#auto-cage).

## Fix missed texels

Press `Shift+C` until the viewport shows **Normal**. Red areas missed the high poly.

- **Red in many places, or you don't know the right distances**: turn on **Auto cage** under
  **High poly**. Crate then fits the distances to the high poly around every low-poly vertex.
- **Red where the high poly sticks out further than the low poly**: raise **Frontal distance**
  (with Auto cage off).
- **Red where the high poly lies inside the low poly**: raise **Rear distance** (with Auto cage
  off).
- **Red on a whole part**: check that the part's `_low` and `_high` names match, or turn off
  **Match by name**.

Rebake after each change.

## Fix wrong detail

- **Detail from a neighboring part shows up** (for example a bolt's edge on the plate under it):
  name the parts in `_low`/`_high` pairs and keep **Match by name** on, or turn on **Auto cage**,
  or lower **Frontal distance**.
- **Waviness or seams along hard edges of the low poly**: split those edges in the low poly
  (separate UV islands with hard edges) and reimport, or soften the low poly's normals so the
  rays spread evenly.
- **Inverted lighting in the game engine**: set the export's normal format to the engine's
  convention (Unreal Engine: DirectX). See [Export](../reference/export.md).

## Use the maps

- The Normal map is applied to the material automatically. To bake it without applying it, turn
  off **Apply Normal map**.
- To use Height in a layer or mask, add a **Mesh Map** node with **Map: Height**, or double-click
  **Height** in **Library → Mesh Maps** to add it as a mask.

## Related

- [Mesh maps](../reference/mesh-maps.md#high-poly-mesh) (reference)
- [About high-poly baking](../explanation/about-high-poly-baking.md) (explanation)
