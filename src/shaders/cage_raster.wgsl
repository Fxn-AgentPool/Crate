// Rasterizes the auto cage in UV space for one UDIM tile: frontal and rear distance per texel,
// interpolated from the low poly's vertices, for highbake.wgsl. Same coverage as uv_raster.wgsl.
// See docs/reference/mesh-maps.md#auto-cage.

struct Params {
    bbox_min: vec4f,
    bbox_size: vec4f,
    // x, y: tile offset (floor of uv)
    tile: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) cage: vec2f,
};

// xy: uv, z: frontal, w: rear (world units)
@vertex
fn vs_main(@location(0) v: vec4f) -> VsOut {
    var o: VsOut;
    let t = v.xy - params.tile.xy;
    o.clip = vec4f(t * 2.0 - 1.0, 0.5, 1.0);
    o.cage = v.zw;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    return vec4f(in.cage, 0.0, 1.0);
}
