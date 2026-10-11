# Viewport lighting

How the viewport lights the material. Lighting is display only: it never changes painted or
exported textures. Why it works this way is in
[About viewport lighting](../explanation/about-viewport-lighting.md).

## Settings

All in the **Viewport** menu, and all saved with the project.

| Setting | Range | Default | Description |
|---|---|---|---|
| View transform, Exposure (EV) | | AgX, 0 | See [Color management](color-management.md). |
| Environment rotation | −3.15 to +3.15 radians | 0 | Turns the environment around the vertical axis. Also `Shift` + right-drag in the viewport. |
| Load environment (HDR/EXR)… | | — | Lights the viewport with an equirectangular `.hdr` or `.exr` image. Double-clicking an environment in the [Library](windows-and-panels.md#library) does the same. |
| Studio lighting | | used by new projects | Returns to the built-in studio lighting. Disabled while it is already in use. |
| Ambient occlusion | on, off | on | Darkens environment lighting with the baked Ambient Occlusion map. See [Ambient occlusion](#ambient-occlusion). |

## Environments

**Studio lighting** is a sky gradient over a dark ground with a soft box above the front, plus a
key and a fill light.

An **HDRI** is read at most 2048 texels wide; larger images are downsampled with a box filter,
keeping values above 1. When it is loaded, Crate precomputes from it:

- the diffuse lighting: the environment convolved with a cosine lobe, 32 × 16 texels;
- the reflections: 7 levels prefiltered with the GGX lobe for roughness 0, 1/6, 2/6 … 1. A
  roughness between two levels blends them.

Roughness is perceptual roughness, as in glTF, Blender and Substance 3D Painter: the GGX width is
roughness².

## Ambient occlusion

**Viewport → Ambient occlusion**, on by default. While it is on, the baked
[Ambient Occlusion](mesh-maps.md#maps) map (AO) darkens the environment lighting:

| Term | Multiplied by |
|---|---|
| Diffuse environment light | AO |
| Environment reflections | Specular occlusion: `clamp((n·v + AO)^exp2(−16 · roughness² − 1) − 1 + AO, 0, 1)`, where `n·v` is the cosine between the surface normal and the view direction |

Specular occlusion is weaker than AO on smooth surfaces seen head-on and approaches AO on rough
ones and at grazing angles.

- The studio lighting's key and fill lights are not occluded.
- While Ambient Occlusion is not baked (**Missing** in the Mesh Maps window), AO is 1 and the
  setting has no effect.
- Exported textures don't change. To bake occlusion into a texture, use the
  [Dirt](nodes.md) generator, or export the map itself with the `ao` source
  (see [Export](export.md)).

Script: `ambient-occlusion on|off` (see [Script commands](script-commands.md#viewport-and-rendering)).
Project file: `viewport.ambient_occlusion` (see [Project format](project-format.md)).
