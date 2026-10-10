# About high-poly baking

A game asset usually exists twice: a detailed **high-poly** sculpt, with millions of triangles,
and a **low-poly** mesh with UVs that the engine actually renders. Baking transfers the
high poly's surface detail into textures for the low poly. This page explains how Crate does
that and why.

## Rays instead of closest points

For each texel of the low poly, Crate needs the point of the high-poly surface that "belongs" to
it. The obvious answer, the closest high-poly point, fails on concave and thin shapes: the
closest point can lie on the far side of a groove, or on a neighboring part.

Instead, Crate shoots a ray along the low poly's normal. The low poly approximates the high
poly, so somewhere along that line, a little outside or a little inside the low-poly surface,
lies the matching high-poly point. **Frontal distance** and **Rear distance** bound that search:
the ray starts outside the surface and travels inward, and the first high-poly surface it meets
is taken. The volume these distances sweep around the low poly is what other bakers call a
**cage**; Crate's is a uniform offset along the normals.

Too small a cage misses parts of the high poly that stick out further; too large a cage reaches
neighboring geometry and picks up the wrong surface. That trade-off is why misses are shown in
red and why the distances are settings rather than fixed values.

## Why interpolated normals

The rays follow the low poly's interpolated (smooth) normal rather than each flat triangle's
normal. At a hard edge, flat normals would leave a wedge where rays from neither side reach,
and a seam of misses. Interpolated normals sweep continuously around edges. The price is that
on a low poly with very sharp edges and soft normals, rays near the edge lean, which shows up as
slight waviness; splitting hard edges in the low poly avoids it.

## Matching parts by name

On an asset with many parts, a ray from one part easily reaches another: a bolt sits on a plate,
and the plate's rays hit the bolt first. Naming parts in `_low`/`_high` pairs tells Crate which
high-poly part each low-poly part is a copy of, and its rays then ignore everything else. This
is the same convention Substance Painter and Marmoset Toolbag use, so assets prepared for them
bake the same way in Crate.

## Tangent space must match the engine

A normal map stores directions relative to the low poly's surface: its tangent, bitangent and
normal. The engine reconstructs the final normal from the same three vectors. If the baker and
the engine compute them differently, the map is subtly wrong: shading seams and gradients appear
even though nothing is missing.

Crate uses MikkTSpace tangents, the de-facto standard that Blender, Unreal Engine and Unity use
for meshes without their own tangents, and bakes with exactly the per-pixel basis its own
viewport uses to display normal maps. A map that looks right in Crate therefore looks right in
those engines, given the same mesh and the right Y convention (OpenGL or DirectX).

## Which maps come from where

Position and World Normal describe the surface you paint on, so they stay the low poly's:
painting projects onto the low poly, and the paint buffer needs its exact geometry. Everything
describing *detail* (Normal, Height, AO, Curvature, Thickness, ID) comes from the high poly when
one is set, so generators like Edge Wear and Dirt follow the sculpt's edges and crevices, not
the low poly's coarse ones.

## One ray per texel

Crate casts one ray through the center of each texel. Detail finer than a texel can alias:
thin, high-contrast features may look jagged at low resolutions. Baking at the tile resolution
you export at keeps the maps as sharp as the textures that use them.

## Related

- [How to bake from a high-poly mesh](../how-to/bake-from-a-high-poly-mesh.md) (how-to)
- [Mesh maps](../reference/mesh-maps.md) (reference)
