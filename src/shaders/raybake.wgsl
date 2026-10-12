// Ray-traced mesh maps: Ambient Occlusion, Curvature and Thickness.
// See docs/reference/mesh-maps.md.

struct Params {
    layer: u32,
    res: u32,
    row_start: u32,
    row_count: u32,
    // 0 = AO, 1 = curvature, 2 = thickness
    mode: u32,
    samples: u32,
    ignore_backfaces: u32,
    in_layer: u32,         // layer of pos_map / nrm_map to read
    max_distance: f32,     // world units
    cos_spread: f32,       // cos of half the AO cone angle
    curvature_radius: f32, // world units
    epsilon: f32,          // world units, ray origin offset
};

const CURVATURE_GAIN: f32 = 3.0;

struct Node {
    min: vec3f,
    index: u32,
    max: vec3f,
    count: u32,
};

struct Tri {
    v0: vec4f,
    v1: vec4f,
    v2: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var pos_map: texture_2d_array<f32>;
@group(0) @binding(2) var nrm_map: texture_2d_array<f32>;
@group(0) @binding(3) var<storage, read> nodes: array<Node>;
@group(0) @binding(4) var<storage, read> tris: array<Tri>;
@group(0) @binding(5) var out_map: texture_storage_2d_array<r32float, write>;

struct Hit {
    t: f32,
    front: bool,
};

fn ray_box(o: vec3f, inv_d: vec3f, bmin: vec3f, bmax: vec3f, t_max: f32) -> bool {
    let t0 = (bmin - o) * inv_d;
    let t1 = (bmax - o) * inv_d;
    let tn = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), min(t0.z, t1.z));
    let tf = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z));
    return tf >= max(tn, 0.0) && tn < t_max;
}

// Closest hit within t_max. Back-facing hits are skipped when `skip_back` is set.
fn trace(o: vec3f, d: vec3f, t_max: f32, skip_back: bool) -> Hit {
    var hit = Hit(t_max, false);
    let inv_d = 1.0 / select(d, vec3f(1e-20), abs(d) < vec3f(1e-20));
    var stack: array<u32, 64>;
    var sp = 0u;
    stack[0] = 0u;
    sp = 1u;
    loop {
        if sp == 0u { break; }
        sp -= 1u;
        let n = nodes[stack[sp]];
        if !ray_box(o, inv_d, n.min, n.max, hit.t) { continue; }
        if n.count == 0u {
            if sp < 62u {
                stack[sp] = n.index;
                stack[sp + 1u] = n.index + 1u;
                sp += 2u;
            }
            continue;
        }
        for (var i = n.index; i < n.index + n.count; i++) {
            let tri = tris[i];
            let e1 = tri.v1.xyz - tri.v0.xyz;
            let e2 = tri.v2.xyz - tri.v0.xyz;
            let p = cross(d, e2);
            let det = dot(e1, p);
            if abs(det) < 1e-12 { continue; }
            // det = -dot(d, cross(e1, e2)): positive when the ray hits the front face of a
            // counter-clockwise triangle.
            let front = det > 0.0;
            if skip_back && !front { continue; }
            let inv = 1.0 / det;
            let s = o - tri.v0.xyz;
            let u = dot(s, p) * inv;
            if u < 0.0 || u > 1.0 { continue; }
            let q = cross(s, e1);
            let v = dot(d, q) * inv;
            if v < 0.0 || u + v > 1.0 { continue; }
            let t = dot(e2, q) * inv;
            if t > 0.0 && t < hit.t {
                hit = Hit(t, front);
            }
        }
    }
    return hit;
}

fn hash(x_in: u32) -> u32 {
    var x = x_in;
    x ^= x >> 16u;
    x *= 0x7feb352du;
    x ^= x >> 15u;
    x *= 0x846ca68bu;
    x ^= x >> 16u;
    return x;
}

fn radical_inverse(b_in: u32) -> f32 {
    var b = b_in;
    b = (b << 16u) | (b >> 16u);
    b = ((b & 0x55555555u) << 1u) | ((b & 0xAAAAAAAAu) >> 1u);
    b = ((b & 0x33333333u) << 2u) | ((b & 0xCCCCCCCCu) >> 2u);
    b = ((b & 0x0F0F0F0Fu) << 4u) | ((b & 0xF0F0F0F0u) >> 4u);
    b = ((b & 0x00FF00FFu) << 8u) | ((b & 0xFF00FF00u) >> 8u);
    return f32(b) * 2.3283064365386963e-10;
}

fn basis(n: vec3f) -> mat3x3f {
    let s = select(-1.0, 1.0, n.z >= 0.0);
    let a = -1.0 / (s + n.z);
    let b = n.x * n.y * a;
    let t = vec3f(1.0 + s * n.x * n.x * a, s * b, -s * n.x);
    let bt = vec3f(b, s + n.y * n.y * a, -n.y);
    return mat3x3f(t, bt, n);
}

// Direction in a cone of half-angle acos(cos_max) around +Z, cosine-weighted when the
// cone is the full hemisphere.
fn cone_dir(i: u32, count: u32, rot: f32, cos_max: f32) -> vec3f {
    let u1 = (f32(i) + 0.5) / f32(count);
    let u2 = fract(radical_inverse(i) + rot);
    // Cosine-weighted within the cone: sample sin^2 uniformly.
    let sin2_max = 1.0 - cos_max * cos_max;
    let sin2 = u1 * sin2_max;
    let cos_t = sqrt(1.0 - sin2);
    let sin_t = sqrt(sin2);
    let phi = 2.0 * 3.14159265 * u2;
    return vec3f(cos(phi) * sin_t, sin(phi) * sin_t, cos_t);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    let xy = vec2u(gid.x, gid.y + params.row_start);
    if xy.x >= params.res || gid.y >= params.row_count || xy.y >= params.res { return; }
    let pos = textureLoad(pos_map, xy, params.in_layer, 0);
    // Islands and padding only (docs/reference/mesh-maps.md#padding).
    if pos.a < 0.3 {
        textureStore(out_map, xy, params.layer, vec4f(select(1.0, 0.5, params.mode == 1u)));
        return;
    }
    let n = normalize(textureLoad(nrm_map, xy, params.in_layer, 0).xyz * 2.0 - 1.0);
    let tbn = basis(n);
    // Seed from the position so padding texels reproduce their source texel.
    let seed = hash(bitcast<u32>(pos.x) ^ hash(bitcast<u32>(pos.y) ^ hash(bitcast<u32>(pos.z))));
    let rot = f32(seed) * 2.3283064365386963e-10;
    let count = params.samples;
    var acc = 0.0;

    switch params.mode {
        case 0u: {
            // Ambient occlusion: fraction of unoccluded rays.
            let o = pos.xyz + n * params.epsilon;
            for (var i = 0u; i < count; i++) {
                let d = tbn * cone_dir(i, count, rot, params.cos_spread);
                let h = trace(o, d, params.max_distance, params.ignore_backfaces != 0u);
                acc += select(1.0, 0.0, h.t < params.max_distance);
            }
            acc /= f32(count);
        }
        case 1u: {
            // Curvature: probe the surface from a point above it. On a flat surface every
            // ray in the cone hits at the expected distance; at convex edges rays travel
            // further or miss, in concave areas they hit early.
            let r = params.curvature_radius;
            let o = pos.xyz + n * r;
            let cos_cone = 0.5;
            var m = 0.0;
            for (var i = 0u; i < count; i++) {
                let local = cone_dir(i, count, rot, cos_cone);
                let d = tbn * vec3f(local.xy, -local.z);
                let expected = r / local.z;
                let h = trace(o, d, expected * 2.0, false);
                var dist = h.t;
                // A back face first means the probe started inside geometry: deeply concave.
                if h.t < expected * 2.0 && !h.front { dist = 0.0; }
                m += clamp((expected - dist) / expected, -1.0, 1.0);
            }
            acc = clamp(0.5 - 0.5 * CURVATURE_GAIN * m / f32(count), 0.0, 1.0);
        }
        default: {
            // Thickness: mean distance to the opposite side, through the inside.
            let o = pos.xyz - n * params.epsilon;
            for (var i = 0u; i < count; i++) {
                let local = cone_dir(i, count, rot, 0.0);
                let d = tbn * vec3f(local.xy, -local.z);
                let h = trace(o, d, params.max_distance, false);
                acc += min(h.t, params.max_distance);
            }
            acc = acc / f32(count) / params.max_distance;
        }
    }
    textureStore(out_map, xy, params.layer, vec4f(acc));
}
