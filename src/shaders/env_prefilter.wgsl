// Prefilters an equirectangular environment with the GGX lobe, one roughness per mip level
// (split-sum, Karis 2013). Samples are importance sampled and read from a box-filtered mip chain
// at a level matching each sample's solid angle (Colbert & Krivanek, GPU Gems 3 ch. 20), so a
// few hundred samples give a smooth result.

struct Params {
    // Output level size.
    size: vec2u,
    // Perceptual roughness of this level (GGX alpha = roughness^2).
    roughness: f32,
    sample_count: u32,
};

const PI: f32 = 3.14159265;

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var src: texture_2d<f32>;
@group(0) @binding(2) var src_samp: sampler;
@group(0) @binding(3) var dst: texture_storage_2d<rgba16float, write>;

// Equirectangular uv <-> direction, matching `equirect_uv` in viewport.wgsl (unrotated).
fn uv_dir(uv: vec2f) -> vec3f {
    let phi = (uv.x - 0.5) * 2.0 * PI;
    let theta = uv.y * PI;
    return vec3f(sin(phi) * sin(theta), cos(theta), -cos(phi) * sin(theta));
}

fn dir_uv(d: vec3f) -> vec2f {
    return vec2f(atan2(d.x, -d.z) / (2.0 * PI) + 0.5, acos(clamp(d.y, -1.0, 1.0)) / PI);
}

fn hammersley(i: u32, n: u32) -> vec2f {
    return vec2f(f32(i) / f32(n), f32(reverseBits(i)) * 2.3283064365386963e-10);
}

@compute @workgroup_size(8, 8)
fn prefilter(@builtin(global_invocation_id) gid: vec3u) {
    if gid.x >= params.size.x || gid.y >= params.size.y { return; }
    let n = uv_dir((vec2f(gid.xy) + 0.5) / vec2f(params.size));
    if params.roughness <= 0.0 {
        textureStore(dst, gid.xy, vec4f(textureSampleLevel(src, src_samp, dir_uv(n), 0.0).rgb, 1.0));
        return;
    }

    let a = params.roughness * params.roughness;
    let a2 = a * a;
    let up = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(n.y) < 0.999);
    let tx = normalize(cross(up, n));
    let ty = cross(n, tx);
    let src_size = vec2f(textureDimensions(src, 0));
    let max_lod = f32(textureNumLevels(src) - 1u);

    // View = normal = reflection direction (the split-sum assumption).
    var sum = vec3f(0.0);
    var weight = 0.0;
    for (var i = 0u; i < params.sample_count; i++) {
        let xi = hammersley(i, params.sample_count);
        let cos_t = sqrt((1.0 - xi.y) / (1.0 + (a2 - 1.0) * xi.y));
        let sin_t = sqrt(1.0 - cos_t * cos_t);
        let phi = 2.0 * PI * xi.x;
        let h = tx * (sin_t * cos(phi)) + ty * (sin_t * sin(phi)) + n * cos_t;
        let l = 2.0 * dot(n, h) * h - n;
        let n_l = dot(n, l);
        if n_l <= 0.0 { continue; }

        // pdf(l) = D * (n.h) / (4 v.h) = D / 4 with v = n.
        let dd = cos_t * cos_t * (a2 - 1.0) + 1.0;
        let pdf = a2 / (PI * dd * dd) / 4.0;
        let sample_angle = 1.0 / (f32(params.sample_count) * pdf);
        let sin_l = sqrt(max(1.0 - l.y * l.y, 1e-4));
        let texel_angle = (2.0 * PI / src_size.x) * (PI / src_size.y) * sin_l;
        let lod = clamp(0.5 * log2(sample_angle / texel_angle) + 1.0, 0.0, max_lod);

        sum += textureSampleLevel(src, src_samp, dir_uv(l), lod).rgb * n_l;
        weight += n_l;
    }
    textureStore(dst, gid.xy, vec4f(sum / max(weight, 1e-6), 1.0));
}
