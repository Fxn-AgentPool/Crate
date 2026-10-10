# Windows and panels

Side panels can be resized by dragging their edges. Windows opened from the menus float over
the viewport.

| Panel | Default location | Contents |
|---|---|---|
| Viewport | center | 3D view of the mesh. Paint buffer and stencils are drawn over it. |
| Layers | right | The root stack, or the selected group. See [Layers and masks](layers.md). |
| Properties | right, below Layers | Parameters of the selected layer, mask layer or node. |
| Node Editor | bottom (hidden) | The graph of the selected layer or mask. See [Nodes](nodes.md). |
| Tools | left, top | Paint, Erase and Paint Through. |
| Brush | left | Tool and brush properties. See [Brushes, tools and stencils](brushes-tools-stencils.md). |
| Brush Values | left, below Brush | Per-channel values painted by the brush. |
| Paint Buffer | left | Bake mode, buffer and projection settings. See [Paint buffer](paint-buffer.md). |
| Stencils | left | Loaded stencils, the active stencil and its properties. |
| Library | left, bottom | Smart materials. Drag one onto the Layers panel, or double-click it, to add it. |
| Mesh Maps | Window menu | Mesh map status and bake settings. See [Mesh maps](mesh-maps.md). |
| Import Log | Window menu | Diagnostics from the last mesh import. |
| Performance | Window menu | Frame time, shader length, GPU memory use and per-node cache cost. |
| Channels | Project menu | Enabled channels, default values and height scale. See [Channels](channels.md). |
| Preferences | Edit menu | Memory budgets, undo steps and keyboard shortcuts. |

## Menus

| Menu | Items |
|---|---|
| File | New Project, Open Project, Save, Save As, Reimport Mesh, Export Textures |
| Edit | Preferences, Undo, Redo |
| Viewport | Exposure, Environment rotation, Load environment (HDR/EXR), Studio lighting |
| Project | Channels |
| Window | Performance, Node Editor, Mesh Maps, Import Log |

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
