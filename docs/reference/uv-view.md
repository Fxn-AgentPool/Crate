# UV view

The UV view shows the material laid out in UV space, tile by tile, and you can paint into it
like into the 3D view. See [How to paint in the UV view](../how-to/paint-in-the-uv-view.md).

## Layouts

The viewport has three layouts. The layout is saved with the project (`viewport.layout`, see
[Project format](project-format.md)); new projects start in **3D**.

| Layout | Shortcut | Shows |
|---|---|---|
| 3D | `F2` | The 3D view only. |
| Split | `F1` | The 3D view on the left and the UV view on the right, each half the width, divided by a 1-point line. |
| UV | `F3` | The UV view only. |

The layout is also chosen with the **3D · Split · UV** segmented control in the viewport toolbar,
left of **Bake Mesh Maps**. Shortcuts can be remapped (**View: 3D**, **View: Split**,
**View: UV** in [Keyboard shortcuts](keyboard-shortcuts.md)).

In **Split**, keys, the wheel and the brush cursor act on the half under the pointer. Each half
has its own display switcher; both show what it sets (Material, a channel, a mesh map), because
the display mode is shared.

## What the UV view shows

| Part | Contents |
|---|---|
| Background | The viewport background. |
| UDIM grid | Every tile from 1001 to the highest tile in use, as unit squares with U to the right and V up. Used tiles are outlined in the border color and labeled with their number in their top-left corner; unused tiles only as faint lines. |
| Material | Each triangle drawn at its UVs, shaded with the same material and display mode as the 3D view. Islands that overlap in UV space overlap here too; the one drawn last shows. |
| UV edges | The triangles' edges, always shown, dark at 35 % opacity. |
| Display switcher | As in the 3D view: **Material** and one button per enabled channel. |

The **Material** display shades every texel as if lit from straight in front of the surface (the
view direction is the surface normal), with the environment and exposure of the 3D view, so the
UV view shows colors and normal detail without the shading changing with the camera. Channels and
mesh maps are shown raw, as in the 3D view.

The tile grid (`U`) and wireframe (`W`) toggles only change the 3D view.

## Navigation

| Input | Result |
|---|---|
| Middle-drag, `Alt` + left-drag or `Alt` + middle-drag | Pan. |
| Mouse wheel | Zoom about the pointer. |
| `Alt` + right-drag | Zoom about the view's center. |
| `F` (Frame mesh) | Fits every used tile in the view, with a 5 % margin. |
| `Shift+F` (Frame cursor) | Centers the view on the point under the pointer. |

The zoom is the height of one tile in view heights, from 0.05 to 64. A new project's UV view
frames the used tiles. The UV view's center and zoom are saved with the project
(`viewport.uvView`).

## Painting

Brushes, tools, stencils, brush values and the paint buffer work as in the 3D view; strokes are
painted into the same buffer and baked into the same tiles. Painting only reaches texels the
mesh covers: the UV islands and their padding (8 texels by default). Elsewhere a tile has no
surface, so there's nothing to paint, show or export.

Differences from the 3D view:

| Setting | In the UV view |
|---|---|
| Falloff start / end, Backface culling, Project through | Don't apply: every covered texel under the brush gets full strength, whichever way its surface faces. |
| Brush size in world units | The size is used in screen pixels. |
| Frame cursor (`Shift+F`) | Centers the view instead of the 3D camera. |
| Color pick (`Alt+Shift` + click) | Picks the displayed color in the UV view. |

The bake is described in [Paint buffer](paint-buffer.md#bake-algorithm). Manual mode keeps a
buffer for one view: in **Split**, starting a stroke in the other half, or panning or zooming
the half the buffer belongs to, first applies **On camera move** (bake, clear, or refuse) as a
camera move does. Panning or zooming the other half never affects the buffer.

## Script commands

`uv-view` switches renders and strokes of a [script](script-commands.md) to the UV view, and
`camera` switches them back to the 3D view.
