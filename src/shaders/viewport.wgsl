// Viewport: shades the mesh with the compiled material graph, once per screen pixel.
// See docs/explanation/about-realtime-evaluation.md.

struct Frame {
    view_proj: mat4x4f,
    eye: vec4f,
    bbox_min: vec4f,
    bbox_size: vec4f,
    // x: width, y: height, z: exposure (EV), w: environment rotation (radians)
    viewport: vec4f,
    // x: display mode, y: channel / map index, z: flags, w: height scale (as bits)
    mode: vec4u,
    // x,y,z: key light direction (towards light), w: wireframe opacity
    light: vec4f,
};

const MODE_MATERIAL: u32 = 0u;
const MODE_CHANNEL: u32 = 1u;
const MODE_MESH_MAP: u32 = 2u;

const FLAG_TILES: u32 = 1u;
const FLAG_HDRI: u32 = 2u;
const FLAG_AO: u32 = 4u;
// The UV view: triangles drawn at their UVs, viewed along the surface normal.
const FLAG_UV: u32 = 8u;

fn uv_view() -> bool {
    return (frame.mode.z & FLAG_UV) != 0u;
}

// Where a vertex is drawn: its position, or (u, v, 0) in the UV view.
fn placed(v_pos: vec3f, v_uv: vec2f) -> vec4f {
    if uv_view() {
        return vec4f(v_uv, 0.0, 1.0);
    }
    return vec4f(v_pos, 1.0);
}

@group(0) @binding(0) var<uniform> frame: Frame;
// UDIM slot (floor(u) + 10 * floor(v)) -> texture array layer, 0xffffffff when unused.
@group(0) @binding(1) var<uniform> tile_map: array<vec4u, 25>;
// Equirectangular environment with a mip chain (used when FLAG_HDRI is set).
@group(0) @binding(2) var env_map: texture_2d<f32>;
@group(0) @binding(3) var env_samp: sampler;
// The environment's diffuse irradiance divided by pi, equirectangular (baked on load).
@group(0) @binding(4) var irradiance_map: texture_2d<f32>;

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
    @location(2) tangent: vec4f,
    @location(3) uv: vec2f,
    @location(4) @interpolate(flat) material: u32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.clip = frame.view_proj * placed(v.pos, v.uv);
    o.pos = v.pos;
    o.normal = v.normal;
    o.tangent = v.tangent;
    o.uv = v.uv;
    o.material = v.material;
    return o;
}

//{{SURFACE}}

fn tile_layer(uv: vec2f) -> i32 {
    let t = floor(uv);
    if t.x < 0.0 || t.y < 0.0 || t.x > 9.0 || t.y > 9.0 { return -1; }
    let slot = u32(t.x + 10.0 * t.y);
    let l = tile_map[slot / 4u][slot % 4u];
    if l == 0xffffffffu { return -1; }
    return i32(l);
}

//{{SCENE}}

//{{MATERIAL}}

// Values written here are shown as-is: decode so the sRGB output encodes them back.
fn show(v: vec3f) -> vec3f {
    return pow(clamp(v, vec3f(0.0), vec3f(1.0)), vec3f(2.2));
}

fn mesh_map_display(s: Surface) -> vec3f {
    let tc = tile_tex(s.tile_uv);
    switch frame.mode.y {
        case 0u: { return show(mesh_position(tc, s.layer)); }
        case 1u: { return show(mesh_world_normal(tc, s.layer)); }
        case 2u: { return show(vec3f(mesh_ao(tc, s.layer))); }
        case 3u: { return show(vec3f(mesh_curvature(tc, s.layer))); }
        case 4u: { return show(vec3f(mesh_thickness(tc, s.layer))); }
        case 6u: {
            // Texels that missed the high poly are red.
            let n = mesh_normal_map(tc, s.layer);
            if n.a < 0.5 { return show(vec3f(1.0, 0.05, 0.05)); }
            return show(n.xyz);
        }
        case 7u: { return show(vec3f(mesh_height(tc, s.layer))); }
        default: { return show(mesh_id(tc, s.layer)); }
    }
}

// ---------------------------------------------------------------- shading

const PI: f32 = 3.14159265;

fn rotate_y(d: vec3f, a: f32) -> vec3f {
    let s = sin(a);
    let c = cos(a);
    return vec3f(c * d.x + s * d.z, d.y, -s * d.x + c * d.z);
}

fn has_hdri() -> bool {
    return (frame.mode.z & FLAG_HDRI) != 0u;
}

// Environment-space direction -> equirectangular uv, with the environment rotation applied.
fn equirect_uv(dir_in: vec3f) -> vec2f {
    let d = normalize(rotate_y(dir_in, frame.viewport.w));
    return vec2f(atan2(d.x, -d.z) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

// Mip level i of env_map is GGX-prefiltered for roughness i / (levels - 1) (see env_prefilter.wgsl).
fn env_sample(dir_in: vec3f, roughness: f32) -> vec3f {
    let levels = f32(textureNumLevels(env_map));
    return textureSampleLevel(env_map, env_samp, equirect_uv(dir_in), roughness * (levels - 1.0)).rgb;
}

// Analytic studio environment: sky gradient, warm ground, soft key light.
fn env_radiance(dir_in: vec3f) -> vec3f {
    if has_hdri() { return env_sample(dir_in, 0.0); }
    let d = rotate_y(dir_in, frame.viewport.w);
    let zenith = vec3f(0.42, 0.55, 0.78);
    let horizon = vec3f(1.0, 0.98, 0.95);
    let ground = vec3f(0.22, 0.2, 0.18);
    let sky = mix(horizon, zenith, smoothstep(0.0, 0.7, d.y));
    var c = mix(ground, sky, smoothstep(-0.08, 0.04, d.y));
    // Large soft box above-front, reads well in reflections.
    let box_dir = normalize(vec3f(0.3, 0.75, 0.6));
    c += vec3f(3.0) * smoothstep(0.93, 0.97, dot(d, box_dir));
    return c;
}

fn env_irradiance(n: vec3f) -> vec3f {
    if has_hdri() { return textureSampleLevel(irradiance_map, env_samp, equirect_uv(n), 0.0).rgb; }
    let d = rotate_y(n, frame.viewport.w);
    let sky = vec3f(0.75, 0.8, 0.88);
    let ground = vec3f(0.3, 0.27, 0.24);
    return mix(ground, sky, d.y * 0.5 + 0.5);
}

fn env_brdf_approx(f0: vec3f, roughness: f32, n_dot_v: f32) -> vec3f {
    // Karis, "Physically Based Shading on Mobile".
    let c0 = vec4f(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4f(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * n_dot_v)) * r.x + r.y;
    let ab = vec2f(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

fn ggx_light(n: vec3f, v: vec3f, l: vec3f, albedo: vec3f, f0: vec3f, rough: f32) -> vec3f {
    let h = normalize(v + l);
    let n_l = max(dot(n, l), 0.0);
    let n_v = max(dot(n, v), 1e-4);
    let n_h = max(dot(n, h), 0.0);
    let v_h = max(dot(v, h), 0.0);
    let a = max(rough * rough, 0.002);
    let a2 = a * a;
    let dd = n_h * n_h * (a2 - 1.0) + 1.0;
    let D = a2 / (PI * dd * dd);
    let k = a * 0.5;
    let G = (n_l / (n_l * (1.0 - k) + k)) * (n_v / (n_v * (1.0 - k) + k));
    let F = f0 + (1.0 - f0) * pow(1.0 - v_h, 5.0);
    let spec = D * G * F / max(4.0 * n_l * n_v, 1e-4);
    let kd = (1.0 - F) * albedo / PI;
    return (kd + spec) * n_l;
}

// Specular occlusion from ambient occlusion (Lagarde & de Rousiers, "Moving Frostbite to PBR").
fn specular_occlusion(n_v: f32, ao: f32, rough: f32) -> f32 {
    return clamp(pow(n_v + ao, exp2(-16.0 * rough * rough - 1.0)) - 1.0 + ao, 0.0, 1.0);
}

// `ao` darkens the environment terms only, not the studio key and fill lights.
fn shade(m: Material, n: vec3f, v: vec3f, ao: f32) -> vec3f {
    let rough = clamp(m.roughness, 0.02, 1.0);
    let metal = clamp(m.metallic, 0.0, 1.0);
    let base = max(m.base_color, vec3f(0.0));
    let f0 = mix(vec3f(0.04), base, metal);
    let albedo = base * (1.0 - metal);
    let n_v = max(dot(n, v), 1e-4);

    var c = vec3f(0.0);
    if !has_hdri() {
        // The analytic environment adds a key and a fill light.
        let key = normalize(frame.light.xyz);
        c += ggx_light(n, v, key, albedo, f0, rough) * vec3f(2.6, 2.5, 2.35);
        let fill = normalize(vec3f(-key.x, 0.35, -key.z));
        c += ggx_light(n, v, fill, albedo, f0, rough) * vec3f(0.35, 0.4, 0.5);
    }

    let r = reflect(-v, n);
    var spec_env: vec3f;
    if has_hdri() {
        spec_env = env_sample(r, rough);
    } else {
        spec_env = mix(env_radiance(r), env_irradiance(r), smoothstep(0.0, 0.8, rough));
    }
    c += spec_env * env_brdf_approx(f0, rough, n_v) * specular_occlusion(n_v, ao, rough);
    c += albedo * env_irradiance(n) * ao;
    return c + m.emissive;
}

// Perturbs a normal by a height field using screen-space derivatives (surface gradient).
fn bump(n: vec3f, pos: vec3f, h: f32) -> vec3f {
    let dpx = dpdx(pos);
    let dpy = dpdy(pos);
    let r1 = cross(dpy, n);
    let r2 = cross(n, dpx);
    let det = dot(dpx, r1);
    let grad = sign(det) * (dpdx(h) * r1 + dpdy(h) * r2);
    let out = abs(det) * n - grad;
    if dot(out, out) < 1e-20 { return n; }
    return normalize(out);
}

fn tile_tint(layer: i32) -> vec3f {
    if layer < 0 { return vec3f(1.0, 0.0, 1.0); }
    let hue = fract(f32(layer) * 0.618034);
    let k = vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3f(hue) + k) * 6.0 - 3.0);
    return mix(vec3f(1.0), clamp(p - 1.0, vec3f(0.0), vec3f(1.0)), 0.55);
}

// Shades a surface point with the display mode: shared by the mesh and the space between islands.
fn display(s: Surface, ng: vec3f, tangent: vec4f, pos: vec3f) -> vec3f {
    let m = eval_material(s);
    // Tangent frame for the Normal channel.
    let t = normalize(tangent.xyz - ng * dot(tangent.xyz, ng));
    let b = cross(ng, t) * tangent.w;
    var n = normalize(t * m.normal.x + b * m.normal.y + ng * m.normal.z);
    let height_scale = bitcast<f32>(frame.mode.w);
    n = bump(n, pos, m.height * height_scale);

    var v = normalize(frame.eye.xyz - pos);
    if uv_view() { v = ng; }
    var c: vec3f;
    switch frame.mode.x {
        case MODE_CHANNEL: {
            c = channel_display(m, n);
        }
        case MODE_MESH_MAP: {
            c = mesh_map_display(s);
        }
        default: {
            var ao = 1.0;
            if (frame.mode.z & FLAG_AO) != 0u { ao = mesh_ao(tile_tex(s.tile_uv), s.layer); }
            c = shade(m, n, v, ao) * exp2(frame.viewport.z);
        }
    }
    if (frame.mode.z & FLAG_TILES) != 0u {
        c *= tile_tint(s.layer);
    }
    return c;
}

// ---------------------------------------------------------------- UV view: between islands
// With Gaps on, a full-screen pass drawn behind the mesh shows every texel of the used tiles
// that the mesh doesn't cover, evaluated at the nearest island texel's surface (docs/reference/
// 2d-paint.md#display). That includes the uncovered parts of texels on an island's edge.

struct GapOut {
    @builtin(position) clip: vec4f,
};

@vertex
fn vs_gaps(@builtin(vertex_index) i: u32) -> GapOut {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: GapOut;
    o.clip = vec4f(p * 2.0 - 1.0, 0.99, 1.0);
    return o;
}

@fragment
fn fs_gaps(in: GapOut) -> @location(0) vec4f {
    // Screen to UDIM coordinates: the inverse of the UV view's view_proj (a scale and offset).
    let ndc = vec2f(in.clip.x / frame.viewport.x * 2.0 - 1.0, 1.0 - in.clip.y / frame.viewport.y * 2.0);
    let vp = frame.view_proj;
    let uv = (ndc - vec2f(vp[3][0], vp[3][1])) / vec2f(vp[0][0], vp[1][1]);
    let layer = tile_layer(uv);
    if layer < 0 { discard; }
    let tile = floor(uv);
    // The texel itself, so 2D paint shows as sharp texels.
    let res = f32(textureDimensions(paint0).x);
    let texel_uv = (floor((uv - tile) * res) + 0.5) / res;
    var s: Surface;
    let a = gap_surface(texel_uv, tile, layer, &s);
    if a <= 0.0 { discard; }
    let t = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(s.normal.x) > 0.9);
    return vec4f(display(s, s.normal, vec4f(t, 1.0), s.pos), 1.0);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    var s: Surface;
    s.pos = in.pos;
    s.pos_norm = (in.pos - frame.bbox_min.xyz) / max(frame.bbox_size.xyz, vec3f(1e-6));
    var ng = normalize(in.normal);
    // Mirrored islands face away in the UV view; their surface doesn't.
    if !front && !uv_view() { ng = -ng; }
    s.normal = ng;
    s.uv = in.uv;
    s.tile_uv = fract(in.uv);
    s.texel_uv = s.tile_uv;
    s.layer = tile_layer(in.uv);
    s.material = in.material;
    return vec4f(display(s, ng, in.tangent, in.pos), 1.0);
}

// Raw channel values, without lighting. Colors are shown as-is (linear), scalars as gray.
fn channel_display(m: Material, n: vec3f) -> vec3f {
    switch frame.mode.y {
        case 0u: { return m.base_color; }
        case 1u: { return show(vec3f(m.roughness)); }
        case 2u: { return show(vec3f(m.metallic)); }
        case 3u: { return show(vec3f(m.height * 0.5 + 0.5)); }
        case 4u: { return show(m.normal * 0.5 + 0.5); }
        case 5u: { return m.emissive; }
        default: { return show(vec3f(m.opacity)); }
    }
}

// ---------------------------------------------------------------- wireframe

struct LineOut {
    @builtin(position) clip: vec4f,
};

@vertex
fn vs_line(v: VsIn) -> LineOut {
    var o: LineOut;
    o.clip = frame.view_proj * placed(v.pos, v.uv);
    o.clip.z -= 2e-4 * o.clip.w;
    return o;
}

@fragment
fn fs_line() -> @location(0) vec4f {
    return vec4f(0.02, 0.02, 0.02, frame.light.w);
}

// Geo Fill (docs/reference/brushes-tools-stencils.md#outlines-and-highlight): the borders between
// elements, and the element under the pointer, drawn with the wireframe's depth offset.
@fragment
fn fs_border() -> @location(0) vec4f {
    return vec4f(1.0, 0.02, 0.02, 1.0);
}

@fragment
fn fs_highlight() -> @location(0) vec4f {
    return vec4f(1.0, 0.02, 0.02, 0.35);
}

// Bake exclusion (docs/reference/bake-exclusion.md#display): triangles and lines with their own
// colors, drawn like the wireframe.
struct TintIn {
    @location(0) pos: vec3f,
    @location(1) uv: vec2f,
    @location(2) color: vec4f,
};

struct TintOut {
    @builtin(position) clip: vec4f,
    @location(0) color: vec4f,
};

@vertex
fn vs_tint(v: TintIn) -> TintOut {
    var o: TintOut;
    o.clip = frame.view_proj * placed(v.pos, v.uv);
    o.clip.z -= 3e-4 * o.clip.w;
    o.color = v.color;
    return o;
}

@fragment
fn fs_tint(i: TintOut) -> @location(0) vec4f {
    return i.color;
}

// Bake source outlines: each edge an instance, drawn as a quad 6 pixels wide on screen.
struct EdgeIn {
    @location(0) a_pos: vec3f,
    @location(1) a_uv: vec2f,
    @location(2) b_pos: vec3f,
    @location(3) b_uv: vec2f,
    @location(4) color: vec4f,
};

@vertex
fn vs_edge(@builtin(vertex_index) i: u32, e: EdgeIn) -> TintOut {
    var ca = frame.view_proj * placed(e.a_pos, e.a_uv);
    var cb = frame.view_proj * placed(e.b_pos, e.b_uv);
    let size = frame.viewport.xy;
    let sa = ca.xy / max(ca.w, 1e-6) * size;
    let sb = cb.xy / max(cb.w, 1e-6) * size;
    let dir = normalize(select(vec2f(1.0, 0.0), sb - sa, length(sb - sa) > 1e-4));
    // Corners: (end, side) for two triangles.
    let ends = array<f32, 6>(0.0, 1.0, 1.0, 0.0, 1.0, 0.0);
    let sides = array<f32, 6>(-1.0, -1.0, 1.0, -1.0, 1.0, 1.0);
    var c = select(ca, cb, ends[i] > 0.5);
    let offset = vec2f(-dir.y, dir.x) * sides[i] * 3.0 / size * 2.0;
    c = vec4f(c.xy + offset * c.w, c.z - 3e-4 * c.w, c.w);
    var o: TintOut;
    o.clip = c;
    o.color = e.color;
    return o;
}
