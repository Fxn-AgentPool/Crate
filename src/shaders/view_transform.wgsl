// View transforms: scene-linear Rec.709 -> sRGB-encoded display values.
// See docs/reference/color-management.md and docs/explanation/about-view-transforms.md.
// Prepended: agx_fit.wgsl. Mirrored by tools/view_transforms.py (which checks the math).

const VT_STANDARD: u32 = 0u;
const VT_AGX: u32 = 1u;
const VT_ACES13: u32 = 2u;
const VT_ACES20: u32 = 3u;
const VT_PBR_NEUTRAL: u32 = 4u;
const VT_REINHARD: u32 = 5u;

// Baked ACES output transforms (tools/view_transforms.py bake-aces).
@group(0) @binding(3) var lut_aces13: texture_3d<f32>;
@group(0) @binding(4) var lut_aces20: texture_3d<f32>;
const LUT_LO: f32 = -12.0;
const LUT_HI: f32 = 10.0;

fn srgb_encode(c: vec3f) -> vec3f {
    let x = clamp(c, vec3f(0.0), vec3f(1.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, x <= vec3f(0.0031308));
}

fn lut_at(which: u32, p: vec3i) -> vec3f {
    if which == VT_ACES13 { return textureLoad(lut_aces13, p, 0).rgb; }
    return textureLoad(lut_aces20, p, 0).rgb;
}

// Tetrahedral interpolation over a log2-shaped input, as the bake tool's `sample`.
fn lut_sample(which: u32, x: vec3f) -> vec3f {
    let n = f32(textureDimensions(lut_aces13).x);
    let shaped = clamp((log2(max(x, vec3f(1e-30)) / 0.18) - LUT_LO) / (LUT_HI - LUT_LO), vec3f(0.0), vec3f(1.0));
    let t = shaped * (n - 1.0);
    let i = vec3i(min(floor(t), vec3f(n - 2.0)));
    let f = t - vec3f(i);
    let c000 = lut_at(which, i);
    let c111 = lut_at(which, i + vec3i(1, 1, 1));
    if f.r >= f.g {
        if f.g >= f.b {
            return (1.0 - f.r) * c000 + (f.r - f.g) * lut_at(which, i + vec3i(1, 0, 0)) + (f.g - f.b) * lut_at(which, i + vec3i(1, 1, 0)) + f.b * c111;
        }
        if f.r >= f.b {
            return (1.0 - f.r) * c000 + (f.r - f.b) * lut_at(which, i + vec3i(1, 0, 0)) + (f.b - f.g) * lut_at(which, i + vec3i(1, 0, 1)) + f.g * c111;
        }
        return (1.0 - f.b) * c000 + (f.b - f.r) * lut_at(which, i + vec3i(0, 0, 1)) + (f.r - f.g) * lut_at(which, i + vec3i(1, 0, 1)) + f.g * c111;
    }
    if f.b > f.g {
        return (1.0 - f.b) * c000 + (f.b - f.g) * lut_at(which, i + vec3i(0, 0, 1)) + (f.g - f.r) * lut_at(which, i + vec3i(0, 1, 1)) + f.r * c111;
    }
    if f.b > f.r {
        return (1.0 - f.g) * c000 + (f.g - f.b) * lut_at(which, i + vec3i(0, 1, 0)) + (f.b - f.r) * lut_at(which, i + vec3i(0, 1, 1)) + f.r * c111;
    }
    return (1.0 - f.g) * c000 + (f.g - f.r) * lut_at(which, i + vec3i(0, 1, 0)) + (f.r - f.b) * lut_at(which, i + vec3i(1, 1, 0)) + f.b * c111;
}

fn agx_curve(t: f32) -> f32 {
    let i = min(u32(t), AGX_KNOTS - 1u);
    return mix(AGX_CURVE[i], AGX_CURVE[i + 1u], t - f32(i));
}

// Crate's AgX model, fitted to Blender's AgX view (agx_fit.wgsl).
fn view_agx(c: vec3f) -> vec3f {
    let x = AGX_INSET * c;
    let t = clamp((log2(max(x, vec3f(1e-10)) / 0.18) - AGX_LO) / (AGX_HI - AGX_LO), vec3f(0.0), vec3f(1.0)) * f32(AGX_KNOTS);
    let s = vec3f(agx_curve(t.r), agx_curve(t.g), agx_curve(t.b));
    // The curve's output is Rec.1886-encoded (gamma 2.4); re-encode for sRGB.
    return srgb_encode(pow(clamp(AGX_OUTSET * s, vec3f(0.0), vec3f(1.0)), vec3f(2.4)));
}

// Khronos PBR Neutral tone mapper, from the Khronos specification.
fn view_pbr_neutral(c_in: vec3f) -> vec3f {
    var c = c_in;
    let start = 0.8 - 0.04;
    let desat = 0.15;
    let lo = min(c.r, min(c.g, c.b));
    let offset = select(0.04, lo - 6.25 * lo * lo, lo < 0.08);
    c -= offset;
    let peak = max(c.r, max(c.g, c.b));
    if peak < start {
        return srgb_encode(c);
    }
    let d = 1.0 - start;
    let new_peak = 1.0 - d * d / (peak + d - start);
    c *= new_peak / peak;
    let g = 1.0 - 1.0 / (desat * (peak - new_peak) + 1.0);
    return srgb_encode(mix(c, vec3f(new_peak), g));
}

fn view_transform(c_in: vec3f, which: u32) -> vec3f {
    let c = max(c_in, vec3f(0.0));
    switch which {
        case VT_AGX: { return view_agx(c); }
        case VT_ACES13, VT_ACES20: { return lut_sample(which, c); }
        case VT_PBR_NEUTRAL: { return view_pbr_neutral(c); }
        case VT_REINHARD: { return srgb_encode(c / (1.0 + c)); }
        default: { return srgb_encode(c); }
    }
}
