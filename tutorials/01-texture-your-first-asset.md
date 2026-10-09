# Texture your first asset

In this tutorial, we will texture a painted metal crate from scratch. We'll give it a coat of blue
paint, let bare metal show through on its worn edges, paint some grime by hand, and export the
result as textures ready for a game engine.

Along the way we'll meet the main parts of Painter: projects, mesh maps, layers, masks, the brush
and export.

It takes about 20 minutes.

## What you need

- Painter, installed and running.
- The sample crate that ships with Painter: `assets/crate.glb`. It uses two UDIM tiles,
  `1001` and `1002`.

## Create the project

First, we'll create a project from the crate.

1. Choose **File → New Project**.
2. Select `assets/crate.glb`.
3. Leave **Tile resolution** at `2048`, and click **Create**.
4. Save the project as `Crate.painter` in your documents folder.

The crate appears in the viewport, in plain gray. Press `U`: a grid appears, with the crate's
UVs spread across two tiles, labeled `1001` and `1002`. Press `U` again to hide it.

Hold `Alt` and drag with the left mouse button to orbit around the crate. Press `F` if you lose
it.

## Bake mesh maps

Painter needs to know the crate's shape (where its edges and crevices are) before it can wear
them procedurally. We get this by baking mesh maps.

1. Choose **Window → Mesh Maps**.
2. **Ambient Occlusion**, **Curvature**, **Thickness** and **ID** are already checked. Click
   **Bake selected**.

The status bar shows the bake's progress. It takes a few seconds.

When it's done, press `Shift+C` to look at the maps. Press it until the viewport shows
**Curvature**: the crate's edges are light and its grooves are dark. Notice how the frame's
outer corners are the brightest. That's where paint would wear first. Press `M` to go back to
the normal view.

## Paint the crate blue

Now we'll add the paint coat, as a fill layer.

1. Press `Ctrl+Shift+F`. A layer called **Fill** appears in the Layers panel.
2. Double-click its name and rename it to `Blue paint`.
3. In **Properties**, set:
   - **Base Color** to `#2A4C7D`
   - **Roughness** to `0.6`
   - **Metallic** to `0`

The whole crate turns a satin blue.

## Reveal metal on the edges

Next, we'll add a bare metal layer above the paint and show it only on the worn edges.

1. Press `Ctrl+Shift+F` again, and rename the new layer to `Bare metal`. Make sure it's above
   `Blue paint` in the list.
2. Set:
   - **Base Color** to `#B4B4B4`
   - **Roughness** to `0.3`
   - **Metallic** to `1`

The crate is now entirely shiny metal, because the new layer covers everything.

3. Right-click `Bare metal` and choose **Add black mask**.

The metal disappears and the crate is blue again. A black mask hides the whole layer.

4. Right-click the mask and choose **Add generator**, then **Edge Wear**.

Metal now shows through on the crate's edges and corners, broken up by noise. Orbit around the
crate to see it from different sides.

5. In **Properties**, set **Amount** to `0.4`.

The wear spreads further from the edges. Try a few values to see how it responds, then set it
back to `0.4`.

## Paint grime by hand

Generators are good for wear that follows the shape. For grime where *we* want it, we'll paint.

1. Press `Ctrl+Shift+N` to add a paint layer, and rename it to `Grime`. It should be at the top.
2. Press `B` for the Paint tool.
3. In **Brush Values**, set:
   - **Base Color** to `#3B2F22`, enabled
   - **Roughness** to `0.9`, enabled
   - **Metallic**, disabled
4. In **Brush**, set **Hardness** to `0.2` and **Flow** to `0.3`.
5. Orbit so you can see the bottom edge of the crate.
6. Paint along the bottom edge with short strokes.

Brown grime appears on the crate as you paint. Paint over the same place again: it builds up,
because the flow is low. Notice that grime painted near the frame's corner continues around it,
since paint is projected onto whatever surface is under the brush.

If you paint somewhere you didn't mean to, press `Ctrl+Z` to undo the last stroke.

Orbit around and paint grime along the other bottom edges.

## Look at the result

Press `C` a few times to look at individual channels. On **Roughness**, the grime is bright
(rough), the metal edges are dark (smooth), and the blue paint sits in between. On **Metallic**,
only the worn edges are white. Press `M` to return to the full material.

## Export the textures

Finally, we'll export textures that any glTF-compatible engine can use.

1. Press `Ctrl+E`.
2. Leave **Preset** at **glTF Metal/Rough**.
3. Click **Export**.

When it finishes, Painter shows the export folder. It contains six files, three maps for each
of the two tiles:

```
crate_BaseColor.1001.png   crate_BaseColor.1002.png
crate_ORM.1001.png         crate_ORM.1002.png
crate_Normal.1001.png      crate_Normal.1002.png
```

There's no Emissive map, because the crate doesn't glow and that channel is turned off.

Press `Ctrl+S` to save the project.

## What we did

We've textured a complete asset: we created a project, baked mesh maps, built a material from fill
layers, used a generator mask to wear its edges, painted grime by hand, and exported textures.

Next, try [Project a decal with a stencil](02-project-a-decal-with-a-stencil.md), which continues
with this crate.
