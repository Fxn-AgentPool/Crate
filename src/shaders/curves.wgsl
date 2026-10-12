// Draws a node's curves into its curve slot, one UDIM tile per dispatch.
// See docs/reference/curves.md "Painting".
//
// Every dab is a disc lying on the surface. A texel is painted by the dabs whose disc it lies in
// (within the radius in the disc's plane, and one radius off it), faded by how far its surface
// turns from the dab's. Coverage builds up per curve as in a stroke (dab.wgsl): each dab adds its
// alpha, and Opacity caps the curve. Curves are composited bottom to top; the slot holds
// premultiplied values with coverage, like paint pools.

struct Params {
    // x: tile layer in the mesh maps, y: output layer, z: 1 = mask target, w: curve count
    dest: vec4u,
};

struct CurveInfo {
    values: array<vec4f, 7>,
    // x: opacity, y: hardness, z: mask value, w: alpha image layer (-1 round)
    params: vec4f,
    // x: channel mask, y: first chunk, z: chunk count
    range: vec4u,
    lo: vec4f,
    hi: vec4f,
};

struct Chunk {
    // xyz: box; lo.w: first dab, hi.w: dab count (as bits)
    lo: vec4f,
    hi: vec4f,
};

struct Dab {
    // xyz: center, w: radius
    pos: vec4f,
    // xyz: normal, w: alpha
    normal: vec4f,
    // xyz: path direction, w: tip angle
    tangent: vec4f,
};

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> curves: array<CurveInfo>;
@group(0) @binding(2) var<storage, read> chunks: array<Chunk>;
@group(0) @binding(3) var<storage, read> dabs: array<Dab>;
@group(0) @binding(4) var mm_pos: texture_2d_array<f32>;
@group(0) @binding(5) var mm_nrm: texture_2d_array<f32>;
@group(0) @binding(6) var images: texture_2d_array<f32>;
@group(0) @binding(7) var samp: sampler;
@group(0) @binding(8) var out0: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(9) var out1: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(10) var out2: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(11) var out3: texture_storage_2d_array<rgba16float, write>;
@group(0) @binding(12) var out4: texture_storage_2d_array<rgba16float, write>;

fn inside(p: vec3f, lo: vec3f, hi: vec3f) -> bool {
    return all(p >= lo) && all(p <= hi);
}

// Strength of one dab at surface point p with normal n.
fn tip(d: Dab, p: vec3f, n: vec3f, hardness: f32, alpha_layer: i32) -> f32 {
    let r = max(d.pos.w, 1e-6);
    let rel = p - d.pos.xyz;
    let dn = d.normal.xyz;
    if abs(dot(rel, dn)) > r { return 0.0; }
    // Full where the normals are within 70° (cos 70° = 0.342), none from 90°.
    let facing = smoothstep(0.0, 0.342, dot(n, dn));
    if facing <= 0.0 { return 0.0; }
    let t = d.tangent.xyz;
    let b = cross(dn, t);
    let q0 = vec2f(dot(rel, t), dot(rel, b)) / r;
    let c = cos(-d.tangent.w);
    let s = sin(-d.tangent.w);
    let q = vec2f(c * q0.x - s * q0.y, s * q0.x + c * q0.y);
    if alpha_layer >= 0 {
        if abs(q.x) > 1.0 || abs(q.y) > 1.0 { return 0.0; }
        let a = textureSampleLevel(images, samp, q * 0.5 + 0.5, alpha_layer, 0.0);
        return dot(a.rgb, vec3f(0.2126, 0.7152, 0.0722)) * a.a * facing;
    }
    let dist = length(q);
    if dist >= 1.0 { return 0.0; }
    let h = clamp(hardness, 0.0, 0.99);
    return (1.0 - smoothstep(h, 1.0, dist)) * facing;
}

// Over-composites premultiplied (value, coverage) with alpha a.
fn over(dst: vec4f, value: vec3f, a: f32) -> vec4f {
    return vec4f(dst.rgb * (1.0 - a) + value * a, dst.a + a * (1.0 - dst.a));
}

@compute @workgroup_size(8, 8)
fn main(@builtin(global_invocation_id) gid: vec3u) {
    let res = textureDimensions(out0).x;
    if gid.x >= res || gid.y >= res { return; }
    let xy = gid.xy;
    let tl = i32(params.dest.x);
    let ol = i32(params.dest.y);
    let mask_target = params.dest.z == 1u;
    let mres = textureDimensions(mm_pos).x;
    let mxy = min(vec2u(vec2f(xy) * f32(mres) / f32(res)), vec2u(mres - 1u));

    var o0 = vec4f(0.0);
    var o1 = vec4f(0.0);
    var o2 = vec4f(0.0);
    var o3 = vec4f(0.0);
    var o4 = vec4f(0.0);
    let pos = textureLoad(mm_pos, mxy, tl, 0);
    // Islands and padding, as ordinary paint (docs/reference/mesh-maps.md#padding).
    if pos.a > 0.3 {
        let p = pos.xyz;
        let n = normalize(textureLoad(mm_nrm, mxy, tl, 0).xyz * 2.0 - 1.0);
        for (var ci = 0u; ci < params.dest.w; ci++) {
            let cv = curves[ci];
            if !inside(p, cv.lo.xyz, cv.hi.xyz) { continue; }
            var cov = 0.0;
            for (var k = 0u; k < cv.range.z; k++) {
                let ch = chunks[cv.range.y + k];
                if !inside(p, ch.lo.xyz, ch.hi.xyz) { continue; }
                let first = bitcast<u32>(ch.lo.w);
                let count = bitcast<u32>(ch.hi.w);
                for (var j = 0u; j < count; j++) {
                    let d = dabs[first + j];
                    cov = cov + (1.0 - cov) * clamp(tip(d, p, n, cv.params.y, i32(cv.params.w)) * d.normal.w, 0.0, 1.0);
                }
            }
            let a = cv.params.x * cov;
            if a <= 0.0 { continue; }
            if mask_target {
                o0 = over(o0, vec3f(cv.params.z), a);
                continue;
            }
            let m = cv.range.x;
            if (m & 1u) != 0u { o0 = over(o0, cv.values[0].rgb, a); }
            if (m & 32u) != 0u { o1 = over(o1, cv.values[5].rgb, a); }
            if (m & 16u) != 0u { o2 = over(o2, cv.values[4].rgb, a); }
            // Scalars: o3 holds premultiplied values, o4 their coverages.
            let chans = array<u32, 4>(1u, 2u, 3u, 6u);
            for (var k = 0; k < 4; k++) {
                let c = chans[k];
                if (m & (1u << c)) != 0u {
                    o3[k] = o3[k] * (1.0 - a) + cv.values[c].x * a;
                    o4[k] = o4[k] + a * (1.0 - o4[k]);
                }
            }
        }
    }
    textureStore(out0, xy, ol, o0);
    if !mask_target {
        textureStore(out1, xy, ol, o1);
        textureStore(out2, xy, ol, o2);
        textureStore(out3, xy, ol, o3);
        textureStore(out4, xy, ol, o4);
    }
}
