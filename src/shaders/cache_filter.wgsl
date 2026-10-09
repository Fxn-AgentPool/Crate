// UV-space post-processing for caches: padding, blur, dilate, height to normal.

struct Params {
    res: u32,
    // Filter radius in texels.
    radius: f32,
    // Blur / dilate direction: 0 = horizontal, 1 = vertical.
    axis: u32,
    // Height to normal strength.
    strength: f32,
};

const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var dst: texture_storage_2d<rgba16float, write>;
// Mesh map nearest-covered-texel index (padding source), one tile.
@group(0) @binding(3) var source: texture_2d<u32>;
@group(0) @binding(4) var mm_pos: texture_2d<f32>;

fn load(p: vec2i) -> vec4f {
    let r = i32(params.res);
    return textureLoad(src, clamp(p, vec2i(0), vec2i(r - 1)), 0);
}

// Copies covered texels and fills padding from the nearest covered texel.
@compute @workgroup_size(8, 8)
fn pad(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let mres = textureDimensions(source).x;
    let k = f32(mres) / f32(params.res);
    let mp = min(vec2u(vec2f(gid.xy) * k), vec2u(mres - 1u));
    let s = textureLoad(source, mp, 0).r;
    var src_xy = gid.xy;
    if s != NONE {
        let sp = vec2u(s % mres, s / mres);
        if any(sp != mp) {
            src_xy = min(vec2u((vec2f(sp) + 0.5) / k), vec2u(params.res - 1u));
        }
    }
    textureStore(dst, gid.xy, textureLoad(src, src_xy, 0));
}

@compute @workgroup_size(8, 8)
fn blur(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let r = max(params.radius, 0.0);
    let n = i32(ceil(r));
    let sigma = max(r * 0.5, 0.5);
    let dir = select(vec2i(1, 0), vec2i(0, 1), params.axis == 1u);
    var sum = vec4f(0.0);
    var wsum = 0.0;
    for (var i = -n; i <= n; i++) {
        let w = exp(-f32(i * i) / (2.0 * sigma * sigma));
        sum += load(vec2i(gid.xy) + dir * i) * w;
        wsum += w;
    }
    textureStore(dst, gid.xy, sum / max(wsum, 1e-6));
}

@compute @workgroup_size(8, 8)
fn dilate(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let n = i32(ceil(max(params.radius, 0.0)));
    let dir = select(vec2i(1, 0), vec2i(0, 1), params.axis == 1u);
    var m = vec4f(-1e30);
    for (var i = -n; i <= n; i++) {
        m = max(m, load(vec2i(gid.xy) + dir * i));
    }
    textureStore(dst, gid.xy, m);
}

fn pos_at(p: vec2i) -> vec3f {
    let mres = f32(textureDimensions(mm_pos).x);
    let q = clamp(vec2i((vec2f(p) + 0.5) * mres / f32(params.res)), vec2i(0), vec2i(i32(mres) - 1));
    return textureLoad(mm_pos, q, 0).xyz;
}

// Height (alpha of the input) to a tangent-space normal, encoded 0..1 in rgb.
@compute @workgroup_size(8, 8)
fn height_normal(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let p = vec2i(gid.xy);
    let px = distance(pos_at(p + vec2i(1, 0)), pos_at(p - vec2i(1, 0)));
    let py = distance(pos_at(p + vec2i(0, 1)), pos_at(p - vec2i(0, 1)));
    // Heights are in world units scaled by 0.01 (the default height scale) times strength.
    let hs = 0.01 * params.strength;
    let du = (load(p + vec2i(1, 0)).a - load(p - vec2i(1, 0)).a) * hs / max(px, 1e-6);
    let dv = (load(p - vec2i(0, 1)).a - load(p + vec2i(0, 1)).a) * hs / max(py, 1e-6);
    let n = normalize(vec3f(-du, -dv, 1.0));
    textureStore(dst, gid.xy, vec4f(n * 0.5 + 0.5, 1.0));
}
