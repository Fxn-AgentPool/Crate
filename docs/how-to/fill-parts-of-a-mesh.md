# How to fill parts of a mesh

This guide shows you how to paint whole faces, UV islands, parts, materials or UDIM tiles with
one click each.

## Set up the fill

1. Select the Paint layer or paint mask effect to fill.
2. Press `L` for the Geo Fill tool.
3. In the viewport toolbar, choose what a click fills under **Fill by**: **Face**, **UV island**,
   **Part**, **Material** or **UDIM tile**.
4. In **Brush Values**, enable the channels to fill and set their values. For a mask, set
   **Mask value**.
5. Lower **Opacity** in the **Brush** section for a partial fill.

## Fill

1. Hover over the mesh. The element a click would fill is named next to the pointer and, in the
   3D view, highlighted.
2. Click to fill it. Each click is one undo step (`Ctrl+Z`).
3. To fill from the UV layout instead, switch to the UV view (`F3`) and click inside an island.
   With **UDIM tile**, click anywhere in the tile.

## Change a fill

A fill becomes ordinary paint. To change it, fill the element again with other values, or paint
over it. To remove it, use **Erase** (`E`), or fill the element of a paint mask effect with
**Mask value** 0.

## Keep fills separate

To adjust filled areas later as a group (blend mode, opacity, mask), fill them on a Paint layer
of their own.

See [Geo Fill](../reference/brushes-tools-stencils.md#geo-fill) for how each kind of element is
found, and [About curves and geometry fills](../explanation/about-curves-and-geometry-fills.md)
for why fills aren't editable afterwards.
