# Windows and panels

Side panels can be resized by dragging their edges. Windows opened from the menus float over
the viewport.

| Panel | Default location | Contents |
|---|---|---|
| Viewport | center | 3D view of the mesh, the [UV view](uv-view.md), or both side by side. Paint buffer and stencils are drawn over it. See [Viewport](#viewport). |
| Viewport toolbar | above the viewport | The brush's **Size**, **Flow** and **Hardness**, and **Bake Mesh Maps**. See [Viewport](#viewport). |
| Layers | right | The root stack, or the selected group. The channel menu in its header chooses the channel whose blend mode and opacity the rows show. See [Layers and masks](layers.md). |
| Properties | right, below Layers | Parameters of the selected layer, mask layer or node. Its header shows the selected layer's or mask layer's name. |
| Node Editor | bottom (hidden) | The graph of the selected layer, or of the whole project. `Ctrl+Space` toggles it. See [Node Editor](node-editor.md). |
| Tools | left edge | A column of icons: Paint, Erase and Paint Through. The current tool is highlighted; hover shows its name and shortcut. |
| Brush | left | Tool and brush properties. See [Brushes, tools and stencils](brushes-tools-stencils.md). |
| Brush Values | left, below Brush | Per-channel values painted by the brush, laid out like a Fill layer's **Values**: a check that turns the channel on, its name, then its value. |
| Paint Buffer | left | Bake mode, buffer and projection settings. See [Paint buffer](paint-buffer.md). |
| Stencils | left | Loaded stencils, the active stencil and its properties. |
| Library | left, bottom | Project resources and the user library, by category. See [Library](#library). |
| Mesh Maps | **Bake Mesh Maps** in the viewport toolbar, or the Window menu | Mesh map status and bake settings, with the [bake preview](mesh-maps.md#bake-preview) on the right. See [Mesh maps](mesh-maps.md). |
| Import Log | Window menu | Diagnostics from the last mesh import. |
| Performance | Window menu | Frame time, shader length, GPU memory use and per-node cache cost. |
| Channels | Project menu | Enabled channels, default values and height scale. See [Channels](channels.md). |
| Preferences | Edit menu | Update checks, memory budgets, undo steps and keyboard shortcuts. |
| Updates | Help menu | Installed and latest version, release notes, download and install. See [Updates](updates.md). |

## Controls

The interface follows the rules in [About the interface](../explanation/about-the-interface.md).

| Control | Looks like | Use |
|---|---|---|
| Field | A box with the label on the left and the value on the right, in Geist Mono. A blue fill shows where the value lies in its range; fields without a fixed range, like **Size**, have no fill. | Drag anywhere in it to change the value, click to type one. See below. |
| Toggle | A small switch, blue when on. | Settings that take effect at once, such as **Show buffer**. Click to switch. |
| Checkbox | A small square, blue with a check when on. | Choosing items of a set, such as the channels a brush paints. |
| Segmented control | A row of options in one box; the chosen one is lighter. | One choice of a few, such as **Bake mode** or **Resolution scale**. |
| Section header | Small capitals with a ▸ or ▾, below a line that runs the full width of the panel. | Click to fold or unfold the section. Some show a summary or an action on the right. |
| Keycap | A shortcut in Geist Mono on a dark chip, like `Ctrl B`. | Shows a keyboard shortcut. |
| Tag | A small colored label in Geist Mono: green **cached**, amber **dirty**. | Shows a state. |
| Color swatch | A box filled with the color. | Click to open the color picker. See below. |

A disabled control is drawn at 45% opacity as a whole.

### Fields

| Input | Effect |
|---|---|
| Drag left or right | Changes the value. For a field with a fixed range, dragging across the field's full width covers the whole range; other fields change by a fixed step per point. |
| `Ctrl` + drag | Changes the value 10 times more slowly. |
| Click | Types a value, with the current one selected. `Enter` or clicking elsewhere applies it, `Esc` cancels. Values outside the range are clamped to it; text that isn't a number leaves the value unchanged. |
| Hover | Shows the field's tooltip, if it has one. |

A click only types once the pointer is released without dragging, so a drag never starts typing. A field's value is clamped to its range
while dragging.

### Color picker

Clicking a color swatch opens the picker below it; clicking outside the picker or pressing `Esc`
closes it. Every change applies at once; on a node or layer parameter, each drag or typed value
is one undo step.

| Part | Shows | Input |
|---|---|---|
| Square | Saturation (left to right) and value (bottom to top) at the current hue. A ring marks the color. | Click or drag to set saturation and value. |
| Hue strip | The hues, below the square. A line marks the current hue. | Click or drag to set the hue. |
| **R**, **G**, **B** | Fields, 0–1, three decimals. | As any [field](#fields). |
| **Hex** | `#RRGGBB`, uppercase. | Click to type or paste; `Enter` or clicking elsewhere applies it, `Esc` cancels. Accepts six hex digits, with or without `#`, in either case, and surrounding spaces; other text leaves the color unchanged. Select it and copy (`Ctrl+C`) to take the color to other software. |

All of them are in the display's sRGB encoding, the numbers other painting software shows: a
mid-gray swatch is `0.5, 0.5, 0.5` and `#808080`, and a hex code copied from another application
gives the same color here. Crate stores colors scene-linear (see
[Color management](color-management.md#picked-colors)).

The picker can't show values above 1: such a color shows as 1 in its brightest channels until it
is edited, and editing it clamps it to 0–1. A gray keeps the hue it was picked with while the
picker stays open.

## Library

The Library shows one category at a time. A row of icon tabs at its top chooses it (hover a tab
for the category's name): first the **Project** categories, which show resources of the open
project, then, after a divider, the **Library** categories, which show the user library. The
chosen tab is underlined. When the categories don't fit the panel's width, the tabs wrap to
more rows.

Below the tabs:

| Part | Contents |
|---|---|
| Search | Shows only the items whose name contains the typed text, ignoring case. The field shows the **Search the library** shortcut (`Ctrl+F`) while empty, and a clear button once something is typed. `Esc` or the clear button empties it. The search is emptied when another category is chosen. |
| Preview size | The slider right of the search field: how large tiles are, 40 to 160 points wide (default 58). Click or drag to set it, or hold `Ctrl` and turn the mouse wheel (or pinch) over the Library; double-click resets it. It applies to every category and is remembered between sessions. |
| Info row | The category's name and whether it is a Project or Library category. On the right: the number of items, or *shown* / *all* while searching. Library categories also have **Refresh** and **Show in file browser** buttons there. |
| Items | Tiles of the preview size: the thumbnail with the name below, in as many columns as fit, from the left. A tile is narrower only when the panel can't hold one. Environments span two columns. Hovering a tile outlines it. Reserved categories list names instead. |

An empty category, and a search that matches nothing, show a note in a dashed box.

The user library is the `library` folder in the user directory (see
[Limits and requirements](limits.md)). Each Library category is a subfolder of it. Crate creates
the built-in folders when its window opens, if they are missing; files can be added to them by
hand. Any other subfolder of `library` appears as an extra category after the built-in ones,
named after the folder, in alphabetical order. Folders and files whose names start with `.` are
ignored, and so are files of other types than the category lists.

**Refresh** rescans the library folders; **Show in file browser** opens the category's folder. Folders are scanned when a category is first shown and on **Refresh**; new extra folders
appear after **Refresh**. Thumbnails are made in the background and appear as they are ready.

| Category | Row | Folder | Files listed |
|---|---|---|---|
| Mesh Maps | Project | — | The [mesh maps](mesh-maps.md), with a thumbnail of the first UDIM tile. Unbaked maps show **not baked**; out-of-date maps have an amber name. |
| Images | Project | — | Imported images (including stencils and brush alphas), with thumbnails, sorted by name. Hover shows the size and the source file. |
| Smart Materials | Library | `smart-materials` | `.smat` files, with thumbnails. Created with **Save as smart material**. |
| Materials | Library | `materials` | Reserved: every file, by name, with no actions yet. |
| Textures | Library | `textures` | First the procedural presets (**Perlin Noise**, **Simplex Noise**, **Value Noise**, **Cellular F1**, **Cellular F2**, **Cellular F2-F1**, **Gradient**: a Noise, Cellular or Gradient node with that type or output, other settings at their defaults), then images, with thumbnails. Preset thumbnails show the node on a flat square at its default scale. |
| Smart Masks | Library | `smart-masks` | Reserved: every file, by name, with no actions yet. |
| Brushes | Library | `brushes` | Reserved: every file, by name, with no actions yet. |
| Alphas | Library | `alphas` | Images, with thumbnails. Color images are used by their luminance. |
| Stencils | Library | `stencils` | Images, with thumbnails. |
| Environments | Library | `environments` | `.hdr` and `.exr` files, with a wide preview of the whole panorama. |
| *other folder* | Library | its name | Images, with thumbnails. |

Images are PNG, JPEG, TGA, TIFF and EXR files. Items are sorted by name. Subfolders inside a
category folder are not scanned. A file that can't be read shows **unreadable** instead of a
thumbnail; using it reports the error in the status bar.

Thumbnails and previews keep the image's aspect ratio: images fit a square tile, environments
a tile twice as wide as it is high. Smart materials without a saved preview show **no preview**. HDR and EXR files are tone-mapped with a fixed filmic curve
(exposure 0), whatever the [view transform](color-management.md#view-transform); other images
are shown as they are. Thumbnails of the project's
images are made from their source files, in the background, when **Images** is first shown and
when an image is added.

| Item | Double-click | Drag | Right-click |
|---|---|---|---|
| Mesh map | Adds it as a mask effect (a Mesh Map node) on the selected layer. | Onto the Node Editor: adds a Mesh Map node. | **Add to mask of selected layer**, **Show in viewport**. |
| Smart material | Adds it above the selected layer. | Onto the Layers panel: adds it there. | **Add to layers**, **Delete**. |
| Procedural preset | — | Onto the Node Editor: adds its node. | — |
| Texture | Imports it into the project's images (it then appears in **Images**). | Onto the Node Editor: imports it and adds an Image node using it. | **Import into project**, **Delete**. |
| Alpha | Sets it as the brush alpha. | — | **Use as brush alpha**, **Delete**. |
| Stencil, or image in another folder | Loads it as a stencil. | — | **Load as stencil**, **Delete**. |
| Environment | Loads it as the viewport environment. | — | **Load environment**, **Delete**. |
| File in a reserved folder | — | — | **Delete**. |

**Delete** moves the file to the system's trash (the Recycle Bin on Windows), so it can be restored
from there. Project items (mesh maps, images) and the procedural presets can't be deleted.

The Library is shown only while a project is open.

## Viewport

| Part | Where | Contents |
|---|---|---|
| Toolbar | Strip above the viewport | **Size** (in pixels), **Flow** (in %) and **Hardness** fields: the same values as in the **Brush** section. On the right, the **3D · Split · UV** layout control (see [UV view](uv-view.md#layouts)), then **Bake Mesh Maps** opens or closes the [Mesh Maps](mesh-maps.md) window; it is highlighted while the window is open, and an amber dot shows while a map that can be baked is missing or out of date. The bake mode is in the [Paint Buffer](paint-buffer.md) section and the status bar; `Ctrl+Shift+B` switches it. |
| Display | Top left, over the view | **Material** and one button per enabled channel. Choosing one shows it in the viewport, like the display shortcuts. While a mesh map is shown, none is highlighted. Hidden while the viewport is too narrow for it. |

## Menus

The menu bar starts with the Crate mark. The open project's name is shown in its middle, with an
amber dot while the project has unsaved changes.


| Menu | Items |
|---|---|
| File | New Project, Open Project, Open Recent, Save, Save As, Reimport Mesh, Export Textures |
| Edit | Preferences, Undo, Redo |
| Viewport | View transform, Exposure, Environment rotation, Load environment (HDR/EXR), Studio lighting, Ambient occlusion. See [Viewport lighting](viewport-lighting.md) and [Color management](color-management.md). |
| Project | Channels |
| Window | Performance, Node Editor, Mesh Maps, Import Log |
| Help | Check for Updates… (**Update Available…** when a newer release exists) |

### Open Recent

**File → Open Recent** lists the projects most recently opened or saved in the application
window, newest first.

| Property | Value |
|---|---|
| Entries | Up to 10. Opening or saving a project moves it to the top; the oldest entry is dropped. |
| Recorded by | **Open Project**, **Open Recent**, a project given on the command line (`crate [PATH]`), **Save** and **Save As**. Scripts, including `crate --script`, and headless commands (`crate run`, `crate batch`, `crate new`, …) don't record projects. |
| Label | The project's folder name without `.crate`. When two entries have the same name, the name of the folder containing each is added in parentheses. Hover shows the full path. |
| Missing projects | Shown greyed out, with **Not found** on hover, and can't be chosen. They stay in the list until they drop off the end or the list is cleared. |
| Clear Recent | Last item. Empties the list. |
| Empty list | The submenu shows **No recent projects**. |
| Stored in | `recent.json` in the user directory (see [Limits and requirements](limits.md)). |

Choosing an entry opens the project. If the open project has unsaved changes, Crate first asks
whether to save them, as **Open Project** does. If the project can't be opened, the error is
shown in the status bar and the entry stays in the list.

## Status bar

| Field | Values |
|---|---|
| Mesh | File name, triangle count (**12,317 tris**), UDIM tiles (**UDIM 1001, 1002**) and tile resolution (**2048²**) |
| Bake mode | **Live**, **Manual** |
| Buffer | **Unbaked** when the buffer holds paint (Manual mode) |
| Target | Name of the layer or mask that painting writes to, or **No paint target** |
| Display | **Material view**, or the channel or mesh map shown followed by **view** |
| Background tasks | Mesh map bake progress |
| Messages | Undo/redo, errors and hints, briefly |

GPU memory use is shown in the **Performance** window.
