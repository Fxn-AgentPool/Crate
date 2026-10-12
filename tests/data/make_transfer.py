"""Generates the paint transfer test meshes (tests/scripts/paint_transfer.txt).

    python tests/data/make_transfer.py

A cube of six faces, each an N x N grid, written three times with the same geometry:

- transfer_a.obj: the faces in a 3 x 2 grid of tile 1001.
- transfer_b.obj: the same faces at the same texel density, but moved, rotated by 90 degrees,
  mirrored, and two of them moved to tile 1002. Paint transferred from A must look the same.
- transfer_c.obj: B's UVs, with the +Y face (1/6 of the UV area) moved out by 5 % of the
  bounding-box diagonal, beyond the 1 % search distance: its texels find no old surface.
"""
import os

HERE = os.path.dirname(os.path.abspath(__file__))
N = 8
# Each face: origin, u axis, v axis (outward normal = u x v), all on the unit cube [-0.5, 0.5]^3.
FACES = [
    ((0.5, -0.5, 0.5), (0, 0, -1), (0, 1, 0)),    # +X
    ((-0.5, -0.5, -0.5), (0, 0, 1), (0, 1, 0)),   # -X
    ((-0.5, 0.5, 0.5), (1, 0, 0), (0, 0, -1)),    # +Y
    ((-0.5, -0.5, -0.5), (1, 0, 0), (0, 0, 1)),   # -Y
    ((-0.5, -0.5, 0.5), (1, 0, 0), (0, 1, 0)),    # +Z
    ((0.5, -0.5, -0.5), (-1, 0, 0), (0, 1, 0)),   # -Z
]
S = 0.3  # a face's size in UV space


def layout_a(face, s, t):
    col, row = face % 3, face // 3
    return (0.03 + col * 0.32 + s * S, 0.03 + row * 0.5 + t * S)


def layout_b(face, s, t):
    # (tile offset, island origin, transform of (s, t)) per face.
    moves = [
        ((0, 0), (0.6, 0.6), lambda s, t: (t, 1 - s)),   # rotated 90 degrees
        ((1, 0), (0.1, 0.1), lambda s, t: (1 - s, t)),   # mirrored, tile 1002
        ((0, 0), (0.1, 0.6), lambda s, t: (s, t)),
        ((1, 0), (0.6, 0.5), lambda s, t: (1 - t, s)),   # rotated the other way, tile 1002
        ((0, 0), (0.6, 0.1), lambda s, t: (s, 1 - t)),   # mirrored
        ((0, 0), (0.1, 0.1), lambda s, t: (t, s)),       # transposed
    ]
    (tu, tv), (ou, ov), f = moves[face]
    a, b = f(s, t)
    return (tu + ou + a * S, tv + ov + b * S)


def write(name, layout, push=None):
    vs, vts, fs = [], [], []
    for k, (o, u, v) in enumerate(FACES):
        n = tuple(u[(i + 1) % 3] * v[(i + 2) % 3] - u[(i + 2) % 3] * v[(i + 1) % 3] for i in range(3))
        base = len(vs)
        for j in range(N + 1):
            for i in range(N + 1):
                s, t = i / N, j / N
                p = [o[c] + u[c] * s + v[c] * t for c in range(3)]
                if push and push[0] == k:
                    p = [p[c] + n[c] * push[1] for c in range(3)]
                vs.append(p)
                vts.append(layout(k, s, t))
        for j in range(N):
            for i in range(N):
                a = base + j * (N + 1) + i
                fs.append((a, a + 1, a + N + 2))
                fs.append((a, a + N + 2, a + N + 1))
    with open(os.path.join(HERE, name), "w") as f:
        f.write("o Cube\n")
        for p in vs:
            f.write("v %.6f %.6f %.6f\n" % tuple(p))
        for t in vts:
            f.write("vt %.6f %.6f\n" % t)
        for a, b, c in fs:
            f.write("f %d/%d %d/%d %d/%d\n" % (a + 1, a + 1, b + 1, b + 1, c + 1, c + 1))


write("transfer_a.obj", layout_a)
write("transfer_b.obj", layout_b)
# 5 % of the diagonal (sqrt(3)) outward.
write("transfer_c.obj", layout_b, push=(2, 0.05 * 3 ** 0.5))
