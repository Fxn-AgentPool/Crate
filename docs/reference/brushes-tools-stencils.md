# Brushes, tools and stencils

## Tools

| Tool | Shortcut | Description |
|---|---|---|
| Paint | `B` | Paints the brush color/material into the paint buffer. |
| Erase | `E` | Reduces coverage in the buffer; on bake, removes paint from the target node. |
| Paint Through | `P` | Paints pixels sampled from the active stencil into the buffer. |
| Geo Fill | `L` | Fills a face, UV island, part, material or UDIM tile with the brush values. See [Geo Fill](#geo-fill). |
| Curve | `K` | Draws editable strokes along smooth paths on the surface. See [Curves](curves.md). |
| Color picker | hold `Alt+Shift`, click | Samples the current channel display at the cursor. |

Paint, Erase, Paint Through, Geo Fill and Curve are also chosen in the tool rail at the left edge
of the window.

The **target** of all tools is the selected Paint layer, or the selected Paint mask layer.
If neither is selected, painting is disabled and the cursor shows ⊘.

Geo Fill and Curve don't use the [paint buffer](paint-buffer.md): they write to the target
directly, in Live and Manual mode alike, and leave unbaked paint in the buffer as it is.

## Brush properties

**Brush** panel. **Size**, **Flow** and **Hardness** are also in the
[viewport toolbar](windows-and-panels.md#viewport), where Flow is shown in percent:

| Property | Range | Default | Description |
|---|---|---|---|
| Size | 1–2000 | 50 | In screen pixels or world units, per **Size space**. |
| Size space | Screen, World | Screen | World scales each dab by the depth under its center. |
| Hardness | 0–1 | 0.8 | Edge falloff of the round tip. Ignored when an alpha is set. |
| Flow | 0–1 | 1.0 | Coverage added per dab. |
| Opacity | 0–1 | 1.0 | Maximum coverage per stroke. |
| Spacing | 1–200 % | 10 % | Distance between dabs, as a percentage of size. |
| Alpha | image | none | Brush tip shape: luminance times alpha. Set from a file or from **Library → Alphas**. |
| Angle | 0–360° | 0° | Rotation of the tip. |
| Follow stroke | on/off | off | Rotates the tip along the stroke direction. |
| Jitter: size / angle / position / flow | 0–1 | 0 | Random variation per dab. |
| Pressure → size / flow / opacity | on/off | size on | Maps pen pressure to the property. |
| Pixel | on/off | off | In the UV view: a hard square of whole texels, snapped to the texel grid. See [Pixel brush](2d-paint.md#pixel-brush). |
| Pixel size | 1–64 texels | 1 | The square's side while **Pixel** is on. |

## Brush values

The brush carries one value per channel. The **Brush Values** panel has a row per enabled project
channel, laid out like a Fill layer's **Values**: a check, the channel's name, then its value (a
color, a number, or X, Y and Z for Normal). The check sets whether the channel is:

| State | Effect |
|---|---|
| Enabled | The channel is painted with the given value. |
| Disabled | The channel is not touched by the stroke. |

**Load material** fills the brush values from a Fill layer or smart material, sampled as constants.

## Brush cursor

The cursor is a circle of the brush size with the tip inside it, faint: the round falloff of the
current hardness, or the alpha, turned to the brush angle. It shows while hovering and while
painting. The tip only shows Angle, not Follow stroke or jitter.

## Size, hardness and angle shortcuts

| Action | Input |
|---|---|
| Smaller / larger | `[` / `]` |
| Softer / harder | `Shift+[` / `Shift+]` |
| Interactive size | `Ctrl`+right-drag horizontally |
| Interactive flow | `Ctrl`+right-drag vertically |
| Interactive angle | `Ctrl+Shift`+left-drag around the cursor |

During the interactive drags the cursor stays where the drag started, so the brush scales and
turns around its center, and the new value shows below it. The angle follows the pointer's
direction from the center: a full circle around it turns the brush once.

## Geo Fill

**Geo Fill** fills whole elements of the mesh with the [brush values](#brush-values). Two
settings choose what it fills. While Geo Fill is the current tool, they are shown in the
[viewport toolbar](windows-and-panels.md#viewport) instead of Size, Flow and Hardness, as rows of
icons; hovering an icon shows its name.

**Select**: how elements are picked.

| Select | Icon | Input | Fills |
|---|---|---|---|
| Click | arrow | Click | The element under the pointer. Default. |
| Brush | circle | Drag | Every element the brush circle touches while dragging. The circle has the brush's **Size**, in screen pixels, also shown in the toolbar after the icons. The elements are highlighted as they are collected, and filled when the button is released. |
| Box | dashed box | Drag a rectangle | Every element with a part inside the rectangle, when the button is released. |

**Fill by**: the kind of element.

| Fill by | Icon | Element |
|---|---|---|
| Face | triangle | One triangle. Polygons are split into triangles on import, so a quad is two faces. |
| UV island | island | The triangles connected to it by shared edges in UV space: edges whose two ends have the same position and UV in both triangles. Default. |
| Part | cube | Every triangle of the same part: glTF node or OBJ object, named as in [Match by name](mesh-maps.md#match-by-name). |
| Material | sphere | Every triangle with the same material. |
| UDIM tile | grid | Every triangle in the same tile. |

| View | Element under the pointer | Brush and Box |
|---|---|---|
| 3D view | The element of the frontmost triangle under the pointer. | The elements of the surface visible inside the circle or rectangle, found every 4 screen pixels (or farther apart for large rectangles: at most 16 384 points). Hidden surfaces aren't selected. |
| UV view | The element of the triangle whose UVs contain the point; where several overlap, the first in the mesh. For UDIM tile, the tile under the pointer, if the mesh uses it. | The elements of every triangle whose UVs overlap the circle or rectangle. For UDIM tile, also every used tile it overlaps. |

A click, stroke or rectangle over nothing shows **Nothing to fill here** in the status bar.

The fill covers the texels whose center is inside the element's triangles, and the
[padding](mesh-maps.md#padding) texels whose nearest island texel is one of those, as ordinary
paint does; also on 2D paint layers. Its edges are hard: each texel is filled entirely or not at
all. It is composited over the target's paint with coverage **Opacity** (from the Brush section).
Enabled channels get their brush value; a mask target gets **Mask value**. The brush's size,
tip, flow, spacing, jitter and pressure aren't used.

Each click, Brush stroke or Box is one undo step. The fill becomes ordinary paint: it can be painted over or erased,
but not changed as a fill afterwards.

### Outlines and highlight

While Geo Fill is the current tool, both views show what a click can fill:

| Item | Shown as |
|---|---|
| Outlines | Red lines along the borders between elements of the current **Fill by** kind, and along the mesh's open edges. Face: every triangle edge. UV island: the UV seams. Part, Material, UDIM tile: the edges where two parts, materials or tiles meet. |
| Element under the pointer | Its triangles tinted red, and its name next to the pointer: **Face**, **UV island** with its number of triangles, **Part:** and **Material:** with their names, or **Tile** with its number. With Brush, the elements collected so far are tinted too. |
| Brush circle | With Brush: a circle of the brush's Size around the pointer. |
| Rectangle | With Box: the rectangle being dragged. |

In the 3D view, the mesh hides the outlines and highlight behind it, as it hides the
[wireframe](keyboard-shortcuts.md#display). In the UV view, an edge that is a border is outlined
on both sides of a seam.

## Stencils

A stencil is an image displayed over the viewport in screen space.

| Property | Range | Default | Description |
|---|---|---|---|
| Image | file | — | PNG, JPEG, TGA, TIFF or EXR, from a file or from the Library (**Stencils** or another image folder). Copied into the project's `stencils/` directory. |
| Opacity (display) | 0–1 | 0.5 | Viewport display opacity only. Does not affect painting. |
| Visible | on/off | on | Toggle with `S`. |
| Paint Through target | per channel | Base Color | Channels the Paint Through tool writes. Scalar channels receive luminance. |
| Use alpha | on/off | on | Image alpha multiplies paint coverage. |
| Tile | on/off | off | Repeats the image infinitely. |

Up to 8 stencils can be loaded per project. One is **active** at a time; Paint Through samples
only the active stencil. Other stencils are hidden.

### Stencil transform

| Action | Input |
|---|---|
| Move | Hold `S`, left-drag |
| Rotate | Hold `S`, `Shift`+left-drag |
| Scale | Hold `S`, `Ctrl`+left-drag |
| Flip horizontal | `S`, then `H` |
| Reset | `S`, then `R` |
| Toggle visibility | Tap `S` |

Stencils stay fixed relative to the viewport when the camera moves.
