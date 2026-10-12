// Small preview of one paint layer tile: premultiplied base color over a checkerboard, with the
// node's curves over it (docs/reference/curves.md#order).

struct Params {
    // x: array layer of the paint (-1: none), y: 1 = mask pool (grayscale value), z: thumbnail size,
    // w: array layer of the curves (-1: none)
    info: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var plane: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    o.uv = uv;
    return o;
}

fn srgb_encode(c: vec3f) -> vec3f {
    let x = clamp(c, vec3f(0.0), vec3f(1.0));
    return select(1.055 * pow(x, vec3f(1.0 / 2.4)) - 0.055, x * 12.92, x <= vec3f(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    // Average a 4 x 4 grid of samples across this thumbnail pixel.
    let px = 1.0 / params.info.z;
    var paint = vec4f(0.0);
    var curves = vec4f(0.0);
    for (var y = 0; y < 4; y++) {
        for (var x = 0; x < 4; x++) {
            let o = (vec2f(f32(x), f32(y)) + 0.5) / 4.0 - 0.5;
            if params.info.x >= 0.0 {
                paint += textureSampleLevel(plane, samp, in.uv + o * px, i32(params.info.x), 0.0);
            }
            if params.info.w >= 0.0 {
                curves += textureSampleLevel(plane, samp, in.uv + o * px, i32(params.info.w), 0.0);
            }
        }
    }
    // Both premultiplied: the curves over the paint.
    let acc = (curves + paint * (1.0 - curves.a / 16.0)) / 16.0;
    let cell = floor(in.uv * 6.0);
    let checker = select(0.12, 0.2, (i32(cell.x + cell.y) % 2) == 0);
    var c: vec3f;
    if params.info.y > 0.5 {
        // Mask paint: r = premultiplied value, a = coverage.
        c = mix(vec3f(checker), vec3f(acc.r / max(acc.a, 1e-4)), acc.a);
        c = srgb_encode(c);
    } else {
        let value = acc.rgb / max(acc.a, 1e-4);
        c = mix(srgb_encode(vec3f(checker)), srgb_encode(value), acc.a);
    }
    return vec4f(c, 1.0);
}
