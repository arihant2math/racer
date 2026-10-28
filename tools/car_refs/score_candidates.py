"""Align raw image-to-3D car meshes to the car frame and score them against the blueprint.

    python tools/car_refs/score_candidates.py <car_dir> <out.json> <cand1.glb> [cand2.glb ...]

<car_dir> is data/cars/<id> (car.toml + refs/blueprint.json). Each candidate
(TRELLIS.2 or Hunyuan3D-2.1 GLB, glTF frame: +Y up, arbitrary scale/heading)
is aligned without any warping:

- up stays +Y; the horizontal principal axis is the length axis;
- a generator base plate (Hunyuan) is detected and trimmed;
- the heading (front at +x or -x) is the one with the best side IoU;
- uniform scale to the car.toml length, ground at the lowest point, centred
  on the length midpoint (the blueprint x origin).

Scores are silhouette IoUs (side, plan, front, rear) of the aligned mesh
against blueprint.json at 5 mm, plus a left/right symmetry IoU of the plan
silhouette. The total is the mean of the four view IoUs. The aligned
transform (4x4, glTF frame -> car frame x fwd, y left, z up) is saved per
candidate so the build can reuse it.
"""
import json
import sys
import tomllib
from pathlib import Path

import numpy as np
import trimesh
from scipy import ndimage

RES = 0.005


def load_mesh(path):
    s = trimesh.load(path, force="scene")
    return s.dump(concatenate=True) if hasattr(s, "dump") else s


def raster(tris, ext):
    """Rasterize 2D triangles (N,3,2) to a bool image over ext ((u0,u1),(v0,v1))."""
    (u0, u1), (v0, v1) = ext
    W, H = int(round((u1 - u0) / RES)), int(round((v1 - v0) / RES))
    img = np.zeros((H, W), bool)
    a = (tris - [u0, v0]) / RES
    mn = np.floor(a.min(1)).astype(int)
    mx = np.ceil(a.max(1)).astype(int)
    keep = (mx[:, 0] >= 0) & (mx[:, 1] >= 0) & (mn[:, 0] < W) & (mn[:, 1] < H)
    a, mn, mx = a[keep], np.maximum(mn[keep], 0), np.minimum(mx[keep], [W - 1, H - 1])
    small = ((mx - mn) <= 1).all(1)
    # Tiny triangles: mark their centroid pixel (the mesh is dense).
    c = np.clip(a[small].mean(1).astype(int), 0, [W - 1, H - 1])
    img[c[:, 1], c[:, 0]] = True
    for tri, lo, hi in zip(a[~small], mn[~small], mx[~small]):
        xs, ys = np.meshgrid(np.arange(lo[0], hi[0] + 1) + 0.5, np.arange(lo[1], hi[1] + 1) + 0.5)
        (x1, y1), (x2, y2), (x3, y3) = tri
        d = (y2 - y3) * (x1 - x3) + (x3 - x2) * (y1 - y3)
        if abs(d) < 1e-12:
            continue
        l1 = ((y2 - y3) * (xs - x3) + (x3 - x2) * (ys - y3)) / d
        l2 = ((y3 - y1) * (xs - x3) + (x1 - x3) * (ys - y3)) / d
        img[lo[1]:hi[1] + 1, lo[0]:hi[0] + 1] |= (l1 >= -1e-3) & (l2 >= -1e-3) & (1 - l1 - l2 >= -1e-3)
    img = ndimage.binary_closing(img, iterations=2)
    return ndimage.binary_fill_holes(img)


def bp_mask(bp, view, ext):
    (u0, u1), (v0, v1) = ext
    W, H = int(round((u1 - u0) / RES)), int(round((v1 - v0) / RES))
    us = u0 + (np.arange(W) + 0.5) * RES
    vs = v0 + (np.arange(H) + 0.5) * RES
    if view == "side":
        x = np.array(bp["side"]["x"])
        top = np.interp(us, x, bp["side"]["z_top"], left=-1, right=-1)
        bot = np.interp(us, x, bp["side"]["z_bottom"], left=9, right=9)
        return (vs[:, None] <= top[None]) & (vs[:, None] >= bot[None])
    if view == "plan":
        x = np.array(bp["plan"]["x"])
        hw = np.interp(us, x, bp["plan"]["half_width"], left=-1, right=-1)
        return np.abs(vs[:, None]) <= hw[None]
    z = np.array(bp[view]["z"])
    o = np.argsort(z)
    hw = np.interp(vs, z[o], np.array(bp[view]["half_width"])[o], left=-1, right=-1)
    return np.abs(us[None]) <= hw[:, None]


def views(L, W, H):
    hx, hy, hz = L / 2 + 0.1, W / 2 + 0.25, H + 0.1
    # (projection axes of the car frame (x fwd, y left, z up), extent)
    return {
        "side": ((0, 2), ((-hx, hx), (-0.05, hz))),
        "plan": ((0, 1), ((-hx, hx), (-hy, hy))),
        "front": ((1, 2), ((-hy, hy), (-0.05, hz))),
        "rear": ((1, 2), ((-hy, hy), (-0.05, hz))),
    }


def iou(a, b):
    return float((a & b).sum() / max((a | b).sum(), 1))


def align(mesh, L, flip):
    """glTF frame -> car frame (x fwd, y left, z up), uniform scale to length L."""
    V = mesh.vertices
    # glTF Y up -> z up; horizontal = glTF (x, z).
    P = np.c_[V[:, 0], -V[:, 2], V[:, 1]]
    xy = P[:, :2] - P[:, :2].mean(0)
    w, vec = np.linalg.eigh(np.cov(xy[:: max(1, len(xy) // 200000)].T))
    ax = vec[:, np.argmax(w)]
    # Snap to the nearest coordinate axis if close (generators output axis-aligned cars).
    ang = np.arctan2(ax[1], ax[0])
    snap = np.round(ang / (np.pi / 2)) * (np.pi / 2)
    if abs(ang - snap) < np.radians(4):
        ang = snap
    c, s = np.cos(-ang), np.sin(-ang)
    R = np.array([[c, -s, 0], [s, c, 0], [0, 0, 1.0]])
    if flip:
        R = np.diag([-1.0, -1.0, 1.0]) @ R
    M = np.zeros((4, 4))
    M[:3, :3] = R @ np.array([[1, 0, 0], [0, 0, -1], [0, 1, 0.0]])
    M[3, 3] = 1
    Q = P @ R.T
    # Hunyuan base plate: a thin slab spanning past both ends near the bottom.
    zlo = Q[:, 2].min()
    span = Q[:, 0].max() - Q[:, 0].min()
    ends = Q[np.abs(Q[:, 0] - 0.5 * (Q[:, 0].max() + Q[:, 0].min())) > 0.48 * span]
    plate = None
    if len(ends) and ends[:, 2].max() - zlo < 0.06 * span:
        plate = ends[:, 2].max() + 0.004 * span
    body = Q[Q[:, 2] > plate] if plate is not None else Q
    x0, x1 = body[:, 0].min(), body[:, 0].max()
    k = L / (x1 - x0)
    ground = Q[:, 2].min()  # tyre contacts (below the plate for Hunyuan)
    T = np.eye(4)
    T[:3, :3] *= k
    T[:3, 3] = [-(x0 + x1) / 2 * k, -body[:, 1].mean() * k, -ground * k]
    return T @ M, (plate - ground) * k if plate is not None else None


def splat(pts, ext):
    """Rasterize dense surface samples (N,2) to a filled bool silhouette."""
    (u0, u1), (v0, v1) = ext
    W, H = int(round((u1 - u0) / RES)), int(round((v1 - v0) / RES))
    img = np.zeros((H, W), bool)
    ij = ((pts - [u0, v0]) / RES).astype(int)
    ok = (ij[:, 0] >= 0) & (ij[:, 1] >= 0) & (ij[:, 0] < W) & (ij[:, 1] < H)
    img[ij[ok, 1], ij[ok, 0]] = True
    img = ndimage.binary_closing(img, iterations=2)
    return ndimage.binary_fill_holes(img)


def score(path, bp, L, W, H):
    mesh = load_mesh(path)
    # Dense area-uniform surface samples (about 2 per 5 mm pixel at car scale).
    n = int(min(6_000_000, max(1_000_000, mesh.area / (RES * 0.5) ** 2 * (L / mesh.extents.max()) ** 2)))
    S, fi = trimesh.sample.sample_surface(mesh, n, seed=0)
    best = None
    for flip in (False, True):
        M, plate_z = align(mesh, L, flip)
        P = trimesh.transform_points(S, M)
        if plate_z is not None:
            P = P[P[:, 2] > plate_z]  # drop the generator's base plate
        res = {}
        masks = {}
        for view, (proj, ext) in views(L, W, H).items():
            t = P[:, list(proj)].copy()
            if view == "rear":
                t[:, 0] *= -1  # viewer's right is the car's left when looking at the rear
            m = splat(t, ext)
            masks[view] = m
            res[view] = round(iou(m, bp_mask(bp, view, ext)), 4)
        pm = masks["plan"]
        res["symmetry"] = round(iou(pm, pm[::-1]), 4)
        res["total"] = round(float(np.mean([res[v] for v in ("side", "plan", "front", "rear")])), 4)
        # Heading: the side silhouette decides (front/rear widths are similar).
        if best is None or res["side"] > best[0]["side"]:
            best = (res, M, flip, plate_z)
    res, M, flip, plate_z = best
    return {"file": Path(path).name, **res, "flip": flip, "plate_z": plate_z, "to_car_frame": M.round(6).tolist(),
            "faces": int(len(mesh.faces))}


def main(car_dir, out, *cands):
    car = Path(car_dir)
    spec = tomllib.loads((car / "car.toml").read_text())["chassis"]
    bp = json.loads((car / "refs/blueprint.json").read_text())
    L, W, H = spec["length_m"], spec["width_m"], spec["height_m"]
    results = []
    for c in cands:
        r = score(c, bp, L, W, H)
        print(f"{r['total']:.4f}  side {r['side']:.3f} plan {r['plan']:.3f} front {r['front']:.3f} rear {r['rear']:.3f} sym {r['symmetry']:.3f}  {r['file']}", flush=True)
        results.append(r)
    results.sort(key=lambda r: -r["total"])
    Path(out).write_text(json.dumps(results, indent=1))


if __name__ == "__main__":
    main(*sys.argv[1:])
