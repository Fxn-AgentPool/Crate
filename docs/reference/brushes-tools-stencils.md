# Brushes, tools and stencils

## Tools

| Tool | Shortcut | Description |
|---|---|---|
| Paint | `B` | Paints the brush color/material into the paint buffer. |
| Erase | `E` | Reduces coverage in the buffer; on bake, removes paint from the target node. |
| Paint Through | `P` | Paints pixels sampled from the active stencil into the buffer. |
| Color picker | hold `Alt+Shift`, click | Samples the current channel display at the cursor. |

Paint, Erase and Paint Through are also chosen in the tool rail at the left edge of the window.

The **target** of all tools is the selected Paint layer, or the selected Paint mask layer.
If neither is selected, painting is disabled and the cursor shows ⊘.

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
