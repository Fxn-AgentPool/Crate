# How to reimport meshes from Blender

This guide shows you how to update a Crate project when you change its model in Blender,
keeping all layers and paint.

Paint is kept when you change UVs in Blender: Crate transfers it to the new UVs by matching
the surface, as long as the shape of painted areas stays about the same. See [what happens on
reimport](../reference/mesh-and-udims.md#reimport).

## Once: reimport from the command line

1. In Blender, choose **File → Export → glTF 2.0** and export the model, for example to
   `crate.glb` next to your project.
2. Run:

   ```
   crate reimport Crate.crate crate.glb
   ```

   Crate replaces the mesh, rebakes mesh maps that are out of date, and saves the project.
3. To export textures in the same step, add `--export textures` (and `--preset` if you need one).

If you don't want mesh maps rebaked yet, add `--no-bake`; they stay marked out of date. If the
output says that part of the UV area found no old surface, check those areas: the shape changed
there and the paint wasn't carried over.

## Every time you export: watch the file

If you export from Blender often, let Crate react to each export:

```
crate watch Crate.crate --mesh crate.glb --export textures
```

Each time Blender writes `crate.glb`, Crate reimports, rebakes, saves and exports. Stop
watching with `Ctrl+C`.

- If an export has UV errors, Crate reports them and keeps watching.
- To process exactly one change and then exit (for example in a script), add `--count 1`.

Close the project in the Crate application while it's being watched: the application does not
reload a project that changed on disk.

## From inside Blender: the Crate Bridge add-on (deprecated)

> **Deprecated.** The Crate Bridge add-on is kept as a reference implementation and is no
> longer maintained; an official Blender integration is being built separately. Prefer
> `crate reimport` or `crate watch` above. The steps below still work.

The Crate repository includes a Blender add-on, `integrations/blender/crate_bridge.py`, that
exports and reimports in one step (Blender 4.0 or later).

1. In Blender, choose **Edit → Preferences → Add-ons → Install from Disk** (**Install…** in
   Blender 4.1 and earlier) and select `crate_bridge.py`. Enable **Crate Bridge**.
2. In the add-on's preferences, set **Crate executable** to the full path of `crate`
   (`crate.exe` on Windows). If `crate` is on your `PATH`, leave it empty.
3. Choose **File → Export → Crate Project (reimport)**.
4. Set **Crate project** to the `.crate` folder. Optionally:
   - set **Mesh file**; by default the mesh is written next to the project as
     `<project name>.glb`, so exported textures are named after the project,
   - set **Export textures to**, to export after the reimport,
   - turn on **Selected only**, to export just the selected objects.
5. Click **OK**. Blender's status bar reports the result, for example
   *Crate: reimported in 1.0s, exported 6 textures*. Errors from Crate appear there too.

See [Command line](../reference/command-line.md) for all `reimport` and `watch` options, and
[Mesh import and UDIM tiles](../reference/mesh-and-udims.md#reimport) for what happens to paint
when tiles change.
