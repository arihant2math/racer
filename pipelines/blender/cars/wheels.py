"""Wheel and brake meshes from tyre/disc sizes (Blender frame: X right, Y forward, Z up).

Lifted from pipelines/blender/porsche_992/build.py and parametrised per car
(spoke count/pairing, caliper colour come from refs/model.toml). Call
setup(collection, materials) once before building.
"""
import math

import bmesh
import numpy as np
from mathutils import Matrix, Vector

import parts

W = None
MAT = None


def setup(coll, mat):
    global W, MAT
    W, MAT = coll, mat

def tyre_mesh(width, r_rim, r_out, name):
    """Lathe a tyre with sidewall bulge and UVs mapped to the tyre atlas."""
    segs = 96
    hw = width / 2
    # Profile (r, x) from inner bead to outer bead, around the tread.
    prof = [
        (r_rim + 0.005, -hw * 0.86, 0.0),
        (r_rim + 0.02, -hw * 0.95, 0.06),
        ((r_rim + r_out) / 2, -hw * 1.0, 0.25),
        (r_out - 0.03, -hw * 0.98, 0.42),
        (r_out - 0.008, -hw * 0.9, 0.5),
        (r_out, -hw * 0.75, 0.55),
        (r_out, 0.0, 0.775),
        (r_out, hw * 0.75, 1.0),
        (r_out - 0.008, hw * 0.9, 0.5),
        (r_out - 0.03, hw * 0.98, 0.42),
        ((r_rim + r_out) / 2, hw * 1.0, 0.25),
        (r_rim + 0.02, hw * 0.95, 0.06),
        (r_rim + 0.005, hw * 0.86, 0.0),
    ]
    # V coordinate in the atlas: outer sidewall 0..0.25 (top quarter = rows 0..256 of 1024),
    # inner sidewall 0.25..0.5, tread 0.5..1.0. Image rows are top-down; UV v is bottom-up.
    def v_of(i):
        if i <= 5:  # inner side (x<0 is inner when mounted on the left? use outer=+x)
            return 0.5 + 0.25 * (1 - prof[i][2] / 0.55) if i < 5 else 0.5
        if i >= 7:
            return 1.0 - 0.25 * (prof[i][2] if i > 7 else 1.0) / 0.55 * 0.55 / 1.0
        return 0.25

    bm = bmesh.new()
    uv = bm.loops.layers.uv.new("UVMap")
    rings = []
    for k in range(segs + 1):
        t = 2 * math.pi * k / segs
        rings.append([bm.verts.new((x, r * math.cos(t), r * math.sin(t))) for r, x, _ in prof] if k < segs else None)
    rings[-1] = rings[0]
    # UV design: u around the tyre (0..1 = 2 wraps of the 2048 strip), v by profile index.
    vmap = [0.0, 0.03, 0.12, 0.2, 0.235, 0.5, 0.25, 0.5, 0.765, 0.8, 0.88, 0.97, 1.0]
    # Map: outer sidewall (+x) -> atlas rows 0..256 (v 1.0..0.75), inner (-x) -> 256..512 (v 0.75..0.5),
    # tread -> 512..1024 (v 0.5..0).
    def atlas_v(i):
        if i >= 8:  # outer sidewall: i=12 rim -> top (v=1.0), i=8 shoulder -> v=0.75
            return 1.0 - 0.25 * (12 - i) / 4
        if i <= 4:  # inner sidewall
            return 0.5 + 0.25 * i / 4
        return 0.5 * (1 - (i - 5) / 2)  # tread 5..7 -> 0.5..0.0

    for k in range(segs):
        r0, r1 = rings[k], rings[k + 1]
        for i in range(len(prof) - 1):
            f = bm.faces.new((r0[i], r0[i + 1], r1[i + 1], r1[i]))
            u0, u1 = 2 * k / segs, 2 * (k + 1) / segs
            for l, (uu, ii) in zip(f.loops, ((u0, i), (u0, i + 1), (u1, i + 1), (u1, i))):
                l[uv].uv = (uu, atlas_v(ii))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return parts.bm_to_object(bm, name, W, MAT["tyre"])


def rim_mesh(r_rim, width, name, spokes=10, paired=True, spoke_w=0.022, twist=0.0, lugs=5):
    """Multi-spoke alloy: `spokes` spokes (in pairs if `paired`), concave face.

    twist (rad) sweeps each spoke from hub to rim (turbine-style rims).
    """
    objs = []
    hw = width / 2
    # Barrel + lip (lathe), outer face at +x.
    barrel = parts.lathe(
        [
            (r_rim * 0.93, -hw * 0.92),
            (r_rim * 0.98, -hw * 0.92),
            (r_rim * 1.0, -hw * 0.85),
            (r_rim * 0.97, -hw * 0.7),
            (r_rim * 0.9, -hw * 0.5),
            (r_rim * 0.9, hw * 0.6),
            (r_rim * 0.97, hw * 0.75),
            (r_rim * 1.02, hw * 0.88),
            (r_rim * 1.02, hw * 0.93),
            (r_rim * 0.95, hw * 0.95),
            (r_rim * 0.9, hw * 0.88),
        ],
        96, name + "_barrel", axis="X", coll=W, mat=MAT["rim"],
    )
    objs.append(barrel)
    # Centre hub disc (concave) + crest cap.
    hub = parts.lathe([(0.0, hw * 0.62), (0.05, hw * 0.62), (0.075, hw * 0.55), (0.09, hw * 0.4), (0.1, hw * 0.2)], 48, name + "_centre", axis="X", coll=W, mat=MAT["rim"])
    objs.append(hub)
    cap = parts.lathe([(0.0, hw * 0.66), (0.032, hw * 0.65), (0.034, hw * 0.6)], 32, name + "_cap", axis="X", coll=W, mat=MAT["badge"])
    # UV for the cap: planar on y,z.
    me = cap.data
    uvl = me.uv_layers.new(name="UVMap")
    for poly in me.polygons:
        for li in poly.loop_indices:
            v = me.vertices[me.loops[li].vertex_index].co
            uvl.data[li].uv = (0.5 + v.y / 0.07, 0.5 + v.z / 0.07)
    objs.append(cap)
    # Spokes: 5 pairs; each spoke a swept quad tube from hub to rim, concave.
    n = spokes
    for k in range(n):
        if paired:
            base = 2 * math.pi * (k // 2) / (n // 2)
            a = base + (0.11 if k % 2 else -0.11)
        else:
            a = 2 * math.pi * k / n
        bm = bmesh.new()
        prof_r = np.linspace(0.085, r_rim * 0.93, 9)
        rows = []
        for j, r in enumerate(prof_r):
            t = j / (len(prof_r) - 1)
            x = hw * (0.62 - 0.28 * math.sin(math.pi * t * 0.9) + 0.25 * t)  # concave dish
            wdt = spoke_w * (1.15 - 0.45 * t)
            dep = 0.03 * (1.0 - 0.4 * t)
            ang = a + ((0.12 if k % 2 else -0.12) * t * 0.2 if paired else 0.0) + twist * t
            c, s_ = math.cos(ang), math.sin(ang)
            radial = Vector((0, c, s_))
            tang = Vector((0, -s_, c))
            p = Vector((x, 0, 0)) + radial * r
            rows.append([bm.verts.new(p + tang * wdt + Vector((0.0, 0, 0))),
                         bm.verts.new(p - tang * wdt),
                         bm.verts.new(p - tang * wdt * 0.6 - Vector((dep, 0, 0))),
                         bm.verts.new(p + tang * wdt * 0.6 - Vector((dep, 0, 0)))])
        for j in range(len(rows) - 1):
            for q in range(4):
                bm.faces.new((rows[j][q], rows[j][(q + 1) % 4], rows[j + 1][(q + 1) % 4], rows[j + 1][q]))
        bm.faces.new(rows[0][::-1])
        bm.faces.new(rows[-1])
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        objs.append(parts.bm_to_object(bm, f"{name}_spoke{k}", W, MAT["rim"]))
    # Lug bolts (5) + valve
    for k in range(lugs):
        t = 2 * math.pi * k / lugs + math.pi / lugs
        bolt = parts.lathe([(0.0, 0.0), (0.011, 0.0), (0.011, 0.018), (0.0, 0.02)], 12, f"{name}_lug{k}", axis="X", coll=W, mat=MAT["chrome"])
        bolt.data.transform(Matrix.Translation((hw * 0.5, 0.06 * math.cos(t), 0.06 * math.sin(t))))
        objs.append(bolt)
    return objs


def brake(disc_mm, name, front=True):
    objs = []
    rd = disc_mm / 2000
    th = 0.034 if front else 0.028
    disc = parts.lathe([(0.075, -th / 2), (rd, -th / 2), (rd, th / 2), (0.075, th / 2)], 96, name + "_disc", axis="X", coll=W, mat=MAT["disc"])
    me = disc.data
    uvl = me.uv_layers.new(name="UVMap")
    for poly in me.polygons:
        for li in poly.loop_indices:
            v = me.vertices[me.loops[li].vertex_index].co
            uvl.data[li].uv = (0.5 + v.y / (2 * rd), 0.5 + v.z / (2 * rd))
    objs.append(disc)
    hat = parts.lathe([(0.0, th / 2 + 0.03), (0.085, th / 2 + 0.03), (0.09, 0.0), (0.075, -th / 2)], 48, name + "_hat", axis="X", coll=W, mat=MAT["hub"])
    objs.append(hat)
    # Caliper: curved block hugging the disc at the trailing top (rear of the wheel).
    bm = bmesh.new()
    arc0, arc1 = math.radians(100), math.radians(160)
    rows = []
    for j in range(9):
        a = arc0 + (arc1 - arc0) * j / 8
        c, s_ = math.cos(a), math.sin(a)
        ring = []
        for (r, x) in ((rd - 0.065, -th / 2 - 0.03), (rd + 0.012, -th / 2 - 0.03), (rd + 0.012, th / 2 + 0.035), (rd - 0.065, th / 2 + 0.035)):
            ring.append(bm.verts.new((x, r * c, r * s_)))
        rows.append(ring)
    for j in range(8):
        for q in range(4):
            bm.faces.new((rows[j][q], rows[j][(q + 1) % 4], rows[j + 1][(q + 1) % 4], rows[j + 1][q]))
    bm.faces.new(rows[0][::-1])
    bm.faces.new(rows[-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    cal = parts.bm_to_object(bm, name + "_caliper", W, MAT["caliper"])
    bev = cal.modifiers.new("bevel", "BEVEL")
    bev.width = 0.008
    bev.segments = 2
    parts.apply_modifiers(cal)
    objs.append(cal)
    return objs


