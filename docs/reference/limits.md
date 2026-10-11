# Limits and requirements

## System requirements

| | Minimum | Recommended |
|---|---|---|
| OS | Windows 10 64-bit, Linux (x86-64, glibc 2.31+) | Windows 11, recent Linux |
| GPU API | Vulkan 1.2 or Direct3D 12, with read-write storage support for 16-bit float textures | — |
| GPU memory | 4 GB | 8 GB or more |
| System memory | 8 GB | 32 GB |
| Input | Mouse | Pen tablet with pressure (Windows Ink, Wacom WinTab, libinput) |

macOS is not supported in the MVP. On Linux, Crate runs under X11 or Wayland; native file
dialogs need an XDG desktop portal. If the GPU lacks a required feature, Crate shows which one
at startup instead of opening a project.

User files (preferences, export presets, the recent project list, the user library) live in `%APPDATA%\Crate` on
Windows and `$XDG_CONFIG_HOME/crate` (usually `~/.config/crate`) on Linux. The user library is
the `library` folder there, with one folder per Library category (see
[Windows and panels](windows-and-panels.md#library)). It has no size limit; large folders take
longer to scan and thumbnail.

## Project limits

| Item | Limit |
|---|---|
| UDIM tiles | 100 (`1001` – `1100`) |
| Tile resolution | 1024, 2048 or 4096; the same for all tiles |
| Texture sets | 1 per project |
| Triangles | 10 million |
| Channels | The 7 built-in channels. Custom channels are not supported. |
| Stencils | 8 per project |
| Layers | No fixed limit. Bounded by memory. |
| Undo history | 64 steps or 2 GB of page data, whichever comes first (configurable) |
| Paint layers | Each painted layer holds its full tiles on the GPU: 40 bytes per texel (about 168 MB per 2048² tile). |

## Memory budgets

**Edit → Preferences → Performance**:

| Setting | Default | Description |
|---|---|---|
| GPU cache budget | 8 GB | Space for paint and cached nodes on the GPU. |
| Undo budget | 2 GB | System memory for undo pages. Older steps are discarded first. |
| Undo steps | 64 | Maximum number of undo steps. |

The **Performance** window shows GPU memory used by paint and caches against the budget, and
warns when it is exceeded. The MVP does not evict anything: paint and cached node results stay
resident.

## Supported image formats

| Use | Formats |
|---|---|
| Import (images, stencils, alphas, library images) | PNG, JPEG, TGA, TIFF, EXR |
| Export | PNG (8/16-bit), TGA (8-bit), EXR (16f/32f) |
| Environment maps | HDR (Radiance), EXR, equirectangular |
