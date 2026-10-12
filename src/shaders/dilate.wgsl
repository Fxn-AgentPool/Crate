// Padding: computes, for every texel, the index of the nearest texel covered by the mesh (jump
// flooding), then copies raster data from that texel. See docs/reference/mesh-maps.md#padding.

struct Params {
    res: u32,
    layer: u32,
    // Jump flooding offset of this step, in texels.
    step: u32,
    // Texels within this distance of an island are padding (alpha 0.5); farther ones 0.25.
    padding: u32,
};

const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> params: Params;

// ---------------------------------------------------------------- init
@group(0) @binding(1) var raster_pos: texture_2d<f32>;
@group(0) @binding(2) var src_out: texture_storage_2d<r32uint, write>;

@compute @workgroup_size(8, 8)
fn init(@builtin(global_invocation_id) id: vec3u) {
    if id.x >= params.res || id.y >= params.res { return; }
    let covered = textureLoad(raster_pos, id.xy, 0).a > 0.5;
    textureStore(src_out, id.xy, vec4u(select(NONE, id.y * params.res + id.x, covered)));
}

// ---------------------------------------------------------------- one jump flooding step
@group(0) @binding(3) var src_in: texture_2d<u32>;

@compute @workgroup_size(8, 8)
fn step(@builtin(global_invocation_id) id: vec3u) {
    if id.x >= params.res || id.y >= params.res { return; }
    let me = vec2f(id.xy);
    var best = textureLoad(src_in, id.xy, 0).r;
    var best_d = 1e30;
    if best != NONE {
        best_d = distance(vec2f(f32(best % params.res), f32(best / params.res)), me);
    }
    let k = i32(params.step);
    for (var dy = -1; dy <= 1; dy++) {
        for (var dx = -1; dx <= 1; dx++) {
            let q = vec2i(id.xy) + vec2i(dx, dy) * k;
            if q.x < 0 || q.y < 0 || q.x >= i32(params.res) || q.y >= i32(params.res) { continue; }
            let s = textureLoad(src_in, vec2u(q), 0).r;
            if s == NONE { continue; }
            let d = distance(vec2f(f32(s % params.res), f32(s / params.res)), me);
            if d < best_d {
                best_d = d;
                best = s;
            }
        }
    }
    textureStore(src_out, id.xy, vec4u(best));
}

// ---------------------------------------------------------------- gather
@group(0) @binding(4) var raster_nrm: texture_2d<f32>;
@group(0) @binding(5) var raster_id: texture_2d<f32>;
@group(0) @binding(6) var out_pos: texture_storage_2d_array<rgba32float, write>;
@group(0) @binding(7) var out_nrm: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(8) var out_id: texture_storage_2d_array<rgba8unorm, write>;
@group(0) @binding(9) var out_src: texture_storage_2d_array<r32uint, write>;

// Alpha: 1 = covered by the mesh, 0.5 = padding, 0.25 = farther between islands (only lends its
// surface to the material there), 0 = empty (a tile without geometry).
@compute @workgroup_size(8, 8)
fn gather(@builtin(global_invocation_id) id: vec3u) {
    if id.x >= params.res || id.y >= params.res { return; }
    let s = textureLoad(src_in, id.xy, 0).r;
    var pos = vec4f(0.0);
    var nrm = vec4f(0.5, 0.5, 1.0, 0.0);
    var mid = vec4f(0.0);
    if s != NONE {
        let sp = vec2u(s % params.res, s / params.res);
        let d = distance(vec2f(sp), vec2f(id.xy));
        let a = select(select(0.25, 0.5, d <= f32(params.padding) + 0.5), 1.0, all(sp == id.xy));
        pos = vec4f(textureLoad(raster_pos, sp, 0).xyz, a);
        nrm = vec4f(textureLoad(raster_nrm, sp, 0).xyz, a);
        mid = vec4f(textureLoad(raster_id, sp, 0).xyz, a);
    }
    textureStore(out_pos, id.xy, params.layer, pos);
    textureStore(out_nrm, id.xy, params.layer, nrm);
    textureStore(out_id, id.xy, params.layer, mid);
    textureStore(out_src, id.xy, params.layer, vec4u(s));
}
