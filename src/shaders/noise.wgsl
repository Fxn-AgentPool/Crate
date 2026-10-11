// Noise for procedural textures: pure functions of a position, with no bindings, shared by
// compiled material graphs (prepended to material_lib.wgsl) and the Library's thumbnails.

fn hash3(p: vec3f) -> vec3f {
    var q = vec3u(vec3i(floor(p)) + vec3i(32768));
    q = q * vec3u(1597334673u, 3812015801u, 2798796415u);
    q = (q.x ^ q.y ^ q.z) * vec3u(1597334673u, 3812015801u, 2798796415u);
    return vec3f(q) * (1.0 / 4294967295.0);
}

fn hash1(p: vec3f) -> f32 {
    return hash3(p).x;
}

fn fade3(t: vec3f) -> vec3f {
    return t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
}

fn grad_noise(p: vec3f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = fade3(f);
    var r = 0.0;
    var acc: array<f32, 8>;
    for (var k = 0u; k < 8u; k++) {
        let o = vec3f(f32(k & 1u), f32((k >> 1u) & 1u), f32((k >> 2u) & 1u));
        let g = normalize(hash3(i + o) * 2.0 - 1.0 + vec3f(1e-4));
        acc[k] = dot(g, f - o);
    }
    let x0 = mix(acc[0], acc[1], u.x);
    let x1 = mix(acc[2], acc[3], u.x);
    let x2 = mix(acc[4], acc[5], u.x);
    let x3 = mix(acc[6], acc[7], u.x);
    return mix(mix(x0, x1, u.y), mix(x2, x3, u.y), u.z) * 1.15;
}

fn value_noise(p: vec3f) -> f32 {
    let i = floor(p);
    let u = fade3(fract(p));
    var c: array<f32, 8>;
    for (var k = 0u; k < 8u; k++) {
        let o = vec3f(f32(k & 1u), f32((k >> 1u) & 1u), f32((k >> 2u) & 1u));
        c[k] = hash1(i + o) * 2.0 - 1.0;
    }
    return mix(mix(mix(c[0], c[1], u.x), mix(c[2], c[3], u.x), u.y),
               mix(mix(c[4], c[5], u.x), mix(c[6], c[7], u.x), u.y), u.z);
}

// Simplex-style noise: gradient noise on a skewed lattice (cheap approximation).
fn simplex_noise(p: vec3f) -> f32 {
    let skew = (p.x + p.y + p.z) * (1.0 / 3.0);
    return grad_noise(p + skew * 0.5) * 0.8 + grad_noise(p * 1.7 - skew) * 0.2;
}

fn noise_kind(p: vec3f, kind: u32) -> f32 {
    switch kind {
        case 1u: { return simplex_noise(p); }
        case 2u: { return value_noise(p); }
        default: { return grad_noise(p); }
    }
}

// Fractal noise in [0, 1].
fn fbm(p_in: vec3f, kind: u32, octaves: f32, lacunarity: f32, gain: f32, seed: f32) -> f32 {
    var p = p_in + vec3f(seed * 17.13, seed * 3.71, seed * 11.97);
    var amp = 0.5;
    var sum = 0.0;
    var norm = 0.0;
    let n = u32(clamp(octaves, 1.0, 10.0));
    for (var i = 0u; i < n; i++) {
        sum += noise_kind(p, kind) * amp;
        norm += amp;
        p = p * lacunarity + vec3f(5.2, 1.3, 7.7);
        amp *= gain;
    }
    return clamp(0.5 + 0.5 * sum / max(norm, 1e-4) * 1.4, 0.0, 1.0);
}

// Cellular noise; returns (F1, F2).
fn cellular(p_in: vec3f, jitter: f32, manhattan: bool, seed: f32) -> vec2f {
    let p = p_in + vec3f(seed * 7.31);
    let i = floor(p);
    let f = fract(p);
    var f1 = 1e9;
    var f2 = 1e9;
    for (var z = -1; z <= 1; z++) {
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let o = vec3f(f32(x), f32(y), f32(z));
                let c = o + hash3(i + o) * jitter - f;
                var d: f32;
                if manhattan { d = abs(c.x) + abs(c.y) + abs(c.z); } else { d = length(c); }
                if d < f1 { f2 = f1; f1 = d; } else if d < f2 { f2 = d; }
            }
        }
    }
    return vec2f(f1, f2);
}
