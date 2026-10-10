# How to paint through a stencil

This guide shows you how to project an image onto the model by painting through a stencil.

## Load the stencil

1. In the **Stencils** section of the left panel, click **Add**.
2. Select an image. It's copied into the project.
3. Under **Paint Through target**, enable the channels to write. For a decal, Base Color is usually
   enough. To also roughen or smooth the area, enable Roughness: it receives the image's luminance.
4. If the image has transparency you want to respect, keep **Use alpha** on.

## Line it up

1. Frame the area to paint, ideally facing it straight on.
2. Hold `S` and drag to move the stencil. Add `Shift` to rotate, `Ctrl` to scale.
3. Lower **Opacity (display)** to see the model through the stencil.
4. Press `S`, then `H` to mirror the image if needed.

## Paint

1. Select a Paint layer.
2. If you want to adjust the result before it lands, switch to Manual mode (`Ctrl+Shift+B`).
3. Press `P` for the Paint Through tool.
4. Paint over the area. Use a soft brush (`Shift+[`) to blend edges.
5. In Manual mode, press `Ctrl+B` to bake.

## Continue from another angle

The stencil stays fixed to the screen, so moving the camera misaligns it.

- To continue the same image around a corner, bake, rotate the camera, then realign the stencil
  to what's already painted before continuing.
- To hide the stencil while you look around, tap `S`.

See [Brushes, tools and stencils](../reference/brushes-tools-stencils.md) for all stencil
properties.
