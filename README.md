# Painter

A realtime 3D texture painting application for UDIM assets: a multi-channel layer stack on top
of a node graph, painted through a projection paint buffer with stencils.

The user documentation lives in its own repository, **Painter-docs** (organized with
[Diátaxis](https://diataxis.fr/)): tutorials, how-to guides, reference and explanation. It was
written first, as the MVP specification, and is kept in sync with the implementation. Source
comments cite it as `Painter-docs/<section>/<page>.md`.

## Building

Requirements: Rust (stable, 2024 edition), a GPU with Vulkan 1.2 or Direct3D 12, and on Windows
the MSVC build tools.

```
cargo build --release
```

On Windows you can also double-click `build.bat`, or run `build.bat run` to build and start
Painter (`build.bat debug` for a debug build). It finds cargo in `%USERPROFILE%\.cargo\bin` if it
isn't on `PATH`, and stops with a message if Painter is running, since Windows can't replace a
running exe.

## Running

```
painter                       # empty window: File > New Project
painter assets/crate.glb      # start with a mesh
painter Crate.painter         # open a project
painter --script setup.txt    # run a script, then continue in the window
```

## Command line

Subcommands run headless, for pipelines (documented in Painter-docs, *Command line* and
*Script commands*):

```
painter new crate.glb -o Crate.painter --bake all --script paint.txt --set color=#C03020 --export textures
painter reimport Crate.painter crate_v2.glb --export textures
painter export Crate.painter -o textures --preset "Unreal Engine"
painter run paint.txt --project Crate.painter --save        # or -c "cmd; cmd", or - for stdin
painter batch edit.txt "projects/*.painter" --save --keep-going
painter watch Crate.painter --mesh crate.glb --export textures
painter info Crate.painter
```

Add `--json` for one machine-readable result on stdout. Exit codes: 0 success, 1 failure,
2 usage error, 3 unsupported GPU.

`integrations/blender/painter_bridge.py` is a **deprecated** Blender add-on (File > Export >
Painter Project) that exports glTF and runs `painter reimport`. It is kept as a reference
implementation for calling the command line from Blender; an official Blender integration is
being built separately.

## Tests

`tests/scripts/` holds scripts that replay workflows headless and check the results:

| Script | Checks |
|---|---|
| `tutorial1.txt` | Tutorial 1 end to end, including the six exported files. |
| `tutorial2.txt` | Tutorial 2: stencil, Paint Through in Manual mode, buffer transform, bake. |
| `painting.txt` | Live and Manual painting, undo/redo, buffer transform, mask paint. |
| `caches.txt` | Cached nodes, Blur and Height to Normal; cached and uncached renders match. |
| `save_load.txt` | Saving and reopening a project renders identically; re-saving writes nothing. |
| `smart_material.txt` | Saving a smart material and adding it to another project renders identically. |

`tests/cli.sh` checks every command-line subcommand, variables, JSON output, watch mode and exit
codes. `tests/blender_bridge_test.py` runs the Blender add-on in Blender's background mode.

Run them (from a POSIX shell, after `cargo build --release`):

```
for t in tests/scripts/*.txt; do target/release/painter run "$t" || echo "FAILED: $t"; done
bash tests/cli.sh
blender --background --factory-startup --python tests/blender_bridge_test.py -- \
    "$PWD/target/release/painter" "$PWD/target/tests/cli/Crate.painter" "$PWD/target/tests/blender"
```

Renders, exports and test projects are written under `target/`.

## Code layout

| Path | Contents |
|---|---|
| `src/mesh.rs` | glTF/OBJ import, UDIM tiles, import diagnostics |
| `src/meshmaps.rs`, `src/bvh.rs` | Mesh map baking (UV raster + GPU ray tracing) |
| `src/graph.rs`, `src/ops.rs` | Node graph model and layer operations |
| `src/compile.rs`, `src/shaders/material_lib.wgsl` | Graph to WGSL compiler and its runtime library |
| `src/cache.rs` | Cached nodes and filters (Blur, Dilate, Height to Normal) |
| `src/paint.rs`, `src/stencil.rs` | Paint buffer, brushes, projection bake, stencils |
| `src/export.rs` | UV-space evaluation, padding and export presets |
| `src/projectfile.rs`, `src/smartmat.rs` | `.painter` projects and `.psmat` smart materials |
| `src/viewport.rs` | Viewport rendering (PBR, HDRI, tone mapping) |
| `src/app.rs`, `src/ui_*.rs` | User interface (egui) |
| `src/cli.rs` | Command-line interface |
| `src/script.rs`, `src/headless.rs` | Script runner and headless mode |
