"""Body shell: lofted grid -> one closed mesh with per-face part and material tags."""

import bmesh
import numpy as np

from . import meshkit

# Part ids stored in the "part" face attribute; build_car.py splits on them.
PARTS = ["body", "hood", "bumper_front", "bumper_rear", "door_l", "door_r", "spoiler", "glass"]
P = {n: i for i, n in enumerate(PARTS)}


def in_range(v, r):
    return (v >= r[0]) & (v <= r[1])


def classify(cfg, s, y, z, band, side, mats):
    """Part and material index per face from face-centre coordinates.

    s, y (>= 0), z: face centres in body coordinates; band: section band index
    (see profile.BANDS); side: +1 right, -1 left.
    """
    pc = cfg["panels"]
    n = len(s)
    part = np.full(n, P["body"])
    mat = np.full(n, mats["car_paint"])
    floor, well, lip, lower, upper, side_top, top = range(7)
    under = (band == floor) | (band == well) | (band == lip)
    mat[under] = mats["trim_black"]

    hood = (band == top) & in_range(s, pc["hood"]["s"])
    part[hood] = P["hood"]

    d = pc["door"]
    s_front = d["s"][0] + d["front_lean"] * (z - d["z_min"])
    door = ((band == lower) | (band == upper)) & (s >= s_front) & (s <= d["s"][1]) & (z >= d["z_min"])
    part[door & (side > 0)] = P["door_r"]
    part[door & (side < 0)] = P["door_l"]

    bf = pc["bumper_front"]
    front = ((s <= bf["s_max"]) & (z <= bf["z_max"])) | ((s <= bf["s_nose"]) & (band != top))
    front &= ~under | (s <= bf["s_nose"])
    part[front] = P["bumper_front"]
    lip_f = front & (z <= bf["lip_z"])
    mat[lip_f] = mats["trim_black"]

    br = pc["bumper_rear"]
    rear = (s >= br["s_min"]) & (z <= br["z_max"]) & ~under
    part[rear] = P["bumper_rear"]
    mat[rear & (z <= br["diffuser_z"])] = mats["trim_black"]

    sp = (band == top) & in_range(s, pc["spoiler"]["s"])
    part[sp] = P["spoiler"]

    grille = (band == top) & in_range(s, pc["grille"]["s"]) & (y <= pc["grille"]["half_width"])
    mat[grille] = mats["trim_black"]

    ws = (band == top) & in_range(s, pc["windscreen"]["s"])
    rw = (band == top) & in_range(s, pc["rear_window"]["s"])
    sw = pc["side_window"]
    z_rear = np.interp(s, *np.asarray(sw["rear_edge"]).T, right=-1.0)
    side_win = (band == side_top) & (s >= sw["s_front"]) & (z <= z_rear) & (z >= sw["z_min"])
    for k in (ws, rw, side_win):
        part[k] = P["glass"]
        mat[k] = mats["glass"]
    bp = (band == side_top) & in_range(s, sw["b_pillar"]) & (z >= sw["z_min"])
    part[bp] = P["body"]
    mat[bp] = mats["trim_black"]
    return part, mat


def build(body, cfg, s_mid, mats, mat_list):
    st, g, bands = body.grid()
    S, N, _ = g.shape
    R = 2 * N - 2
    # Ring: right half bottom->top, then left half top->bottom (centre points shared).
    ring_j = np.concatenate([np.arange(N), np.arange(N - 2, 0, -1)])
    ring_side = np.concatenate([np.ones(N), -np.ones(N - 2)])
    pts = g[:, ring_j, :]  # (S, R, 3) s, y, z
    yy = pts[..., 1] * ring_side[None, :]
    verts = np.stack([yy, s_mid - pts[..., 0], pts[..., 2]], axis=-1).reshape(-1, 3)

    # UVs: u along the length, v along the section arc (metres / 5).
    arc = np.zeros((S, R + 1))
    ring_closed = np.concatenate([pts, pts[:, :1]], axis=1)
    seg = np.linalg.norm(np.diff(ring_closed[..., 1:] * np.array([1, 1]), axis=1), axis=-1)
    arc[:, 1:] = np.cumsum(seg, axis=1)
    faces, uvs, fb, fside, fs, fy, fz = [], [], [], [], [], [], []
    for i in range(S - 1):
        for r in range(R):
            r2 = (r + 1) % R
            a, b, c, d = i * R + r, i * R + r2, (i + 1) * R + r2, (i + 1) * R + r
            faces.append((a, d, c, b))
            u0, u1 = st[i] / 5, st[i + 1] / 5
            v0, v1 = arc[i, r] / 5, arc[i, r + 1] / 5
            v0b, v1b = arc[i + 1, r] / 5, arc[i + 1, r + 1] / 5
            uvs.append([(u0, v0), (u1, v0b), (u1, v1b), (u0, v1)])
            j0, j1 = ring_j[r], ring_j[r2]
            fb.append(bands[max(j0, j1)])
            fside.append(ring_side[r] if r < N - 1 else -1)
            c4 = pts[[i, i, i + 1, i + 1], [r, r2, r2, r]]
            fs.append(c4[:, 0].mean())
            fy.append(np.abs(c4[:, 1]).mean())
            fz.append(c4[:, 2].mean())
    fb, fside, fs, fy, fz = map(np.asarray, (fb, fside, fs, fy, fz))
    part, mat = classify(cfg, fs, fy, fz, fb, fside, mats)
    ob = meshkit.mesh_object("body_shell", verts, faces, uvs=uvs, mat_index=mat,
                             materials=mat_list, face_ints={"part": part, "band": fb})
    # Collapse the degenerate nose/tail columns and zero-length wheel-well rows.
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    bmesh.ops.dissolve_degenerate(bm, edges=bm.edges, dist=1e-6)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(ob.data)
    bm.free()
    ob.data.update()
    return ob
