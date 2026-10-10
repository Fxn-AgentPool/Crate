# Painter documentation

> **Status: MVP implemented.** These are the user docs for the Painter application, which
> lives in its own repository (see its README for building and running it). The docs were
> written first, as the specification; where the implementation made a different choice, the
> docs were updated to match. When the software and these docs disagree, either the software
> has a bug or the docs need a deliberate update — never silently both.
>
> Tutorials 1 and 2 are replayed automatically by the application repository's
> `tests/scripts/tutorial1.txt` and `tutorial2.txt`, and the command line by its
> `tests/cli.sh`; the sample files they use are in its `assets/` folder.
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
3. [Automate Painter from the command line](tutorials/03-automate-with-the-command-line.md)

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
- [How to create projects from the command line](how-to/create-projects-from-the-command-line.md)
- [How to reimport meshes from Blender](how-to/reimport-meshes-from-blender.md)
- [How to edit many projects at once](how-to/edit-many-projects-at-once.md)
- [How to call Painter from other tools](how-to/call-painter-from-other-tools.md)

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
- [Command line](reference/command-line.md)
- [Script commands](reference/script-commands.md)
- [Limits and requirements](reference/limits.md)

### Explanation

Background, design decisions and context. Read these away from the application.

- [About layers and nodes](explanation/about-layers-and-nodes.md)
- [About the paint buffer](explanation/about-the-paint-buffer.md)
- [About realtime evaluation and caching](explanation/about-realtime-evaluation.md)
- [About UDIM tiles and paint storage](explanation/about-tile-storage.md)
- [About the MVP scope](explanation/about-mvp-scope.md)
- [About automation](explanation/about-automation.md)

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
