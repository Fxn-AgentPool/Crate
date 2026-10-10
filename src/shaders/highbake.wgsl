// High-poly projection: for every texel, a ray from the low-poly surface (moved Frontal
// distance outward) travels inward along the low-poly normal and takes the first front-facing
// high-poly triangle. Writes the Normal (tangent space), Height and ID maps, and the hit
// position and normal that the AO, Curvature and Thickness bakes then use. With auto cage, the
// distances come per texel from cage_raster.wgsl.
// See docs/reference/mesh-maps.md#high-poly-mesh.

struct Params {
    layer: u32,
    res: u32,
    row_start: u32,
    row_count: u32,
    // World units; with auto cage, the cage's largest distances (they scale Height).
    frontal: f32,
    rear: f32,
    // bit 0: write Normal, bit 1: write Height, bit 2: write ID, bit 3: auto cage
    flags: u32,
    _pad: u32,
};

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

// Vertex normals; n0.w = material, n1.w = match id (bit patterns).
struct TriData {
    n0: vec4f,
    n1: vec4f,
    n2: vec4f,
};

const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var pos_map: texture_2d_array<f32>;
@group(0) @binding(2) var nrm_map: texture_2d_array<f32>;
@group(0) @binding(3) var tan_map: texture_2d_array<f32>;
@group(0) @binding(4) var src_map: texture_2d_array<u32>;
@group(0) @binding(5) var<storage, read> nodes: array<Node>;
@group(0) @binding(6) var<storage, read> tris: array<Tri>;
@group(0) @binding(7) var<storage, read> tdata: array<TriData>;
// Match id per low-poly part (0 = hits every part).
@group(0) @binding(8) var<storage, read> low_match: array<u32>;
@group(0) @binding(9) var out_hpos: texture_storage_2d_array<rgba32float, write>;
@group(0) @binding(10) var out_hnrm: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(11) var out_tnrm: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(12) var out_height: texture_storage_2d_array<r32float, write>;
@group(0) @binding(13) var out_id: texture_storage_2d_array<rgba8unorm, write>;
// [0] covered texels, [1] missed texels
@group(0) @binding(14) var<storage, read_write> counters: array<atomic<u32>, 2>;
// Auto cage of this tile: frontal, rear per texel (world units).
@group(0) @binding(15) var cage_map: texture_2d<f32>;

struct Hit {
    t: f32,
    tri: u32,
    u: f32,
    v: f32,
};

fn ray_box(o: vec3f, inv_d: vec3f, bmin: vec3f, bmax: vec3f, t_max: f32) -> bool {
    let t0 = (bmin - o) * inv_d;
    let t1 = (bmax - o) * inv_d;
    let tn = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), min(t0.z, t1.z));
    let tf = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z));
    return tf >= max(tn, 0.0) && tn < t_max;
}

// First front-facing hit within t_max on a triangle the part `lo` may hit.
fn trace(o: vec3f, d: vec3f, t_max: f32, lo: u32) -> Hit {
    var hit = Hit(t_max, NONE, 0.0, 0.0);
    let inv_d = 1.0 / select(d, vec3f(1e-20), abs(d) < vec3f(1e-20));
    var stack: array<u32, 64>;
    var sp = 1u;
    stack[0] = 0u;
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
            // Front faces only (counter-clockwise seen from the ray origin).
            if det < 1e-12 { continue; }
            let hi = bitcast<u32>(tdata[i].n1.w);
            if lo != 0u && hi != 0u && lo != hi { continue; }
            let inv = 1.0 / det;
            let s = o - tri.v0.xyz;
            let u = dot(s, p) * inv;
            if u < 0.0 || u > 1.0 { continue; }
            let q = cross(s, e1);
            let v = dot(d, q) * inv;
            if v < 0.0 || u + v > 1.0 { continue; }
            let t = dot(e2, q) * inv;
            if t >= 0.0 && t < hit.t {
                hit = Hit(t, i, u, v);
            }
        }
    }
    return hit;
}

fn id_color(i: u32) -> vec3f {
    // Same palette as uv_raster.wgsl.
    let hue = fract(f32(i) * 0.618034 + 0.05);
    let k = vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3f(hue) + k) * 6.0 - 3.0);
    let val = 1.0 - 0.25 * f32((i / 7u) % 3u);
    return clamp(p - 1.0, vec3f(0.0), vec3f(1.0)) * val;
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    let xy = vec2u(gid.x, gid.y + params.row_start);
    if xy.x >= params.res || gid.y >= params.row_count || xy.y >= params.res { return; }
    let layer = params.layer;
    let pos = textureLoad(pos_map, xy, layer, 0);
    let flat_n = vec4f(0.5, 0.5, 1.0, 1.0);
    if pos.a == 0.0 {
        textureStore(out_hpos, xy, 0, vec4f(0.0));
        textureStore(out_hnrm, xy, 0, vec4f(0.5, 0.5, 1.0, 0.0));
        if (params.flags & 1u) != 0u { textureStore(out_tnrm, xy, layer, flat_n); }
        if (params.flags & 2u) != 0u { textureStore(out_height, xy, layer, vec4f(0.5)); }
        return;
    }
    // Padding texels repeat their source texel; only the tangent map isn't padded.
    let s = textureLoad(src_map, xy, layer, 0).r;
    let sxy = select(xy, vec2u(s % params.res, s / params.res), s != NONE);
    let nrm = textureLoad(nrm_map, xy, layer, 0);
    let tan = textureLoad(tan_map, sxy, layer, 0);

    // The viewport's tangent basis (viewport.wgsl), so a baked map reproduces the high poly.
    let ng = normalize(nrm.xyz * 2.0 - 1.0);
    var t = tan.xyz - ng * dot(tan.xyz, ng);
    if dot(t, t) < 1e-12 {
        t = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(ng.x) > 0.9);
        t = t - ng * dot(t, ng);
    }
    t = normalize(t);
    let sign = select(-1.0, 1.0, tan.w >= 0.0);
    let b = cross(ng, t) * sign;
    let part = u32(abs(tan.w));
    var lo = 0u;
    if part > 0u && part - 1u < arrayLength(&low_match) {
        lo = low_match[part - 1u];
    }

    var frontal = params.frontal;
    var rear = params.rear;
    if (params.flags & 8u) != 0u {
        let c = textureLoad(cage_map, sxy, 0).xy;
        frontal = c.x;
        rear = c.y;
    }
    let o = pos.xyz + ng * frontal;
    let hit = trace(o, -ng, frontal + rear, lo);
    let missed = hit.tri == NONE;
    if pos.a > 0.75 {
        atomicAdd(&counters[0], 1u);
        if missed { atomicAdd(&counters[1], 1u); }
    }

    if missed {
        // Flat normal with alpha 0 marks the miss; AO and friends use the low-poly surface.
        textureStore(out_hpos, xy, 0, pos);
        textureStore(out_hnrm, xy, 0, nrm);
        if (params.flags & 1u) != 0u { textureStore(out_tnrm, xy, layer, vec4f(0.5, 0.5, 1.0, 0.0)); }
        if (params.flags & 2u) != 0u { textureStore(out_height, xy, layer, vec4f(0.5)); }
        return;
    }
    let td = tdata[hit.tri];
    let w = 1.0 - hit.u - hit.v;
    var nh = td.n0.xyz * w + td.n1.xyz * hit.u + td.n2.xyz * hit.v;
    nh = select(normalize(nh), ng, dot(nh, nh) < 1e-12);
    let p = o - ng * hit.t;
    textureStore(out_hpos, xy, 0, vec4f(p, pos.a));
    textureStore(out_hnrm, xy, 0, vec4f(nh * 0.5 + 0.5, pos.a));
    if (params.flags & 1u) != 0u {
        let n_ts = normalize(vec3f(dot(nh, t), dot(nh, b), dot(nh, ng)));
        textureStore(out_tnrm, xy, layer, vec4f(n_ts * 0.5 + 0.5, 1.0));
    }
    if (params.flags & 2u) != 0u {
        let d = frontal - hit.t;
        let h = clamp(0.5 + 0.5 * d / max(max(params.frontal, params.rear), 1e-12), 0.0, 1.0);
        textureStore(out_height, xy, layer, vec4f(h));
    }
    if (params.flags & 4u) != 0u {
        textureStore(out_id, xy, layer, vec4f(id_color(bitcast<u32>(td.n0.w)), pos.a));
    }
}
