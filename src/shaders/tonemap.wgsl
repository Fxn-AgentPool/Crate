// Resolves the HDR viewport into the 8-bit, gamma-encoded image egui displays.

struct Params {
    // x: 1 = apply filmic tone mapping (material view), 0 = clamp (raw channel views)
    flags: vec4u,
    background_top: vec4f,
    background_bottom: vec4f,
};

@group(0) @binding(0) var hdr: texture_2d<f32>;
@group(0) @binding(1) var depth: texture_depth_multisampled_2d;
@group(0) @binding(2) var<uniform> params: Params;

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

fn aces(x: vec3f) -> vec3f {
    // Narkowicz 2015 fit.
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3f(0.0), vec3f(1.0));
}

fn srgb_encode(c: vec3f) -> vec3f {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3f(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3f(0.0031308));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let p = vec2i(in.pos.xy);
    // Coverage of the mesh in this pixel, from the multisampled depth.
    var covered = 0.0;
    for (var i = 0; i < 4; i++) {
        covered += select(0.0, 0.25, textureLoad(depth, p, i) < 1.0);
    }
    let background = mix(params.background_top.rgb, params.background_bottom.rgb, in.uv.y);
    if covered == 0.0 {
        // Background gradient, already display-referred.
        return vec4f(background, 1.0);
    }
    // The resolved HDR color averages mesh samples with black clear samples; undo that.
    var c = textureLoad(hdr, p, 0).rgb / covered;
    if params.flags.x == 1u {
        c = aces(c * 0.8);
    } else {
        c = clamp(c, vec3f(0.0), vec3f(1.0));
    }
    return vec4f(mix(background, srgb_encode(c), covered), 1.0);
}
