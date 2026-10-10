# Windows and panels

Side panels can be resized by dragging their edges. Windows opened from the menus float over
the viewport.

| Panel | Default location | Contents |
|---|---|---|
| Viewport | center | 3D view of the mesh. Paint buffer and stencils are drawn over it. |
| Layers | right | The root stack, or the selected group. See [Layers and masks](layers.md). |
| Properties | right, below Layers | Parameters of the selected layer, mask layer or node. |
| Node Editor | bottom (hidden) | The graph of the selected layer, or of the whole project. `Ctrl+Space` toggles it. See [Node Editor](node-editor.md). |
| Tools | left, top | Paint, Erase and Paint Through. |
| Brush | left | Tool and brush properties. See [Brushes, tools and stencils](brushes-tools-stencils.md). |
| Brush Values | left, below Brush | Per-channel values painted by the brush. |
| Paint Buffer | left | Bake mode, buffer and projection settings. See [Paint buffer](paint-buffer.md). |
| Stencils | left | Loaded stencils, the active stencil and its properties. |
| Library | left, bottom | Project resources and the user library, by category. See [Library](#library). |
| Mesh Maps | Window menu | Mesh map status and bake settings. See [Mesh maps](mesh-maps.md). |
| Import Log | Window menu | Diagnostics from the last mesh import. |
| Performance | Window menu | Frame time, shader length, GPU memory use and per-node cache cost. |
| Channels | Project menu | Enabled channels, default values and height scale. See [Channels](channels.md). |
| Preferences | Edit menu | Update checks, memory budgets, undo steps and keyboard shortcuts. |
| Updates | Help menu | Installed and latest version, release notes, download and install. See [Updates](updates.md). |

## Library

The Library has two rows of categories. **Project** categories show resources of the open
project; **Library** categories show the user library, stored in the user directory (see
[Limits and requirements](limits.md)).

| Category | Row | Contents |
|---|---|---|
| Mesh Maps | Project | The six [mesh maps](mesh-maps.md), with a thumbnail of the first UDIM tile. Unbaked maps show **not baked**; out-of-date maps have an orange name. |
| Images | Project | Imported images, with their size. Hover shows the source file. |
| Smart Materials | Library | Saved smart materials, with thumbnails. **Refresh** rescans the folder; **Show in file browser** opens it. |
| Materials, Smart Masks, Brushes, Alphas, Stencils, Environments | Library | Reserved; empty. |

| Item | Double-click | Drag | Right-click |
|---|---|---|---|
| Mesh map | Adds it as a mask effect (a Mesh Map node) on the selected layer. | Onto the Node Editor: adds a Mesh Map node. | **Add to mask of selected layer**, **Show in viewport**. |
| Smart material | Adds it above the selected layer. | Onto the Layers panel: adds it there. | **Add to layers**. |

## Menus

| Menu | Items |
|---|---|
| File | New Project, Open Project, Save, Save As, Reimport Mesh, Export Textures |
| Edit | Preferences, Undo, Redo |
| Viewport | Exposure, Environment rotation, Load environment (HDR/EXR), Studio lighting |
| Project | Channels |
| Window | Performance, Node Editor, Mesh Maps, Import Log |
| Help | Check for Updates… (**Update Available…** when a newer release exists) |

## Status bar

| Field | Values |
|---|---|
| Mesh | File name, triangle count, tiles and tile resolution |
| Bake mode | **Live**, **Manual** |
| Buffer | **Unbaked** when the buffer holds paint (Manual mode) |
| Target | Name of the layer or mask that painting writes to, or **No paint target** |
| Display | **Material**, or the channel or mesh map shown |
| Background tasks | Mesh map bake progress |
| Messages | Undo/redo, errors and hints, briefly |

GPU memory use is shown in the **Performance** window.
