// Library thumbnail of a procedural texture preset (procedural.rs). Prepended with noise.wgsl.
// The thumbnail is a plane: (u, v, 0.5) stands in for the normalized surface position the
// material graph evaluates the node at, with the preset's other settings at their defaults.

struct Params {
    // x: node (0 noise, 1 cellular, 2 gradient), y: noise type or cellular output index, z: scale
    info: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;

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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    // v grows upwards, like a surface position.
    let p = vec3f(in.uv.x, 1.0 - in.uv.y, 0.5);
    let variant = u32(params.info.y);
    let scale = params.info.z;
    var v = p.y;
    switch u32(params.info.x) {
        case 0u: {
            v = fbm(p * scale, variant, 5.0, 2.0, 0.5, 0.0);
        }
        case 1u: {
            // The same mapping of F1 / F2 / F2-F1 as the compiled Cellular node (compile.rs).
            let c = cellular(p * scale, 1.0, false, 0.0);
            if variant == 1u {
                v = clamp(c.y * 0.75, 0.0, 1.0);
            } else if variant == 2u {
                v = clamp((c.y - c.x) * 1.5, 0.0, 1.0);
            } else {
                v = clamp(c.x, 0.0, 1.0);
            }
        }
        default: {}
    }
    return vec4f(vec3f(v), 1.0);
}
