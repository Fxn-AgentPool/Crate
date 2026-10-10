# Blend modes

Blend modes are set per layer per channel. `a` is the value below, `b` the layer's value. The
result is then interpolated with `a` by the layer's mask and opacity:
`out = lerp(a, blend(a, b), mask * opacity)`.

## Color and scalar channels

Base Color, Roughness, Metallic, Emissive, Opacity.

| Mode | Formula |
|---|---|
| Normal | `b` |
| Multiply | `a * b` |
| Screen | `1 - (1 - a)(1 - b)` |
| Overlay | `a < 0.5 ? 2ab : 1 - 2(1 - a)(1 - b)` |
| Add | `a + b` |
| Subtract | `a - b` |
| Lighten | `max(a, b)` |
| Darken | `min(a, b)` |
| Color Dodge | `a / (1 - b)` |
| Color Burn | `1 - (1 - a) / b` |

Results are clamped to `[0, 1]` except for Emissive, which is clamped to `[0, ∞)`.
Color blends operate on linear values.

## Height channel

| Mode | Formula |
|---|---|
| Normal | `b` |
| Add | `a + b` |
| Subtract | `a - b` |
| Max | `max(a, b)` |
| Min | `min(a, b)` |
| Multiply | `a * b` |

Height is not clamped.

## Normal channel

| Mode | Description |
|---|---|
| Normal | Replaces the normal: `normalize(lerp(a, b, t))`. |
| Combine | Reoriented normal mapping (RNM) of `b` onto `a`. Default for new layers. |

## Mask layers

Mask layers support Normal, Multiply, Screen, Add, Subtract, Lighten, Darken and Overlay, using
the scalar formulas above.
