// Projects the paint buffer through the bake camera into one UDIM tile of a paint target.
// See docs/reference/paint-buffer.md "Bake algorithm".
//
// Reads the target's contents from `base` (a copy of the target slot, one layer per tile,
// taken at stroke start) and writes the result into `out`, so Live mode can re-bake the
// whole stroke every frame.

struct Params {
    view_proj: mat4x4f,
    // w: world size of one depth-buffer pixel per unit of depth
    eye: vec4f,
    // x, y: viewport size (screen px); z: buffer scale; w: depth tolerance (relative)
    screen: vec4f,
    // Inverse buffer transform: screen px -> untransformed screen px (rows of a 2x3 affine).
    inv_row0: vec4f,
    inv_row1: vec4f,
    // x: cos(falloff start), y: cos(falloff end), z: backface culling, w: project through
    proj: vec4f,
    // x: tile layer in mesh maps, y: target base layer (slot * tiles + tile), z: 1 = mask target,
    // w: pages per row
    dest: vec4u,
    // UV view (docs/reference/uv-view.md#painting): x = screen px per UV unit, y, z = UDIM
    // coordinate at the screen's center, w = 1 in the UV view.
    uv: vec4f,
    // x, y: this tile's UDIM offset
    uv_tile: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var mm_pos: texture_2d_array<f32>;
@group(0) @binding(2) var mm_nrm: texture_2d_array<f32>;
@group(0) @binding(3) var depth: texture_2d<f32>;
@group(0) @binding(4) var buf0: texture_2d<f32>;
@group(0) @binding(5) var buf1: texture_2d<f32>;
@group(0) @binding(6) var buf2: texture_2d<f32>;
@group(0) @binding(7) var buf3: texture_2d<f32>;
@group(0) @binding(8) var buf4: texture_2d<f32>;
@group(0) @binding(9) var buf_erase: texture_2d<f32>;
@group(0) @binding(10) var samp: sampler;
@group(0) @binding(11) var base0: texture_2d_array<f32>;
@group(0) @binding(12) var base1: texture_2d_array<f32>;
@group(0) @binding(13) var base2: texture_2d_array<f32>;
@group(0) @binding(14) var base3: texture_2d_array<f32>;
@group(0) @binding(15) var base4: texture_2d_array<f32>;
@group(0) @binding(16) var out0: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(17) var out1: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(18) var out2: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(19) var out3: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(20) var out4: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(21) var<storage, read_write> dirty: array<atomic<u32>>;

fn over(dst: vec4f, src: vec4f, t: f32) -> vec4f {
    // src.rgb premultiplied by src.a; composite with weight t.
    let a = src.a * t;
    let value = src.rgb / max(src.a, 1e-5);
    return vec4f(dst.rgb * (1.0 - a) + value * a, dst.a + a * (1.0 - dst.a));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    let res = textureDimensions(out0).x;
    if gid.x >= res || gid.y >= res { return; }
    let xy = gid.xy;
    let tl = i32(params.dest.x);
    let ol = i32(params.dest.y);
    let mres = textureDimensions(mm_pos).x;
    // Mesh maps may have a different resolution than the paint target.
    let mxy = min(vec2u(vec2f(xy) * f32(mres) / f32(res)), vec2u(mres - 1u));

    var b0 = textureLoad(base0, xy, tl, 0);
    var b1: vec4f;
    var b2: vec4f;
    var b3: vec4f;
    var b4: vec4f;
    let mask_target = params.dest.z == 1u;
    if !mask_target {
        b1 = textureLoad(base1, xy, tl, 0);
        b2 = textureLoad(base2, xy, tl, 0);
        b3 = textureLoad(base3, xy, tl, 0);
        b4 = textureLoad(base4, xy, tl, 0);
    }

    var strength = 0.0;
    let pos = textureLoad(mm_pos, mxy, tl, 0);
    var suv = vec2f(0.0);
    if pos.a > 0.0 && params.uv.w > 0.5 {
        // The texel's UDIM coordinate (row 0 is the top of the tile, V up), placed by the view.
        let res_f = f32(res);
        let uv = params.uv_tile.xy + vec2f((f32(xy.x) + 0.5) / res_f, 1.0 - (f32(xy.y) + 0.5) / res_f);
        let d = (uv - params.uv.yz) * params.uv.x;
        let screen = vec2f(params.screen.x * 0.5 + d.x, params.screen.y * 0.5 - d.y);
        let sp = vec2f(dot(params.inv_row0.xyz, vec3f(screen, 1.0)), dot(params.inv_row1.xyz, vec3f(screen, 1.0)));
        suv = sp / params.screen.xy;
        let inside = all(suv >= vec2f(0.0)) && all(suv <= vec2f(1.0)) && all(screen >= vec2f(0.0)) && all(screen <= params.screen.xy);
        if inside {
            strength = 1.0;
        }
    } else if pos.a > 0.0 {
        let clip = params.view_proj * vec4f(pos.xyz, 1.0);
        if clip.w > 0.0 {
            let ndc = clip.xyz / clip.w;
            let screen = vec2f((ndc.x * 0.5 + 0.5) * params.screen.x, (0.5 - ndc.y * 0.5) * params.screen.y);
            let to_eye = params.eye.xyz - pos.xyz;
            let n = normalize(textureLoad(mm_nrm, mxy, tl, 0).xyz * 2.0 - 1.0);
            let facing = dot(n, normalize(to_eye));
            // Buffer coordinates after the inverse buffer transform.
            let sp = vec2f(dot(params.inv_row0.xyz, vec3f(screen, 1.0)), dot(params.inv_row1.xyz, vec3f(screen, 1.0)));
            suv = sp / params.screen.xy;
            let inside = all(suv >= vec2f(0.0)) && all(suv <= vec2f(1.0)) && all(screen >= vec2f(0.0)) && all(screen <= params.screen.xy);
            if inside {
                // Angle falloff: full strength above cos(start), zero below cos(end).
                strength = smoothstep(params.proj.y, params.proj.x, facing);
                if params.proj.z > 0.5 && facing <= 0.0 { strength = 0.0; }
                if params.proj.w < 0.5 && strength > 0.0 {
                    // Occlusion: compare view depth with the bake camera's depth pass.
                    let dims = vec2f(textureDimensions(depth));
                    let dp = vec2i(clamp(screen / params.screen.xy * dims, vec2f(0.0), dims - 1.0));
                    let scene_depth = textureLoad(depth, dp, 0).r;
                    let clip_depth = clip.w;
                    // Slope-scaled bias: at grazing angles one depth pixel spans a larger
                    // depth range than the texel's own position error.
                    let footprint = clip_depth * params.eye.w;
                    let slope = footprint * 1.5 / max(facing, 0.08);
                    if clip_depth > scene_depth * (1.0 + params.screen.w) + slope + 1e-4 {
                        strength = 0.0;
                    }
                }
            }
        }
    }

    var o0 = b0;
    var o1 = b1;
    var o2 = b2;
    var o3 = b3;
    var o4 = b4;
    if strength > 0.0 {
        let p0 = textureSampleLevel(buf0, samp, suv, 0.0);
        let e = textureSampleLevel(buf_erase, samp, suv, 0.0).r * strength;
        o0 = over(b0, p0, strength) * (1.0 - e);
        if !mask_target {
            o1 = over(b1, textureSampleLevel(buf1, samp, suv, 0.0), strength) * (1.0 - e);
            let n2 = textureSampleLevel(buf2, samp, suv, 0.0);
            o2 = over(b2, n2, strength) * (1.0 - e);
            let s3 = textureSampleLevel(buf3, samp, suv, 0.0);
            let s4 = textureSampleLevel(buf4, samp, suv, 0.0);
            for (var k = 0; k < 4; k++) {
                let a = s4[k] * strength;
                let v = s3[k] / max(s4[k], 1e-5);
                o3[k] = (b3[k] * (1.0 - a) + v * a) * (1.0 - e);
                o4[k] = (b4[k] + a * (1.0 - b4[k])) * (1.0 - e);
            }
        }
        let changed = p0.a > 0.0 || e > 0.0 || (!mask_target && (o1.a != b1.a || o2.a != b2.a || any(o4 != b4)));
        if changed {
            let page = (xy.y / 128u) * params.dest.w + xy.x / 128u;
            let bit = page + params.dest.x * params.dest.w * params.dest.w;
            atomicOr(&dirty[bit / 32u], 1u << (bit % 32u));
        }
    }
    textureStore(out0, xy, ol, o0);
    if !mask_target {
        textureStore(out1, xy, ol, o1);
        textureStore(out2, xy, ol, o2);
        textureStore(out3, xy, ol, o3);
        textureStore(out4, xy, ol, o4);
    }
}
