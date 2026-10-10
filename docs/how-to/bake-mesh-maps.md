# How to bake mesh maps

This guide shows you how to bake the mesh maps that generators and ID masks rely on.

Position and World Normal are baked on import. Ambient Occlusion, Curvature, Thickness and ID
must be baked by you. To bake them, and a Normal and Height map, from a detailed sculpt, see
[How to bake from a high-poly mesh](bake-from-a-high-poly-mesh.md).

## Bake

1. Open **Window → Mesh Maps**.
2. Check the maps to bake. Maps marked **Missing** or **Out of date** are pre-checked.
3. Click **Bake selected**.

Baking runs in the background. You can keep working; generators update when each map finishes.

## Adjust the results

- **If AO is too dark in crevices that should be lit**, lower **Max distance**.
- **If AO is noisy**, raise **Samples**.
- **If closed parts of the mesh darken each other**, for example separate bolts sitting on a plate,
  turn on **Ignore backfaces** or move the parts apart in your modeling tool.
- **If Curvature picks up mesh faceting instead of edges**, raise **Curvature radius**.
- **If Edge Wear ignores small details**, lower **Curvature radius**.
- **To save memory and bake time on large assets**, set **Resolution** below the tile resolution.
  Generators remain usable at half resolution.

Rebake after each change. To check the maps, look at their thumbnails in **Library → Mesh Maps**,
or press `Shift+C` in the viewport to cycle through them.

See [Mesh maps](../reference/mesh-maps.md) for every setting and value range.
