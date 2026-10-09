# About UDIM tiles and paint storage

## Why UDIMs

A single 0–1 UV square limits how much texture resolution an asset can have. UDIMs extend UV
space into a grid of tiles, each with its own texture, so an asset can carry many textures of
detail while keeping a single material. Film pipelines have relied on them for a long time, and
game tools increasingly support them.

Substance Painter historically mapped UV regions to separate *texture sets*, and added UDIM
support later. Mari was built around UDIMs from the start. Painter follows Mari here: tiles are
simply where texels live. You don't manage them individually. You paint across them, and
projection takes care of tile boundaries.

## Why paint is stored in pages

The obvious storage for paint is one full texture per tile per Paint node. On a 20-tile asset at
4K, with all seven channels and their coverage at 16-bit float (40 bytes per texel), that is
around 13 GB for **one** paint layer, nearly all
of it empty, because most paint layers touch a small part of the asset.

Painter divides each tile into 128 × 128 **pages**. On disk and in the undo history, only pages
that contain paint are stored, so a layer of scratches on one corner of a crate costs a few
megabytes, not gigabytes.

On the GPU, the MVP still keeps every painted layer's tiles whole. That keeps painting and
evaluation simple, and is affordable for assets with a handful of tiles; moving the GPU side to
sparse pages is the step toward the virtual texturing described below.

Pages also make several other things cheap:

- **Undo.** Before a bake writes to a page, the old page is saved. An undo step is just the set of
  pages that stroke changed, never whole textures.
- **Saving.** Only changed pages are rewritten to disk.
- **Finer invalidation (planned).** Cached nodes downstream of a paint layer could recompute
  only the dirty pages; the MVP recomputes whole tiles.

## Toward virtual texturing

Large film assets go further: hundreds of tiles at 8K or more, where even the painted pages don't
fit in GPU memory. The standard answer is *virtual texturing*: a page table on the GPU, with
pages streamed in from disk based on what's visible, and evicted when not.

The MVP doesn't stream paint pages; they all stay resident. But the page layout is the same one a
virtual texturing system needs, so streaming can be added later without changing the file
format or how paint is addressed. See [About the MVP scope](about-mvp-scope.md).

## Seams and padding

Texture filtering and mipmapping sample slightly outside UV islands. If those texels are empty,
seams show up as thin dark lines. Painter dilates paint by a few texels into empty space at bake
time, and export can dilate all the way to fill empty space ("infinite padding").

Tile boundaries are a special case. A UV island that ends at a tile edge has its neighbor in a
different texture, and filtering cannot cross the edge. This is why the importer warns about
triangles crossing tiles: they texture correctly, but the seam where they cross can't be hidden
by padding.

## Related

- [Mesh import and UDIM tiles](../reference/mesh-and-udims.md) and [Project format](../reference/project-format.md) (reference)
