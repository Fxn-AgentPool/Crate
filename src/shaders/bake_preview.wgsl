// Bake preview: the high poly (opaque, light gray) and the outer cage (translucent blue, with
// its edges), lit from the camera. See docs/reference/mesh-maps.md#bake-preview.

struct Frame {
    view_proj: mat4x4f,
    eye: vec4f,
    bbox_min: vec4f,
    bbox_size: vec4f,
    viewport: vec4f,
    mode: vec4u,
    light: vec4f,
};

@group(0) @binding(0) var<uniform> frame: Frame;

struct VsIn {
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
};

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
};

fn transform(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = frame.view_proj * vec4f(v.pos, 1.0);
    o.pos = v.pos;
    o.normal = v.normal;
    return o;
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    return transform(v);
}

// Headlight: brightest where the surface faces the camera, either side.
fn lit(in: VsOut, albedo: vec3f) -> vec3f {
    let n = normalize(in.normal);
    let v = normalize(frame.eye.xyz - in.pos);
    return albedo * (0.2 + 0.8 * abs(dot(n, v)));
}

@fragment
fn fs_high(in: VsOut) -> @location(0) vec4f {
    return vec4f(lit(in, vec3f(0.6)), 1.0);
}

const CAGE_ALPHA: f32 = 0.25;

@fragment
fn fs_cage(in: VsOut) -> @location(0) vec4f {
    return vec4f(lit(in, vec3f(0.15, 0.4, 1.0)), CAGE_ALPHA);
}

@vertex
fn vs_line(v: VsIn) -> VsOut {
    var o = transform(v);
    o.clip.z -= 2e-4 * o.clip.w;
    return o;
}

@fragment
fn fs_line(in: VsOut) -> @location(0) vec4f {
    return vec4f(0.3, 0.6, 1.0, 0.8);
}
