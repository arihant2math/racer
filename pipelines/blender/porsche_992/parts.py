"""Mesh construction helpers for the car build (Blender frame: X right, Y forward, Z up).

Car-frame conventions used by build.py: grid/blueprint "x forward" is Blender +Y,
"y left" is Blender -X. Everything is generated with bmesh, no operators that
depend on UI context except modifier application.
"""
import math

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree


# ---------------------------------------------------------------------------
# Object helpers
# ---------------------------------------------------------------------------
def new_object(name, me, coll=None, mat=None, smooth=True):
    ob = bpy.data.objects.new(name, me)
    (coll or bpy.context.scene.collection).objects.link(ob)
    if mat is not None:
        me.materials.append(mat)
    if smooth:
        for p in me.polygons:
            p.use_smooth = True
    return ob


def bm_to_object(bm, name, coll=None, mat=None, smooth=True):
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    return new_object(name, me, coll, mat, smooth)


def apply_modifiers(ob):
    dg = bpy.context.evaluated_depsgraph_get()
    ev = ob.evaluated_get(dg)
    me = bpy.data.meshes.new_from_object(ev, preserve_all_data_layers=True, depsgraph=dg)
    old = ob.data
    ob.modifiers.clear()
    ob.data = me
    if old.users == 0:
        bpy.data.meshes.remove(old)
    return ob


def boolean(ob, cutter, op="DIFFERENCE", solver="EXACT"):
    m = ob.modifiers.new("bool", "BOOLEAN")
    m.operation = op
    m.object = cutter
    m.solver = solver
    if solver == "EXACT":
        m.use_hole_tolerant = True  # the fitted body is an open shell
    apply_modifiers(ob)
    return ob


def remove(ob):
    me = ob.data
    bpy.data.objects.remove(ob)
    if me and me.users == 0:
        bpy.data.meshes.remove(me)


def tri_count(ob):
    return sum(len(p.vertices) - 2 for p in ob.data.polygons)


# ---------------------------------------------------------------------------
# Primitives
# ---------------------------------------------------------------------------
def lathe(profile, segments, name, axis="X", coll=None, mat=None, cap=False, angle=2 * math.pi):
    """Revolve a (radius, axial) profile about the given axis through the origin.

    profile: list of (r, a). Faces are outward if the profile runs so that the
    surface is on its left when walking in increasing index (standard lathe).
    """
    bm = bmesh.new()
    rings = []
    full = abs(angle - 2 * math.pi) < 1e-6
    n = segments if full else segments + 1
    for k in range(n):
        t = angle * k / segments
        c, s = math.cos(t), math.sin(t)
        ring = []
        for r, a in profile:
            if axis == "X":
                co = (a, r * c, r * s)
            elif axis == "Y":
                co = (r * s, a, r * c)
            else:
                co = (r * c, r * s, a)
            ring.append(bm.verts.new(co))
        rings.append(ring)
    for k in range(segments):
        r0, r1 = rings[k], rings[(k + 1) % n]
        for i in range(len(profile) - 1):
            bm.faces.new((r0[i], r0[i + 1], r1[i + 1], r1[i]))
    bm.normal_update()
    return bm_to_object(bm, name, coll, mat)


def tube_along(points, radius, name, sides=12, coll=None, mat=None, closed=False):
    """Circular tube along a polyline (Blender coordinates)."""
    pts = [Vector(p) for p in points]
    bm = bmesh.new()
    rings = []
    n = len(pts)
    for i, p in enumerate(pts):
        if closed:
            t = (pts[(i + 1) % n] - pts[i - 1]).normalized()
        else:
            t = (pts[min(i + 1, n - 1)] - pts[max(i - 1, 0)]).normalized()
        a = Vector((0, 0, 1)) if abs(t.z) < 0.9 else Vector((1, 0, 0))
        u = t.cross(a).normalized()
        v = t.cross(u).normalized()
        rings.append([bm.verts.new(p + radius * (math.cos(2 * math.pi * k / sides) * u + math.sin(2 * math.pi * k / sides) * v)) for k in range(sides)])
    for i in range(n if closed else n - 1):
        r0, r1 = rings[i], rings[(i + 1) % n]
        for k in range(sides):
            bm.faces.new((r0[k], r0[(k + 1) % sides], r1[(k + 1) % sides], r1[k]))
    if not closed:
        bm.faces.new(rings[0][::-1])
        bm.faces.new(rings[-1])
    bm.normal_update()
    return bm_to_object(bm, name, coll, mat)


def prism(outline, z0, z1, name, coll=None, mat=None, axis="Y", bevel=0.0):
    """Extrude a closed 2D outline (u, v) between depths z0..z1 along an axis.

    axis 'Y': outline in (x, z), extruded along y. axis 'X': outline in (y, z).
    axis 'Z': outline in (x, y).
    """
    bm = bmesh.new()

    def co(u, v, w):
        if axis == "Y":
            return (u, w, v)
        if axis == "X":
            return (w, u, v)
        return (u, v, w)

    a = [bm.verts.new(co(u, v, z0)) for u, v in outline]
    b = [bm.verts.new(co(u, v, z1)) for u, v in outline]
    n = len(outline)
    for i in range(n):
        bm.faces.new((a[i], a[(i + 1) % n], b[(i + 1) % n], b[i]))
    bm.faces.new(a[::-1])
    bm.faces.new(b)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    if bevel > 0:
        bmesh.ops.bevel(bm, geom=list(bm.edges), offset=bevel, segments=2, profile=0.5, affect="EDGES")
    return bm_to_object(bm, name, coll, mat, smooth=bevel > 0)


def resample_closed(pts, n):
    """Resample a closed polyline to n points by arc length."""
    p = np.asarray(pts, float)
    if np.linalg.norm(p[0] - p[-1]) > 1e-6:
        p = np.vstack([p, p[:1]])
    d = np.r_[0, np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))]
    t = np.linspace(0, d[-1], n + 1)[:-1]
    return np.c_[[np.interp(t, d, p[:, k]) for k in range(p.shape[1])]].T


def resample_open(pts, n):
    p = np.asarray(pts, float)
    d = np.r_[0, np.cumsum(np.linalg.norm(np.diff(p, axis=0), axis=1))]
    t = np.linspace(0, d[-1], n)
    return np.c_[[np.interp(t, d, p[:, k]) for k in range(p.shape[1])]].T


def smooth_closed(p, it=3):
    p = np.asarray(p, float)
    for _ in range(it):
        p = 0.25 * np.roll(p, 1, 0) + 0.5 * p + 0.25 * np.roll(p, -1, 0)
    return p


# ---------------------------------------------------------------------------
# Surface projection: build parts that hug the body shell
# ---------------------------------------------------------------------------
class Surface:
    """Ray-cast projection onto a mesh object (the body shell)."""

    def __init__(self, ob):
        dg = bpy.context.evaluated_depsgraph_get()
        self.bvh = BVHTree.FromObject(ob, dg)
        self.mw = ob.matrix_world.copy()

    def project(self, p, d):
        """Hit along direction d from far outside p (returns location, normal)."""
        d = Vector(d).normalized()
        org = Vector(p) - d * 3.0
        loc, nrm, _, _ = self.bvh.ray_cast(org, d, 6.0)
        if loc is None:
            # Holes in the shell: take the surface point closest to the ray.
            best = None
            for k in range(121):
                q = org + d * (6.0 * k / 120)
                hit = self.bvh.find_nearest(q, 0.08)
                if hit[0] is not None and (best is None or hit[3] < best[3]):
                    best = hit
            if best is not None:
                loc, nrm = best[0], best[1]
                if nrm.dot(d) > 0:
                    nrm = -nrm
        return loc, nrm

    def side(self, x_fwd, z, left=True):
        """Point on the body side at car x (forward) and height z."""
        s = -1.0 if left else 1.0
        loc, n = self.project((s * 2.0, x_fwd, z), (-s, 0, 0))
        if loc is None:
            return None, None
        return loc, n

    def top(self, x_fwd, y_left):
        loc, n = self.project((-y_left, x_fwd, 3.0), (0, 0, -1))
        return loc, n

    def front(self, y_left, z, rear=False):
        s = -1.0 if rear else 1.0
        loc, n = self.project((-y_left, s * 3.0, z), (0, -s, 0))
        return loc, n

    def nearest(self, p):
        loc, n, _, _ = self.bvh.find_nearest(Vector(p))
        return loc, n


def shell_patch(surface, outline_pts, direction, name, offset=0.002, thickness=0.0, grid=24, coll=None, mat=None, inset=0.0):
    """A patch that follows the shell inside a closed outline.

    outline_pts: closed polyline in Blender coords lying roughly on the shell.
    direction: projection direction (from outside toward the shell).
    The patch is a fan/grid: rings shrinking toward the outline centroid,
    each point re-projected onto the shell and pushed out by `offset`.
    """
    d = Vector(direction).normalized()
    pts = [Vector(p) for p in outline_pts]
    c = sum(pts, Vector()) / len(pts)
    bm = bmesh.new()
    rings = []
    for k in range(grid + 1):
        f = 1.0 - k / grid
        f = f ** 0.8
        ring = []
        for p in pts:
            q = c + (p - c) * f
            if inset and k == 0:
                q = c + (p - c) * (1 - inset)
            loc, n = surface.project(q, d)
            if loc is None:
                loc, n = Vector(q), -d
            ring.append(loc + n * offset)
        rings.append(ring)
    vr = [[bm.verts.new(p) for p in r] for r in rings[:-1]]
    centre = bm.verts.new(sum(rings[-1], Vector()) / len(rings[-1]))
    n = len(pts)
    for k in range(len(vr) - 1):
        for i in range(n):
            bm.faces.new((vr[k][i], vr[k][(i + 1) % n], vr[k + 1][(i + 1) % n], vr[k + 1][i]))
    for i in range(n):
        bm.faces.new((vr[-1][i], vr[-1][(i + 1) % n], centre))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    # Orient outward (against the projection direction).
    out = sum((f.normal for f in bm.faces), Vector())
    if out.dot(d) > 0:
        for f in bm.faces:
            f.normal_flip()
    if thickness > 0:
        ret = bmesh.ops.solidify(bm, geom=list(bm.faces), thickness=thickness)
    return bm_to_object(bm, name, coll, mat)


def ribbon_on_surface(surface, curve_pts, direction, width, name, offset=0.001, coll=None, mat=None, normal_offset=0.0, closed=False, min_facing=0.25):
    """A thin strip following a curve on the shell (trim, seals, light strips).

    Points whose surface normal does not face the projection (grazing hits on
    another panel) are dropped.
    """
    d = Vector(direction).normalized()
    pts = []
    nrms = []
    for p in curve_pts:
        loc, n = surface.project(p, d)
        if loc is None or n.dot(-d) < min_facing:
            continue
        pts.append(loc)
        nrms.append(n)
    bm = bmesh.new()
    a, b = [], []
    m = len(pts)
    for i in range(m):
        t = (pts[(i + 1) % m] - pts[i - 1]) if closed else (pts[min(i + 1, m - 1)] - pts[max(i - 1, 0)])
        t.normalize()
        side = nrms[i].cross(t).normalized()
        base = pts[i] + nrms[i] * (offset + normal_offset)
        a.append(bm.verts.new(base - side * width / 2))
        b.append(bm.verts.new(base + side * width / 2))
    for i in range(m if closed else m - 1):
        j = (i + 1) % m
        bm.faces.new((a[i], a[j], b[j], b[i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    return bm_to_object(bm, name, coll, mat)


def points_in_polygon(pts, poly):
    """Even-odd test, vectorised over pts (N,2) against a closed polygon (M,2)."""
    x, y = pts[:, 0][:, None], pts[:, 1][:, None]
    px, py = poly[:, 0][None, :], poly[:, 1][None, :]
    qx, qy = np.roll(poly[:, 0], -1)[None, :], np.roll(poly[:, 1], -1)[None, :]
    cond = (py > y) != (qy > y)
    xint = px + (y - py) * (qx - px) / np.where(qy - py == 0, 1e-12, qy - py)
    return np.sum(cond & (x < xint), axis=1) % 2 == 1


def open_aperture(ob, poly, to2d, facing, min_facing=0.2):
    """Delete faces whose centres project inside `poly` and face `facing`.

    to2d: function mapping a face centre (Vector) to the outline's 2D frame.
    """
    me = ob.data
    n = len(me.polygons)
    cen = np.empty(n * 3)
    nor = np.empty(n * 3)
    me.polygons.foreach_get("center", cen)
    me.polygons.foreach_get("normal", nor)
    cen, nor = cen.reshape(-1, 3), nor.reshape(-1, 3)
    uv = np.array([to2d(Vector(c)) for c in cen])
    inside = points_in_polygon(uv, np.asarray(poly, float))
    face_ok = nor @ np.asarray(facing, float) > min_facing
    kill = np.nonzero(inside & face_ok)[0]
    bm = bmesh.new()
    bm.from_mesh(me)
    bm.faces.ensure_lookup_table()
    bmesh.ops.delete(bm, geom=[bm.faces[i] for i in kill], context="FACES")
    bm.to_mesh(me)
    bm.free()
    return len(kill)
