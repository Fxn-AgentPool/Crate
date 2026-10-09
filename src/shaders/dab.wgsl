// Draws brush dabs into the screen-space paint buffer.
// See docs/reference/paint-buffer.md and brushes-tools-stencils.md.
//
// Buffer planes store premultiplied values with per-channel coverage, in the same layout
// as paint pools (scene.rs). `stroke` holds the current stroke's own coverage, so Opacity
// caps a single stroke while later strokes can still paint over earlier ones.

struct Dab {
    // x, y: buffer pixels; z: radius (px); w: rotation (radians)
    geom: vec4f,
    // x: alpha (flow x pressure), y: hardness, z: aspect, w: unused
    params: vec4f,
};

struct Params {
    // x, y: rect origin; z, w: rect size (buffer pixels)
    rect: vec4u,
    // x: tool (0 paint, 1 erase, 2 paint through), y: dab count, z: alpha image layer (-1 none),
    // w: channel mask (bit per channel)
    mode: vec4i,
    // x: opacity, y: 1 = target is a mask, z: mask value, w: stencil use alpha
    opts: vec4f,
    // Brush values per channel (rgb), in channel order.
    values: array<vec4f, 7>,
    // Stencil: screen -> stencil uv affine (rows), and buffer pixel -> screen scale (x).
    stencil_row0: vec4f,
    stencil_row1: vec4f,
    // x: buffer->screen scale, y: stencil layer (-1 none), z: stencil target mask (bits), w: tile
    stencil_info: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> dabs: array<Dab>;
@group(0) @binding(2) var plane0: texture_storage_2d<rgba16float, read_write>;
@group(0) @binding(3) var plane1: texture_storage_2d<rgba16float, read_write>;
@group(0) @binding(4) var plane2: texture_storage_2d<rgba16float, read_write>;
@group(0) @binding(5) var plane3: texture_storage_2d<rgba16float, read_write>;
@group(0) @binding(6) var plane4: texture_storage_2d<rgba16float, read_write>;
@group(0) @binding(7) var erase: texture_storage_2d<rgba16float, read_write>;
@group(0) @binding(8) var stroke: texture_storage_2d<rgba16float, read_write>;
@group(0) @binding(9) var images: texture_2d_array<f32>;
@group(0) @binding(10) var samp: sampler;

fn tip(d: Dab, p: vec2f) -> f32 {
    let r = max(d.geom.z, 0.5);
    let rel = p - d.geom.xy;
    let c = cos(-d.geom.w);
    let s = sin(-d.geom.w);
    let q = vec2f(c * rel.x - s * rel.y, s * rel.x + c * rel.y) / r;
    if params.mode.z >= 0 {
        if abs(q.x) > 1.0 || abs(q.y) > 1.0 { return 0.0; }
        let uv = q * 0.5 + 0.5;
        let a = textureSampleLevel(images, samp, uv, params.mode.z, 0.0);
        return dot(a.rgb, vec3f(0.2126, 0.7152, 0.0722)) * a.a;
    }
    let dist = length(q);
    if dist >= 1.0 { return 0.0; }
    // Hardness: fraction of the radius at full strength.
    let h = clamp(d.params.y, 0.0, 0.99);
    return 1.0 - smoothstep(h, 1.0, dist);
}

// Over-composites premultiplied (value, coverage) with alpha a.
fn over(dst: vec4f, value: vec3f, a: f32) -> vec4f {
    return vec4f(dst.rgb * (1.0 - a) + value * a, dst.a + a * (1.0 - dst.a));
}

fn srgb_to_linear(c: vec3f) -> vec3f {
    return select(pow((c + 0.055) / 1.055, vec3f(2.4)), c / 12.92, c <= vec3f(0.04045));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.rect.z || gid.y >= params.rect.w { return; }
    let xy = params.rect.xy + gid.xy;
    let dims = textureDimensions(plane0);
    if xy.x >= dims.x || xy.y >= dims.y { return; }
    let p = vec2f(xy) + 0.5;

    // Accumulate this frame's dabs into the stroke coverage.
    let s0 = textureLoad(stroke, xy).r;
    var s1 = s0;
    for (var i = 0; i < params.mode.y; i++) {
        let d = dabs[i];
        s1 = s1 + (1.0 - s1) * clamp(tip(d, p) * d.params.x, 0.0, 1.0);
    }
    if s1 <= s0 { return; }
    textureStore(stroke, xy, vec4f(s1));

    // Incremental alpha so the stroke's total contribution is opacity * s1.
    let o = params.opts.x;
    var a = (o * s1 - o * s0) / max(1.0 - o * s0, 1e-4);

    let tool = params.mode.x;
    var values = params.values;
    var mask = u32(params.mode.w);
    if tool == 2 {
        // Paint through: values come from the stencil under this pixel.
        let screen = p * params.stencil_info.x;
        let suv = vec2f(dot(params.stencil_row0.xyz, vec3f(screen, 1.0)), dot(params.stencil_row1.xyz, vec3f(screen, 1.0)));
        let layer = i32(params.stencil_info.y);
        // stencil_row0.w = 1 when the stencil tiles.
        let outside = any(suv < vec2f(0.0)) || any(suv > vec2f(1.0));
        if layer < 0 || (outside && params.stencil_row0.w == 0.0) {
            return;
        }
        let st = textureSampleLevel(images, samp, fract(suv), layer, 0.0);
        if params.opts.w > 0.5 { a *= st.a; }
        let lin = srgb_to_linear(st.rgb);
        let lum = dot(lin, vec3f(0.2126, 0.7152, 0.0722));
        mask = u32(params.stencil_info.z);
        values[0] = vec4f(lin, 0.0);
        values[5] = vec4f(lin, 0.0);
        for (var c = 1; c < 7; c++) {
            if c != 5 { values[c] = vec4f(lum); }
        }
        values[4] = vec4f(normalize(st.rgb * 2.0 - 1.0), 0.0);
        if params.opts.y > 0.5 { values[0] = vec4f(lum); }
    }
    if a <= 0.0 { return; }

    if tool == 1 {
        // Erase: accumulate erase coverage and remove paint already in the buffer.
        let e = textureLoad(erase, xy);
        textureStore(erase, xy, vec4f(e.r + a * (1.0 - e.r)));
        let k = 1.0 - a;
        textureStore(plane0, xy, textureLoad(plane0, xy) * k);
        textureStore(plane1, xy, textureLoad(plane1, xy) * k);
        textureStore(plane2, xy, textureLoad(plane2, xy) * k);
        textureStore(plane3, xy, textureLoad(plane3, xy) * k);
        textureStore(plane4, xy, textureLoad(plane4, xy) * k);
        return;
    }

    if params.opts.y > 0.5 {
        // Mask target: one scalar value in plane 0.
        let v = select(params.opts.z, values[0].x, tool == 2);
        textureStore(plane0, xy, over(textureLoad(plane0, xy), vec3f(v), a));
        return;
    }
    let has = function_mask(mask);
    if has.x { textureStore(plane0, xy, over(textureLoad(plane0, xy), values[0].rgb, a)); }
    if has.y { textureStore(plane1, xy, over(textureLoad(plane1, xy), values[5].rgb, a)); }
    if has.z { textureStore(plane2, xy, over(textureLoad(plane2, xy), values[4].rgb, a)); }
    // Scalars: plane3 holds premultiplied values, plane4 their coverages.
    var p3 = textureLoad(plane3, xy);
    var p4 = textureLoad(plane4, xy);
    let chans = array<u32, 4>(1u, 2u, 3u, 6u);
    for (var k = 0; k < 4; k++) {
        let c = chans[k];
        if (mask & (1u << c)) != 0u {
            p3[k] = p3[k] * (1.0 - a) + values[c].x * a;
            p4[k] = p4[k] + a * (1.0 - p4[k]);
        }
    }
    textureStore(plane3, xy, p3);
    textureStore(plane4, xy, p4);
}

fn function_mask(m: u32) -> vec3<bool> {
    return vec3<bool>((m & 1u) != 0u, (m & 32u) != 0u, (m & 16u) != 0u);
}
