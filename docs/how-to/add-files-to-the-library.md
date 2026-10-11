# How to add your own files to the library

This guide shows you how to add textures, environment maps, brush alphas, stencils and your own image
collections to the user library, so they're available in every project.

## Open the library folder

1. In the **Library** section of the left panel, click the tab of the category to add to, for
   example **Environments** (hover a tab to see its name).
2. Click **Show in file browser** (the folder button right of the category's name). The
   category's folder opens.

The folders are listed in [Windows and panels](../reference/windows-and-panels.md#library).

## Add textures

1. Copy images into the `textures` folder.
2. In **Library → Textures**, click **Refresh**.
3. Drag a texture onto the Node Editor. An Image node using it is added where you drop it.

The procedural presets at the start of **Textures** (noise, cellular, gradient) are built in and
drag onto the Node Editor the same way.

## Add environment maps

1. Copy equirectangular `.hdr` or `.exr` files into the `environments` folder.
2. In **Library → Environments**, click **Refresh**.
3. Double-click an environment to light the viewport with it. **Viewport → Studio lighting**
   switches back.

## Add brush alphas

1. Copy images into the `alphas` folder. White paints, black doesn't; transparent areas don't
   paint either.
2. In **Library → Alphas**, click **Refresh**.
3. Double-click an alpha to use it as the brush tip. To go back to the round tip, click **×**
   next to **Alpha** in the **Brush** panel.

## Add stencils

1. Copy images into the `stencils` folder.
2. In **Library → Stencils**, click **Refresh**.
3. Double-click a stencil to load it. It's copied into the project, as with **+** in the **Stencils** section.
   See [How to paint through a stencil](paint-through-a-stencil.md).

## Add a collection of your own

1. In the file browser, go up one level to the `library` folder and create a new folder, for
   example `grunge`.
2. Copy images into it.
3. Click **Refresh** in any Library category. The folder appears as a new tab, with a folder
   icon, after the built-in categories.
4. Double-click an image in it to load it as a stencil.

## Use library files in scripts

Name the item instead of a file. Scripts look in the command's folder, or in `<folder>/<name>`:

```
environment studio_small_03
brush alpha=dots
stencil-add grunge/scratches
```

See [Script commands](../reference/script-commands.md#library-items).
