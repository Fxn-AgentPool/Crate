// Shows unbaked Manual-mode paint (base color plane) over the viewport, through the
// buffer transform.

struct Params {
    // Screen px -> untransformed screen px (rows of a 2x3 affine).
    row0: vec4f,
    row1: vec4f,
    // x, y: screen size
    screen: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var plane0: texture_2d<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4f,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    return o;
}

fn srgb_encode(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(max(c, vec3f(0.0)), vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let q = in.pos.xy;
    let p = vec2f(dot(params.row0.xyz, vec3f(q, 1.0)), dot(params.row1.xyz, vec3f(q, 1.0)));
    let uv = p / params.screen.xy;
    if any(uv < vec2f(0.0)) || any(uv > vec2f(1.0)) { discard; }
    let v = textureSampleLevel(plane0, samp, uv, 0.0);
    if v.a <= 0.001 { discard; }
    return vec4f(srgb_encode(clamp(v.rgb / v.a, vec3f(0.0), vec3f(1.0))), v.a * 0.9);
}
