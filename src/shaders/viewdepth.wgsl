// Linear view depth (clip w) of the mesh from the bake camera, for paint occlusion.

struct Params {
    view_proj: mat4x4f,
};

@group(0) @binding(0) var<uniform> params: Params;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) depth: f32,
};

@vertex
fn vs_main(@location(0) pos: vec3f) -> VsOut {
    var o: VsOut;
    o.clip = params.view_proj * vec4f(pos, 1.0);
    o.depth = o.clip.w;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    return vec4f(in.depth, 0.0, 0.0, 1.0);
}
