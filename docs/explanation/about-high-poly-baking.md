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
**cage**; Crate's is a uniform offset along the normals, unless [Auto cage](#auto-cage) is on.

Too small a cage misses parts of the high poly that stick out further; too large a cage reaches
neighboring geometry and picks up the wrong surface. That trade-off is why misses are shown in
red and why the distances are settings rather than fixed values.

## Auto cage

A single pair of distances is a compromise for the whole asset: a deep groove in one place and a
thin panel next to another part elsewhere want different cages. Other bakers solve this with a
cage mesh that the artist edits by hand. Crate instead measures one: **Auto cage** asks, for
every piece of the high poly, which part of the low poly it belongs to, and how far out or in it
lies. Each low-poly vertex then gets just the distance its surroundings need, plus a margin.

"Belongs to" is the closest point on the low poly, restricted by the `_low`/`_high` names. Here
closest points are a good fit, even though they fail for picking the hit itself: an error only
makes the cage a little larger, and the rays still decide which surface is taken. The same
weakness shows where unnamed parts touch, though: a bolt's high poly may be closest to the plate
under it and swell the plate's cage until its rays reach the bolt. Naming parts in pairs avoids
that. Distances are capped at a tenth of the bounding box diagonal, so a stray high-poly piece far
from the low poly can't inflate the cage without limit.

The cage follows the same interpolated normals as the rays, and vertices at the same position
share their distances, so the cage doesn't tear open at hard edges. Auto cage is off by default
so that existing projects bake exactly as before, and because a uniform cage is easier to
predict when you tune it by hand.

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
