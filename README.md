# Painter

A realtime 3D texture painting application for UDIM assets: a multi-channel layer stack on top
of a node graph, painted through a projection paint buffer with stencils.

The user documentation lives in [`docs/`](docs/README.md) (organized with
[Diátaxis](https://diataxis.fr/)). It was written first, as the MVP specification, and kept in
sync with the implementation.

## Building

Requirements: Rust (stable, 2024 edition), a GPU with Vulkan 1.2 or Direct3D 12, and on Windows
the MSVC build tools.

```
cargo build --release
```

## Running

```
target/release/painter                     # empty window: File > New Project
target/release/painter assets/crate.glb    # start with a mesh
target/release/painter --script setup.txt  # run a script (see below), then continue in the UI
```

## Scripts and tests

Every user action has a script command, so workflows can be replayed without a window:

```
target/release/painter --headless run tests/scripts/tutorial1.txt
```

`tests/scripts/` contains:

| Script | Checks |
|---|---|
| `tutorial1.txt` | Tutorial 1 end to end, including the six exported files. |
| `tutorial2.txt` | Tutorial 2: stencil, Paint Through in Manual mode, buffer transform, bake. |
| `painting.txt` | Live and Manual painting, undo/redo, buffer transform. |
| `caches.txt` | Cached nodes, Blur and Height to Normal; cached and uncached renders match. |
| `save_load.txt` | Saving and reopening a project renders identically; re-saving writes nothing. |
| `smart_material.txt` | Saving a smart material and adding it to another project renders identically. |

Run them all (from a POSIX shell):

```
for t in tests/scripts/*.txt; do target/release/painter --headless run "$t" || echo "FAILED: $t"; done
```

Renders and exports are written under `target/`.

The full command list is in `src/script.rs`. The most common:

| Command | Effect |
|---|---|
| `new <mesh> [resolution]` | New project |
| `bake [maps…]` | Bake mesh maps |
| `add-fill [name]`, `add-paint [name]` | Add layers |
| `set key=value…` | Layer or node parameters (`base_color=#2A4C7D`, `amount=0.4`) |
| `add-black-mask`, `add-generator edge-wear` | Masks |
| `brush …`, `brush-values …`, `tool paint`, `stroke x,y x,y…` | Painting (viewport fractions) |
| `mode manual`, `buffer-transform dx dy deg scale`, `bake-buffer` | Paint buffer |
| `stencil-add <image>`, `stencil-transform dx dy deg scale` | Stencils |
| `node <var> <type>`, `link a.out b.in`, `cache <var> on` | Node graph |
| `camera <yaw> <pitch> [zoom]`, `view <channel or map>`, `render <out.png>` | Viewport |
| `export <dir> [preset=…]`, `save <dir.painter>`, `open <dir.painter>` | Files |
| `expect-layers …`, `expect-files …`, `expect-same a.png b.png` | Assertions |

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
| `src/script.rs`, `src/headless.rs` | Script runner and headless mode |
