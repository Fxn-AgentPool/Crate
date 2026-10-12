// Evaluates the material graph in UV space for one UDIM tile (export and caches).
// Outputs: 0 base color + opacity, 1 roughness, metallic, height, coverage,
//          2 tangent-space normal (-1..1) + mesh AO, 3 emissive.

struct Params {
    // x, y: tile offset (floor of uv); z: tile layer; w: tile resolution
    tile: vec4f,
};

@group(0) @binding(0) var<uniform> eval_params: Params;

//{{SURFACE}}

//{{SCENE}}

//{{MATERIAL}}

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
    @location(2) uv: vec2f,
    @location(3) @interpolate(flat) material: u32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    let t = v.uv - eval_params.tile.xy;
    o.clip = vec4f(t * 2.0 - 1.0, 0.5, 1.0);
    o.pos = v.pos;
    o.normal = v.normal;
    o.uv = v.uv;
    o.material = v.material;
    return o;
}

struct FsOut {
    @location(0) base: vec4f,
    @location(1) scalars: vec4f,
    @location(2) normal: vec4f,
    @location(3) emissive: vec4f,
};

@fragment
fn fs_main(in: VsOut) -> FsOut {
    var s: Surface;
    s.pos = in.pos;
    s.pos_norm = (in.pos - scene.bbox_min.xyz) / scene.bbox_size.xyz;
    s.normal = normalize(in.normal);
    s.uv = in.uv;
    s.tile_uv = clamp(in.uv - eval_params.tile.xy, vec2f(0.0), vec2f(0.99999));
    s.texel_uv = s.tile_uv;
    s.layer = i32(eval_params.tile.z);
    s.material = in.material;
    let m = eval_material(s);
    var o: FsOut;
    o.base = vec4f(m.base_color, m.opacity);
    o.scalars = vec4f(m.roughness, m.metallic, m.height, 1.0);
    o.normal = vec4f(m.normal, mesh_ao(tile_tex(s.tile_uv), s.layer));
    o.emissive = vec4f(m.emissive, 0.0);
    return o;
}

// Texels between islands that 2D paint covers (docs/reference/2d-paint.md#export): drawn over the
// whole tile; island texels and texels without 2D paint are left to the raster pass and padding.
// Scalars' alpha 0.4 marks them for export_finish.wgsl `overlay`.
@vertex
fn vs_full(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(p * 2.0 - 1.0, 0.5, 1.0);
}

@fragment
fn fs_gap(@builtin(position) frag: vec4f) -> FsOut {
    let res = eval_params.tile.w;
    let texel_uv = vec2f(frag.x / res, 1.0 - frag.y / res);
    var s: Surface;
    let a = gap_surface(texel_uv, eval_params.tile.xy, i32(eval_params.tile.z), &s);
    if a > 0.75 || a <= 0.0 || twod_coverage(s) <= 0.0 { discard; }
    let m = eval_material(s);
    var o: FsOut;
    o.base = vec4f(m.base_color, m.opacity);
    o.scalars = vec4f(m.roughness, m.metallic, m.height, 0.4);
    o.normal = vec4f(m.normal, mesh_ao(tile_tex(s.tile_uv), s.layer));
    o.emissive = vec4f(m.emissive, 0.0);
    return o;
}
