// Paint transfer to new UVs on reimport. See docs/reference/mesh-and-udims.md#reimport.
//
// `locate`: for each texel of one new tile, finds the nearest point of the old mesh along the
// new surface normal (within max_dist, in front or behind) and writes which old texel holds the
// paint there: (old pool layer index of the tile, x, y, 1), or 0 where nothing was found.
// `gather`: copies one paint plane of one slot through that map.

struct LocateParams {
    layer: u32,      // new tile: layer of pos_map / nrm_map
    res: u32,
    row_start: u32,
    row_count: u32,
    max_dist: f32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
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

@group(0) @binding(0) var<uniform> lp: LocateParams;
@group(0) @binding(1) var pos_map: texture_2d_array<f32>;
@group(0) @binding(2) var nrm_map: texture_2d_array<f32>;
@group(0) @binding(3) var<storage, read> nodes: array<Node>;
@group(0) @binding(4) var<storage, read> tris: array<Tri>;
// Two per triangle, in BVH order: (u0, v0, u1, v1), (u2, v2, -, -).
@group(0) @binding(5) var<storage, read> tri_uv: array<vec4f>;
// Old pool layer index of each UDIM (udim - 1001), or -1.
@group(0) @binding(6) var<storage, read> tile_layer: array<i32>;
@group(0) @binding(7) var hits: texture_storage_2d<rgba32float, write>;
// [texels covered by the new UVs, of those, texels that found no old surface]
@group(0) @binding(8) var<storage, read_write> counts: array<atomic<u32>, 2>;

fn ray_box(o: vec3f, inv_d: vec3f, bmin: vec3f, bmax: vec3f, t_lo: f32, t_hi: f32) -> bool {
    let t0 = (bmin - o) * inv_d;
    let t1 = (bmax - o) * inv_d;
    let tn = max(max(min(t0.x, t1.x), min(t0.y, t1.y)), min(t0.z, t1.z));
    let tf = min(min(max(t0.x, t1.x), max(t0.y, t1.y)), max(t0.z, t1.z));
    return tf >= max(tn, t_lo) && tn <= t_hi;
}

struct Found {
    // Distance from the surface point, along the normal (either direction).
    dist: f32,
    tri: u32,
    b1: f32,
    b2: f32,
};

// Nearest triangle crossed by the line p + t * d, |t| < max_dist, both faces.
fn nearest(p: vec3f, d: vec3f, max_dist: f32) -> Found {
    var best = Found(max_dist, 0xffffffffu, 0.0, 0.0);
    let inv_d = 1.0 / select(d, vec3f(1e-20), abs(d) < vec3f(1e-20));
    var stack: array<u32, 64>;
    stack[0] = 0u;
    var sp = 1u;
    loop {
        if sp == 0u { break; }
        sp -= 1u;
        let n = nodes[stack[sp]];
        if !ray_box(p, inv_d, n.min, n.max, -best.dist, best.dist) { continue; }
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
            let q = cross(d, e2);
            let det = dot(e1, q);
            if abs(det) < 1e-20 { continue; }
            let inv = 1.0 / det;
            let s = p - tri.v0.xyz;
            let u = dot(s, q) * inv;
            if u < 0.0 || u > 1.0 { continue; }
            let r = cross(s, e1);
            let v = dot(d, r) * inv;
            if v < 0.0 || u + v > 1.0 { continue; }
            let t = dot(e2, r) * inv;
            if abs(t) < best.dist {
                best = Found(abs(t), i, u, v);
            }
        }
    }
    return best;
}

@compute @workgroup_size(8, 8)
fn locate(@builtin(global_invocation_id) id: vec3u) {
    let y = lp.row_start + id.y;
    if id.x >= lp.res || id.y >= lp.row_count || y >= lp.res { return; }
    let xy = vec2i(i32(id.x), i32(y));
    let pos = textureLoad(pos_map, xy, i32(lp.layer), 0);
    // Islands and padding only (alpha 1 / 0.5, docs/reference/mesh-maps.md#padding).
    if pos.a < 0.3 {
        textureStore(hits, xy, vec4f(0.0));
        return;
    }
    atomicAdd(&counts[0], 1u);
    // Stored as n * 0.5 + 0.5 (shaders/uv_raster.wgsl).
    let nrm = textureLoad(nrm_map, xy, i32(lp.layer), 0).xyz * 2.0 - 1.0;
    let d = select(vec3f(0.0, 0.0, 1.0), normalize(nrm), dot(nrm, nrm) > 1e-12);
    let f = nearest(pos.xyz, d, lp.max_dist);
    if f.tri == 0xffffffffu {
        atomicAdd(&counts[1], 1u);
        textureStore(hits, xy, vec4f(0.0));
        return;
    }
    let a = tri_uv[f.tri * 2u];
    let b = tri_uv[f.tri * 2u + 1u];
    let uv0 = a.xy;
    let uv1 = a.zw;
    let uv2 = b.xy;
    let uv = uv0 * (1.0 - f.b1 - f.b2) + uv1 * f.b1 + uv2 * f.b2;
    // The tile of the triangle (its UV center), so points on a tile border stay in it.
    let tile = floor((uv0 + uv1 + uv2) / 3.0);
    let udim = i32(tile.x) + 10 * i32(tile.y);
    var layer = -1;
    if tile.x >= 0.0 && tile.x < 10.0 && tile.y >= 0.0 && udim < 100 {
        layer = tile_layer[udim];
    }
    if layer < 0 {
        atomicAdd(&counts[1], 1u);
        textureStore(hits, xy, vec4f(0.0));
        return;
    }
    // Texel rows run top to bottom (shaders/uv_raster.wgsl), V up.
    let t = clamp(uv - tile, vec2f(0.0), vec2f(0.99999));
    let res = f32(lp.res);
    let px = vec2f(floor(t.x * res), floor((1.0 - t.y) * res));
    textureStore(hits, xy, vec4f(f32(layer), min(px, vec2f(res - 1.0)), 1.0));
}

// ---- gather (compiled as its own module)

struct GatherParams {
    res: u32,
    src_base: u32,   // slot * old tile count
    dst_layer: u32,  // slot * new tile count + new tile
    _pad: u32,
};

@group(0) @binding(0) var<uniform> gp: GatherParams;
@group(0) @binding(1) var hit_map: texture_2d<f32>;
@group(0) @binding(2) var src: texture_2d_array<f32>;
@group(0) @binding(3) var dst: texture_storage_2d_array<rgba16float, write>;

@compute @workgroup_size(8, 8)
fn gather(@builtin(global_invocation_id) id: vec3u) {
    if id.x >= gp.res || id.y >= gp.res { return; }
    let xy = vec2i(id.xy);
    let h = textureLoad(hit_map, xy, 0);
    var v = vec4f(0.0);
    if h.a > 0.0 {
        v = textureLoad(src, vec2i(h.yz), i32(gp.src_base) + i32(h.x), 0);
    }
    textureStore(dst, xy, i32(gp.dst_layer), v);
}
