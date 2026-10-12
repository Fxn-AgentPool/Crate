// Rasterizes a Geo Fill element's triangles in UV space for one UDIM tile, as uv_raster.wgsl
// does, so a texel belongs to the element exactly when its center is inside one of them.
// See docs/reference/brushes-tools-stencils.md#geo-fill.

struct Params {
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
    @location(5) group: u32,
};

@vertex
fn vs_main(v: VsIn) -> @builtin(position) vec4f {
    let t = v.uv - params.tile.xy;
    return vec4f(t * 2.0 - 1.0, 0.5, 1.0);
}

@fragment
fn fs_main() -> @location(0) vec4f {
    return vec4f(1.0);
}
