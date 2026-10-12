# 2D paint

2D paint writes texels of the textures directly. Unlike ordinary paint, it can reach every
texel of a used tile from the [UV view](uv-view.md): texels that are only partly on a UV island,
the padding around islands, and the empty space between them. Use it to fix single texels at
the end of a project, to control what mipmaps blend into at island borders, or to leave
something in the unused parts of a tile.

2D paint stays where it is in UV space: a reimport with new UVs doesn't move it (see
[Reimport](#reimport)).

## Layers and mask effects

| Item | Where | Node | Description |
|---|---|---|---|
| 2D paint layer | Layers panel toolbar: the brush button with "2D" (**Add 2D paint layer**); `add-paint-2d [name]` | **2D Paint** (Material) | A layer like a paint layer, painted with the same tools and brush values. |
| 2D paint mask effect | Mask context menu, **Add 2D paint**; `add-mask-paint-2d` | **2D Paint Mask** (Float) | A mask effect like a paint mask effect. |

Both can be used wherever their ordinary counterparts can: in any stack or group, at any
position, in any layer's mask, and in node graphs. In the Layers panel they show the paint
icon with a small "2D" badge, and their type reads **2D paint layer** / **2D paint** on hover.

## The space between islands

Every texel of a used tile has a surface point: texels of a UV island have their own, and every
other texel borrows the surface point of the nearest island texel (as found when the
[mesh maps](mesh-maps.md#padding) are baked). In a texel between islands, every node of the graph
is evaluated at that nearest island texel, except 2D Paint and 2D Paint Mask nodes, which read
their own texel.

So without 2D paint, the space between islands repeats the nearest island texel, as export
padding always has. Where 2D paint covers a texel between islands, the layers above and below it,
its blend modes and masks combine with it exactly as on the islands.

## Painting

| View | What 2D paint layers and effects take |
|---|---|
| UV view | Every texel of a used tile under the brush, at full strength. |
| 3D view | The same texels as ordinary paint: the UV islands and their padding. |

Everything else (brushes, stencils, Live and Manual mode, undo, the eraser) works as for ordinary
paint.

### Pixel brush

**Pixel** (on/off, default off) in the **Brush** section, and the `brush pixel=on|off` script
property. While on, in the UV view:

| Property | Behavior |
|---|---|
| Shape | A square of whole texels, without antialiasing: a texel is painted fully or not at all. |
| Size | In texels (1–64), as **Pixel size**, default 1. **Size**, **Size space**, **Hardness**, **Spacing** and the alpha are not used. |
| Position | Each dab is centered on the texel under the pointer (for even sizes, the texel up and left of the pointer's corner). |
| Spacing | One dab per texel the pointer crosses. |
| Flow, Opacity, brush values, pressure → flow and opacity | As usual. |

In the 3D view the Pixel option has no effect. It works for ordinary paint too, restricted to
the texels that paint reaches.

### Texel grid

When one texel is 8 screen points wide or more, the UV view draws the texel grid over the used
tiles in faint lines.

## Display

The 3D view shows 2D paint on the islands, each texel sampled without filtering, so a painted
texel appears as a sharp square.

The UV view's **Gaps** toggle (top right of the UV view; default on; saved as
`viewport.uvView.gaps`) chooses what the UV view shows between islands:

| Gaps | Between islands |
|---|---|
| On | The final texture: the material, or the channel or mesh map shown, evaluated as described above, as export writes it with **Infinite** padding. Costs a material evaluation per screen pixel between islands. |
| Off | The background, as before. 2D paint between islands isn't visible, but can still be painted. |

## Export

Export evaluates islands as before, then fills padding by repeating the nearest island texel, then
writes the material at every texel between islands that 2D paint covers. Those texels are kept
whatever the **Padding** setting is, so 2D paint between islands is exported even with Padding
**None**.

Between islands, the Height channel of 2D paint doesn't contribute to the exported normal map;
its Normal channel does.

## Caching

A node that reads a 2D Paint or 2D Paint Mask node, directly or through other nodes, isn't read
from a cache: **Cached** has no effect on it, and a Filter node (Blur, Dilate, Height to Normal)
reading one passes its input through.

## Reimport

On a [reimport](mesh-and-udims.md#reimport) that transfers paint to new UVs, 2D paint isn't
transferred: its texels stay where they are in UV space, in the tiles the new mesh still uses.
The report adds **; N 2D paint layers kept in UV space** (**layer** when N is 1) when there are
any; 2D paint mask effects count as layers here. Tiles the old mesh didn't use start without 2D
paint.

## Saving

2D paint is saved like ordinary paint, in pages of the node's paint directory (see
[Project format](project-format.md#paint-pages)). Its node types are `paint_2d` and
`paint_mask_2d`.

See [About painting between UV islands](../explanation/about-painting-between-islands.md) for the
design and its costs.
