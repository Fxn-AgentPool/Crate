// Post-processing for UV-space evaluation: height to normal, then padding by jump flooding.

struct Params {
    res: u32,
    step: u32,
    // Padding limit in texels; 0xffffffff = infinite, 0 = none.
    limit: u32,
    _pad: u32,
    // x: height scale (world units per height unit), y: mesh map resolution
    height: vec4f,
};

const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> params: Params;

// ------------------------------------------------------------------ height to normal
@group(0) @binding(1) var scalars: texture_2d<f32>;
@group(0) @binding(2) var normal_in: texture_2d<f32>;
@group(0) @binding(3) var mm_pos: texture_2d<f32>;
@group(0) @binding(4) var normal_out: texture_storage_2d<rgba16float, write>;

fn rnm(b: vec3f, d: vec3f) -> vec3f {
    let t = b + vec3f(0.0, 0.0, 1.0);
    let u = d * vec3f(-1.0, -1.0, 1.0);
    return normalize(t * dot(t, u) / max(t.z, 1e-4) - u);
}

fn covered(p: vec2i) -> bool {
    let r = i32(params.res);
    if p.x < 0 || p.y < 0 || p.x >= r || p.y >= r { return false; }
    return textureLoad(scalars, p, 0).a > 0.5;
}

fn height_at(p: vec2i, fallback: f32) -> f32 {
    if !covered(p) { return fallback; }
    return textureLoad(scalars, p, 0).z;
}

fn pos_at(p: vec2i) -> vec3f {
    let mres = f32(textureDimensions(mm_pos).x);
    let q = clamp(vec2i((vec2f(p) + 0.5) * mres / f32(params.res)), vec2i(0), vec2i(i32(mres) - 1));
    return textureLoad(mm_pos, q, 0).xyz;
}

@compute @workgroup_size(8, 8)
fn height_normal(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let p = vec2i(gid.xy);
    let n_in = textureLoad(normal_in, p, 0);
    if !covered(p) {
        textureStore(normal_out, p, n_in);
        return;
    }
    let h = textureLoad(scalars, p, 0).z;
    let hs = params.height.x;
    // World distance between neighboring texels, from the position map.
    let px = distance(pos_at(p + vec2i(1, 0)), pos_at(p - vec2i(1, 0)));
    let py = distance(pos_at(p + vec2i(0, 1)), pos_at(p - vec2i(0, 1)));
    let du = (height_at(p + vec2i(1, 0), h) - height_at(p - vec2i(1, 0), h)) * hs / max(px, 1e-6);
    // Texture rows run down while v (bitangent) runs up.
    let dv = (height_at(p - vec2i(0, 1), h) - height_at(p + vec2i(0, 1), h)) * hs / max(py, 1e-6);
    let nh = normalize(vec3f(-du, -dv, 1.0));
    let detail = normalize(select(n_in.xyz, vec3f(0.0, 0.0, 1.0), dot(n_in.xyz, n_in.xyz) < 1e-8));
    textureStore(normal_out, p, vec4f(rnm(nh, detail), n_in.w));
}

// ------------------------------------------------------------------ jump flood padding
@group(0) @binding(5) var seed_in: texture_2d<u32>;
@group(0) @binding(6) var seed_out: texture_storage_2d<r32uint, write>;

@compute @workgroup_size(8, 8)
fn seed_init(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let c = textureLoad(scalars, gid.xy, 0).a > 0.5;
    textureStore(seed_out, gid.xy, vec4u(select(NONE, gid.y * params.res + gid.x, c)));
}

@compute @workgroup_size(8, 8)
fn seed_step(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let me = vec2f(gid.xy);
    var best = textureLoad(seed_in, gid.xy, 0).r;
    var best_d = 1e30;
    if best != NONE {
        best_d = distance(vec2f(f32(best % params.res), f32(best / params.res)), me);
    }
    let k = i32(params.step);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = vec2i(gid.xy) + vec2i(dx, dy) * k;
            if q.x < 0 || q.y < 0 || q.x >= i32(params.res) || q.y >= i32(params.res) { continue; }
            let s = textureLoad(seed_in, vec2u(q), 0).r;
            if s == NONE { continue; }
            let d = distance(vec2f(f32(s % params.res), f32(s / params.res)), me);
            if d < best_d {
                best_d = d;
                best = s;
            }
        }
    }
    textureStore(seed_out, gid.xy, vec4u(best));
}

@group(0) @binding(7) var src0: texture_2d<f32>;
@group(0) @binding(8) var src1: texture_2d<f32>;
@group(0) @binding(9) var src2: texture_2d<f32>;
@group(0) @binding(10) var src3: texture_2d<f32>;
@group(0) @binding(11) var dst0: texture_storage_2d<rgba16float, write>;
@group(0) @binding(12) var dst1: texture_storage_2d<rgba16float, write>;
@group(0) @binding(13) var dst2: texture_storage_2d<rgba16float, write>;
@group(0) @binding(14) var dst3: texture_storage_2d<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn gather(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.res || gid.y >= params.res { return; }
    let s = textureLoad(seed_in, gid.xy, 0).r;
    var src = gid.xy;
    var keep = textureLoad(src1, gid.xy, 0).a > 0.5;
    if !keep && s != NONE {
        let sp = vec2u(s % params.res, s / params.res);
        let d = distance(vec2f(sp), vec2f(gid.xy));
        if params.limit == NONE || d <= f32(params.limit) {
            src = sp;
            keep = true;
        }
    }
    if keep {
        textureStore(dst0, gid.xy, textureLoad(src0, src, 0));
        textureStore(dst1, gid.xy, textureLoad(src1, src, 0));
        textureStore(dst2, gid.xy, textureLoad(src2, src, 0));
        textureStore(dst3, gid.xy, textureLoad(src3, src, 0));
    } else {
        textureStore(dst0, gid.xy, vec4f(0.0));
        textureStore(dst1, gid.xy, vec4f(0.0));
        textureStore(dst2, gid.xy, vec4f(0.0, 0.0, 1.0, 1.0));
        textureStore(dst3, gid.xy, vec4f(0.0));
    }
}
