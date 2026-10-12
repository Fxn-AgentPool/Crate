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

## Inputs and settings

A node has inputs (sockets on its left edge), outputs (on its right edge) and settings.

- A **value input** has a value of its own, used while nothing is connected to it. In the Node
  Editor its field is on the input's row; connecting a link hides the field and the link's value
  is used instead. Every number, color or vector a node reads per texel is a value input.
- A **setting** is the same for the whole node and has no socket: choices (like Math's
  operation), on/off settings, whole numbers (like octaves and seeds), images, ramp stops, ID
  colors, the radius of filters, and every setting of the Layer nodes.

In the catalog below, value inputs are in *italics* with their range and default value, like
*Scale* (0.01–1000, 4). Ranges are hard limits: typed values are clamped to them. A connected
link's value is not clamped.

Settings marked "(Float only)", "(Custom only)" and the like are shown only while the setting
they depend on has that value.

## Node properties

Every node has:

| Property | Description |
|---|---|
| Id | Unique, read-only. |
| Label | Optional display name. |
| Cached | If on, the node's output is evaluated in UV space and stored per tile. See [About realtime evaluation](../explanation/about-realtime-evaluation.md). Available for nodes with one Material output, or at most one Color and one Float output and no Vector output. A Stack used as a Group layer is not read from its cache, nor is a node that reads [2D paint](2d-paint.md#caching). |
| Exposed parameters | Settings and value inputs marked **Expose** appear on the owning layer in the Layers panel and on smart materials. An exposed value input that is connected has no effect. |

## Categories

Every node belongs to one category. The category sets the color of the node's header in the
Node Editor and where the node is listed in the add menu, in this order:

| Category | Header color | Nodes |
|---|---|---|
| Input | rose `#83314A` | Value, Vector, UV, Geometry, Mesh Map, ID Select |
| Texture | brown `#79461D` | Image, Noise, Cellular, Gradient |
| Generator | green `#1F7050` | Edge Wear, Dirt, Curvature Mask, Position Gradient |
| Color | olive `#6C6C00` | Levels, Invert, HSV Adjust, Merge |
| Vector | indigo `#3C3C83` | UV Transform, Mapping, Vector Math |
| Converter | blue `#246283` | Math, Clamp, Ramp, Combine XYZ, Separate XYZ, Material Combine, Material Split |
| Filter | purple `#6C3B75` | Blur, Dilate, Height to Normal |
| Layer | dark green `#2D6A2D` | Paint, Paint Mask, 2D Paint, 2D Paint Mask, Fill, Stack, Mask Stack |
| Layout | gray `#4A4D55` | Reroute |
| Output | dark red `#4D0017` | Material Output (not in the add menu) |

The add menu also lists **Factor** (a Value node of type Factor) and **Color** (a Vector node of
type Color) under Input. Layer groups, frames and the Layer node of the Node Editor have their
own looks; see [Node Editor](node-editor.md#layer-groups).

## Node catalog

### Input

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Value | — | Value (Float) | Type: Float or Factor (default Float). Value (Float only): −1 000 000–1 000 000, default 0.5. Factor (Factor only): 0–1, default 1, shown as a slider. The node's title is **Value** or **Factor**, after its type. |
| Vector | — | Vector (Vector, or Color while the type is Color) | Type: Vector or Color (default Vector). Vector (Vector only): X, Y, Z, default `(0, 0, 0)`. Color (Color only): a color, default `(0.5, 0.5, 0.5)` (linear). The node's title is **Vector** or **Color**, after its type. |
| UV | — | UV (Vector) | — . The texel's UV within its UDIM tile: X = U, from 0 at the tile's left edge to 1 at its right edge; Y = V, from 0 at the bottom to 1 at the top; Z = 0. This is the coordinate the Image node's UV projection uses. |
| Geometry | — | Position, Generated, Normal (Vector) | — . Position: the object-space position. Generated: the position normalized to the mesh's bounding box, 0–1 on each axis (what Noise, Cellular and Gradient use by default). Normal: the object-space normal. |
| Mesh Map | — | Float, Color | Map: AO, Curvature, Thickness, World Normal, Position, ID, Normal, Height. Normal outputs the tangent-space normal as a color (`n * 0.5 + 0.5`). |
| ID Select | *Tolerance* (0–1, 0.1) | Out (Float) | Selected ID colors. Reads the ID mesh map. |

### Texture

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Image | Vector, *Scale* (0.001–1000, 1), *Rotation* (−360–360, 0), *Offset* (Vector, `(0, 0, 0)`), *Triplanar blend* (0–1, 0.25) | Color, Alpha (Float) | Image file, sRGB, projection (UV / Triplanar). With **Vector** connected, the image is sampled at the vector's X and Y as a UV, after its scale, rotation and offset; the projection is not used. A **UV** node connected to **Vector** gives the same result as the UV projection. |
| Noise | Vector, *Scale* (0.01–1000, 4), *Lacunarity* (1–4, 2), *Gain* (0–1, 0.5) | Out (Float) | Type (Perlin, Simplex, Value), octaves (1–10, 5), seed (0–9999, 0). Evaluated at **Vector** × scale; with **Vector** unconnected, at the Generated position. |
| Cellular | Vector, *Scale* (0.01–1000, 4), *Jitter* (0–1, 1) | Out (Float) | Distance metric (Euclidean, Manhattan), output (F1, F2, F2−F1), seed (0–9999, 0). Coordinates as for Noise. |
| Gradient | Vector, *Start* (−1–2, 0), *End* (−1–2, 1) | Out (Float) | Axis (X, Y, Z, Custom; default Y), custom axis (Custom only; X, Y, Z, default `(0, 1, 0)`), space (Object, World). With **Vector** connected, the gradient runs along the axis through the vector, and space is not used. |

### Generator

Generators are built-in subgraphs. **Ungroup** (Node Editor, Properties) turns a generator into
its component nodes (Mesh Map, Noise, Levels, Math, Invert, Gradient), with its current
input values baked into them; links into the generator's value inputs are dropped. The result
closely matches the generator.

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Edge Wear | *Amount* (0–1, 0.2), *Sharpness* (0–1, 0.6), *Noise scale* (0.1–200, 30), *Noise amount* (0–1, 0.7) | Out (Float) | Seed. Uses Curvature. |
| Dirt | *Amount* (0–1, 0.5), *Contrast* (0–1, 0.5), *Noise scale* (0.1–200, 8) | Out (Float) | Seed. Uses AO. |
| Curvature Mask | *Threshold* (0–1, 0.1), *Softness* (0–1, 0.2) | Out (Float) | Convex/concave selection. |
| Position Gradient | *Falloff* (0–1, 0.5), *Noise amount* (0–1, 0.2) | Out (Float) | Axis (X, Y, Z). Uses Position. |

### Color

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Levels | In, *In black* (0–1, 0), *In white* (0–1, 1), *Gamma* (0.01–10, 1), *Out black* (0–1, 0), *Out white* (0–1, 1) | Out (Color), Float | — |
| Invert | In | Out (Color), Float | — |
| HSV Adjust | In, *Hue* (−0.5–0.5, 0), *Saturation* (0–2, 1), *Value* (0–2, 1) | Out (Color) | — |
| Merge | A, B, Mask, *Opacity* (0–1, 1) | Out (Color), Float | Blend mode. See [Blend modes](blend-modes.md). |

An unconnected **In**, **A** or **B** is black, and an unconnected **Mask** is 1.

### Vector

An unconnected **Vector** input of these nodes reads the UV, as the **UV** node outputs it.

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| UV Transform | Vector, *Tile U*, *Tile V* (0–1000, 1), *Rotation* (degrees, −360–360, 0), *Offset U*, *Offset V* (−1000–1000, 0) | Vector | — . Repeats, rotates and moves UVs about the tile's center: `out.xy = rotate(in.xy − 0.5, rotation) × (tile U, tile V) + 0.5 + (offset U, offset V)`. Z passes through. |
| Mapping | Vector, *Location* (Vector, `(0, 0, 0)`), *Rotation* (Vector, degrees, applied about X, then Y, then Z, `(0, 0, 0)`), *Scale* (Vector, `(1, 1, 1)`) | Vector | — . `out = rotate(in × scale) + location`, like Blender's Mapping node of type Point. |
| Vector Math | A, *B* (Vector, `(0, 0, 0)`), *Scale* (−1 000 000–1 000 000, 1) | Vector, Value (Float) | Operation (default Add). See below. |

Vector inputs' components range from −1 000 000 to 1 000 000.

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

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Math | A, *B* (−1 000 000–1 000 000, 1) | Out (Float) | Operation: Add, Subtract, Multiply, Divide, Power, Min, Max, Abs, Smoothstep (default Multiply). An unconnected **A** is 0. |
| Clamp | In, *Min* (−1 000 000–1 000 000, 0), *Max* (−1 000 000–1 000 000, 1) | Out (Color), Float | — |
| Ramp | In (Float) | Out (Color) | Color stops. |
| Combine XYZ | *X*, *Y*, *Z* (−1 000 000–1 000 000, 0) | Vector | — |
| Separate XYZ | Vector | X, Y, Z (Float) | — . An unconnected **Vector** is `(0, 0, 0)`. |
| Material Combine | Float/Color per channel | Out (Material) | — |
| Material Split | In (Material) | Float/Color per channel | — |

### Filter

Filters read neighboring texels (they are non-local). They are always **Cached**; the property
cannot be turned off. Their radius and strength are settings, the same for the whole node.

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Blur | In | Out (Color), Float | Radius (texels at tile resolution, 0–64, 4). |
| Dilate | In (Float) | Out (Float) | Radius (0–64, 2). |
| Height to Normal | In (Float) | Out (Color) | Strength (0–10, 1). Used internally for export. |

### Layer

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Paint | — | Out (Material) | Channels painted. Holds paint pages. |
| Paint Mask | — | Out (Float) | Holds single-channel paint pages. |
| 2D Paint | — | Out (Material) | Like Paint, but read at its own texel between UV islands. See [2D paint](2d-paint.md). |
| 2D Paint Mask | — | Out (Float) | Like Paint Mask, read at its own texel between UV islands. |
| Fill | — | Out (Material) | Same as [Fill layer properties](layers.md#fill-layer-properties). |
| Stack | Material slots | Out (Material) | Ordered layer slots. Displayed by the Layers panel. |
| Mask Stack | Float slots | Out (Float) | Ordered mask layer slots. |

In the Node Editor, a Stack shows two inputs per layer (the layer's content, and its mask) and a
**+ New layer** input: connecting a material there adds a layer. A Mask Stack shows one input
per node-based mask effect and a **+ New effect** input that adds one.

### Layout

| Node | Inputs | Outputs | Settings |
|---|---|---|---|
| Reroute | In (any type) | Out (the type of what's connected to **In**; Float while nothing is) | — . Passes its input on unchanged, to route links. See [Reroutes](node-editor.md#reroutes). |

### Output

| Node | Inputs | Description |
|---|---|---|
| Material Output | Material | One per project. Its input is the root Stack. |

## Graph rules

- The graph must be acyclic. Connections that would create a cycle are rejected.
- A Stack node may be referenced by any number of layers but appears in the Layers panel only
  when it is the root stack or a Group.
- Deleting a node with connections removes those connections.
