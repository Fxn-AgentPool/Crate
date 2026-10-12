# About painting between UV islands

Crate's material is evaluated per surface point: a Fill projects an image along the surface, a
noise reads the object-space position, a mask reads the curvature there. A texel between UV
islands belongs to no surface point, so for most of Crate's history there was nothing to
evaluate there; export simply repeated the nearest island texel outward ("padding"), and painting
stopped at the islands.

That leaves out work that only makes sense on the texture itself: fixing a texel that's half on
an island and half off, choosing what mipmaps blend into at a seam, or hiding something in the
empty corner of a tile. [2D paint](../reference/2d-paint.md) is for that work.

## Borrowing the nearest surface

2D paint should combine with the layers around it, in masks and blend modes, between islands
just as on them. That needs every other layer to have a value there. Crate gives each texel
between islands the surface point of the nearest island texel, and evaluates every node there,
except 2D paint, which reads its own texel.

This choice reproduces the old padding exactly: where no 2D paint is, the space between islands
repeats the nearest island texel, as before. Evaluating every node at the gap texel itself, with
the nearest surface, was the alternative; it would have extended images and procedurals into
the gaps, which looks plausible in a viewer but changes every export and isn't what mip padding
needs.

## Keeping the cost down

Evaluating a material is the expensive part of export and display, and procedural nodes make
it more so. Between islands the material is only evaluated where it can differ from the nearest
island texel, which is where 2D paint is:

- Export evaluates islands, repeats them outward as before, and then evaluates only the texels
  between islands that 2D paint covers.
- The UV view evaluates the gaps per screen pixel while **Gaps** is on, like the 3D view
  evaluates the mesh. The toggle turns it off for heavy materials.
- The 3D view only ever sees the islands and pays nothing extra.
- Caches would have to store the gaps for every cached node above 2D paint; instead such nodes
  aren't cached. The cost of this shows when a heavy, cached procedural sits above 2D paint in
  the stack: it's evaluated live again. Keeping 2D paint near the top of the stack avoids that.

## Why 2D paint doesn't follow new UVs

Ordinary paint is moved to new UVs on reimport by matching the surface (see [About keeping
paint when UVs change](about-paint-transfer.md)). 2D paint between islands has no surface of its
own to match, and the point of 2D paint is to control exact texels, so it stays where it is in
UV space.
