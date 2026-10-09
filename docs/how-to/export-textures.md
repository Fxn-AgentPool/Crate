# How to export textures for a game engine

This guide shows you how to export packed textures for glTF, Unreal Engine or Unity.

## Export

1. Choose **File → Export Textures** (`Ctrl+E`).
2. Choose a **Preset**:
   - For glTF viewers, Blender and Godot: **glTF Metal/Rough**.
   - For Unreal Engine: **Unreal Engine**. Its normal maps are DirectX-style.
   - For Unity HDRP: **Unity HDRP**.
   - For a pipeline that packs maps itself: **Raw Channels**.
3. Set the **Output directory**.
4. Click **Export**.

## Adjust for your target

- **If the target doesn't support UDIMs**, export each tile and assign them to separate materials,
  or remove `{udim}` from the template only if the project has a single tile.
- **If normals look inverted in the engine**, switch **Normal format** between OpenGL and
  DirectX.
- **To reduce file size**, lower **Resolution**.
- **To export only the tiles you changed**, select them under **Tiles**.
- **If mipmaps show seams**, keep **Padding** at **Infinite**.

## Make a custom preset

1. Select the closest existing preset and click **New…** next to it.
2. Add or rename output maps, and set each map's R, G, B and A source.
3. Click **Save**. The preset is available in every project.

See [Export](../reference/export.md) for the full list of preset contents, template tokens and
sources.
