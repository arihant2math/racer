"""Wheels, mirrors, interior, sockets, collision, LODs and export."""

import math
import os
import re

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector
from mathutils.bvhtree import BVHTree

from . import meshkit

WHEELS = ("fl", "fr", "rl", "rr")


def tire_dims(spec):
    """'245/35 ZR20' -> (section width, sidewall height, rim radius, outer radius) m."""
    m = re.match(r"(\d+)/(\d+)\s*Z?R\s*(\d+)", spec)
    w, ar, rim = int(m.group(1)), int(m.group(2)), int(m.group(3))
    width = w / 1000
    side = width * ar / 100
    rr = rim * 0.0254 / 2
    return width, side, rr, rr + side


class Frame:
    """Body coordinates (s from the nose, y lateral, z up) <-> Blender space
    (+X right, +Y forward, +Z up, origin on the ground at the wheelbase midpoint)."""

    def __init__(self, body, s_mid, chassis, tires, cfg):
        self.body = body
        self.s_mid = s_mid
        self.ch = chassis
        self.cfg = cfg
        self.wheel = {}
        for key, axle, spec, track in (("fl", 0, tires["front"], chassis["track_front_m"]),
                                       ("fr", 0, tires["front"], chassis["track_front_m"]),
                                       ("rl", 1, tires["rear"], chassis["track_rear_m"]),
                                       ("rr", 1, tires["rear"], chassis["track_rear_m"])):
            width, side, rr, r = tire_dims(spec)
            x = track / 2 * (-1 if key[1] == "l" else 1)
            s = body.arches[axle]["s"]
            self.wheel[key] = dict(center=Vector((x, s_mid - s, r)), width=width, sidewall=side,
                                   rim_r=rr, radius=r, spec=spec, axle=axle)
        self.bvh = None
        self.brakes = {}

    def b(self, s, y, z):
        return Vector((y, self.s_mid - s, z))

    def set_shell(self, ob):
        me = ob.data
        verts = [v.co.copy() for v in me.vertices]
        polys = [tuple(p.vertices) for p in me.polygons]
        self.bvh = BVHTree.FromPolygons(verts, polys)

    def ray(self, origin, direction, dist=10.0):
        """First hit (location, normal) on the original shell, or (None, None)."""
        loc, nor, _, _ = self.bvh.ray_cast(Vector(origin), Vector(direction).normalized(), dist)
        return loc, nor

    def surface_from_front(self, y, z):
        return self.ray(self.b(-1.0, y, z), (0, -1, 0))

    def surface_from_rear(self, y, z):
        return self.ray(self.b(self.body.length + 1.0, y, z), (0, 1, 0))

    def surface_from_top(self, s, y):
        return self.ray(self.b(s, y, 3.0), (0, 0, -1))

    def surface_from_side(self, s, z, side=1):
        return self.ray(self.b(s, side * 3.0, z), (-side, 0, 0))


# ---------------------------------------------------------------------------
# Wheels


def _tire(width, side, rr, r, seg):
    """Tire profile revolved about X; tread with four circumferential grooves."""
    half = width / 2
    bead = half * 0.86
    tread = half * 0.84
    prof = [(rr + 0.004, -bead), (rr + side * 0.25, -half * 0.97), (rr + side * 0.55, -half),
            (rr + side * 0.82, -half * 0.97), (r - 0.012, -tread - 0.004), (r - 0.003, -tread + 0.004)]
    grooves = [-0.55, -0.18, 0.18, 0.55]
    xs = np.linspace(-tread + 0.004, tread - 0.004, 41)
    for x in xs[1:-1]:
        depth = 0.0
        for gc in grooves:
            if abs(x / tread - gc) < 0.045:
                depth = 0.007
        prof.append((r - depth, x))
    prof += [(r - 0.003, tread - 0.004), (r - 0.012, tread + 0.004), (rr + side * 0.82, half * 0.97),
             (rr + side * 0.55, half), (rr + side * 0.25, half * 0.97), (rr + 0.004, bead)]
    v, f, uv = meshkit.revolve(prof, seg, axis="X", u_scale=8.0)
    return v, f, uv


def _rim(width, rr, seg, spokes):
    """Rim barrel and face with paired spokes, outer face toward +X."""
    bm = bmesh.new()
    uvl = bm.loops.layers.uv.new("UVMap")
    lip = rr + 0.012
    wr = width * 0.9 / 2
    face_x = wr - 0.012
    prof = [(rr - 0.03, -wr + 0.01), (lip, -wr), (lip, -wr + 0.012), (rr - 0.004, -wr + 0.02),
            (rr - 0.004, -wr * 0.2), (rr - 0.03, 0.0), (rr - 0.03, wr * 0.35), (rr - 0.004, wr * 0.55),
            (rr - 0.004, wr - 0.02), (lip, wr - 0.008), (lip + 0.002, wr), (rr - 0.006, wr + 0.002),
            (rr - 0.022, wr - 0.012)]
    v, f, uv = meshkit.revolve(prof, seg, axis="X", u_scale=4.0)
    _add(bm, uvl, v, f, uv, 0)
    # Spokes: tapered, slightly concave toward the hub, 5 pairs (Carrera S wheel).
    n_pairs = spokes // 2
    r_in, r_out = 0.075, rr - 0.018
    for k in range(n_pairs):
        for off in (-0.13, 0.13):
            th = 2 * math.pi * k / n_pairs
            _spoke(bm, uvl, (th + off * 0.3, th + off), r_in, r_out, face_x, 0.022, 0.016, 0.03)
    # Hub and centre cap.
    hv, hf, huv = meshkit.revolve([(0.0, face_x + 0.012), (0.04, face_x + 0.012), (0.05, face_x + 0.006),
                                   (0.088, face_x), (0.092, face_x - 0.03), (0.06, face_x - 0.06)],
                                  48, axis="X")
    _add(bm, uvl, hv, hf, huv, 0)
    cv, cf, cuv = meshkit.revolve([(0.0, face_x + 0.016), (0.032, face_x + 0.015), (0.035, face_x + 0.01)],
                                  32, axis="X")
    _add(bm, uvl, cv, cf, cuv, 1)
    for k in range(5):
        th = 2 * math.pi * (k + 0.5) / 5
        bv, bf, buv = meshkit.revolve([(0.0, face_x + 0.011), (0.009, face_x + 0.011), (0.01, face_x)], 12,
                                      axis="X")
        bv = bv + np.array([0, 0.062 * math.cos(th), 0.062 * math.sin(th)])
        _add(bm, uvl, bv, bf, buv, 1)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-6)
    return bm


def _spoke(bm, uvl, ths, r_in, r_out, face_x, w_in, w_out, depth):
    """One spoke from the hub to the rim; its angle runs from ths[0] to ths[1]."""
    n = 10
    rings = []
    for i in range(n + 1):
        t = i / n
        th = ths[0] + (ths[1] - ths[0]) * t
        c, s = math.cos(th), math.sin(th)
        radial = np.array([0, c, s])
        tang = np.array([0, -s, c])
        r = r_in + (r_out - r_in) * t
        w = (w_in + (w_out - w_in) * t) / 2
        x = face_x - 0.012 * math.sin(math.pi * t) - 0.004 * t
        d = depth * (1 - 0.3 * t)
        base = radial * r
        rings.append([base + tang * -w + np.array([x, 0, 0]),
                      base + tang * w + np.array([x, 0, 0]),
                      base + tang * w * 1.1 + np.array([x - d, 0, 0]),
                      base + tang * -w * 1.1 + np.array([x - d, 0, 0])])
    vs = [[bm.verts.new(p) for p in ring] for ring in rings]
    for i in range(n):
        for k in range(4):
            k2 = (k + 1) % 4
            f = bm.faces.new((vs[i][k], vs[i][k2], vs[i + 1][k2], vs[i + 1][k]))
            f.material_index = 0
            for li, (a, b) in zip(f.loops, ((i, k), (i, k2), (i + 1, k2), (i + 1, k))):
                li[uvl].uv = (b * 0.25, a / n)
    f = bm.faces.new(vs[0][::-1])
    f.material_index = 0


def _add(bm, uvl, verts, faces, uvs, mat):
    vs = [bm.verts.new(v) for v in verts]
    for fi, f in enumerate(faces):
        try:
            face = bm.faces.new([vs[i] for i in f])
        except ValueError:
            continue
        face.material_index = mat
        if uvs is not None:
            for li, uv in zip(face.loops, uvs[fi]):
                li[uvl].uv = uv
    return vs


def _brake(disc_r, caliper_side):
    bm = bmesh.new()
    uvl = bm.loops.layers.uv.new("UVMap")
    t = 0.032
    x0 = -0.01
    dv, df, duv = meshkit.revolve([(0.10, x0 + t / 2), (disc_r, x0 + t / 2), (disc_r, x0 - t / 2),
                                   (0.10, x0 - t / 2), (0.10, x0 + t / 2)], 64, axis="X")
    _add(bm, uvl, dv, df, duv, 0)
    hv, hf, huv = meshkit.revolve([(0.0, x0 + 0.03), (0.1, x0 + 0.03), (0.1, x0)], 32, axis="X")
    _add(bm, uvl, hv, hf, huv, 1)
    # Caliper: a curved block over the disc edge, toward the rear and up.
    cal = bmesh.new()
    bmesh.ops.create_cube(cal, size=1.0)
    for v in cal.verts:
        v.co = Vector((v.co.x * 0.075, v.co.y * 0.16, v.co.z * 0.07))
    bmesh.ops.bevel(cal, geom=list(cal.edges), offset=0.012, segments=2, affect="EDGES")
    ang = math.radians(caliper_side)
    rot = Matrix.Rotation(ang, 4, "X")
    for v in cal.verts:
        v.co = rot @ (v.co + Vector((x0, 0, disc_r - 0.025)))
    me = bpy.data.meshes.new("tmp")
    cal.to_mesh(me)
    cal.free()
    bm.from_mesh(me)
    bpy.data.meshes.remove(me)
    return bm


def wheels(geo, mats):
    out = {}
    for key in WHEELS:
        w = geo.wheel[key]
        seg = 128
        tv, tf, tuv = _tire(w["width"], w["sidewall"], w["rim_r"], w["radius"], seg)
        bm = _rim(w["width"], w["rim_r"], seg, spokes=10)
        uvl = bm.loops.layers.uv.active
        _add(bm, uvl, tv, tf, tuv, 2)
        left = key[1] == "l"
        if left:
            bmesh.ops.scale(bm, vec=(-1, 1, 1), verts=bm.verts)
            bmesh.ops.reverse_faces(bm, faces=bm.faces)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        ob = meshkit.bm_to_object(f"wheel_{key}", bm, [mats["rim"], mats["chrome"], mats["rubber"]])
        bm.free()
        meshkit.set_sharp_by_angle(ob, 35)
        ob.location = w["center"]
        out[ob.name] = ob
        # Brake disc and caliper (do not spin with the wheel; same origin).
        disc_mm = geo.brakes.get("front_disc_mm" if w["axle"] == 0 else "rear_disc_mm", 330.0)
        disc_r = disc_mm / 2000
        bm = _brake(disc_r, 60 if w["axle"] == 0 else -60)
        if left:
            bmesh.ops.scale(bm, vec=(-1, 1, 1), verts=bm.verts)
            bmesh.ops.reverse_faces(bm, faces=bm.faces)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        ob = meshkit.bm_to_object(f"brake_{key}", bm, [mats["brake_caliper"], mats["trim_black"]])
        bm.free()
        _fix_brake_materials(ob, disc_r)
        meshkit.set_sharp_by_angle(ob, 35)
        meshkit.box_uv(ob, 2.0)
        ob.location = w["center"]
        out[ob.name] = ob
    return out


def _fix_brake_materials(ob, disc_r):
    """Disc faces (revolved) get the metal slot; caliper faces the red slot."""
    me = ob.data
    if len(me.materials) < 2:
        return
    for p in me.polygons:
        c = p.center
        r = math.hypot(c.y, c.z)
        on_disc = abs(c.x + 0.01) <= 0.0165 + 1e-4 and 0.099 <= r <= disc_r + 1e-4
        hat = r < 0.101 and -0.011 <= c.x <= 0.021
        p.material_index = 1 if (on_disc or hat) else 0


# ---------------------------------------------------------------------------
# Mirrors


def mirrors(geo, mats):
    m = geo.cfg["mirror"]
    out = {}
    for side, name in ((1, "mirror_r"), (-1, "mirror_l")):
        bm = bmesh.new()
        uvl = bm.loops.layers.uv.new("UVMap")
        c = geo.b(m["s"], m["y"], m["z"])
        hv, hf, huv = meshkit.ellipsoid((0, 0, 0), (m["size"][0] / 2, m["size"][1] / 2, m["size"][2] / 2), 40, 20)
        # Flatten the rear face (mirror glass side) and taper the inner end.
        hv = np.array(hv)
        rear = hv[:, 1] < -m["size"][1] * 0.15
        hv[rear, 1] = -m["size"][1] * 0.15 + (hv[rear, 1] + m["size"][1] * 0.15) * 0.25
        hv[:, 2] *= 1.0 - 0.25 * np.clip(-hv[:, 0] / (m["size"][0] / 2), 0, 1)
        _add(bm, uvl, hv, hf, huv, 0)
        # Glass: an inset ellipse on the flat rear face.
        n = 32
        gy = -m["size"][1] * 0.15 - m["size"][1] / 2 * 0.25 - 0.002
        ring = [Vector((m["size"][0] / 2 * 0.85 * math.cos(2 * math.pi * k / n),
                        gy, m["size"][2] / 2 * 0.78 * math.sin(2 * math.pi * k / n))) for k in range(n)]
        vs = [bm.verts.new(p) for p in ring]
        f = bm.faces.new(vs[::-1])
        f.material_index = 1
        # Stalk from the door to the housing.
        st = bmesh.new()
        bmesh.ops.create_cube(st, size=1.0)
        L = m["stalk_len"]
        for v in st.verts:
            v.co = Vector((v.co.x * L - L / 2 - m["size"][0] * 0.3, v.co.y * 0.05, v.co.z * 0.022 - m["size"][2] * 0.25))
        bmesh.ops.bevel(st, geom=list(st.edges), offset=0.008, segments=2, affect="EDGES")
        me = bpy.data.meshes.new("tmp")
        st.to_mesh(me)
        st.free()
        bm.from_mesh(me)
        bpy.data.meshes.remove(me)
        if side < 0:
            bmesh.ops.scale(bm, vec=(-1, 1, 1), verts=bm.verts)
            bmesh.ops.reverse_faces(bm, faces=bm.faces)
        bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
        bmesh.ops.translate(bm, vec=Vector((c.x * side, c.y, c.z)), verts=bm.verts)
        ob = meshkit.bm_to_object(name, bm, [mats["car_paint"], mats["chrome"], mats["trim_black"]])
        bm.free()
        meshkit.set_sharp_by_angle(ob, 40)
        meshkit.box_uv(ob, 2.0)
        out[name] = ob
    return out


# ---------------------------------------------------------------------------
# Interior


def interior(geo, mats):
    """Simple cabin: inner shell (headliner, door cards, floor), seats, dash,
    steering wheel, console. Enough to read through the glass."""
    ic = geo.cfg["interior"]
    body = geo.body
    objs = []
    mat = [mats["interior"], mats["trim_black"], mats["chrome"]]
    # Inner shell: cabin sections shrunk toward the cabin centre, normals inward.
    s0, s1 = ic["shell_s"]
    st = np.linspace(s0, s1, 48)
    rings = []
    for s in st:
        p, b = body.section(s)
        sel = b >= 3
        p = p[sel]
        cz = ic["shell_center_z"]
        q = p.copy()
        q[:, 0] = p[:, 0] * ic["shell_scale"][0]
        q[:, 1] = cz + (p[:, 1] - cz) * ic["shell_scale"][1]
        q[:, 1] = np.maximum(q[:, 1], ic["floor_z"])
        full = np.vstack([q, (q[::-1] * [-1, 1])[1:]])
        rings.append(full)
    R = len(rings[0])
    verts = []
    for s, ring in zip(st, rings):
        for y, z in ring:
            verts.append(tuple(geo.b(s, y, z)))
    faces, uvs = [], []
    for i in range(len(st) - 1):
        for r in range(R - 1):
            a, b2, c, d = i * R + r, i * R + r + 1, (i + 1) * R + r + 1, (i + 1) * R + r
            faces.append((a, b2, c, d))
            uvs.append([(st[i], r / R), (st[i], (r + 1) / R), (st[i + 1], (r + 1) / R), (st[i + 1], r / R)])
    # Floor and end walls (firewall, rear bulkhead) close the tub.
    shell = meshkit.mesh_object("interior_shell", np.array(verts), faces, uvs=uvs, materials=mat)
    bm = bmesh.new()
    bm.from_mesh(shell.data)
    bm.verts.ensure_lookup_table()
    for i in (0, len(st) - 1):
        ring = [bm.verts[i * R + r] for r in range(R)]
        try:
            f = bm.faces.new(ring if i == 0 else ring[::-1])
            f.material_index = 0
        except ValueError:
            pass
    floor_ring = []
    bm.to_mesh(shell.data)
    bm.free()
    _flip_inward(shell)
    objs.append(shell)
    fz = ic["floor_z"]
    dx = ic["driver_x"]
    for x in (dx, -dx):
        sx = x
        objs.append(meshkit.rounded_box("seat_base", (0.50, 0.52, 0.12), geo.b(ic["seat_s"], sx, fz + 0.12), 0.04, mat))
        back = meshkit.rounded_box("seat_back", (0.50, 0.12, 0.62), (0, 0, 0), 0.05, mat)
        back.data.transform(Matrix.Rotation(math.radians(-18), 4, "X"))
        back.data.transform(Matrix.Translation(geo.b(ic["seat_s"] + 0.27, sx, fz + 0.45)))
        objs.append(back)
        for bx in (-0.22, 0.22):
            objs.append(meshkit.rounded_box("bolster", (0.07, 0.40, 0.14), geo.b(ic["seat_s"], sx + bx, fz + 0.22), 0.03, mat))
        objs.append(meshkit.rounded_box("headrest", (0.26, 0.10, 0.18), geo.b(ic["seat_s"] + 0.38, sx, fz + 0.86), 0.04, mat))
    # Rear seats (2+2), dash, console.
    objs.append(meshkit.rounded_box("rear_seats", (1.10, 0.35, 0.30), geo.b(ic["rear_seat_s"], 0, fz + 0.22), 0.05, mat))
    dash_s = ic["dash_s"]
    objs.append(meshkit.rounded_box("dash", (1.45, 0.42, 0.20), geo.b(dash_s, 0, ic["dash_z"]), 0.06, mat, mat=1))
    objs.append(meshkit.rounded_box("dash_top", (1.40, 0.30, 0.10), geo.b(dash_s - 0.08, 0, ic["dash_z"] + 0.12), 0.05, mat))
    objs.append(meshkit.rounded_box("console", (0.24, 0.85, 0.28), geo.b(dash_s + 0.55, 0, fz + 0.18), 0.04, mat, mat=1))
    # Steering wheel: torus in a plane tilted toward the driver.
    sw = bmesh.new()
    bmesh.ops.create_circle(sw, segments=40, radius=0.18)
    me = bpy.data.meshes.new("swc")
    sw.to_mesh(me)
    sw.free()
    tv, tf, tuv = meshkit.revolve([(0.17 + 0.016 * math.cos(a), 0.016 * math.sin(a))
                                   for a in np.linspace(0, 2 * math.pi, 13)], 48, axis="Z")
    wheel = meshkit.mesh_object("steering_wheel", tv, tf, uvs=tuv, materials=mat, mat_index=[1] * len(tf))
    hub = meshkit.rounded_box("sw_hub", (0.12, 0.10, 0.04), (0, 0, 0), 0.015, mat, mat=1)
    spoke = meshkit.rounded_box("sw_spoke", (0.34, 0.035, 0.02), (0, 0, 0), 0.008, mat, mat=1)
    wheel = meshkit.join([wheel, hub, spoke], "steering_wheel")
    wheel.data.transform(Matrix.Rotation(math.radians(90 - ic["wheel_tilt_deg"]), 4, "X"))
    wheel.data.transform(Matrix.Translation(geo.b(ic["wheel_s"], dx, ic["wheel_z"])))
    objs.append(wheel)
    bpy.data.meshes.remove(me)
    ob = meshkit.join(objs, "interior")
    for i, m in enumerate(mat):
        if len(ob.data.materials) <= i:
            ob.data.materials.append(m)
    meshkit.box_uv(ob, 1.0)
    return ob


def _flip_inward(ob):
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bmesh.ops.reverse_faces(bm, faces=bm.faces)
    bm.to_mesh(ob.data)
    bm.free()


# ---------------------------------------------------------------------------
# Sockets, collision, finishing, export


def sockets(geo, cfg):
    out = []
    for key in WHEELS:
        out.append(meshkit.empty(f"socket_wheel_{key}", geo.wheel[key]["center"]))
    back = (0, 0, math.pi)
    for name, (s, y, z), rear in [(k, v, "tail" in k or "brake" in k or "reverse" in k)
                                 for k, v in cfg["sockets"]["lights"].items()]:
        for side, sfx in (((1, "_r"), (-1, "_l")) if y > 0 else ((1, ""),)):
            out.append(meshkit.empty(f"socket_light_{name}{sfx}", geo.b(s, side * y, z),
                                     back if rear else (0, 0, 0)))
    s, y, z = cfg["sockets"]["exhaust"]
    out.append(meshkit.empty("socket_exhaust_r", geo.b(s, y, z), back))
    out.append(meshkit.empty("socket_exhaust_l", geo.b(s, -y, z), back))
    s, y, z = cfg["sockets"]["camera_hood"]
    out.append(meshkit.empty("socket_camera_hood", geo.b(s, y, z)))
    s, y, z = cfg["sockets"]["camera_cockpit"]
    out.append(meshkit.empty("socket_camera_cockpit", geo.b(s, y, z)))
    return out


def _drop_loose(bm, hull_result):
    """Deletes the points convex_hull left inside or unused."""
    kill = {g for k in ("geom_interior", "geom_unused") for g in hull_result[k]
            if isinstance(g, bmesh.types.BMVert)}
    bmesh.ops.delete(bm, geom=list(kill), context="VERTS")


def _hull_points(p):
    tmp = bmesh.new()
    vs = [tmp.verts.new(v) for v in p[:: max(1, len(p) // 6000)]]
    res = bmesh.ops.convex_hull(tmp, input=vs)
    _drop_loose(tmp, res)
    out = np.array([v.co[:] for v in tmp.verts])
    tmp.free()
    return out


def _farthest_points(p, n):
    if len(p) <= n:
        return p
    idx = [int(np.argmax(p[:, 2]))]
    d = np.linalg.norm(p - p[idx[0]], axis=1)
    for _ in range(n - 1):
        i = int(np.argmax(d))
        idx.append(i)
        d = np.minimum(d, np.linalg.norm(p - p[i], axis=1))
    return p[idx]


def collision(objs, geo):
    """A few convex pieces: lower body split along the length, plus the cabin."""
    pts = []
    for name in ("body", "hood", "bumper_front", "bumper_rear", "door_l", "door_r", "spoiler", "glass"):
        ob = objs.get(name)
        if ob is None:
            continue
        a = np.zeros(len(ob.data.vertices) * 3)
        ob.data.vertices.foreach_get("co", a)
        pts.append(a.reshape(-1, 3))
    pts = np.vstack(pts)
    belt = geo.cfg["collision"]["belt_z"]
    cuts = geo.cfg["collision"]["cuts_s"]
    ys = [geo.s_mid - c for c in cuts]
    bounds = [10.0] + ys + [-10.0]
    pieces = []
    for hi, lo in zip(bounds[:-1], bounds[1:]):
        sel = (pts[:, 1] <= hi + 0.02) & (pts[:, 1] >= lo - 0.02) & (pts[:, 2] <= belt + 0.02)
        pieces.append(pts[sel])
    c0, c1 = geo.cfg["collision"]["cabin_s"]
    sel = (pts[:, 1] <= geo.s_mid - c0) & (pts[:, 1] >= geo.s_mid - c1) & (pts[:, 2] >= belt - 0.02)
    pieces.append(pts[sel])
    bm = bmesh.new()
    for p in pieces:
        if len(p) < 4:
            continue
        hull_pts = _hull_points(p)
        # Fewer vertices by farthest-point sampling of the hull's own points:
        # a subset of the points, so the simplified hull stays inside the body.
        keep = _farthest_points(hull_pts, 48)
        tmp = bmesh.new()
        vs = [tmp.verts.new(v) for v in keep]
        res = bmesh.ops.convex_hull(tmp, input=vs)
        _drop_loose(tmp, res)
        me = bpy.data.meshes.new("hull")
        tmp.to_mesh(me)
        tmp.free()
        bm.from_mesh(me)
        bpy.data.meshes.remove(me)
    ob = meshkit.bm_to_object("collision", bm, [], smooth=False)
    bm.free()
    ob.display_type = "WIRE"
    meshkit.box_uv(ob)
    return ob


def finish(objs, geo, cfg, mats):
    for name, ob in objs.items():
        ob.name = name
        ob.data.name = name
        if not ob.data.uv_layers:
            meshkit.box_uv(ob, 1.0)


def measure(objs):
    def bbox(names):
        lo = np.full(3, 1e9)
        hi = np.full(3, -1e9)
        for n in names:
            ob = objs.get(n)
            if ob is None:
                continue
            a = np.zeros(len(ob.data.vertices) * 3)
            ob.data.vertices.foreach_get("co", a)
            a = a.reshape(-1, 3) + np.array(ob.location)
            lo = np.minimum(lo, a.min(0))
            hi = np.maximum(hi, a.max(0))
        return lo, hi

    body_parts = ["body", "hood", "bumper_front", "bumper_rear", "door_l", "door_r", "spoiler", "glass",
                  "lights_front", "lights_rear"]
    lo, hi = bbox(body_parts)
    lo_w, hi_w = bbox(body_parts + [f"wheel_{k}" for k in WHEELS])
    lo_m, hi_m = bbox(body_parts + ["mirror_l", "mirror_r"])
    return {
        "length_m": round(hi[1] - lo[1], 4),
        "width_body_m": round(hi[0] - lo[0], 4),
        "width_with_mirrors_m": round(hi_m[0] - lo_m[0], 4),
        "height_m": round(max(hi[2], hi_w[2]), 4),
        "ground_clearance_m": round(lo[2], 4),
        "nose_y_m": round(hi[1], 4),
        "tail_y_m": round(lo[1], 4),
        "center_x_m": round((hi[0] + lo[0]) / 2, 4),
        "wheels_min_z_m": round(lo_w[2], 4),
    }


def _gltf_export(path, objects):
    bpy.ops.object.select_all(action="DESELECT")
    for o in objects:
        o.select_set(True)
    kw = dict(filepath=path, export_format="GLB", use_selection=True, export_apply=True,
              export_yup=True, export_texcoord=True, export_normals=True, export_materials="EXPORT",
              export_cameras=False, export_lights=False, export_extras=False)
    try:
        bpy.ops.export_scene.gltf(**kw)
    except TypeError:
        kw.pop("export_texcoord")
        bpy.ops.export_scene.gltf(**kw)


def export_all(objs, sockets_, collision_ob, out_dir, lods=True):
    meshes = list(objs.values())
    everything = meshes + sockets_ + [collision_ob]
    stats = {"lod0": {}, "files": {}}
    stats["lod0"] = {o.name: meshkit.tri_count(o) for o in meshes}
    stats["lod0_total_tris"] = sum(stats["lod0"].values())
    stats["collision_tris"] = meshkit.tri_count(collision_ob)
    path = os.path.join(out_dir, "car.glb")
    _gltf_export(path, everything)
    stats["files"]["car.glb"] = os.path.getsize(path)
    if not lods:
        return stats
    total0 = stats["lod0_total_tris"]
    for lod, ratio in ((1, 0.3), (2, 0.1)):
        for o in meshes:
            target = max(int(stats["lod0"][o.name] * ratio), 12)
            cur = meshkit.tri_count(o)
            if cur <= target:
                continue
            mod = o.modifiers.new("dec", "DECIMATE")
            mod.ratio = target / cur
            mod.use_collapse_triangulate = True
            meshkit.apply_modifiers(o)
        stats[f"lod{lod}_total_tris"] = sum(meshkit.tri_count(o) for o in meshes)
        stats[f"lod{lod}_ratio"] = round(stats[f"lod{lod}_total_tris"] / total0, 3)
        path = os.path.join(out_dir, f"car_lod{lod}.glb")
        _gltf_export(path, everything)
        stats["files"][f"car_lod{lod}.glb"] = os.path.getsize(path)
    return stats
