# How to bake stacked UVs once

This guide shows you how to bake mesh maps for repeated objects that share UV space (bolts,
planks, tiles) from one prepared copy, so the others don't spoil the shared texels.

## Find the stacks

1. Press `X` for the Bake Exclude tool.
2. Look at the mesh: islands that share UV space are tinted, one color per stack. All the bolts
   stacked on one island have the same color.

## Choose the copy that bakes

1. In the toolbar, set **Select** to Click and **Fill by** to UV island.
2. Click the copy you prepared for baking: the one whose surroundings give the best ambient
   occlusion, or whose high poly is cleanest. `Shift` + click a copy of every other stack you
   want to set at the same time.
3. Click **Make bake source**. The chosen copies get an outline in their stack's color; the
   other copies are excluded from baking.

## Exclude other geometry

To leave out geometry that isn't stacked, select it with any Select and Fill by setting and click
**Exclude**. Darkened geometry is excluded. **Include** brings it back.

## Bake

1. Open the Mesh Maps window (**Bake Mesh Maps**). Maps baked before are now Out of date.
2. Bake them again.

Paint the bake source when painting by projection: the copies show the same texels.

See [Bake exclusion](../reference/bake-exclusion.md) for what exclusion changes.
