// Rasterizes the mesh in UV space for one UDIM tile, writing object-space position,
// normal and material ID per texel. Texture row 0 is the top of the tile (v = 1).

struct Params {
    bbox_min: vec4f,
    bbox_size: vec4f,
    // x, y: tile offset (floor of uv)
    tile: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;

struct VsIn {
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
    @location(2) tangent: vec4f,
    @location(3) uv: vec2f,
    @location(4) material: u32,
};

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) pos: vec3f,
    @location(1) normal: vec3f,
    @location(2) @interpolate(flat) material: u32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    let t = v.uv - params.tile.xy;
    o.clip = vec4f(t * 2.0 - 1.0, 0.5, 1.0);
    o.pos = v.pos;
    o.normal = v.normal;
    o.material = v.material;
    return o;
}

struct FsOut {
    @location(0) pos: vec4f,
    @location(1) normal: vec4f,
    @location(2) id: vec4f,
};

fn id_color(i: u32) -> vec3f {
    // Well-separated hues, then varying value for many materials.
    let hue = fract(f32(i) * 0.618034 + 0.05);
    let k = vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3f(hue) + k) * 6.0 - 3.0);
    let val = 1.0 - 0.25 * f32((i / 7u) % 3u);
    return clamp(p - 1.0, vec3f(0.0), vec3f(1.0)) * val;
}

@fragment
fn fs_main(in: VsOut) -> FsOut {
    var o: FsOut;
    // Position stays in object space (32-bit); the normalized form is derived when sampled.
    o.pos = vec4f(in.pos, 1.0);
    o.normal = vec4f(normalize(in.normal) * 0.5 + 0.5, 1.0);
    o.id = vec4f(id_color(in.material), 1.0);
    return o;
}
