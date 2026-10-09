# Painter documentation

> **Status: MVP implemented.** The application in this repository implements these documents
> (see the repository README for building and running it). The docs were written first, as the
> specification; where the implementation made a different choice, the docs were updated to
> match. When the software and these docs disagree, either the software has a bug or the docs
> need a deliberate update — never silently both.
>
> Both tutorials are replayed automatically by `tests/scripts/tutorial1.txt` and
> `tutorial2.txt`.
>
> "Painter" is a working title.

Painter is a realtime 3D texture painting application for UDIM assets. It combines:

- a **multi-channel layer stack** for everyday texturing,
- a **node graph** underneath it for procedural work,
- a **projection paint buffer** with **stencils** for painting, live or under manual control.

## How these docs are organized

The docs follow [Diátaxis](https://diataxis.fr/), which separates documentation into four kinds,
each serving a different need:

| | For **study** (acquiring skill) | For **work** (applying skill) |
|---|---|---|
| **Doing** (action) | [Tutorials](#tutorials) | [How-to guides](#how-to-guides) |
| **Thinking** (cognition) | [Explanation](#explanation) | [Reference](#reference) |

### Tutorials

Lessons that take you through texturing an asset from start to finish. Start here if you are new.

1. [Texture your first asset](tutorials/01-texture-your-first-asset.md)
2. [Project a decal with a stencil](tutorials/02-project-a-decal-with-a-stencil.md)

### How-to guides

Directions for specific tasks, assuming you already know your way around.

- [How to import a mesh with UDIMs](how-to/import-a-mesh-with-udims.md)
- [How to bake mesh maps](how-to/bake-mesh-maps.md)
- [How to paint in manual bake mode](how-to/paint-in-manual-bake-mode.md)
- [How to paint through a stencil](how-to/paint-through-a-stencil.md)
- [How to mask a layer with a generator](how-to/mask-a-layer-with-a-generator.md)
- [How to edit a layer as a node graph](how-to/edit-a-layer-as-a-graph.md)
- [How to create a smart material](how-to/create-a-smart-material.md)
- [How to speed up a slow graph](how-to/speed-up-a-slow-graph.md)
- [How to export textures for a game engine](how-to/export-textures.md)

### Reference

Technical descriptions of the machinery. Structured like the application.

- [Project format](reference/project-format.md)
- [Mesh import and UDIM tiles](reference/mesh-and-udims.md)
- [Channels](reference/channels.md)
- [Layers and masks](reference/layers.md)
- [Blend modes](reference/blend-modes.md)
- [Nodes](reference/nodes.md)
- [Paint buffer](reference/paint-buffer.md)
- [Brushes, tools and stencils](reference/brushes-tools-stencils.md)
- [Mesh maps](reference/mesh-maps.md)
- [Export](reference/export.md)
- [Windows and panels](reference/windows-and-panels.md)
- [Keyboard shortcuts](reference/keyboard-shortcuts.md)
- [Limits and requirements](reference/limits.md)

### Explanation

Background, design decisions and context. Read these away from the application.

- [About layers and nodes](explanation/about-layers-and-nodes.md)
- [About the paint buffer](explanation/about-the-paint-buffer.md)
- [About realtime evaluation and caching](explanation/about-realtime-evaluation.md)
- [About UDIM tiles and paint storage](explanation/about-tile-storage.md)
- [About the MVP scope](explanation/about-mvp-scope.md)

## Contributing to the docs

Before writing, decide which of the four kinds your content is (the Diátaxis *compass*):

| If the content… | …and serves the reader's… | …it belongs in |
|---|---|---|
| informs action | acquisition of skill | `tutorials/` |
| informs action | application of skill | `how-to/` |
| informs cognition | application of skill | `reference/` |
| informs cognition | acquisition of skill | `explanation/` |

Rules of thumb:

- **Tutorials** use "we", produce a visible result at every step, offer no alternatives, and must
  work every time. They only use files shipped in `assets/`.
- **How-to guides** are titled "How to …", start with "This guide shows you how to …", and contain
  actions only. Link to reference for options and to explanation for reasons.
- **Reference** is neutral and complete, and mirrors the structure of the application. No
  instructions, no opinions.
- **Explanation** is titled "About …", may discuss alternatives and history, and contains no steps.

If a page starts mixing kinds, split it and link between the parts.
