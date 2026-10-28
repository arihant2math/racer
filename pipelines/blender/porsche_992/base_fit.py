"""Fit the image-to-3D base mesh to the blueprint (Blender module).

The base mesh (Hunyuan3D-2.1 from a 3/4 photo, AI frame: +X forward, +Z up,
sitting on a plate, arbitrary scale) is:

1. scaled and centred from its wheel positions to the real wheelbase,
2. trimmed of the base plate,
3. warped by smooth per-station factors so its side top silhouette, plan
   width and greenhouse width match the blueprint exactly (the warp factors
   are low-pass along x, so the AI surface quality is kept),
4. relaxed (Laplacian smoothing) to remove baked-in panel gaps, lamp relief
   and generation noise, then decimated for the LOD0 budget.

Returns a Blender object in the car frame (X right, Y forward, Z up).
"""
import json
import math

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector


def smooth1d(a, sigma):
    r = int(4 * sigma + 0.5)
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    pad = np.concatenate([np.repeat(a[:1], r), a, np.repeat(a[-1:], r)])
    return np.convolve(pad, k, mode="valid")


def bridge(xs, f, lo, hi):
    f = f.copy()
    m = (xs > lo) & (xs < hi)
    i0, i1 = np.argmax(xs > lo) - 1, len(xs) - np.argmax((xs < hi)[::-1])
    d0 = (f[i0] - f[i0 - 3]) / (xs[i0] - xs[i0 - 3])
    d1 = (f[i1 + 3] - f[i1]) / (xs[i1 + 3] - xs[i1])
    h = xs[i1] - xs[i0]
    t = (xs[m] - xs[i0]) / h
    f[m] = (2 * t**3 - 3 * t**2 + 1) * f[i0] + (t**3 - 2 * t**2 + t) * h * d0 + (-2 * t**3 + 3 * t**2) * f[i1] + (t**3 - t**2) * h * d1
    return f


def station_max(x, v, xs, width):
    out = np.full(len(xs), np.nan)
    order = np.argsort(x)
    xsorted = x[order]
    for i, c in enumerate(xs):
        a, b = np.searchsorted(xsorted, [c - width, c + width])
        if b > a:
            out[i] = v[order[a:b]].max()
    return out


def fit(glb_path, bp, spec, name="body_shell", smooth_iters=40, target_faces=180_000, log=print):
    L, H = spec["chassis"]["length_m"], spec["chassis"]["height_m"]
    WB = spec["chassis"]["wheelbase_m"]

    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=glb_path)
    new = [o for o in bpy.data.objects if o not in before]
    ob = next(o for o in new if o.type == "MESH")
    for o in new:
        if o is not ob:
            bpy.data.objects.remove(o)
    ob.parent = None
    me = ob.data
    me.transform(ob.matrix_world)
    ob.matrix_world = Matrix.Identity(4)
    ob.name = name

    V = np.empty(len(me.vertices) * 3)
    me.vertices.foreach_get("co", V)
    V = V.reshape(-1, 3)

    # --- 1. Scale / centre from the wheels -------------------------------
    span = V[:, 0].max() - V[:, 0].min()
    edge = V[np.abs(V[:, 0] - 0.5 * (V[:, 0].max() + V[:, 0].min())) > 0.48 * span]
    plate_top = edge[:, 2].max()
    low = V[V[:, 2] < edge[:, 2].min() - 0.005]  # tyre contact patches below the plate
    wf, wr = low[low[:, 0] > 0][:, 0].mean(), low[low[:, 0] < 0][:, 0].mean()
    k = WB / (wf - wr)
    mid = 0.5 * (wf + wr)
    ground = V[:, 2].min()
    log(f"base mesh: wheels {wr:.4f} {wf:.4f}, scale {k:.4f}")
    # AI (x fwd, y left, z up) -> car frame (X right = -y, Y fwd = x, Z up).
    P = np.c_[-(V[:, 1]) * k, (V[:, 0] - mid) * k, (V[:, 2] - ground) * k]
    plate_z = (plate_top - ground) * k + 0.01

    # --- 2. Trim the plate --------------------------------------------------
    keep = P[:, 2] > plate_z
    me.vertices.foreach_set("co", P.ravel())
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.verts.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[v for v, kk in zip(bm.verts, keep) if not kk], context="VERTS")
    # Remove small islands (plate fragments).
    islands = []
    seen = set()
    for v in bm.verts:
        if v.index in seen:
            continue
        stack, comp = [v], []
        seen.add(v.index)
        while stack:
            a = stack.pop()
            comp.append(a)
            for e in a.link_edges:
                b = e.other_vert(a)
                if b.index not in seen:
                    seen.add(b.index)
                    stack.append(b)
        islands.append(comp)
    islands.sort(key=len, reverse=True)
    for comp in islands[1:]:
        bmesh.ops.delete(bm, geom=comp, context="VERTS")
    bm.to_mesh(me)
    bm.free()
    P = np.empty(len(me.vertices) * 3)
    me.vertices.foreach_get("co", P)
    P = P.reshape(-1, 3)

    # --- 3. Warp to the blueprint -------------------------------------------
    xs = np.linspace(-L / 2 + 0.01, L / 2 - 0.01, 400)
    sx = np.array(bp["side"]["x"])
    zt_bp = np.interp(xs, sx, np.array(bp["side"]["z_top"]))
    px = np.array(bp["plan"]["x"])
    hw_bp = bridge(px, np.array(bp["plan"]["half_width"]), 0.12, 0.50)
    hw_bp = np.interp(xs, px, hw_bp)

    # Length: map AI x extent onto the blueprint length (tiny, uniform).
    x0, x1 = P[:, 1].min(), P[:, 1].max()
    P[:, 1] = -L / 2 + (P[:, 1] - x0) * L / (x1 - x0)
    # Top silhouette (z) and body half width (y, mirrors bridged).
    zt_ai = station_max(P[:, 1], P[:, 2], xs, 0.006)
    hw_ai = station_max(P[:, 1], np.abs(P[:, 0]), xs, 0.006)
    ok = ~np.isnan(zt_ai)
    zt_ai = np.interp(xs, xs[ok], zt_ai[ok])
    hw_ai = np.interp(xs, xs[ok], hw_ai[ok])
    hw_ai = bridge(xs, hw_ai, 0.12, 0.50)
    fz = smooth1d(zt_bp / zt_ai, 6)
    fy = smooth1d(hw_bp / hw_ai, 6)
    # Ends: the ratio blows up where silhouettes go to zero; clamp.
    fz = np.clip(fz, 0.85, 1.15)
    fy = np.clip(fy, 0.85, 1.15)
    log(f"warp z factor {fz.min():.3f}..{fz.max():.3f}, y factor {fy.min():.3f}..{fy.max():.3f}")
    P[:, 2] *= np.interp(P[:, 1], xs, fz)
    P[:, 0] *= np.interp(P[:, 1], xs, fy)

    # Greenhouse width from the front (windscreen/A-pillar) and rear (rear
    # screen/C-pillar) views, blended along x so there is no seam.
    zz = np.linspace(0.95, 1.29, 35)
    F = {}
    for view, lo, hi in (("front", -0.3, 0.8), ("rear", -1.6, -0.6)):
        bz, bw = np.array(bp[view]["z"]), np.array(bp[view]["half_width"])
        o = np.argsort(bz)
        bz, bw = bz[o], bridge(bz[o], bw[o], 0.78, 1.0)
        sel = (P[:, 1] > lo) & (P[:, 1] < hi)
        mw = np.array([np.abs(P[sel & (np.abs(P[:, 2] - z) < 0.01), 0]).max(initial=1e-3) for z in zz])
        F[view] = smooth1d(np.clip(np.interp(zz, bz, bw) / mw, 0.85, 1.15), 2)
        log(f"{view} greenhouse factors {np.round(F[view][::7], 3)}")
    t = np.clip((P[:, 1] + 0.9) / 0.7, 0, 1)  # 0 at x<-0.9 (rear), 1 at x>-0.2 (front)
    t = t * t * (3 - 2 * t)
    f = (1 - t) * np.interp(P[:, 2], zz, F["rear"]) + t * np.interp(P[:, 2], zz, F["front"])
    w = np.clip((P[:, 2] - 0.92) / 0.06, 0, 1)
    P[:, 0] *= 1 + w * (f - 1)

    # Trim the skirt the generator grows down to its base plate: keep the
    # body above the blueprint bottom silhouette (wheels bridged).
    zb = np.array(bp["side"]["z_bottom"])
    for a in (-WB / 2, WB / 2):
        zb = bridge(sx, zb, a - 0.46, a + 0.46)
    zcut = np.interp(P[:, 1], sx, zb) - 0.004
    below = P[:, 2] < zcut
    # Mirrors are modelled separately: drop the generated ones (outside the
    # bridged plan width, beside the A pillar).
    hw_body = np.interp(P[:, 1], xs, hw_bp)
    # Generated mirrors are replaced by modelled ones: cut them off here, the
    # hole is capped and smoothed after remeshing (see close_and_remesh).

    # Symmetrise: average each vertex with the mirror image of its neighbour
    # is expensive; instead keep the left half and mirror it (exact symmetry).
    me.vertices.foreach_set("co", P.ravel())
    me.update()
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.verts.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[v for v, b in zip(bm.verts, below) if b], context="VERTS")
    bmesh.ops.bisect_plane(bm, geom=bm.verts[:] + bm.edges[:] + bm.faces[:], plane_co=(0, 0, 0), plane_no=(1, 0, 0), clear_outer=True)
    for v in bm.verts:
        if abs(v.co.x) < 1e-4:
            v.co.x = 0.0
    bm.to_mesh(me)
    bm.free()
    mir = ob.modifiers.new("mirror", "MIRROR")
    mir.use_axis[0] = True
    mir.use_clip = True
    mir.use_mirror_merge = True
    mir.merge_threshold = 0.0005

    # --- 4. Relax + decimate ---------------------------------------------------
    sm = ob.modifiers.new("relax", "LAPLACIANSMOOTH")
    sm.iterations = smooth_iters
    sm.lambda_factor = 0.6
    sm.use_volume_preserve = True
    sm.use_normalized = True
    dec = ob.modifiers.new("decimate", "DECIMATE")
    dec.ratio = 1.0
    bpy.context.view_layer.update()
    faces_now = len(me.polygons) * 2
    dec.ratio = min(1.0, target_faces / max(faces_now, 1))
    log(f"base mesh faces (mirrored) {faces_now}, decimate ratio {dec.ratio:.3f}")
    return ob


def retopo(ai_ob, grid_path, name="body", limit=0.08, relax=8):
    """Shrinkwrap the clean loft cage (grid_to_mesh) onto the fitted base mesh.

    The cage has regular quads from the blueprint stations, so the result is
    watertight with even topology (clean reflections, UVs, LODs), while the
    surface detail comes from the image-to-3D mesh.
    """
    from grid_to_mesh import grid_mesh

    cage = grid_mesh(grid_path, name)
    sw = cage.modifiers.new("shrinkwrap", "SHRINKWRAP")
    sw.target = ai_ob
    sw.wrap_method = "PROJECT"
    sw.use_negative_direction = True
    sw.use_positive_direction = True
    sw.project_limit = limit
    sm = cage.modifiers.new("relax", "LAPLACIANSMOOTH")
    sm.iterations = relax
    sm.lambda_factor = 0.5
    sm.use_volume_preserve = True
    return cage


def _gauss2(a, sx, st):
    """Separable gaussian on a (stations, theta) grid; NaN-free input."""
    def g1(v, s, axis):
        r = int(4 * s + 0.5)
        k = np.exp(-0.5 * (np.arange(-r, r + 1) / s) ** 2)
        k /= k.sum()
        v = np.moveaxis(v, axis, -1)
        pad = np.concatenate([np.repeat(v[..., :1], r, -1), v, np.repeat(v[..., -1:], r, -1)], -1)
        out = np.apply_along_axis(lambda q: np.convolve(q, k, mode="valid"), -1, pad)
        return np.moveaxis(out, -1, axis)
    return g1(g1(a, sx, 0), st, 1)


def refine_grid(grid_path, ai_ob, out_path, wb, arch_r=0.47, limit=0.10, sigma=(4.0, 3.0), log=print):
    """Displace the loft grid along its normals onto the fitted base mesh.

    The signed distances are hole-filled (wheel arches, mirrors, misses are
    masked and inpainted by diffusion) and gaussian-smoothed on the regular
    grid, so the large-scale form comes from the image-to-3D mesh while its
    small-scale noise does not.
    """
    from mathutils.bvhtree import BVHTree

    g = dict(np.load(grid_path))
    X, Y, Z = g["X"], g["Y"], g["Z"]  # x fwd, y left, z up
    ns, nt = X.shape
    P = np.stack([-Y, X, Z], -1)  # Blender coords (left half)
    du = np.gradient(P, axis=0)
    dv = np.gradient(P, axis=1)
    N = np.cross(du, dv)
    N /= np.linalg.norm(N, axis=-1, keepdims=True) + 1e-12
    centre = np.stack([np.zeros_like(X), X, 0.5 * (Z.min(1, keepdims=True) + Z.max(1, keepdims=True)).repeat(nt, 1)], -1)
    flip = np.sum(N * (P - centre), -1) < 0
    N[flip] *= -1
    dg = bpy.context.evaluated_depsgraph_get()
    bvh = BVHTree.FromObject(ai_ob, dg)
    D = np.full((ns, nt), np.nan)
    for i in range(ns):
        for k in range(nt):
            p = Vector(P[i, k])
            n = Vector(N[i, k])
            best = None
            for sgn in (1, -1):
                hit = bvh.ray_cast(p, n * sgn, limit)
                if hit[0] is not None:
                    d = sgn * (hit[0] - p).length
                    if best is None or abs(d) < abs(best):
                        best = d
            if best is not None:
                D[i, k] = best
    theta = g["theta"]
    # Masks: wheel arches (tyre hits), underside, ends.
    for a in (-wb / 2, wb / 2):
        m = (np.hypot(X - a, Z - 0.34) < arch_r + 0.05) & (theta[None, :] > np.radians(45))
        D[m] = np.nan
    D[:, theta > np.radians(150)] = 0.0
    D[np.abs(D) > limit * 0.95] = np.nan
    log(f"refine: {np.isnan(D).mean()*100:.1f}% masked/missed, |d| mean {np.nanmean(np.abs(D))*1000:.1f} mm")
    # Inpaint NaNs by diffusion.
    known = ~np.isnan(D)
    F = np.where(known, D, 0.0)
    for _ in range(400):
        up = np.vstack([F[:1], F[:-1]])
        dn = np.vstack([F[1:], F[-1:]])
        lf = np.hstack([F[:, :1], F[:, :-1]])
        rt = np.hstack([F[:, 1:], F[:, -1:]])
        F = np.where(known, D, 0.25 * (up + dn + lf + rt))
    F = _gauss2(F, *sigma)
    F[:, theta > np.radians(150)] = 0.0
    # Fade to zero at the very ends (cap fans).
    w = np.clip(np.minimum(np.arange(ns), ns - 1 - np.arange(ns)) / 4.0, 0, 1)[:, None]
    F *= w
    P2 = P + N * F[..., None]
    g["X"], g["Y"], g["Z"] = P2[..., 1], -P2[..., 0], P2[..., 2]
    np.savez_compressed(out_path, **g)
    return out_path


def close_and_remesh(ob, voxel=0.005, smooth=10, target_faces=220_000, loft=None, log=print):
    """Make the fitted base watertight (cap the bottom), voxel-remesh, relax.

    Voxel remeshing gives even topology without the generator's sliver
    triangles; corrective smoothing removes voxel stair-stepping while keeping
    the form. The bottom cap sits under the car and is hidden by the floor.
    """
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.holes_fill(bm, edges=[e for e in bm.edges if e.is_boundary], sides=0)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.to_mesh(ob.data)
    bm.free()
    r = ob.modifiers.new("remesh", "REMESH")
    r.mode = "VOXEL"
    r.voxel_size = voxel
    r.adaptivity = 0.0
    apply_mods(ob)
    # Heal the mirror cut-outs: strong smoothing inside a soft region.
    vg = ob.vertex_groups.new(name="mirror_heal")
    for v in ob.data.vertices:
        c = v.co
        d = max(abs(c.y - 0.30) / 0.38, abs(c.z - 0.93) / 0.17)
        if abs(c.x) > 0.6 and d < 1.0:
            vg.add([v.index], float(min(1.0, 2.5 * (1.0 - d))), "REPLACE")
    if loft is not None:
        # Pull the region onto the loft (mirrors bridged there), then blend.
        sw = ob.modifiers.new("heal_wrap", "SHRINKWRAP")
        sw.target = loft
        sw.wrap_method = "NEAREST_SURFACEPOINT"
        sw.vertex_group = "mirror_heal"
    hs = ob.modifiers.new("heal", "SMOOTH")
    hs.vertex_group = "mirror_heal"
    hs.factor = 0.5
    hs.iterations = 10
    apply_mods(ob)
    cs = ob.modifiers.new("smooth", "CORRECTIVE_SMOOTH")
    cs.iterations = smooth
    cs.smooth_type = "SIMPLE"
    cs.use_only_smooth = True
    cs.factor = 0.5
    apply_mods(ob)
    n = sum(len(p.vertices) - 2 for p in ob.data.polygons)
    d = ob.modifiers.new("decimate", "DECIMATE")
    d.ratio = min(1.0, target_faces / n)
    apply_mods(ob)
    log(f"remeshed body: {n} -> {sum(len(p.vertices) - 2 for p in ob.data.polygons)} tris")
    return ob


def apply_mods(ob):
    dg = bpy.context.evaluated_depsgraph_get()
    me = bpy.data.meshes.new_from_object(ob.evaluated_get(dg), preserve_all_data_layers=True, depsgraph=dg)
    ob.modifiers.clear()
    old = ob.data
    ob.data = me
    if old.users == 0:
        bpy.data.meshes.remove(old)
