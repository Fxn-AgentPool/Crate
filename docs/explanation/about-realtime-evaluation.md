# About realtime evaluation and caching

A texturing project can contain dozens of layers, each with procedural masks, across many UDIM
tiles at 4K. Evaluating all of that into textures every time something changes would be far too
slow for interactive work. Crate stays realtime by evaluating as little as possible, and in the
right place.

## Evaluating where the pixels are

Crate compiles the node graph into a single GPU shader. In the viewport, that shader runs
**once per screen pixel**: for each pixel it looks up the surface position and UV, then evaluates
the whole graph for that point.

The cost of drawing a frame therefore depends on screen resolution and graph complexity, not on
how many tiles the asset has or how large they are. A 100-tile asset at 4K costs about the same to
display as a single 1K tile. Only stored data has to exist per texel: paint pages, images and
mesh maps. Procedural nodes are computed on demand.

Export uses the same compiled graph, but runs it once per texel of each tile instead of once per
screen pixel.

## Why some nodes must be cached

Per-pixel evaluation only works for **local** operations, those that compute a texel from that
texel's inputs alone. Noise, levels, merges and fills are local.

Some operations need *neighboring* texels: a blur averages a neighborhood, and dilation looks for
the nearest non-empty texel. In a per-pixel shader, each neighbor would require re-evaluating the
entire upstream graph at a different point, multiplying the cost by the size of the
neighborhood. Worse, "neighbor" means neighbor in UV space, which has no direct relationship to
neighboring screen pixels.

These nodes are therefore always **cached**. Their input is evaluated in UV space, per tile, into
a texture. The filter runs on that texture, and downstream nodes read the result like an image.
Any node can be cached manually too, which helps when an expensive local subgraph feeds a lot of
downstream work.

## Invalidation

Cached results are recomputed only when something upstream changes:

- Each node has a content version. Editing a parameter, painting into a Paint node or rebaking a
  mesh map bumps it.
- A cached result records the versions it was computed from. If any differ, it is stale.
- Stale results are recomputed before the next frame or export, for every tile.
- Painting marks caches that read paint as stale, so a cached blur below a Paint node follows
  the stroke as you paint. (Recomputing only the pages a stroke touched is a planned
  optimization.)

## Shader compilation

Changing the *structure* of a graph (adding, removing or reconnecting nodes) requires a new shader.
Changing a *parameter* does not, because parameters are passed as uniforms. Compiled shaders are
cached by a hash of the graph structure, so toggling a layer off and on again doesn't recompile.

A recompile takes a moment and blocks the frame it happens in, which is why parameter edits
are designed never to need one.

## Trade-offs

- Very deep graphs mean a long shader, which costs time on every frame. Caching a subgraph turns
  it into a single texture read.
- Caching costs GPU memory per tile. On a 100-tile 4K asset, one cached Color node is around
  10 GB at full resolution. The Performance window compares paint and cache memory with the
  GPU cache budget; the MVP warns when it is exceeded but does not yet evict caches.

## Related

- [How to speed up a slow graph](../how-to/speed-up-a-slow-graph.md)
- [Nodes](../reference/nodes.md) and [Limits and requirements](../reference/limits.md) (reference)
