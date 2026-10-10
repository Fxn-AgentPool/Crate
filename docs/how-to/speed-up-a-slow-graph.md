# How to speed up a slow graph

This guide shows you how to restore interactive frame rates when a project becomes slow.

## Find the cause

Open **Window → Performance**. It lists frame time, shader length, GPU cache use and the cost of
each cached node.

## Long shader

If **Shader length** is high and frame time grows with each layer:

1. Find a group or subgraph that you've finished editing.
2. Select its top node in the Node Editor and turn on **Cached**.

Everything below that node is now evaluated once per tile and read back as a texture.

## Cached nodes recomputing too often

If frame time spikes while you paint or adjust parameters:

- Check whether a cached node sits downstream of the layer you're editing. Every edit upstream
  recomputes it. Move filters such as **Blur** below the content they process rather than the
  whole stack, if you can.
- Lower **Blur** radii. Recompute cost grows with radius.

## GPU memory full

If **GPU cache** is near its budget, or a warning reports evictions:

- Turn **Cached** off for nodes that don't need it. Filters can't be uncached.
- Lower the mesh map **Resolution** and rebake. See [How to bake mesh maps](bake-mesh-maps.md).
- Delete or merge unused paint layers.
- If the GPU has spare memory, raise **Preferences → Performance → GPU cache budget**.

## Painting feels laggy

- Lower **Paint Buffer → Resolution scale** to 1× or 0.5×.
- In Live mode, lower brush **Spacing** only as far as you need. Each dab is a bake.

For the reasons behind these steps, see
[About realtime evaluation and caching](../explanation/about-realtime-evaluation.md).
