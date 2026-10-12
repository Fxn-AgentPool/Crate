// Rasterize curves (docs/reference/curves.md#rasterize): composites a node's curve slot over its
// freehand paint for one UDIM tile. Both are premultiplied with coverage; `base` and `curves` are
// copies (one layer per tile), `out` the node's paint slot. Changed pages are marked in `dirty`.

struct Params {
    // x: tile layer, y: output layer (slot * tiles + tile), z: 1 = mask target, w: pages per row
    dest: vec4u,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var base0: texture_2d_array<f32>;
@group(0) @binding(2) var base1: texture_2d_array<f32>;
@group(0) @binding(3) var base2: texture_2d_array<f32>;
@group(0) @binding(4) var base3: texture_2d_array<f32>;
@group(0) @binding(5) var base4: texture_2d_array<f32>;
@group(0) @binding(6) var top0: texture_2d_array<f32>;
@group(0) @binding(7) var top1: texture_2d_array<f32>;
@group(0) @binding(8) var top2: texture_2d_array<f32>;
@group(0) @binding(9) var top3: texture_2d_array<f32>;
@group(0) @binding(10) var top4: texture_2d_array<f32>;
@group(0) @binding(11) var out0: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(12) var out1: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(13) var out2: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(14) var out3: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(15) var out4: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(16) var<storage, read_write> dirty: array<atomic<u32>>;

// Premultiplied top over premultiplied below, with their coverages in w.
fn over(below: vec4f, top: vec4f) -> vec4f {
    return top + below * (1.0 - top.a);
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    let res = textureDimensions(out0).x;
    if gid.x >= res || gid.y >= res { return; }
    let xy = gid.xy;
    let tl = i32(params.dest.x);
    let ol = i32(params.dest.y);
    let t0 = textureLoad(top0, xy, tl, 0);
    var changed = t0.a > 0.0;
    textureStore(out0, xy, ol, over(textureLoad(base0, xy, tl, 0), t0));
    if params.dest.z == 0u {
        let t1 = textureLoad(top1, xy, tl, 0);
        let t2 = textureLoad(top2, xy, tl, 0);
        let t3 = textureLoad(top3, xy, tl, 0);
        let t4 = textureLoad(top4, xy, tl, 0);
        let b3 = textureLoad(base3, xy, tl, 0);
        let b4 = textureLoad(base4, xy, tl, 0);
        textureStore(out1, xy, ol, over(textureLoad(base1, xy, tl, 0), t1));
        textureStore(out2, xy, ol, over(textureLoad(base2, xy, tl, 0), t2));
        // Scalars: values in plane 3, their coverages in plane 4.
        textureStore(out3, xy, ol, t3 + b3 * (1.0 - t4));
        textureStore(out4, xy, ol, t4 + b4 * (1.0 - t4));
        changed = changed || t1.a > 0.0 || t2.a > 0.0 || any(t4 > vec4f(0.0));
    }
    if changed {
        let page = (xy.y / 128u) * params.dest.w + xy.x / 128u;
        let bit = page + params.dest.x * params.dest.w * params.dest.w;
        atomicOr(&dirty[bit / 32u], 1u << (bit % 32u));
    }
}
