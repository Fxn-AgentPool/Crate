# About the paint buffer

There are two broad ways to paint on a 3D model.

**Direct painting** places each brush dab straight into the texture. For every dab, the
application works out which texels lie under the brush on the surface and writes to them. This is
the approach that makes Substance Painter feel immediate.

**Buffer painting**, the approach popularized by Mari, separates painting from texturing. You paint
on a flat 2D canvas aligned with the screen. Then, in a separate step called *baking*, that
canvas is projected through the camera onto the surface and written into the textures.

Crate uses buffer painting internally and offers both experiences on top of it.

## Why a buffer

A screen-aligned buffer has some useful properties:

- **Painting is plain 2D painting.** Brush tips, spacing, jitter and pressure behave exactly as in
  an image editor, at a predictable resolution. They aren't distorted by UV stretching or mesh
  density.
- **Projection happens in one place.** Occlusion, backface handling, grazing-angle falloff, tile
  borders and seam padding are all handled by the bake. Brushes don't need to know about any of
  them.
- **Paint can be adjusted before it lands.** In Manual mode you can move, rotate and scale what
  you painted, then bake it. This is what makes stencils and projected photographs practical.
- **Stencils are natural.** A stencil is just another screen-aligned image. Paint Through copies
  from one screen-space image to another. No 3D math is involved until the bake.

The cost is that the buffer is tied to a single camera view. Paint in the buffer only means
something from the viewpoint where it was painted.

## Live and Manual modes

**Manual** mode exposes the buffer directly. Paint accumulates on screen, unbaked, until you bake.
If the camera moves before then, the buffer must either be baked, cleared or the camera locked,
which is the **On camera move** setting. This is the Mari workflow, and it is the right one for
projecting photographs, aligning decals and careful hero work.

**Live** mode bakes after every dab, but only the small screen rectangle that dab touched, then
clears it. The buffer is never visibly "full", the camera can move freely between strokes, and
paint appears on the model as you work. This feels like direct painting, and it is the default,
because it is what most people expect.

Both modes run the same code. Live mode is simply "bake the dirty region, often". This matters
for consistency: a stroke painted live and the same stroke painted in Manual and then baked
produce identical texels.

## Limits of projection

Projection paints what the camera sees. Some consequences:

- Surfaces seen at a grazing angle receive little or no paint, because of the falloff. This
  avoids smeared, stretched paint. To paint such a surface, look at it more directly.
- Surfaces hidden behind other geometry are not painted, unless **Project through** is on.
- Brush size is in screen pixels by default, so a stroke covers less surface when zoomed out. The
  **World** size space compensates by scaling each dab by its depth.
- Buffer resolution limits detail. A 2K viewport painting onto a 4K tile that fills a quarter of
  the screen is writing far fewer texels than the tile holds. Zoom in, or raise the buffer
  resolution scale, for fine detail.

## Related

- [Paint buffer](../reference/paint-buffer.md) (reference)
- [How to paint in manual bake mode](../how-to/paint-in-manual-bake-mode.md)
- [Project a decal with a stencil](../tutorials/02-project-a-decal-with-a-stencil.md) (tutorial)

## Painting in UV space

The [UV view](../reference/uv-view.md) doesn't have a painting system of its own. It's another
way of mapping texels to the screen: where the 3D view projects each texel's surface position
through the camera, the UV view places it by its UV. Strokes go into the same buffer and are
baked by the same pass, so brushes, stencils, Manual mode and undo behave identically.

Ordinary paint only reaches the UV islands and their padding, from either view: a texel
between islands has no surface of its own, so ordinary paint there couldn't be carried to new
UVs on reimport, and it would mostly be hidden by padding. [2D paint](../reference/2d-paint.md)
is the deliberate exception; see [About painting between UV
islands](about-painting-between-islands.md).
