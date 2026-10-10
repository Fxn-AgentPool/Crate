# Project format

A Painter project is a directory with the extension `.painter`. The application presents it as a
single document.

## Directory layout

```
Crate.painter/
├── project.json          Project manifest and layer/node graph
├── mesh/
│   └── source.glb        Copy of the imported mesh
├── maps/
│   └── <map>.<udim>.exr  Baked mesh maps, one file per map per tile
├── paint/
│   └── <node-id>/
│       └── <udim>/
│           └── <px>_<py>.page   Painted pages (see "Paint pages")
├── stencils/
│   └── <name>.<ext>      Copies of loaded stencil images
├── images/
│   └── <name>.<ext>      Copies of images used by Image nodes
└── cache/                Derived data. Safe to delete while the project is closed.
```

## `project.json`

UTF-8 JSON. Top-level keys:

| Key | Type | Description |
|---|---|---|
| `format` | string | Always `"painter-project"`. |
| `version` | integer | Format version. MVP: `1`. |
| `mesh` | object | Source mesh path and import settings. See [Mesh import](mesh-and-udims.md). |
| `tileResolution` | integer | Texel resolution of every UDIM tile: `1024`, `2048` or `4096`. |
| `tiles` | array of integer | UDIM numbers in use, e.g. `[1001, 1002]`. Derived on import. |
| `channels` | array of object | Enabled channels and their formats. See [Channels](channels.md). |
| `graph` | object | Nodes and connections. See [Nodes](nodes.md). |
| `rootStack` | string | Id of the Stack node shown in the Layers panel. |
| `viewport` | object | Camera, exposure, environment rotation and environment file. |
| `export` | object | Last-used export settings. See [Export](export.md). |
| `stencils` | object | Loaded stencils (image name, transform, display and Paint Through settings) and the active one. |
| `meshMaps` | object | Bake settings and the list of baked maps. |
| `images` | array of string | Images in `images/`. |

### `graph`

```json
{
  "nodes": [
    { "id": "n7", "type": "noise", "params": { "scale": 4.0, "octaves": 5 }, "cached": false }
  ],
  "connections": [
    { "from": "n7.out", "to": "n9.mask" }
  ]
}
```

Node ids are unique within the project and never reused. Stack nodes also carry their `slots`
(the layers: name, visibility, opacity, `content` and `mask` node ids, per-channel blend
settings) and Mask Stack nodes their `mask_slots`.

## Paint pages

Paint data is stored sparsely in **pages** of 128 × 128 texels. A page file exists only if at
least one texel in it has been painted.

- `<px>_<py>` is the page coordinate within the tile, from `0_0` (bottom-left) upward.
- A page file contains every channel of the paint node, as 16-bit float, zstd-compressed: five
  planes of 128 × 128 RGBA texels (base color + coverage, emissive + coverage, normal +
  coverage, roughness/metallic/height/opacity, and their coverages), values premultiplied by
  coverage. Paint Mask pages have one plane (premultiplied value in RGB, coverage in A).
- Pages without any coverage are not stored.

## Smart material files

Smart materials are stored outside projects, as `.psmat` files in the user library directory.
A `.psmat` file is a zip archive containing:

| Entry | Description |
|---|---|
| `material.json` | `format: "painter-smart-material"`, `version`, the layer slot, the subgraph's nodes, the list of exposed parameters, and the UDIM tiles the paint was made on. |
| `paint/` | Paint pages of any Paint nodes in the subgraph, same layout as in a project. |
| `images/` | Images used by Image and Fill nodes. |
| `thumbnail.png` | 256 × 256 preview: the viewport at the time the material was saved. |

## Versioning

- Painter MVP opens projects with `version` ≤ `1`.
- Projects are saved atomically: written to `project.json.tmp`, then renamed.
- Saving rewrites only paint pages that changed since the last save, and only mesh maps that
  were rebaked.
- Position and World Normal maps are saved but rebaked from the mesh on open.
- Unknown keys are preserved on save.
