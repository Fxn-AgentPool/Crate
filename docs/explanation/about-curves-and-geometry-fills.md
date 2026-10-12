# About curves and geometry fills

Crate's ordinary painting is projection painting: strokes land in a screen-space buffer and are
baked through the camera onto the textures (see
[About the paint buffer](about-the-paint-buffer.md)). Two tools work differently, because their
jobs don't fit that model: **Geo Fill** paints by mesh structure, and **Curve** paints strokes that
stay editable. This page explains how they are built and why.

## Two kinds of result

Geo Fill and Curve sit at opposite ends of a trade-off.

A **geometry fill** is baked into the layer's texels the moment you click, like a brush stroke.
Clicking an element is quick to redo, so making fills editable objects would add bookkeeping for
little gain. Baking also keeps fills cheap: they cost nothing after the click, and a layer with
hundreds of fills is just paint. The price is that a fill can't be changed as a fill later; it
can only be filled or painted over.

A **curve** is the opposite: it's kept as a description (points and brush settings) and drawn
again whenever it changes. Placing points carefully around a model takes time, so redoing a curve
to change its width or color would be expensive. Keeping it editable makes those changes free.

## Where curves live

Curves are stored on the Paint or Paint Mask node, in the project's graph, not in paint pages.
This has a few consequences that come for free:

- Undo and redo restore curves like any other graph edit.
- Duplicating a layer, or saving it as a smart material, copies its curves.
- The project file holds a few hundred bytes per curve instead of texels.

The curves are drawn into a second texture slot of their own, which the material reads together
with the layer's freehand paint: freehand paint first, then each curve over it. Keeping the two
apart is what lets freehand painting and erasing leave the curves untouched. The cost is GPU
memory: a layer with curves holds a second set of full tiles.

An alternative would have been to bake curves into the freehand paint and keep a record so they
could be "unbaked". That breaks as soon as anything is painted over a curve: removing the curve
would also remove the paint on top, or leave a hole. With separate storage, the order is always
clear: curves are above. **Rasterize** is there for when you want them merged.

## Painting on the surface, not through the camera

A curve is drawn in object space: dabs are discs lying on the surface, and texels are painted
by their distance to them in 3D. Projection painting would have tied a curve to the view it was
drawn from: moving a point on the far side of the model would need that side to be visible, and
the same curve would paint differently after orbiting. Drawing on the surface makes a curve
independent of the camera, so it can be edited from anywhere and always looks the same.

What projection painting gets from the depth pass, surface painting must get another way. Without
occlusion, a dab near a thin wall would reach through to the other side, so texels are only
painted where the surface faces the same way as the dab. A dab on a sharp edge still wraps a little
around it, as a disc of that size would.

## Following the surface

Points are clicked on the surface, but a smooth path between them generally isn't on it: on a
curved model it cuts through the inside or floats above. So the path is pulled onto the surface,
sample by sample, to the nearest point that faces the same way. The result follows the surface
closely between nearby points. Between points far apart on a strongly curved surface it takes the
nearest surface, which may not be the route you had in mind; adding a point fixes the route.

Because points are stored as positions rather than as places in the UV layout, curves survive a
reimport with new UVs without being transferred: they are simply drawn on the new surface.

## What counts as an element

Geo Fill's elements come from the mesh as imported. Parts, materials and tiles are recorded
directly. UV islands are found by following edges that are shared in UV space, which is exactly
what an island is.

Faces are harder, because triangulation erases them. OBJ files keep their polygons, so Crate
remembers which triangles came from which face. glTF files only contain triangles, so Crate
pairs each triangle with the neighbor across its longest edge when that edge is the neighbor's
longest too. That recovers the quads of a triangulated quad mesh, which is what modeling tools
export, but not larger polygons.

## Related

- [Curves](../reference/curves.md) (reference)
- [Geo Fill](../reference/brushes-tools-stencils.md#geo-fill) (reference)
- [How to paint editable curves](../how-to/paint-editable-curves.md) (how-to)
- [How to fill parts of a mesh](../how-to/fill-parts-of-a-mesh.md) (how-to)
