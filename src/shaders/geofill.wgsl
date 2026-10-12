// Geo Fill: composites the brush values over one UDIM tile of a paint target, where the texel or
// the island texel its padding comes from belongs to the element (docs/reference/brushes-tools-stencils.md#geo-fill).
//
// Reads the target from `base` (a copy of the target slot, one layer per tile) and writes the
// result into `out`, marking changed pages in `dirty`, as paintbake.wgsl does.

struct Params {
    // x: tile layer, y: output layer (slot * tiles + tile), z: 1 = mask target, w: pages per row
    dest: vec4u,
    // x: opacity, y: mask value, z: channel mask (as float)
    opts: vec4f,
    values: array<vec4f, 7>,
};

const NONE: u32 = 0xffffffffu;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var mm_pos: texture_2d_array<f32>;
@group(0) @binding(2) var mm_src: texture_2d_array<u32>;
@group(0) @binding(3) var element: texture_2d<f32>;
@group(0) @binding(4) var base0: texture_2d_array<f32>;
@group(0) @binding(5) var base1: texture_2d_array<f32>;
@group(0) @binding(6) var base2: texture_2d_array<f32>;
@group(0) @binding(7) var base3: texture_2d_array<f32>;
@group(0) @binding(8) var base4: texture_2d_array<f32>;
@group(0) @binding(9) var out0: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(10) var out1: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(11) var out2: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(12) var out3: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(13) var out4: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(14) var<storage, read_write> dirty: array<atomic<u32>>;

fn over(dst: vec4f, value: vec3f, a: f32) -> vec4f {
    return vec4f(dst.rgb * (1.0 - a) + value * a, dst.a + a * (1.0 - dst.a));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    let res = textureDimensions(out0).x;
    if gid.x >= res || gid.y >= res { return; }
    let xy = gid.xy;
    let tl = i32(params.dest.x);
    let ol = i32(params.dest.y);
    let mask_target = params.dest.z == 1u;
    let mres = textureDimensions(mm_pos).x;
    let mxy = min(vec2u(vec2f(xy) * f32(mres) / f32(res)), vec2u(mres - 1u));

    var o0 = textureLoad(base0, xy, tl, 0);
    var o1: vec4f;
    var o2: vec4f;
    var o3: vec4f;
    var o4: vec4f;
    if !mask_target {
        o1 = textureLoad(base1, xy, tl, 0);
        o2 = textureLoad(base2, xy, tl, 0);
        o3 = textureLoad(base3, xy, tl, 0);
        o4 = textureLoad(base4, xy, tl, 0);
    }
    // Islands and padding: the element owns a padding texel when it owns its island texel.
    var inside = false;
    if textureLoad(mm_pos, mxy, tl, 0).a > 0.3 {
        let s = textureLoad(mm_src, mxy, tl, 0).r;
        if s != NONE {
            inside = textureLoad(element, vec2u(s % mres, s / mres), 0).r > 0.5;
        }
    }
    let a = params.opts.x;
    if inside && a > 0.0 {
        if mask_target {
            o0 = over(o0, vec3f(params.opts.y), a);
        } else {
            let m = u32(params.opts.z);
            if (m & 1u) != 0u { o0 = over(o0, params.values[0].rgb, a); }
            if (m & 32u) != 0u { o1 = over(o1, params.values[5].rgb, a); }
            if (m & 16u) != 0u { o2 = over(o2, params.values[4].rgb, a); }
            let chans = array<u32, 4>(1u, 2u, 3u, 6u);
            for (var k = 0; k < 4; k++) {
                let c = chans[k];
                if (m & (1u << c)) != 0u {
                    o3[k] = o3[k] * (1.0 - a) + params.values[c].x * a;
                    o4[k] = o4[k] + a * (1.0 - o4[k]);
                }
            }
        }
        let page = (xy.y / 128u) * params.dest.w + xy.x / 128u;
        let bit = page + params.dest.x * params.dest.w * params.dest.w;
        atomicOr(&dirty[bit / 32u], 1u << (bit % 32u));
    }
    textureStore(out0, xy, ol, o0);
    if !mask_target {
        textureStore(out1, xy, ol, o1);
        textureStore(out2, xy, ol, o2);
        textureStore(out3, xy, ol, o3);
        textureStore(out4, xy, ol, o4);
    }
}
