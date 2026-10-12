# How to paint editable curves

This guide shows you how to paint seams, stitches, panel lines and other strokes along smooth
paths that you can still change afterwards.

## Set up the brush

1. Select the Paint layer or paint mask effect for the curves.
2. Set up the brush and brush values as for a normal stroke: Size, Hardness, Spacing, an alpha
   for stitches, and the channels to paint.
3. Press `K` for the Curve tool.

## Draw a curve

1. In the 3D view, click on the mesh where the curve starts.
2. Click along the path. The curve passes through every point, smoothly, and follows the surface
   between them.
3. Press `Enter` to finish, or click the first point to close the curve into a loop.

`Backspace` removes the last point while drawing. Points can be placed around the model: orbit
with `Alt` + left-drag between clicks.

## Change the path

1. Click the curve to select it. Its points appear.
2. Drag a point to move it along the surface.
3. `Ctrl` + click the curve's line to add a point, or a point to delete it.

## Change how it paints

1. Select the curve in the viewport, or in the **Curves** section of the Properties panel.
2. Change its Size, Hardness, Flow, Opacity, Spacing, Alpha, Angle or Values there. The layer
   updates as you drag.
3. To give the curve the current brush instead, click **Copy brush**.

## Order and delete curves

- **Move up** and **Move down** in the Curves section change which curve is drawn on top.
- `Delete` in the viewport, or **Delete** in the Curves section, removes the selected curve.

## Make the curves ordinary paint

To erase into curves, smudge them with freehand paint, or keep the project light, click
**Rasterize** in the Curves section. The curves become freehand paint and can't be edited as
curves any more.

See [Curves](../reference/curves.md) for every setting, and
[About curves and geometry fills](../explanation/about-curves-and-geometry-fills.md) for how
curves relate to freehand paint.
