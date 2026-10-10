// Bake preview pane: the low poly, high poly and cage, each in one color and see-through.
// See docs/reference/mesh-maps.md#bake-preview and docs/explanation/about-high-poly-baking.md.
//
// Each part is drawn on its own with a depth test, so only its nearest surface remains, into one
// layer of `colors` (lit color, alpha) and `dists` (distance from the eye). The composite then
// layers the three parts by distance at every sample, farthest first, and averages the
// supersamples of each pixel.

struct Part {
    view_proj: mat4x4f,
    eye: vec4f,
    // rgb: linear color, a: opacity
    color: vec4f,
};

@group(0) @binding(0) var<uniform> part: Part;

struct VsIn {
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
};

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
};

@vertex
fn vs_part(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = part.view_proj * vec4f(v.pos, 1.0);
    o.pos = v.pos;
    o.normal = v.normal;
    return o;
}

struct PartOut {
    @location(0) color: vec4f,
    @location(1) dist: f32,
};

@fragment
fn fs_part(in: VsOut) -> PartOut {
    let to_eye = part.eye.xyz - in.pos;
    let v = normalize(to_eye);
    var facing = 1.0;
    if dot(in.normal, in.normal) > 1e-12 {
        facing = abs(dot(normalize(in.normal), v));
    }
    var o: PartOut;
    // Lit from the camera; up to twice as opaque at grazing angles, so outlines stay visible.
    o.color = vec4f(part.color.rgb * (0.35 + 0.65 * facing), min(part.color.a * (2.0 - facing), 1.0));
    o.dist = length(to_eye);
    return o;
}

// ---------------------------------------------------------------- composite

struct Composite {
    // x: supersampling factor
    params: vec4u,
    background_top: vec4f,
    background_bottom: vec4f,
};

@group(0) @binding(0) var colors: texture_2d_array<f32>;
@group(0) @binding(1) var dists: texture_2d_array<f32>;
@group(0) @binding(2) var<uniform> comp: Composite;

struct FullOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> FullOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: FullOut;
    o.pos = vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn srgb_encode(c: vec3f) -> vec3f {
    let x = clamp(c, vec3f(0.0), vec3f(1.0));
    return select(1.055 * pow(x, vec3f(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3f(0.0031308));
}

@fragment
fn fs_composite(in: FullOut) -> @location(0) vec4f {
    let s = i32(comp.params.x);
    let background = mix(comp.background_top.rgb, comp.background_bottom.rgb, in.uv.y);
    var sum = vec3f(0.0);
    for (var sy = 0; sy < s; sy++) {
        for (var sx = 0; sx < s; sx++) {
            let q = vec2i(in.pos.xy) * s + vec2i(sx, sy);
            var c: array<vec4f, 3>;
            var d: array<f32, 3>;
            for (var k = 0; k < 3; k++) {
                c[k] = textureLoad(colors, q, k, 0);
                d[k] = textureLoad(dists, q, k, 0).r;
            }
            // Farthest first: sort the three layers by distance, descending.
            var o = array<i32, 3>(0, 1, 2);
            if d[o[0]] < d[o[1]] { let t = o[0]; o[0] = o[1]; o[1] = t; }
            if d[o[1]] < d[o[2]] { let t = o[1]; o[1] = o[2]; o[2] = t; }
            if d[o[0]] < d[o[1]] { let t = o[0]; o[0] = o[1]; o[1] = t; }
            var col = background;
            for (var k = 0; k < 3; k++) {
                let l = c[o[k]];
                col = mix(col, l.rgb, l.a);
            }
            sum += col;
        }
    }
    return vec4f(srgb_encode(sum / f32(s * s)), 1.0);
}
