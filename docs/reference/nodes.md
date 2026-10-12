# Nodes

The [Node Editor](node-editor.md) shows the graph behind a layer, mask or stack, or the whole
project. Open it with `G` on a selected layer, with `Ctrl+Space`, or with **Window → Node Editor**.

## Socket types

| Type | Color | Value |
|---|---|---|
| Float | gray | One scalar per texel. |
| Color | yellow | RGB per texel. |
| Vector | purple | Three numbers (X, Y, Z) per texel: a coordinate, a direction or an offset. |
| Material | green | One value for every enabled [channel](channels.md), plus a per-channel coverage. |

Implicit conversions:

| From → To | Rule |
|---|---|
| Float → Color or Vector | `(x, x, x)` |
| Color → Float | Luminance (Rec. 709 weights) |
| Vector → Float | Average of X, Y and Z |
| Color ↔ Vector | Components unchanged (R = X, G = Y, B = Z) |
| Float/Color/Vector → Material | Not implicit. Use **Material Combine**. |
| Material → Float/Color/Vector | Not implicit. Use **Material Split**. |

## Node properties

Every node has:

| Property | Description |
|---|---|
| Id | Unique, read-only. |
| Label | Optional display name. |
| Cached | If on, the node's output is evaluated in UV space and stored per tile. See [About realtime evaluation](../explanation/about-realtime-evaluation.md). Available for nodes with one Material output, or at most one Color and one Float output and no Vector output. A Stack used as a Group layer is not read from its cache. |
| Exposed parameters | Parameters marked **Expose** appear on the owning layer in the Layers panel and on smart materials. |

## Categories

Every node belongs to one category. The category sets the color of the node's header in the
Node Editor and where the node is listed in the add menu, in this order:

| Category | Header color | Nodes |
|---|---|---|
| Input | rose `#83314A` | Value, Color, Vector, UV, Geometry, Mesh Map, ID Select |
| Texture | brown `#79461D` | Image, Noise, Cellular, Gradient |
| Generator | green `#1F7050` | Edge Wear, Dirt, Curvature Mask, Position Gradient |
| Color | olive `#6C6C00` | Levels, Invert, HSV Adjust, Merge |
| Vector | indigo `#3C3C83` | UV Transform, Mapping, Vector Math |
| Converter | blue `#246283` | Math, Clamp, Ramp, Combine XYZ, Separate XYZ, Material Combine, Material Split |
| Filter | purple `#6C3B75` | Blur, Dilate, Height to Normal |
| Layer | dark green `#2D6A2D` | Paint, Paint Mask, Fill, Stack, Mask Stack |
| Output | dark red `#4D0017` | Material Output (not in the add menu) |

Layer groups and the Layer node of the Node Editor have their own header colors; see
[Node Editor](node-editor.md#layer-groups).

## Node catalog

An input listed with a parameter in parentheses, like **B** (*b value*), uses that parameter
while nothing is connected to it. In the Node Editor the parameter's field is shown on the
input's row, and disappears while a link is connected.

### Input

| Node | Outputs | Parameters |
|---|---|---|
| Value | Value (Float) | Value, 0–1, default 1. |
| Color | Color (Color) | Color, default `(0.5, 0.5, 0.5)` (linear). |
| Vector | Vector (Vector) | Vector: X, Y, Z, default `(0, 0, 0)`. |
| UV | UV (Vector) | — . The texel's UV within its UDIM tile: X = U, from 0 at the tile's left edge to 1 at its right edge; Y = V, from 0 at the bottom to 1 at the top; Z = 0. This is the coordinate the Image node's UV projection uses. |
| Geometry | Position, Generated, Normal (Vector) | — . Position: the object-space position. Generated: the position normalized to the mesh's bounding box, 0–1 on each axis (what Noise, Cellular and Gradient use by default). Normal: the object-space normal. |
| Mesh Map | Float or Color | Map: AO, Curvature, Thickness, World Normal, Position, ID, Normal, Height. Normal outputs the tangent-space normal as a color (`n * 0.5 + 0.5`). |
| ID Select | Float | Selected ID colors, tolerance. Reads the ID mesh map. |

### Texture

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Image | Vector → Color, Alpha (Float) | Image file, sRGB, projection (UV / Triplanar), scale, rotation, offset, triplanar blend. With **Vector** connected, the image is sampled at the vector's X and Y as a UV, after its scale, rotation and offset; the projection is not used. A **UV** node connected to **Vector** gives the same result as the UV projection. |
| Noise | Vector → Float | Type (Perlin, Simplex, Value), scale, octaves, lacunarity, gain, seed. Evaluated at **Vector** × scale; unconnected, at the Generated position (object space normalized to the bounding box). |
| Cellular | Vector → Float | Scale, jitter, distance metric (Euclidean, Manhattan), output (F1, F2, F2−F1), seed. Coordinates as for Noise. |
| Gradient | Vector → Float | Axis (X, Y, Z, custom), start, end, space (Object, World). With **Vector** connected, the gradient runs along the axis through the vector, and space is not used. |

### Generator

Generators are built-in subgraphs. **Ungroup** (Node Editor, Properties) turns a generator into
its component nodes (Mesh Map, Noise, Levels, Math, Invert, Gradient), with its current
parameter values baked into them. The result closely matches the generator.

| Node | Outputs | Parameters |
|---|---|---|
| Edge Wear | Float | Amount, sharpness, noise scale, noise amount, seed. Uses Curvature. |
| Dirt | Float | Amount, contrast, noise scale, seed. Uses AO. |
| Curvature Mask | Float | Convex/concave selection, threshold, softness. |
| Position Gradient | Float | Axis, falloff, noise amount. Uses Position. |

### Color

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Levels | Color → Color, Float | In black, in white, gamma, out black, out white. |
| Invert | Color → Color, Float | — |
| HSV Adjust | Color → Color | Hue shift, saturation, value. |
| Merge | A, B, Mask → Color, Float | Blend mode, opacity. See [Blend modes](blend-modes.md). |

### Vector

An unconnected **Vector** input of these nodes reads the UV, as the **UV** node outputs it.

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| UV Transform | Vector → Vector | Tile U, tile V (repeats, 0–1000, default 1), rotation (degrees, −360–360, default 0), offset U, offset V (−1000–1000, default 0). Repeats, rotates and moves UVs about the tile's center: `out.xy = rotate(in.xy − 0.5, rotation) × (tile U, tile V) + 0.5 + (offset U, offset V)`. Z passes through. |
| Mapping | Vector → Vector | Location, rotation (degrees, applied about X, then Y, then Z) and scale, each X, Y, Z; defaults `(0, 0, 0)`, `(0, 0, 0)` and `(1, 1, 1)`. `out = rotate(in × scale) + location`, like Blender's Mapping node of type Point. |
| Vector Math | A, B (*b value*) → Vector, Value (Float) | Operation (default Add), *b value* (X, Y, Z, default `(0, 0, 0)`), scale (−1000–1000, default 1). See below. |

Vector Math operations. An unconnected **A** is `(0, 0, 0)`; the output an operation doesn't
compute is 0.

| Operation | Vector | Value |
|---|---|---|
| Add, Subtract, Multiply, Divide | `A + B`, `A − B`, `A × B`, `A ÷ B` per component; dividing by 0 gives 0 | — |
| Scale | `A × scale` | — |
| Dot Product | — | `A · B` |
| Cross Product | `A × B` | — |
| Length | — | length of A |
| Distance | — | length of `A − B` |
| Normalize | A divided by its length; 0 for a zero vector | — |
| Absolute, Floor, Fraction | per component of A | — |
| Minimum, Maximum | per component of A and B | — |

### Converter

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Math | A, B (*b value*) → Float | Operation: Add, Subtract, Multiply, Divide, Power, Min, Max, Abs, Smoothstep; *b value*, −10–10, default 1. |
| Clamp | Color → Color, Float | Min, max. |
| Ramp | Float → Color | Color stops. |
| Combine XYZ | X (*x*), Y (*y*), Z (*z*) → Vector | X, Y, Z, −1000–1000, default 0. |
| Separate XYZ | Vector → X, Y, Z (Float) | — . An unconnected **Vector** is `(0, 0, 0)`. |
| Material Combine | Float/Color per channel → Material | — |
| Material Split | Material → Float/Color per channel | — |

### Filter

Filters read neighboring texels (they are non-local). They are always **Cached**; the property
cannot be turned off.

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Blur | Color → Color, Float | Radius (texels at tile resolution). |
| Dilate | Float → Float | Radius. |
| Height to Normal | Float → Color | Strength. Used internally for export. |

### Layer

| Node | Inputs → Outputs | Parameters |
|---|---|---|
| Paint | — → Material | Channels painted. Holds paint pages. |
| Paint Mask | — → Float | Holds single-channel paint pages. |
| Fill | — → Material | Same as [Fill layer properties](layers.md#fill-layer-properties). |
| Stack | Material slots → Material | Ordered layer slots. Displayed by the Layers panel. |
| Mask Stack | Float slots → Float | Ordered mask layer slots. |

In the Node Editor, a Stack shows two inputs per layer (the layer's content, and its mask) and a
**+ New layer** input: connecting a material there adds a layer. A Mask Stack shows one input
per node-based mask effect and a **+ New effect** input that adds one.

### Output

| Node | Inputs | Description |
|---|---|---|
| Material Output | Material | One per project. Its input is the root Stack. |

## Graph rules

- The graph must be acyclic. Connections that would create a cycle are rejected.
- A Stack node may be referenced by any number of layers but appears in the Layers panel only
  when it is the root stack or a Group.
- Deleting a node with connections removes those connections.
