# Nodes

The **Node Editor** shows the graph behind a layer, mask or stack. Open it with `G` on a selected
layer, or with **Window → Node Editor**.

## Socket types

| Type | Color | Value |
|---|---|---|
| Float | gray | One scalar per texel. |
| Color | yellow | RGB per texel. |
| Material | green | One value for every enabled [channel](channels.md), plus a per-channel coverage. |

Implicit conversions:

| From → To | Rule |
|---|---|
| Float → Color | `(x, x, x)` |
| Color → Float | Luminance (Rec. 709 weights) |
| Float/Color → Material | Not implicit. Use **Material Combine**. |
| Material → Float/Color | Not implicit. Use **Material Split**. |

## Node properties

Every node has:

| Property | Description |
|---|---|
| Id | Unique, read-only. |
| Label | Optional display name. |
| Cached | If on, the node's output is evaluated in UV space and stored per tile. See [About realtime evaluation](../explanation/about-realtime-evaluation.md). Available for nodes with one Material output, or at most one Color and one Float output. A Stack used as a Group layer is not read from its cache. |
| Exposed parameters | Parameters marked **Expose** appear on the owning layer in the Layers panel and on smart materials. |

## Node catalog

### Sources

| Node | Outputs | Parameters |
|---|---|---|
| Paint | Material | Channels painted. Holds paint pages. |
| Paint Mask | Float | Holds single-channel paint pages. |
| Constant | Float / Color | Value. |
| Image | Color, Float (alpha) | Image file, projection (UV / Triplanar), scale, rotation, offset. |
| Mesh Map | Float or Color | Map: AO, Curvature, Thickness, World Normal, Position, ID. |

### Procedural

| Node | Outputs | Parameters |
|---|---|---|
| Noise | Float | Type (Perlin, Simplex, Value), scale, octaves, lacunarity, gain, seed. Evaluated in object space. |
| Cellular | Float | Scale, jitter, distance metric (Euclidean, Manhattan), output (F1, F2, F2−F1), seed. |
| Gradient | Float | Axis (X, Y, Z, custom), start, end, space (Object, World). |
| ID Select | Float | Selected ID colors, tolerance. Reads the ID mesh map. |

### Generators

Generators are built-in subgraphs. **Ungroup** (Node Editor, Properties) turns a generator into
its component nodes (Mesh Map, Noise, Levels, Math, Invert, Gradient), with its current
parameter values baked into them. The result closely matches the generator.

| Node | Outputs | Parameters |
|---|---|---|
| Edge Wear | Float | Amount, sharpness, noise scale, noise amount, seed. Uses Curvature. |
| Dirt | Float | Amount, contrast, noise scale, seed. Uses AO. |
| Curvature Mask | Float | Convex/concave selection, threshold, softness. |
| Position Gradient | Float | Axis, falloff, noise amount. Uses Position. |

### Adjust

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Levels | Float/Color → same | In black, in white, gamma, out black, out white. |
| Ramp | Float → Color | Color stops. |
| Invert | Float/Color → same | — |
| Clamp | Float/Color → same | Min, max. |
| HSV Adjust | Color → Color | Hue shift, saturation, value. |
| Math | Float, Float → Float | Operation: Add, Subtract, Multiply, Divide, Power, Min, Max, Abs, Smoothstep. |

### Combine

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Merge | A, B, Mask → same type | Blend mode, opacity. See [Blend modes](blend-modes.md). |
| Material Combine | Float/Color per channel → Material | Channels enabled. |
| Material Split | Material → Float/Color per channel | — |
| Fill | — → Material | Same as [Fill layer properties](layers.md#fill-layer-properties). |
| Stack | Material slots → Material | Ordered layer slots. Displayed by the Layers panel. |
| Mask Stack | Float slots → Float | Ordered mask layer slots. |

### Filters (non-local)

Filters read neighboring texels. They are always **Cached**; the property cannot be turned off.

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Blur | Float/Color → same | Radius (texels at tile resolution). |
| Dilate | Float → Float | Radius. |
| Height to Normal | Float → Color | Strength. Used internally for export. |

### Output

| Node | Inputs | Description |
|---|---|---|
| Material Output | Material | One per project. Its input is the root Stack. |

## Graph rules

- The graph must be acyclic. Connections that would create a cycle are rejected.
- A Stack node may be referenced by any number of layers but appears in the Layers panel only
  when it is the root stack or a Group.
- Deleting a node with connections removes those connections.
