# Project a decal with a stencil

In this tutorial, we will paint a white arrow onto the side of the crate, the kind of
marking that shows which way up it goes. We'll load the
decal as a stencil, line it up on screen, paint it onto the crate, and adjust it before
committing it.

This tutorial continues from [Texture your first asset](01-texture-your-first-asset.md). It takes
about 10 minutes.

## What you need

- The `Crate.crate` project from the first tutorial.
- The sample decal that ships with Crate: `assets/D_SampleDecal.png`.

## Open the project and frame the side

1. Choose **File → Open Project** and open `Crate.crate`.
2. Orbit the crate until you're looking straight at one of its sides. Each side has a recessed
   panel in the middle, surrounded by a frame.
3. Zoom with the mouse wheel until that side fills most of the viewport.

## Add a layer for the decal

1. Press `Ctrl+Shift+N` and rename the new layer to `Decal`.
2. Drag it below `Grime`, so any grime will sit on top of the decal.

## Load the stencil

1. In the **Stencils** section of the left panel, click **Add**.
2. Select `assets/D_SampleDecal.png`.

A white arrow pointing up appears over the viewport, semi-transparent. Orbit
the crate a little: the arrow stays exactly where it is on the screen, while the crate moves behind it. Return
to looking straight at the side.

## Line it up

1. Hold `S` and drag with the left mouse button. The stencil follows the mouse.
2. Hold `S` and `Ctrl`, and drag left. The stencil shrinks.
3. Move and scale it until the arrow sits in the middle of the recessed panel, filling about
   two-thirds of its width. Keep it inside the panel, clear of the frame.

## Switch to Manual mode

So far, every stroke has gone onto the crate as we painted it. This time we want to check the
decal before it lands.

Press `Ctrl+Shift+B`. The status bar now shows **Manual**.

## Paint the decal

1. Press `P` for the Paint Through tool.
2. In **Brush**, set **Size** to `150` and **Hardness** to `0.9`.
3. Paint over the arrow.

As you paint, the arrow appears in white on the blue paint where the brush passes. The space
around it stays blue, because the stencil is transparent there. The viewport border turns
orange, and the status bar says **Unbaked**: the paint is in the buffer, not yet on the crate.

4. Tap `S` to hide the stencil, so you can see just what we've painted.

## Adjust before baking

We'll move the arrow up a little, to practice adjusting paint before it lands.

1. Hold `V` and drag upward a short distance. The painted arrow moves across the panel. Keep
   it inside the panel.
2. Hold `V` and `Shift`, and drag slightly. The decal tilts. Tilt it back so it's straight.

Notice that we're moving the paint itself now, not the stencil.

## Bake

Press `Ctrl+B`.

The orange border disappears and **Unbaked** clears from the status bar. The decal is now part of
the `Decal` layer. Orbit around the crate: the arrow stays on the panel, wherever we look from.

## Return to Live mode

Press `Ctrl+Shift+B`, so painting works as it did before. Press `Ctrl+S` to save.

## What we did

We loaded an image as a stencil, aligned it to the crate on screen, painted through it in Manual
mode, adjusted the paint before committing it, and baked it into a layer.

To learn why painting works this way, read
[About the paint buffer](../explanation/about-the-paint-buffer.md). For a quick reference of the
steps, see [How to paint through a stencil](../how-to/paint-through-a-stencil.md).
