# Export

**File → Export Textures** (`Ctrl+E`) evaluates the root stack in UV space for every tile and
writes image files.

## Export settings

| Setting | Default | Description |
|---|---|---|
| Output directory | `<project dir>/../export` | Destination. Created if missing. |
| Preset | glTF Metal/Rough | See "Presets". |
| File name template | `{mesh}_{map}.{udim}.{ext}` | See "Template tokens". |
| Format | per preset | PNG, TGA, EXR. |
| Bit depth | per preset | PNG: 8, 16. TGA: 8. EXR: 16f, 32f. |
| Resolution | tile resolution | Downscaling only. |
| Padding | Infinite | `None`, `N texels`, or `Infinite` (fill all empty space by dilation). |
| Normal format | OpenGL | OpenGL (+Y) or DirectX (−Y). |
| Tiles | all | Subset of tiles to export. |

## Template tokens

| Token | Value |
|---|---|
| `{mesh}` | Mesh file name without extension. |
| `{project}` | Project name. |
| `{map}` | Output map name from the preset. |
| `{udim}` | UDIM number, e.g. `1001`. |
| `{ext}` | File extension for the format. |

## Presets

Each preset defines output maps. Each output map assigns sources to its R, G, B and A components.

### glTF Metal/Rough

| Map | R | G | B | A | Format |
|---|---|---|---|---|---|
| `BaseColor` | Base Color R | Base Color G | Base Color B | Opacity | PNG 8 |
| `ORM` | AO (mesh map) | Roughness | Metallic | — | PNG 8 |
| `Normal` | Normal* X | Normal* Y | Normal* Z | — | PNG 8 |
| `Emissive` | Emissive R | Emissive G | Emissive B | — | PNG 8 |

### Unreal Engine

| Map | R | G | B | A | Format |
|---|---|---|---|---|---|
| `BaseColor` | Base Color R | Base Color G | Base Color B | Opacity | PNG 8 |
| `ORM` | AO (mesh map) | Roughness | Metallic | — | PNG 8 |
| `Normal` | Normal* X | Normal* Y | Normal* Z | — | PNG 8, DirectX |
| `Emissive` | Emissive R | Emissive G | Emissive B | — | PNG 8 |

### Unity HDRP

| Map | R | G | B | A | Format |
|---|---|---|---|---|---|
| `BaseColor` | Base Color R | Base Color G | Base Color B | Opacity | PNG 8 |
| `MaskMap` | Metallic | AO (mesh map) | — (1.0) | 1 − Roughness | PNG 8 |
| `Normal` | Normal* X | Normal* Y | Normal* Z | — | PNG 8, OpenGL |
| `Emissive` | Emissive R | Emissive G | Emissive B | — | PNG 8 |

### Raw Channels

One EXR 16f file per enabled channel, unpacked. Height is exported as a separate map, not
converted to normals.

\* **Normal\*** is the combined normal: Height converted with Height to Normal, combined with the
Normal channel (RNM).

## Custom presets

**New…** next to the preset in the Export dialog creates a preset from the selected one.
Presets are stored as JSON in the `export-presets` folder of the user configuration directory
(`%APPDATA%\Crate` on Windows, `~/.config/crate` on Linux) and can be shared as files.

Valid component sources, as written in the preset editor:

| Source | Meaning |
|---|---|
| `base_color.r`, `.g`, `.b` | A channel component (also `emissive`, `normal.x/y/z` for the raw Normal channel). |
| `roughness`, `metallic`, `height`, `opacity` | A scalar channel. |
| `normal.x`, `normal.y`, `normal.z` | The combined normal (Normal*). |
| `ao` | The Ambient Occlusion mesh map. |
| `1-<source>` | One minus a source. |
| `0`, `1` | Constants. |

An output map whose channel sources are all disabled channels is skipped. An output map that
mixes enabled and disabled channels exports the disabled ones as their default value. A warning
is shown when a disabled channel feeds a color (R, G or B) component; a disabled channel feeding
only the alpha component, such as Opacity in `BaseColor`, exports as its default silently.
