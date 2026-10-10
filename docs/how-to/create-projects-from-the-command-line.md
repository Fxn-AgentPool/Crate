# How to create projects from the command line

This guide shows you how to turn a mesh into a ready-to-use Crate project, and optionally
textures, without opening the application.

## Create a project

```
crate new crate.glb -o Crate.crate --bake all
```

- To choose the tile resolution, add `--tile-resolution 1024` or `4096` (default `2048`).
- To bake only some maps, list them: `--bake ao,curvature`. Generators such as Edge Wear and
  Dirt need curvature and AO.
- If the mesh has import errors (missing or out-of-range UVs), the command fails and no project
  is created. Fix the mesh and run it again.

## Start from a material

To set up layers as part of the same command, pass a [script](../reference/script-commands.md):

```
crate new crate.glb -o Crate.crate --bake all --script base-material.txt
```

To start from a smart material in your library, the script can be a single line:

```
add-smart "Worn Red Paint"
```

## Export straight away

Add an export directory and, if needed, export options:

```
crate new crate.glb -o Crate.crate --bake all --script base-material.txt --export textures --preset "Unity HDRP"
```

## Make several variants

Use a variable in the script (for example `set base_color=${color}`) and run `new` once per
variant:

```
crate new crate.glb -o CrateRed.crate --bake all --script paint.txt --set color=#C03020
crate new crate.glb -o CrateBlue.crate --bake all --script paint.txt --set color=#2A4C7D
```

See [Command line](../reference/command-line.md#new) for every option.
