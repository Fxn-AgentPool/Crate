# Script commands

Scripts drive Crate without a window: `crate run`, `crate batch`, `crate new --script`,
or `crate --script` for the application. Each command mirrors a user action.

## Syntax

- One command per line. Arguments are separated by spaces; quote arguments that contain spaces:
  `rename "Bare metal"`.
- Lines starting with `#` are comments. Empty lines are ignored.
- With `crate run -c`, commands are separated by `;` instead of lines.
- A failing command stops the script with an error naming the file, line and command.

### Variables

`${name}` is replaced before a line runs, by:

1. a `--set name=value` option, then
2. the environment variable `name`.

An unset variable is an error. In `crate batch`, `${project}` is the current project's name.

```
add-fill
rename "Paint ${project}"
set base_color=${color}
```

### Values

| Kind | Format | Example |
|---|---|---|
| Color | sRGB hex, or linear `r,g,b` | `#2A4C7D`, `0.2,0.3,0.5` |
| Number | decimal | `0.4` |
| Boolean | `true`, `false` | `visible=false` |
| Option | name, case-insensitive | `projection=triplanar` |
| Screen point | viewport fractions `x,y`, optionally `,pressure` | `0.3,0.82,0.5` |

### Library items

Commands that take a file also take an item of the [user library](windows-and-panels.md#library).
An argument without a file extension names a library item:

| Form | Looks in | Example |
|---|---|---|
| `<name>` | The command's category folder. | `stencil-add rust_streaks` |
| `<folder>/<name>` | Another folder of the library. | `stencil-add grunge/scratches` |

The name is the file name without its extension. If several files share a name, the first by
full file name is used. An argument with an extension is a file path, relative to the script.

## Projects and files

| Command | Description |
|---|---|
| `new <mesh> [resolution]` | New project from a mesh. Resolution `1024`, `2048` (default) or `4096`. |
| `open <project>` | Open a `.crate` project. |
| `save [project]` | Save. Without a path, saves to where the project was opened or last saved. |
| `reimport <mesh>` | Replace the mesh, keeping layers and paint. Mesh maps become out of date. |
| `bake [maps]` | Bake mesh maps: `all` (AO, curvature, thickness, ID, and Normal and Height when a high poly is set), `outdated`, or names: `ao`, `curvature`, `thickness`, `id`, `normal` (or `normal-map`), `height` (or `height-map`), `position`, `worldnormal`. |
| `high-poly <mesh>` | Set the high-poly mesh to bake from. `high-poly off` removes it. |
| `bake-settings key=value...` | Change [bake settings](mesh-maps.md#bake-settings): `resolution` (0 = tile resolution), `samples`, `max_distance`, `spread_angle`, `ignore_backfaces`, `curvature_radius`, `padding`, and for the high poly `auto_cage`, `frontal`, `rear`, `match_by_name`, `apply_normal` (`on`/`off` for `auto_cage`, `match_by_name` and `apply_normal`). |
| `save-map <map> <file.png> [udim]` | Save one tile of a mesh map (default: the first tile) as a 16-bit PNG, encoded as in [Mesh maps](mesh-maps.md#maps). Map names as for `bake`. |
| `export <dir> [key=value]...` | Export textures. Keys: `preset`, `format`, `normal`, `padding`, `resolution`, `tiles`, `template` (as in [Command line](command-line.md#export-options)). |
| `info` | Print the project summary. |

## Layers

| Command | Description |
|---|---|
| `add-fill [name]`, `add-paint [name]` | Add a layer above the selected one and select it. |
| `select <layer name>` | Select a layer (searches folders too). |
| `select-mask <index>` | Select a mask layer of the selected layer (0 = bottom). |
| `rename <name>` | Rename the selected layer. |
| `set key=value...` | Set properties of the selection (see "set" below). |
| `move-below <layer>`, `move-above <layer>` | Move the selected layer. |
| `group [layer...]` | Group the named layers (default: the selection) into a folder. |
| `delete` | Delete the selected layer. |
| `undo`, `redo` | Undo or redo. |
| `add-mask`, `add-black-mask` | Add a white or black mask to the selected layer. |
| `add-generator <type>` | Add a generator effect: `edge-wear`, `dirt`, `curvature-mask`, `position-gradient`. |
| `add-mask-paint`, `add-mask-fill`, `add-mask-levels` | Add a paint, fill or levels effect to the mask. |
| `save-smart [name]` | Save the selected layer as a smart material. |
| `add-smart <name or file.smat>` | Add a smart material from the library (`smart-materials`) or a file. |
| `delete-smart <name>` | Delete a smart material from the library. |

### `set`

On a layer:

| Key | Description |
|---|---|
| `opacity` | Layer opacity, 0–1. |
| `<channel>.enabled`, `<channel>.mode`, `<channel>.opacity` | Per-channel settings, e.g. `roughness.mode=Multiply`. Channel names without spaces: `basecolor`, `roughness`, … |
| any other key | A parameter of the layer's content node, e.g. a Fill layer's `base_color`, `roughness`, `projection`. See [Nodes](nodes.md). |

On a mask layer: `opacity`, `mode`, or a parameter of its node (e.g. a generator's `amount`).

## Node graph

| Command | Description |
|---|---|
| `node <var> <type>` | Add a node of a [type](nodes.md) and name it `<var>` for later commands. |
| `link <var>.<output> <var>.<input>` | Connect two nodes. |
| `node-set <var> key=value...` | Set node parameters. |
| `cache <var> on\|off` | Turn caching on or off. |
| `node-collapse <var> on\|off` | Collapse or expand the node in the Node Editor. |
| `layer-content <var>` | Make a node the selected layer's content. |
| `layer-mask <var>` | Use a Float node as the selected layer's mask. |
| `ungroup` | Replace the selected generator with its component nodes. |
| `caches` | Bring caches up to date and list them. |

## Painting

| Command | Description |
|---|---|
| `tool paint\|erase\|paint-through` | Select the tool. |
| `mode live\|manual` | Bake mode. |
| `on-camera-move bake\|clear\|lock` | Manual mode camera policy. |
| `brush key=value...` | `size`, `hardness`, `flow`, `opacity`, `spacing`, `angle`, `space=screen\|world`, `jitter_size`, `jitter_angle`, `jitter_position`, `jitter_flow`, `alpha=<library item or image>` (library folder `alphas`; `alpha=round` for the round tip). |
| `brush-values channel=value\|off...` | Values the brush paints, e.g. `base_color=#3B2F22 metallic=off`; `mask=0.0` for masks. |
| `stroke <point> <point>...` | Paint a stroke through screen points. |
| `bake-buffer`, `clear-buffer` | Manual mode: bake or clear the buffer. |
| `buffer-transform <dx> <dy> <degrees> <scale>` | Transform the buffer (screen pixels). |
| `stencil-add <library item or image>` | Load a stencil (library folder `stencils`) and make it active. |
| `stencil key=value...` | `opacity`, `visible`, `use_alpha`, `tile`, `flip`, `targets=base_color,roughness`. |
| `stencil-transform <dx> <dy> <degrees> <scale>` | Place the stencil (offset in viewport fractions). |

## Viewport and rendering

| Command | Description |
|---|---|
| `size <W>x<H>` | Size of renders and of the screen strokes are painted on (default `960x720`). |
| `camera <yaw> <pitch> [zoom]` | Degrees around and above the mesh; `zoom` > 1 moves closer. |
| `view material\|<channel>\|<map>` | What renders show, e.g. `view roughness`, `view curvature`. `normal` and `height` are channels; the mesh maps are `normal-map` and `height-map`. |
| `tiles on\|off` | Tint by UDIM tile. |
| `environment <library item or file.hdr\|exr>`, `exposure <ev>` | Lighting. Library folder `environments`. |
| `view-transform <name>` | `standard`, `agx`, `aces-1.3`, `aces-2.0`, `pbr-neutral` or `reinhard`. See [Color management](color-management.md). Default `agx`. |
| `preview <var>[.<output>]` | Render a node's output instead of the material, as the Node Editor's [preview](node-editor.md#preview) does. Without `.<output>`, its first output. |
| `preview off` | Render the material again. |
| `render <file.png>` | Render the viewport to an image. |

## Checks

For tests; each fails the script when its condition does not hold.

| Command | Condition |
|---|---|
| `expect-layers <name>...` | The top-level layers, top to bottom, are exactly these. |
| `expect-files <dir> <name>...` | The directory contains exactly these files. |
| `expect-same <a.png> <b.png> [tolerance]` | The images differ by at most `tolerance` per channel (default 2). |
| `expect-different <a.png> <b.png> [minimum]` | The images differ by at least `minimum` somewhere (default 20). |
| `expect-color <file.png> <r,g,b> [tolerance]` | The image's center pixel has these 8-bit values, within `tolerance` per channel (default 2). |
| `expect-unbaked true\|false` | Whether the paint buffer holds unbaked paint. |
| `expect-misses <max percent>` | The last high-poly bake missed at most this share of texels. |
| `expect-library <folder> <name>...` | The Library lists exactly these items, in this order, for that library folder (e.g. `environments`). |
