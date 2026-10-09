// Evaluates one cache target in UV space for one tile.
// fs_value writes a Float/Color result (rgb = color, a = float);
// fs_mat writes a material in paint-plane layout (premultiplied, see scene.rs).

struct Params {
    // x, y: tile offset (floor of uv); z: tile layer
    tile: vec4f,
};

@group(0) @binding(0) var<uniform> eval_params: Params;

//{{SURFACE}}

//{{SCENE}}

//{{LIB}}

struct CacheOut {
    value: vec4f,
    mat: Mat,
};

//{{TARGET}}

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

fn surface(in: VsOut) -> Surface {
    var s: Surface;
    s.pos = in.pos;
    s.pos_norm = (in.pos - scene.bbox_min.xyz) / scene.bbox_size.xyz;
    s.normal = normalize(in.normal);
    s.uv = in.uv;
    s.tile_uv = clamp(in.uv - eval_params.tile.xy, vec2f(0.0), vec2f(0.99999));
    s.layer = i32(eval_params.tile.z);
    s.material = in.material;
    return s;
}

@fragment
fn fs_value(in: VsOut) -> @location(0) vec4f {
    return eval_target(surface(in)).value;
}

struct MatOut {
    @location(0) p0: vec4f,
    @location(1) p1: vec4f,
    @location(2) p2: vec4f,
    @location(3) p3: vec4f,
    @location(4) p4: vec4f,
};

@fragment
fn fs_mat(in: VsOut) -> MatOut {
    let m = eval_target(surface(in)).mat;
    var o: MatOut;
    o.p0 = vec4f(m.v[0] * m.a[0], m.a[0]);
    o.p1 = vec4f(m.v[5] * m.a[5], m.a[5]);
    o.p2 = vec4f(m.v[4] * m.a[4], m.a[4]);
    o.p3 = vec4f(m.v[1].x * m.a[1], m.v[2].x * m.a[2], m.v[3].x * m.a[3], m.v[6].x * m.a[6]);
    o.p4 = vec4f(m.a[1], m.a[2], m.a[3], m.a[6]);
    return o;
}
