# About the MVP scope

The MVP aims to prove one idea: that a **layer stack on a node-graph engine**, painted through a
**projection buffer** with **live and manual baking**, makes a coherent and pleasant tool. Every
feature in the MVP supports that loop:

> import → bake mesh maps → build layers and masks → paint, live or with stencils → export.

Anything that doesn't serve the loop, or that can be added later without changing the core
design, is left out.

## What's left out, and why

| Not in MVP | Reason |
|---|---|
| Opening Substance Painter (`.spp`) files | Proprietary format. Planned as a separate export plugin that runs inside Substance Painter, rather than a file parser. |
| Substance (`.sbsar`) materials | Requires Adobe's licensed engine. The node graph covers the MVP's procedural needs. |
| High-poly to low-poly baking | Mesh maps from the low-poly mesh are enough for generators. High-poly baking is a large, separate subsystem. |
| Multiple texture sets | One set with UDIMs covers the core workflow. Multiple sets mostly add bookkeeping. |
| Symmetry painting | Needs mirrored projection in the bake. Fits the existing design, but isn't needed to prove it. |
| Clone and smudge tools | Paint Through covers the main projection use case. |
| Particle and physics brushes | Large feature with little bearing on the core design. |
| Streaming virtual textures | Page storage is already compatible. See [About UDIM tiles and paint storage](about-tile-storage.md). |
| Per-tile resolution | Simplifies storage and export. Revisit with virtual texturing. |
| Custom channels | The seven built-in channels cover standard PBR. |
| USD and MaterialX | Valuable for pipelines, but glTF and OBJ are enough to test the workflow. |
| Scripting API (a programming interface) | The data model should stabilize before it's exposed. Crate does have a [command line](../reference/command-line.md) and [script commands](../reference/script-commands.md) that mirror user actions; see [About automation](about-automation.md). |
| macOS | GPU backend and tablet input work needed. Not required to validate the design. |

## Choices made to keep the core extensible

Some decisions cost a little now so that later features don't require rewrites:

- **Paint is stored in pages, not whole textures**, so virtual texturing can slot in later.
- **The Layers panel is a view of the graph**, so adding nodes never requires matching layer
  features.
- **All painting goes through the buffer**, so new tools (clone, smudge, symmetry) only need to
  write into a 2D buffer.
- **Export is the graph evaluated in UV space**, so any new node is automatically exportable.
