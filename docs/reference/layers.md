# Layers and masks

The **Layers panel** displays a **Stack node** from the node graph as a list. The project's root
stack is shown by default. Layers are composited bottom to top.

## Layer types

| Type | Icon | Content |
|---|---|---|
| Paint layer | brush | A multi-channel Paint node. Receives baked paint from the paint buffer and Geo Fill, and holds [curves](curves.md). |
| 2D paint layer | brush with "2D" | A 2D Paint node: paint that reaches every texel from the UV view. See [2D paint](2d-paint.md). |
| Fill layer | paint bucket | Per-channel constant values or images, projected by UV or triplanar mapping. |
| Group | folder | A nested Stack node. Has its own layers, mask, opacity and blend modes. |
| Graph layer | nodes | Any node graph that outputs a Material. Shown when the content is not a Paint, Fill or Stack node. |
| Smart material | material sphere | A Group or Graph layer instanced from the smart material library, showing its exposed parameters. |

Each row shows, from the left: visibility, the expand arrow of folders and layers with
effects, the content and mask thumbnails, an icon for the layer type (hover for the type), the
name, and the blend mode and opacity of the channel chosen at the top of the panel. The eye
is closed on hidden layers. The blend
mode is amber when it isn't Normal, so non-default layers stand out. Hidden layers are also faded.

## Layer properties

| Property | Range | Default | Description |
|---|---|---|---|
| Name | text | type name | Display name. |
| Visible | on/off | on | Hidden layers are not evaluated. |
| Opacity | 0–100 % | 100 % | Multiplies every channel's contribution. Set by the `set` script command; the Properties panel edits channel opacity instead. |
| Channels | per channel | all enabled | Which channels the layer affects. |
| Channel blend mode | see [Blend modes](blend-modes.md) | Normal | Set per channel. |
| Channel opacity | 0–100 % | 100 % | Set per channel. Multiplies with layer opacity. |

## Properties panel

The Properties panel shows the selected layer in sections:

| Section | Layers | Rows |
|---|---|---|
| Channels | all | One row per project channel: a toggle with the channel name, the blend mode and the channel opacity. The toggle is lit when the layer affects the channel; the blend mode and opacity of a channel that is off are greyed out. The blend mode is amber when it isn't Normal. |
| Projection | Fill | Type (UV or Triplanar), Scale, Offset (X, Y, Z) and Rotation. Triplanar blend is shown when Type is Triplanar. |
| Values | Fill | One row per channel that is on in Channels: the channel name, its value (a color swatch, a number, or X, Y, Z for Normal, on a line of their own when the panel is too narrow; the image when the source is Image) and its source (Constant or Image). A row is greyed out when the Fill node's own output for the channel is off, e.g. after editing it in the Node Editor. |
| Curves | Paint, 2D paint | The layer's [curves](curves.md#curves-in-the-properties-panel): their list, the selected curve's settings and values, and **Rasterize**. |
| Exposed parameters | Group, Graph, Smart material | The parameters marked Expose in the layer's node graph. |

A Paint layer shows a hint to paint with the Paint tool (B) below Channels. Paint and 2D paint
mask effects show their Curves section below their opacity and blend mode.

On a Fill layer, the channel toggle also turns the Fill node's output for that channel on or off,
so the two stay in step. A new Fill layer affects Base Color, Roughness and Metallic.

## Fill layer properties

| Property | Values | Description |
|---|---|---|
| Source (per channel) | Constant, Image | Constant value or an image from `images/`. |
| Projection | UV, Triplanar | UV uses the mesh UVs; Triplanar projects in object space. |
| Scale | > 0 | Tiling scale. UV: repeats per tile. Triplanar: repeats per world unit. |
| Rotation | degrees | Rotation of the projection. |
| Offset | 2D / 3D | Offset of the projection. |
| Triplanar blend | 0–1 | Width of blending between projection axes. |

## Masks

Every layer and group has an optional **mask**. A mask is a single-channel stack: it contains
mask layers composited the same way as layers, producing a value from 0 to 1.

| Mask layer type | Content |
|---|---|
| Paint | Single-channel paint. Painted with the same tools as layers, curves included. |
| 2D paint | Single-channel [2D paint](2d-paint.md): reaches every texel from the UV view. |
| Fill | Constant value or grayscale image. |
| Generator | A node graph with a Float output, typically from the generator nodes. |
| Levels | Adjusts the mask below it (input/output black and white points, gamma). |

The final layer contribution is:

```
result = lerp(below, blend(below, layer, mode), coverage * mask * channel_opacity * layer_opacity)
```

where `coverage` is how much of the channel the layer covers (painted coverage for Paint
layers, 1 for enabled Fill channels).

A Group composites its own layers on top of the layers below it, then fades that result in
with the group's mask and opacity. With the Normal blend mode this is exactly like the layers
being outside the group, faded together. Other group blend modes apply the mode to the group's
composited result; this is an approximation of fully isolated group compositing.

## Layer operations

| Operation | Shortcut | Description |
|---|---|---|
| Add paint layer | `Ctrl+Shift+N` | Inserts above the selected layer. |
| Add fill layer | `Ctrl+Shift+F` | Inserts above the selected layer. |
| Add 2D paint layer | toolbar (brush with "2D") | Inserts a [2D paint](2d-paint.md) layer above the selected layer. |
| Group selected | `Ctrl+G` | Moves the selected layers into a new Group. `Ctrl`+click selects several layers of the same stack. |
| Add mask | context menu | Adds an empty mask (white = fully visible). |
| Add black mask | context menu | Adds a mask with value 0. |
| Add generator | context menu on a mask | Adds a Generator mask layer. |
| Edit as graph | `G` | Opens the selected layer's content or mask in the Node Editor. |
| Save as smart material | context menu | Saves a Group or Graph layer to the library. |
| Duplicate | `Ctrl+D` | Duplicates the layer. Paint data is copied. |
| Delete | `Delete` | Deletes the layer. |

## Correspondence with the node graph

| Layers panel | Node graph |
|---|---|
| Stack | Stack node |
| Layer | One input slot of the Stack node, with blend, opacity and mask parameters |
| Paint layer | Paint node connected to the slot |
| Fill layer | Fill node connected to the slot |
| Group | Nested Stack node connected to the slot |
| Mask | Mask Stack node connected to the slot's mask input |
