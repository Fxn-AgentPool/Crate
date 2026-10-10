"""Generates the high-poly bake test meshes and their analytic reference maps.

    python tests/data/make_highpoly.py

Wave test: the low poly is a flat 1x1 plane (y = 0, UVs covering tile 1001); the high poly is
the heightfield y = A sin(4 pi x) sin(4 pi z) with analytic vertex normals. The expected
tangent-space normal and height of every texel follow from the formula.

Parts test: two low-poly quads, A_low (x < 0) and B_low (x > 0). The high poly has A_high, a
plane at y = 0.01 over A's half, and B_high, a plane at y = 0.02 over the whole square. With
Match by name, A's texels reach A_high (height 0.01); without it, they hit B_high first (0.02).

Conventions (docs/reference/mesh-maps.md): texture row 0 is v = 1; the low plane's tangent is
+x and its bitangent (+v) is +z, so n_ts = (n.x, n.z, n.y), encoded n * 0.5 + 0.5.
"""
import os

import numpy as np
from PIL import Image

HERE = os.path.dirname(os.path.abspath(__file__))
A = 0.03          # wave amplitude
N = 128           # high-poly grid quads per side
RES = 1024        # tile resolution used by tests/scripts/highpoly.txt
DIAG = np.sqrt(2.0)
DIST = 0.04       # frontal = rear, relative to the low poly's bounding box diagonal


def write_obj(path, objects):
    """objects: list of (name, vertices[(x,y,z)], normals or None, uvs or None, faces[(i,j,k)])."""
    with open(path, 'w', newline='\n') as f:
        base = 0
        for name, verts, normals, uvs, faces in objects:
            f.write(f'o {name}\n')
            for v in verts:
                f.write('v %.6f %.6f %.6f\n' % tuple(v))
            for n in normals or []:
                f.write('vn %.6f %.6f %.6f\n' % tuple(n))
            for t in uvs or []:
                f.write('vt %.6f %.6f\n' % tuple(t))
            for tri in faces:
                idx = [base + i + 1 for i in tri]
                if normals and uvs:
                    f.write('f ' + ' '.join(f'{i}/{i}/{i}' for i in idx) + '\n')
                elif normals:
                    f.write('f ' + ' '.join(f'{i}//{i}' for i in idx) + '\n')
                elif uvs:
                    f.write('f ' + ' '.join(f'{i}/{i}' for i in idx) + '\n')
                else:
                    f.write('f ' + ' '.join(str(i) for i in idx) + '\n')
            base += len(verts)


def quad(x0, x1, z0, z1, y=0.0):
    """Two triangles facing +y, with UVs u = x + 0.5, v = z + 0.5."""
    verts = [(x0, y, z0), (x1, y, z0), (x1, y, z1), (x0, y, z1)]
    uvs = [(x + 0.5, z + 0.5) for x, _, z in verts]
    return verts, uvs, [(0, 2, 1), (0, 3, 2)]


def wave(x, z):
    return A * np.sin(4 * np.pi * x) * np.sin(4 * np.pi * z)


def wave_normal(x, z):
    hx = A * 4 * np.pi * np.cos(4 * np.pi * x) * np.sin(4 * np.pi * z)
    hz = A * 4 * np.pi * np.sin(4 * np.pi * x) * np.cos(4 * np.pi * z)
    n = np.stack([-hx, np.ones_like(hx), -hz], axis=-1)
    return n / np.linalg.norm(n, axis=-1, keepdims=True)


def texel_xz():
    i = (np.arange(RES) + 0.5) / RES
    u, row = np.meshgrid(i, i)
    v = 1.0 - row                       # row 0 is the top of the tile
    return u - 0.5, v - 0.5


def save16(path, data):
    data = np.clip(data, 0, 1)
    if data.ndim == 2:
        Image.fromarray((data * 65535 + 0.5).astype(np.uint16)).save(path)
    else:  # Pillow can't write 16-bit RGB; 8-bit is enough for the comparison tolerance
        Image.fromarray((data * 255 + 0.5).astype(np.uint8), 'RGB').save(path)


def main():
    # Wave test meshes.
    verts, uvs, faces = quad(-0.5, 0.5, -0.5, 0.5)
    write_obj(os.path.join(HERE, 'hp_low.obj'), [('Plane_low', verts, None, uvs, faces)])
    g = np.linspace(-0.5, 0.5, N + 1)
    gx, gz = np.meshgrid(g, g)
    hv = np.stack([gx, wave(gx, gz), gz], axis=-1).reshape(-1, 3)
    hn = wave_normal(gx, gz).reshape(-1, 3)
    hf = []
    for r in range(N):
        for c in range(N):
            a, b = r * (N + 1) + c, r * (N + 1) + c + 1
            d, e = a + N + 1, b + N + 1      # next row: larger z
            hf += [(a, e, b), (a, d, e)]      # faces up (+y)
    write_obj(os.path.join(HERE, "hp_high.obj"), [("Plane_high", hv.tolist(), hn.tolist(), None, hf)])

    x, z = texel_xz()
    n = wave_normal(x, z)
    n_ts = np.stack([n[..., 0], n[..., 2], n[..., 1]], axis=-1)
    save16(os.path.join(HERE, 'hp_normal_ref.png'), n_ts * 0.5 + 0.5)
    save16(os.path.join(HERE, 'hp_height_ref.png'), 0.5 + 0.5 * wave(x, z) / (DIST * DIAG))

    # Parts test meshes.
    va, ua, fa = quad(-0.5, 0.0, -0.5, 0.5)
    vb, ub, fb = quad(0.0, 0.5, -0.5, 0.5)
    write_obj(os.path.join(HERE, 'hp_parts_low.obj'), [('A_low', va, None, ua, fa), ('B_low', vb, None, ub, fb)])
    ha, _, fha = quad(-0.5, 0.0, -0.5, 0.5, y=0.01)
    hb, _, fhb = quad(-0.5, 0.5, -0.5, 0.5, y=0.02)
    up = [(0.0, 1.0, 0.0)] * 4
    write_obj(os.path.join(HERE, 'hp_parts_high.obj'), [('A_high', ha, up, None, fha), ('B_high', hb, up, None, fhb)])
    h = lambda y: 0.5 + 0.5 * y / (0.05 * DIAG)
    on = np.where(x < 0.0, h(0.01), h(0.02))
    save16(os.path.join(HERE, 'hp_match_on_ref.png'), on)
    save16(os.path.join(HERE, 'hp_match_off_ref.png'), np.full_like(x, h(0.02)))
    print('ok')


if __name__ == '__main__':
    main()
