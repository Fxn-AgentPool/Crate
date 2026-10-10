# Color management

The viewport renders in scene-linear light and converts the result for the screen with a
**view transform**. The choice changes only how the viewport looks: painted values, exports and
baked maps are not affected.

| Stage | Value |
|---|---|
| Working space | Linear Rec.709 (sRGB primaries, D65 white). |
| Display | sRGB, standard dynamic range. |
| Exposure | Multiplies the scene by 2^EV before the view transform. |
| View transform | Maps scene values to display values (see below). |

## View transform

**Viewport → View transform**, a submenu with one item per transform; the current one is
marked. The Viewport menu stays open while you change its settings, so transforms can be compared
directly; click outside it to close it. Saved with the project. New projects, and projects
saved before view transforms existed, use **AgX**.

| View transform | Script name | Description |
|---|---|---|
| Standard | `standard` | No tone mapping: values are clipped to 0–1 and sRGB-encoded. Bright areas clip to white. |
| AgX | `agx` | Filmic curve that desaturates very bright colors toward white. Crate's own model, fitted to Blender's AgX view (see [About view transforms](../explanation/about-view-transforms.md)). |
| ACES 1.3 | `aces-1.3` | The ACES 1.3 SDR output transform for sRGB displays ("ACES 1.0 - SDR Video"). |
| ACES 2.0 | `aces-2.0` | The ACES 2.0 SDR output transform, 100 nits, Rec.709 primaries. |
| Khronos PBR Neutral | `pbr-neutral` | Keeps base colors close to their values up to about 0.76 and compresses only highlights. |
| Reinhard | `reinhard` | Per channel: `c / (1 + c)`, then sRGB-encoded. |

The view transform applies to the **Material** display, including node previews. Channel and
mesh map displays show raw values (clipped to 0–1 and sRGB-encoded) whatever the view transform.
The viewport background is not transformed.

Accuracy against Blender 5.2's views of the same scene values, in 8-bit display levels:

| View transform | Mean | 99 % of colors within | Worst |
|---|---|---|---|
| Standard | 0 | 0 | 0 |
| AgX | 0.5 | 5.7 | 40, in very saturated colors, which come out slightly less saturated |
| ACES 1.3 | 0.06 | 0.3 | 10, in a narrow band of saturated cyan-greens |
| ACES 2.0 | 0.05 | 0.3 | 2.2 |
| Khronos PBR Neutral | 0.3 | 3.5 | 6.9 (Blender approximates it with a lookup table) |

## Exposure

**Viewport → Exposure (EV)**: −5 to +5, default 0. Saved with the project.

## Limits

- One display: sRGB, SDR. HDR displays and other display spaces aren't supported.
- The working space is fixed. OpenColorIO configs can't be loaded.
- Library thumbnails are tone-mapped with a fixed filmic curve, not the view transform.
