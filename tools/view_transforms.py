"""Builds the data behind Crate's view transforms. See docs/explanation/about-view-transforms.md.

    python tools/view_transforms.py bake-aces            # writes src/luts/aces13.lut.zst, aces20.lut.zst
    python tools/view_transforms.py fit-agx CONFIG.ocio  # writes src/shaders/agx_fit.wgsl
    python tools/view_transforms.py check [CONFIG.ocio]  # error of every transform
    python tools/view_transforms.py test-script          # writes tests/scripts/view_transforms.txt

`check` (with a config) and `test-script` use the AgX fit that `fit-agx` saves in
target/agx_fit.npy, so run `fit-agx` first in a fresh checkout.

Needs `pip install opencolorio numpy scipy zstandard`. ACES 1.3 and 2.0 are baked from the ACES
configs built into OpenColorIO (BSD-3-Clause). AgX is a fit of our own model to Blender's AgX
view (CONFIG.ocio is Blender's datafiles/colormanagement/config.ocio); none of Blender's data
is stored.

LUT file (zstd-compressed): 4-byte magic b"CLUT", u32 size N, then N^3 RGB triplets of little-endian f16, red
fastest. The input is shaped per channel: t = clamp((log2(x / 0.18) - LO) / (HI - LO), 0, 1).
Output is sRGB-encoded display values.
"""
import struct
import sys
from pathlib import Path

import numpy as np
import PyOpenColorIO as ocio

ROOT = Path(__file__).resolve().parent.parent
LO, HI = -12.0, 10.0  # shaper range in stops around 0.18: 4.4e-5 .. 184
SIZE = 65

ACES = {
    "aces13": ("cg-config-v2.2.0_aces-v1.3_ocio-v2.4", "ACES 1.0 - SDR Video"),
    "aces20": ("cg-config-v4.0.0_aces-v2.0_ocio-v2.5", "ACES 2.0 - SDR 100 nits (Rec.709)"),
}


def aces_processor(key):
    config, view = ACES[key]
    cfg = ocio.Config.CreateFromBuiltinConfig(config)
    t = ocio.DisplayViewTransform(src="Linear Rec.709 (sRGB)", display="sRGB - Display", view=view)
    return cfg.getProcessor(t).getDefaultCPUProcessor()


def apply(proc, x):
    y = np.ascontiguousarray(x, dtype=np.float32).copy()
    proc.applyRGB(y)
    return y.astype(np.float64)


def unshape(t):
    return 0.18 * np.exp2(t * (HI - LO) + LO)


def shape(x):
    return np.clip((np.log2(np.maximum(x, 1e-30) / 0.18) - LO) / (HI - LO), 0, 1)


def bake(proc, n):
    g = np.linspace(0, 1, n)
    b, gg, r = np.meshgrid(g, g, g, indexing="ij")  # red fastest
    x = unshape(np.stack([r, gg, b], -1).reshape(-1, 3))
    return np.clip(apply(proc, x), 0, 1).reshape(n, n, n, 3)  # [b][g][r]


def sample(lut, x):
    """Tetrahedral lookup, as the viewport shader does it."""
    n = lut.shape[0]
    t = shape(np.maximum(x, 0)) * (n - 1)
    i = np.minimum(t.astype(int), n - 2)
    f = t - i
    at = lambda o: lut[i[:, 2] + o[2], i[:, 1] + o[1], i[:, 0] + o[0]]
    fr, fg, fb = f[:, 0:1], f[:, 1:2], f[:, 2:3]
    c000, c111 = at((0, 0, 0)), at((1, 1, 1))
    out = np.zeros_like(x)
    # The six tetrahedra of the cube, by the order of the fractions.
    cases = [
        ((fr >= fg) & (fg >= fb), (1, 0, 0), (1, 1, 0), fr, fg, fb),
        ((fr >= fb) & (fb > fg), (1, 0, 0), (1, 0, 1), fr, fb, fg),
        ((fb > fr) & (fr >= fg), (0, 0, 1), (1, 0, 1), fb, fr, fg),
        ((fg > fr) & (fr >= fb), (0, 1, 0), (1, 1, 0), fg, fr, fb),
        ((fg >= fb) & (fb > fr), (0, 1, 0), (0, 1, 1), fg, fb, fr),
        ((fb > fg) & (fg > fr), (0, 0, 1), (0, 1, 1), fb, fg, fr),
    ]
    done = np.zeros((len(x), 1), bool)
    for cond, a, b, f1, f2, f3 in cases:
        m = cond & ~done
        v = (1 - f1) * c000 + (f1 - f2) * at(a) + (f2 - f3) * at(b) + f3 * c111
        out = np.where(m, v, out)
        done |= m
    return out


def sample_trilinear(lut, x):
    """Trilinear lookup (the GPU's texture filtering), for comparison."""
    n = lut.shape[0]
    t = shape(np.maximum(x, 0)) * (n - 1)
    i = np.minimum(t.astype(int), n - 2)
    f = t - i
    out = np.zeros_like(x)
    for corner in range(8):
        o = [(corner >> k) & 1 for k in range(3)]
        w = np.prod([np.where(o[k], f[:, k], 1 - f[:, k]) for k in range(3)], axis=0)
        out += w[:, None] * lut[i[:, 2] + o[2], i[:, 1] + o[1], i[:, 0] + o[0]]
    return out


def samples(seed, n=20000):
    rng = np.random.default_rng(seed)
    lum = 0.18 * 2.0 ** rng.uniform(-10, 8, (n, 1))
    mix = rng.uniform(0, 1, (n, 1)) ** 2
    x = lum * (mix * rng.dirichlet([0.5, 0.5, 0.5], n) * 3 + (1 - mix))
    grays = np.array([[v] * 3 for v in 0.18 * 2.0 ** np.arange(-10, 8.01, 0.25)])
    return np.vstack([x, grays])


def report(name, a, b):
    d = np.abs(a - b) * 255
    print(f"{name:24} max {d.max():6.2f}  mean {d.mean():.3f}  99th {np.percentile(d, 99):.2f}  (8-bit levels)")


def bake_aces():
    out = ROOT / "src" / "luts"
    out.mkdir(exist_ok=True)
    for key in ACES:
        proc = aces_processor(key)
        lut = bake(proc, SIZE)
        data = b"CLUT" + struct.pack("<I", SIZE) + lut.astype("<f2").tobytes()
        import zstandard
        (out / f"{key}.lut.zst").write_bytes(zstandard.ZstdCompressor(level=19).compress(data))
        x = samples(5)
        report(f"{key} ({SIZE}^3)", sample(lut, x), np.clip(apply(proc, x), 0, 1))


# --- AgX: our own model, fitted to Blender's AgX view -----------------------------------

AGX_LO, AGX_HI = -12.47393, 4.026069  # log2 range of the curve, in stops around 0.18
KNOTS = 40


def srgb_oetf(c):
    c = np.clip(c, 0, 1)
    return np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)


def agx_unpack(p):
    curve = np.concatenate([[0.0], np.cumsum(p[18:18 + KNOTS] ** 2)])  # monotonic
    return p[0:9].reshape(3, 3), p[9:18].reshape(3, 3), curve


def agx_model(p, x):
    """Inset matrix, log2 encoding, one curve per channel, outset matrix, Rec.1886 decode,
    sRGB encode. Mirrored by view_agx() in src/shaders/view_transform.wgsl."""
    m_in, m_out, curve = agx_unpack(p)
    t = (np.log2(np.maximum(x @ m_in.T, 1e-10) / 0.18) - AGX_LO) / (AGX_HI - AGX_LO)
    t = np.clip(t, 0, 1) * KNOTS
    i = np.minimum(t.astype(int), KNOTS - 1)
    f = t - i
    s = curve[i] * (1 - f) + curve[i + 1] * f
    return srgb_oetf(np.power(np.clip(s @ m_out.T, 0, 1), 2.4))


def blender(config, view, x):
    """Blender's display values, clipped to 0..1 as the screen does."""
    return np.clip(apply(blender_processor(config, view), x), 0, 1)


def blender_processor(config, view):
    cfg = ocio.Config.CreateFromFile(config)
    t = ocio.DisplayViewTransform(src="Linear Rec.709", display="sRGB", view=view)
    return cfg.getProcessor(t).getDefaultCPUProcessor()


def fit_agx(config):
    from scipy.optimize import least_squares

    proc = blender_processor(config, "AgX")
    x = samples(3, 6000)
    x = np.vstack([x, np.eye(3) * 0.5, np.eye(3) * 4.0])
    target = apply(proc, x)
    # Start from the classic AgX matrices and polynomial curve (re-encoded for Rec.1886).
    inset = np.array([[0.842479, 0.0784336, 0.0792237], [0.0423282, 0.878469, 0.0791661], [0.0423757, 0.0784336, 0.879143]])
    xs = np.linspace(0, 1, KNOTS + 1)
    poly = 15.5 * xs**6 - 40.14 * xs**5 + 31.96 * xs**4 - 6.868 * xs**3 + 0.4298 * xs**2 + 0.1191 * xs - 0.00232
    poly = np.power(np.clip(poly, 1e-6, 1), 2.2 / 2.4)
    p0 = np.concatenate([inset.ravel(), np.linalg.inv(inset).ravel(), np.sqrt(np.maximum(np.diff(poly), 1e-6))])
    p = least_squares(lambda p: (agx_model(p, x) - target).ravel() * 255, p0, method="trf", max_nfev=4000).x
    m_in, m_out, curve = agx_unpack(p)
    fmt = lambda v: ", ".join(f"{a:.9f}" for a in v)
    mat = lambda m: "mat3x3f(" + ", ".join(f"vec3f({fmt(m[:, c])})" for c in range(3)) + ")"
    wgsl = f"""// Generated by tools/view_transforms.py fit-agx: our AgX model fitted to Blender's AgX view.
// Applied as m * v (columns below), matching the fit's row-vector x @ M.T.
const AGX_INSET: mat3x3f = {mat(m_in)};
const AGX_OUTSET: mat3x3f = {mat(m_out)};
const AGX_LO: f32 = {AGX_LO};
const AGX_HI: f32 = {AGX_HI};
const AGX_KNOTS: u32 = {KNOTS}u;
// var<private>, not const: the curve is indexed with a runtime value.
var<private> AGX_CURVE: array<f32, {KNOTS + 1}> = array<f32, {KNOTS + 1}>({fmt(curve)});
"""
    (ROOT / "src" / "shaders" / "agx_fit.wgsl").write_bytes(wgsl.encode("utf-8"))
    np.save(ROOT / "target" / "agx_fit.npy", p)
    report("AgX fit", agx_model(p, samples(4)), apply(proc, samples(4)))


# --- Analytic transforms, mirrored by src/shaders/view_transform.wgsl ----------------------

def pbr_neutral(x):
    c = x.copy()
    lo = c.min(1, keepdims=True)
    c -= np.where(lo < 0.08, lo - 6.25 * lo * lo, 0.04)
    start, desat = 0.8 - 0.04, 0.15
    peak = c.max(1, keepdims=True)
    d = 1 - start
    new_peak = 1 - d * d / (peak + d - start)
    comp = c * (new_peak / np.maximum(peak, 1e-10))
    g = 1 - 1 / (desat * (peak - new_peak) + 1)
    comp = comp * (1 - g) + new_peak * g
    return srgb_oetf(np.where(peak < start, c, comp))


def check(config=None):
    x = samples(8)
    for key in ACES:
        lut = bake(aces_processor(key), SIZE)
        exact = np.clip(apply(aces_processor(key), x), 0, 1)
        report(f"{key} LUT vs OCIO", sample(lut, x), exact)
        if config:
            view = {"aces13": "ACES 1.3", "aces20": "ACES 2.0"}[key]
            report(f"{key} LUT vs Blender", sample(lut, x), blender(config, view, x))
    if config:
        report("Standard vs Blender", srgb_oetf(x), blender(config, "Standard", x))
        report("PBR Neutral vs Blender", pbr_neutral(x), blender(config, "Khronos PBR Neutral", x))
        p = np.load(ROOT / "target" / "agx_fit.npy")
        report("AgX fit vs Blender", agx_model(p, x), blender(config, "AgX", x))


CASES = [  # (color, exposure): scene value = color * 2^exposure
    ((0.18, 0.18, 0.18), 0.0),
    ((0.9, 0.3, 0.05), 2.0),
    ((0.05, 0.2, 0.6), 0.0),
]


def test_script():
    """tests/scripts/view_transforms.txt: renders an unlit color under each transform and
    checks the pixel against the reference math (OpenColorIO for ACES)."""
    p = np.load(ROOT / "target" / "agx_fit.npy")
    exact = {
        "standard": lambda x: srgb_oetf(x),
        "agx": lambda x: agx_model(p, x),
        "aces-1.3": lambda x: np.clip(apply(aces_processor("aces13"), x), 0, 1),
        "aces-2.0": lambda x: np.clip(apply(aces_processor("aces20"), x), 0, 1),
        "pbr-neutral": pbr_neutral,
        "reinhard": lambda x: srgb_oetf(x / (1 + x)),
    }
    lines = [
        "# Generated by tools/view_transforms.py test-script. Renders a Color node, unlit",
        "# under a black environment, through every view transform and compares the center",
        "# pixel with the reference math (OpenColorIO for ACES).",
        "new assets/crate.glb 1024",
        "size 96x96",
        "environment tests/data/black.hdr",
        "node c color",
        "preview c.color",
    ]
    for i, (color, ev) in enumerate(CASES):
        lines.append(f"node-set c color={color[0]},{color[1]},{color[2]}")
        lines.append(f"exposure {ev}")
        x = np.array([color]) * 2.0 ** ev
        for key, f in exact.items():
            want = np.round(f(x)[0] * 255).astype(int)
            out = f"target/view_transforms/{i}_{key}.png"
            lines += [f"view-transform {key}", f"render {out}", f"expect-color {out} {want[0]},{want[1]},{want[2]} 2"]
    (ROOT / "tests" / "scripts" / "view_transforms.txt").write_bytes(("\n".join(lines) + "\n").encode())
    print("\n".join(l for l in lines if l.startswith("expect")))


def check_sizes():
    x = samples(6)
    for key in ACES:
        proc = aces_processor(key)
        exact = np.clip(apply(proc, x), 0, 1)
        for n in (33, 48, 65):
            report(f"{key} {n}^3", sample(bake(proc, n), x), exact)


if __name__ == "__main__":
    cmd = sys.argv[1] if len(sys.argv) > 1 else ""
    if cmd == "bake-aces":
        bake_aces()
    elif cmd == "sizes":
        check_sizes()
    elif cmd == "fit-agx":
        fit_agx(sys.argv[2])
    elif cmd == "test-script":
        test_script()
    elif cmd == "check":
        check(sys.argv[2] if len(sys.argv) > 2 else None)
    else:
        print(__doc__)
