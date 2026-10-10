# How to mask a layer with a generator

This guide shows you how to restrict a layer to part of the mesh, such as worn edges or crevices,
with a generator.

Generators need baked mesh maps. If you haven't baked them, see
[How to bake mesh maps](bake-mesh-maps.md).

## Add the generator

1. Right-click the layer and choose **Add black mask**. The layer disappears.
2. Right-click the mask and choose **Add generator**.
3. Pick a generator:
   - For worn, exposed edges: **Edge Wear**.
   - For grime in crevices: **Dirt**.
   - For a convex or concave selection with no noise: **Curvature Mask**.
   - For a gradient from bottom to top, such as dust or water marks: **Position Gradient**.
4. Adjust the generator's parameters in the **Properties** panel.

## Use a mesh map directly

For a mask straight from a baked map, such as ambient occlusion or thickness, select the layer and
double-click the map in **Library → Mesh Maps**. It's added as a mask layer, which you can refine
like a generator.

## Refine it

- **To sharpen or soften the transition**, add a **Levels** mask layer above the generator, and
  move its input black and white points closer together or further apart.
- **To remove the effect in specific places**, add a Paint mask layer above the generator, set its
  blend mode to **Subtract**, and paint where the effect shouldn't appear.
- **To add the effect in specific places**, add a Paint mask layer with blend mode **Lighten**,
  and paint white.
- **To combine two generators**, add both and set the upper one's blend mode to **Multiply**
  (intersection) or **Lighten** (union).

## Go further

If the mask needs more than a list of mask layers can express, select the mask and press `G` to
open it as a graph. See [How to edit a layer as a node graph](edit-a-layer-as-a-graph.md).
