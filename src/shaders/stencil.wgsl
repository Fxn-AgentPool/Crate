// Draws the active stencil over the viewport at its display opacity.

struct Params {
    // Screen px -> stencil uv (rows of a 2x3 affine).
    row0: vec4f,
    row1: vec4f,
    // x: display opacity, y: image layer, z: 1 = tile
    info: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var images: texture_2d_array<f32>;
@group(0) @binding(2) var samp: sampler;

struct VsOut {
    @builtin(position) pos: vec4f,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let q = vec3f(in.pos.xy, 1.0);
    let uv = vec2f(dot(params.row0.xyz, q), dot(params.row1.xyz, q));
    if params.info.z < 0.5 && (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0))) { discard; }
    let c = textureSampleLevel(images, samp, uv, i32(params.info.y), 0.0);
    // Stencil images are already display-referred (sRGB), like the output.
    return vec4f(c.rgb, c.a * params.info.x);
}
