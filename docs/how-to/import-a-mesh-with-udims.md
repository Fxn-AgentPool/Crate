# How to import a mesh with UDIMs

This guide shows you how to create a project from a mesh whose UVs span several UDIM tiles.

## Before you start

- Export the mesh as glTF (`.glb`/`.gltf`) or OBJ, with UVs laid out in tiles from `1001` upward.
- Make sure no UVs are negative or beyond `u = 10` / `v = 10`.

## Create the project

1. Choose **File → New Project** (`Ctrl+N`).
2. Select the mesh file.
3. Choose a **Tile resolution**. All tiles use the same resolution.
4. If the mesh was authored Z-up, set **Up axis** to `Z`.
5. If the mesh is in centimeters and you want world-unit brush sizes in meters, set **Scale** to
   `0.01`.
6. Click **Create**, and choose where to save the `.crate` project directory.

## Check the result

- Press `U` to show the tile grid and confirm the tiles you expect are listed.
- Open **Window → Import Log** to review diagnostics.

If the log shows:

- **`UV_OUT_OF_RANGE`**: move the offending UVs into tiles `1001`–`1100` in your modeling tool, and
  import again.
- **`UV_CROSSES_TILE`**: if the seam is visible, split the island at the tile edge in your modeling
  tool, then use **File → Reimport Mesh**.
- **`UV_OVERLAP`**: if the overlap is unintentional, fix it in your modeling tool. Overlapping UVs
  share paint.

## Replace the mesh later

Choose **File → Reimport Mesh** and select the updated file. Then rebake mesh maps, see
[How to bake mesh maps](bake-mesh-maps.md).

See [Mesh import and UDIM tiles](../reference/mesh-and-udims.md) for all settings and diagnostics.
