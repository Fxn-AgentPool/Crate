// Runtime library for compiled material graphs. Requires the scene bindings (scene::WGSL)
// and the Surface / Material structs of the including shader.

// Multi-channel material value. Channel order: base color, roughness, metallic, height,
// normal (tangent space, -1..1), emissive, opacity. Scalars live in .x.
struct Mat {
    v: array<vec3f, 7>,
    a: array<f32, 7>,
};

const CH_BASE: u32 = 0u;
const CH_ROUGH: u32 = 1u;
const CH_METAL: u32 = 2u;
const CH_HEIGHT: u32 = 3u;
const CH_NORMAL: u32 = 4u;
const CH_EMISSIVE: u32 = 5u;
const CH_OPACITY: u32 = 6u;

// params[0..7]: channel defaults (normal stored encoded 0..1).
fn channel_default(c: u32) -> vec3f {
    let d = params[c].xyz;
    if c == CH_NORMAL { return normalize(d * 2.0 - 1.0); }
    return d;
}

fn mat_defaults() -> Mat {
    var m: Mat;
    for (var c = 0u; c < 7u; c++) {
        m.v[c] = channel_default(c);
        m.a[c] = 0.0;
    }
    return m;
}

// Node Editor preview: the value as unlit color.
fn mat_preview(c: vec3f) -> Mat {
    var m = mat_defaults();
    m.v[CH_BASE] = vec3f(0.0);
    m.v[CH_ROUGH] = vec3f(1.0);
    m.v[CH_METAL] = vec3f(0.0);
    m.v[CH_EMISSIVE] = c;
    return m;
}

fn mat_uncovered(m_in: Mat) -> Mat {
    var m = m_in;
    for (var c = 0u; c < 7u; c++) { m.a[c] = 0.0; }
    return m;
}

fn mat_to_material(m: Mat) -> Material {
    var o: Material;
    o.base_color = m.v[0];
    o.roughness = m.v[1].x;
    o.metallic = m.v[2].x;
    o.height = m.v[3].x;
    o.normal = normalize(select(m.v[4], vec3f(0.0, 0.0, 1.0), dot(m.v[4], m.v[4]) < 1e-8));
    o.emissive = m.v[5];
    o.opacity = m.v[6].x;
    return o;
}

// ------------------------------------------------------------------ blending
// Mode codes match graph::BlendMode.
const B_NORMAL: u32 = 0u;
const B_MULTIPLY: u32 = 1u;
const B_SCREEN: u32 = 2u;
const B_OVERLAY: u32 = 3u;
const B_ADD: u32 = 4u;
const B_SUBTRACT: u32 = 5u;
const B_LIGHTEN: u32 = 6u;
const B_DARKEN: u32 = 7u;
const B_DODGE: u32 = 8u;
const B_BURN: u32 = 9u;
const B_MAX: u32 = 10u;
const B_MIN: u32 = 11u;
const B_COMBINE: u32 = 12u;

fn blend3(a: vec3f, b: vec3f, mode: u32) -> vec3f {
    switch mode {
        case 1u: { return a * b; }
        case 2u: { return 1.0 - (1.0 - a) * (1.0 - b); }
        case 3u: { return select(1.0 - 2.0 * (1.0 - a) * (1.0 - b), 2.0 * a * b, a < vec3f(0.5)); }
        case 4u: { return a + b; }
        case 5u: { return a - b; }
        case 6u, 10u: { return max(a, b); }
        case 7u, 11u: { return min(a, b); }
        case 8u: { return a / max(1.0 - b, vec3f(1e-4)); }
        case 9u: { return 1.0 - (1.0 - a) / max(b, vec3f(1e-4)); }
        default: { return b; }
    }
}

fn blendf(a: f32, b: f32, mode: u32) -> f32 {
    return blend3(vec3f(a), vec3f(b), mode).x;
}

// Applies the baked high-poly Normal map under the material's normal
// (docs/reference/mesh-maps.md#normal-map).
fn apply_baked_normal(s: Surface, m: Material) -> Material {
    if scene.info.w == 0u || !map_baked(6u) || s.layer < 0 { return m; }
    var o = m;
    let base = normalize(mesh_normal_map(tile_tex(s.tile_uv), s.layer).xyz * 2.0 - 1.0);
    o.normal = rnm(base, m.normal);
    return o;
}

// Reoriented normal mapping of detail `d` onto base `b` (both tangent space, -1..1).
fn rnm(b: vec3f, d: vec3f) -> vec3f {
    let t = b + vec3f(0.0, 0.0, 1.0);
    let u = d * vec3f(-1.0, -1.0, 1.0);
    return normalize(t * dot(t, u) / max(t.z, 1e-4) - u);
}

fn blend_channel(c: u32, a: vec3f, b: vec3f, mode: u32) -> vec3f {
    if c == CH_NORMAL {
        if mode == B_COMBINE { return rnm(a, b); }
        return b;
    }
    var r = blend3(a, b, mode);
    if c == CH_HEIGHT { return r; }
    if c == CH_EMISSIVE { return max(r, vec3f(0.0)); }
    return clamp(r, vec3f(0.0), vec3f(1.0));
}

// Composites `lay` over `acc` for channel c with weight t.
fn composite(acc: ptr<function, Mat>, c: u32, value: vec3f, t: f32, mode: u32) {
    var r = blend_channel(c, (*acc).v[c], value, mode);
    if c == CH_NORMAL {
        (*acc).v[c] = normalize(mix((*acc).v[c], r, t) + vec3f(0.0, 0.0, 1e-6));
    } else {
        (*acc).v[c] = mix((*acc).v[c], r, t);
    }
    (*acc).a[c] = (*acc).a[c] + t * (1.0 - (*acc).a[c]);
}

// ------------------------------------------------------------------ paint
fn paint_layer(slot: u32, s: Surface) -> i32 {
    if s.layer < 0 { return -1; }
    return i32(slot * scene.info.y) + s.layer;
}

// Paint planes store premultiplied values so filtering at stroke edges stays correct.
fn paint_sample(slot: u32, s: Surface) -> Mat {
    var m = mat_defaults();
    let l = paint_layer(slot, s);
    if l < 0 { return m; }
    let tc = tile_tex(s.tile_uv);
    let p0 = textureSampleLevel(paint0, samp_clamp, tc, l, 0.0);
    let p1 = textureSampleLevel(paint1, samp_clamp, tc, l, 0.0);
    let p2 = textureSampleLevel(paint2, samp_clamp, tc, l, 0.0);
    let p3 = textureSampleLevel(paint3, samp_clamp, tc, l, 0.0);
    let p4 = textureSampleLevel(paint4, samp_clamp, tc, l, 0.0);
    let e = 1e-5;
    m.v[0] = p0.rgb / max(p0.a, e); m.a[0] = p0.a;
    m.v[5] = p1.rgb / max(p1.a, e); m.a[5] = p1.a;
    m.v[4] = select(vec3f(0.0, 0.0, 1.0), normalize(p2.rgb), dot(p2.rgb, p2.rgb) > 1e-10); m.a[4] = p2.a;
    m.v[1] = vec3f(p3.x / max(p4.x, e)); m.a[1] = p4.x;
    m.v[2] = vec3f(p3.y / max(p4.y, e)); m.a[2] = p4.y;
    m.v[3] = vec3f(p3.z / max(p4.z, e)); m.a[3] = p4.z;
    m.v[6] = vec3f(p3.w / max(p4.w, e)); m.a[6] = p4.w;
    return m;
}

// Cached Float/Color node results: rgb = color output, a = float output.
fn cache_sample(slot: u32, s: Surface) -> vec4f {
    let l = paint_layer(slot, s);
    if l < 0 { return vec4f(0.0); }
    return textureSampleLevel(cache, samp_clamp, tile_tex(s.tile_uv), l, 0.0);
}

// Single-channel paint for masks. The pool stores (premultiplied value in rgb, coverage);
// returns (premultiplied value, coverage).
fn paint_mask_sample(slot: u32, s: Surface) -> vec2f {
    let l = paint_layer(slot, s);
    if l < 0 { return vec2f(0.0); }
    return textureSampleLevel(mask_paint, samp_clamp, tile_tex(s.tile_uv), l, 0.0).ra;
}

// ------------------------------------------------------------------ images & projection
fn srgb_to_linear(c: vec3f) -> vec3f {
    return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
}

fn rot2(p: vec2f, deg: f32) -> vec2f {
    let r = radians(deg);
    let c = cos(r);
    let s = sin(r);
    return vec2f(c * p.x - s * p.y, s * p.x + c * p.y);
}

fn image_at(layer: i32, uv: vec2f) -> vec4f {
    if layer < 0 { return vec4f(0.0, 0.0, 0.0, 1.0); }
    return textureSampleLevel(images, samp_repeat, uv, layer, 0.0);
}

fn image_uv(layer: i32, s: Surface, scale: f32, rotation: f32, offset: vec3f) -> vec4f {
    return image_vec(layer, s.tile_uv, scale, rotation, offset);
}

// The Image node at a UV from its Vector input (V up, like tile_uv).
fn image_vec(layer: i32, uv: vec2f, scale: f32, rotation: f32, offset: vec3f) -> vec4f {
    let p = rot2(vec2f(uv.x, 1.0 - uv.y) - 0.5, rotation) * scale + 0.5 + offset.xy;
    return image_at(layer, p);
}

// Rotation about X, then Y, then Z (degrees), as Blender's Euler XYZ.
fn euler_xyz(deg: vec3f) -> mat3x3f {
    let r = radians(deg);
    let c = cos(r);
    let s = sin(r);
    let rx = mat3x3f(1.0, 0.0, 0.0, 0.0, c.x, s.x, 0.0, -s.x, c.x);
    let ry = mat3x3f(c.y, 0.0, -s.y, 0.0, 1.0, 0.0, s.y, 0.0, c.y);
    let rz = mat3x3f(c.z, s.z, 0.0, -s.z, c.z, 0.0, 0.0, 0.0, 1.0);
    return rz * ry * rx;
}

// Object-space triplanar projection; scale is repeats per world unit.
fn image_triplanar(layer: i32, s: Surface, scale: f32, rotation: f32, offset: vec3f, blend_w: f32) -> vec4f {
    let p = s.pos * scale + offset;
    let n = abs(s.normal);
    let k = 1.0 + 15.0 * (1.0 - blend_w);
    var w = pow(n, vec3f(k));
    w /= max(w.x + w.y + w.z, 1e-6);
    let x = image_at(layer, rot2(p.zy, rotation));
    let y = image_at(layer, rot2(p.xz, rotation));
    let z = image_at(layer, rot2(p.xy, rotation));
    return x * w.x + y * w.y + z * w.z;
}

// Noise (hash3, fbm, cellular, ...) is in noise.wgsl, prepended to this file.

// ------------------------------------------------------------------ adjustments
fn levels3(c: vec3f, in_black: f32, in_white: f32, gamma: f32, out_black: f32, out_white: f32) -> vec3f {
    let t = clamp((c - in_black) / max(in_white - in_black, 1e-5), vec3f(0.0), vec3f(1.0));
    return mix(vec3f(out_black), vec3f(out_white), pow(t, vec3f(1.0 / max(gamma, 1e-3))));
}

fn rgb_to_hsv(c: vec3f) -> vec3f {
    let k = vec4f(0.0, -1.0 / 3.0, 2.0 / 3.0, -1.0);
    let p = select(vec4f(c.gb, k.xy), vec4f(c.bg, k.wz), c.g < c.b);
    let q = select(vec4f(p.xyw, c.r), vec4f(c.r, p.yzx), p.x < c.r);
    let d = q.x - min(q.w, q.y);
    let e = 1.0e-10;
    return vec3f(abs(q.z + (q.w - q.y) / (6.0 * d + e)), d / (q.x + e), q.x);
}

fn hsv_to_rgb(c: vec3f) -> vec3f {
    let p = abs(fract(c.xxx + vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0)) * 6.0 - 3.0);
    return c.z * mix(vec3f(1.0), clamp(p - 1.0, vec3f(0.0), vec3f(1.0)), c.y);
}

fn luminance(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

// ------------------------------------------------------------------ generators
fn gen_edge_wear(s: Surface, amount: f32, sharpness: f32, noise_scale: f32, noise_amount: f32, seed: f32) -> f32 {
    let tc = tile_tex(s.tile_uv);
    // Small dead zone: baked curvature of flat areas is noisy around 0.5.
    let convex = clamp((mesh_curvature(tc, s.layer) - 0.52) * 3.2, 0.0, 1.0);
    let n = smoothstep(0.3, 0.7, fbm(s.pos_norm * noise_scale, 0u, 5.0, 2.0, 0.55, seed));
    // Noise scales the convexity, so edges chip instead of forming an even outline and
    // flat areas (convexity 0) never wear.
    let v = convex * mix(1.0, n * 2.0, noise_amount);
    let threshold = max(1.0 - 1.4 * amount, 0.05);
    let w = 0.5 * (1.0 - sharpness) + 0.01;
    // The window never reaches below zero, so convexity 0 stays unworn.
    return smoothstep(max(threshold - w, 0.0), threshold + w, v);
}

fn gen_dirt(s: Surface, amount: f32, contrast: f32, noise_scale: f32, seed: f32) -> f32 {
    let tc = tile_tex(s.tile_uv);
    let occ = 1.0 - mesh_ao(tc, s.layer);
    let n = fbm(s.pos_norm * noise_scale, 0u, 4.0, 2.0, 0.5, seed);
    let v = occ + (n - 0.5) * 0.35;
    let threshold = 1.0 - amount;
    let w = 0.5 * (1.0 - contrast) + 0.02;
    return smoothstep(max(threshold - w, 0.0), threshold + w, v);
}

fn gen_curvature_mask(s: Surface, concave: bool, threshold: f32, softness: f32) -> f32 {
    let c = mesh_curvature(tile_tex(s.tile_uv), s.layer);
    var v = (c - 0.5) * 2.0;
    if concave { v = -v; }
    let w = softness * 0.5 + 0.005;
    return smoothstep(threshold - w, threshold + w, v);
}

fn gen_position_gradient(s: Surface, axis: u32, falloff: f32, noise_amount: f32) -> f32 {
    let p = s.pos_norm;
    var t = p.y;
    if axis == 0u { t = p.x; } else if axis == 2u { t = p.z; }
    let n = fbm(p * 6.0, 0u, 4.0, 2.0, 0.5, 0.0);
    t += (n - 0.5) * noise_amount;
    let w = falloff * 0.5 + 0.005;
    return 1.0 - smoothstep(0.5 - w, 0.5 + w, t);
}

fn id_match(s: Surface, color: vec3f, tolerance: f32) -> f32 {
    let id = mesh_id(tile_tex(s.tile_uv), s.layer);
    return select(0.0, 1.0, distance(id, color) <= tolerance + 1e-3);
}
