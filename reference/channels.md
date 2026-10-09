# Channels

A channel is one material property, evaluated for every texel of every tile. All layers are
multi-channel: a single layer can contribute to any subset of the enabled channels.

## Channel list

| Channel | Components | Default value | Enabled by default | Color space |
|---|---|---|---|---|
| Base Color | RGB | `0.5, 0.5, 0.5` | Yes | sRGB (stored linear) |
| Roughness | 1 | `0.5` | Yes | Linear |
| Metallic | 1 | `0.0` | Yes | Linear |
| Height | 1 | `0.0` | Yes | Linear, signed |
| Normal | RGB | `0.5, 0.5, 1.0` | Yes | Tangent space, OpenGL (+Y) |
| Emissive | RGB | `0, 0, 0` | No | sRGB (stored linear) |
| Opacity | 1 | `1.0` | No | Linear |

- All channels are stored and computed as 16-bit float.
- Height is in **world units** multiplied by the project's height scale (default `0.01`).
- The Normal channel holds *detail* normals only. Height is converted to normals and combined with
  the Normal channel for display and export.

## Channel settings

Per project, under **Project → Channels**:

| Setting | Description |
|---|---|
| Enabled | Disabled channels are not evaluated, displayed or exported. |
| Default value | Value of the channel where no layer contributes. |
| Height scale | Height channel only. World units per height unit. |

## Viewport channel display

| Mode | Shortcut | Shows |
|---|---|---|
| Material | `M` | Full PBR shading. |
| Single channel | `C` (cycles) | The channel's raw value, without lighting. |
| Mesh map | `Shift+C` (cycles) | The selected baked mesh map. |
