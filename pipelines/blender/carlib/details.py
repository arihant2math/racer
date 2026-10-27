"""Details on the body shell: boolean pockets (headlights, intakes, exhausts),
panel gaps, and overlay geometry (lamp internals, light bar, grille slats,
door handles, tailpipes, badge) placed on the surface by ray casting."""

import math

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

from . import bodymesh, meshkit

PANELS = ("body", "hood", "bumper_front", "bumper_rear", "door_l", "door_r", "spoiler")


def _cutter_obj(name, bm, part, mat, materials=()):
    for f in bm.faces:
        f.material_index = mat
    ob = meshkit.bm_to_object(name, bm, materials, smooth=False)
    a = ob.data.attributes.new("part", "INT", "FACE")
    a.data.foreach_set("value", np.full(len(ob.data.polygons), part, np.int32))
    a = ob.data.attributes.new("band", "INT", "FACE")
    a.data.foreach_set("value", np.full(len(ob.data.polygons), -1, np.int32))
    return ob


def _cylinder_y(center, radius, y0, y1, seg=64, scale_z=1.0, scale_x=1.0):
    """Closed cylinder along Blender Y from y0 to y1 (y0 > y1 is fine)."""
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, cap_tris=False, segments=seg, radius1=radius,
                          radius2=radius, depth=abs(y1 - y0))
    rot = Matrix.Rotation(math.radians(90), 4, "X")
    bmesh.ops.transform(bm, matrix=rot, verts=bm.verts)
    bmesh.ops.scale(bm, vec=(scale_x, 1, scale_z), verts=bm.verts)
    bmesh.ops.translate(bm, vec=Vector((center.x, (y0 + y1) / 2, center.z)), verts=bm.verts)
    return bm


def _box(x0, x1, y0, y1, z0, z1, bevel=0.0, along="Y"):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    for v in bm.verts:
        v.co = Vector(((x0 + x1) / 2 + v.co.x * abs(x1 - x0), (y0 + y1) / 2 + v.co.y * abs(y1 - y0),
                       (z0 + z1) / 2 + v.co.z * abs(z1 - z0)))
    if bevel > 0:
        axis = {"X": 0, "Y": 1, "Z": 2}[along]
        edges = [e for e in bm.edges
                 if abs((e.verts[0].co - e.verts[1].co)[axis]) > 1e-6]
        bmesh.ops.bevel(bm, geom=edges, offset=bevel, segments=4, affect="EDGES", profile=0.5)
    return bm


def _nearest_on_polyline(p, line):
    best, bd = None, 1e9
    for a, b in zip(line[:-1], line[1:]):
        ab = b - a
        t = np.clip(np.dot(p - a, ab) / max(np.dot(ab, ab), 1e-12), 0, 1)
        q = a + t * ab
        d = np.linalg.norm(p - q)
        if d < bd:
            best, bd = q, d
    return best, bd


def straighten_seams(shell, geo, cfg):
    """Moves vertices on part/material seams onto configured lines (s, z
    polylines), so seams cut across the loft grid are straight instead of
    stair-stepped; vertices are re-projected onto the original surface."""
    sc = cfg["details"]["seams"]
    lines = [np.asarray(l, float) for l in sc["lines"]]
    tol = sc["tol"]
    bm = bmesh.new()
    bm.from_mesh(shell.data)
    part = bm.faces.layers.int.get("part")
    seam = set()
    for e in bm.edges:
        if len(e.link_faces) == 2:
            f0, f1 = e.link_faces
            if f0[part] != f1[part] or f0.material_index != f1.material_index:
                seam.update(e.verts)
    moved = 0
    for v in seam:
        p = np.array([geo.s_mid - v.co.y, v.co.z])
        best, d = None, tol
        for line in lines:
            q, dq = _nearest_on_polyline(p, line)
            if dq < d:
                best, d, bl = q, dq, line
        if best is None:
            continue
        co = Vector((v.co.x, geo.s_mid - best[0], best[1]))
        for _ in range(3):
            hit = geo.bvh.find_nearest(co)[0]
            if hit is None:
                break
            q, _ = _nearest_on_polyline(np.array([geo.s_mid - hit.y, hit.z]), bl)
            co = Vector((hit.x, geo.s_mid - q[0], q[1]))
        if (co - v.co).length > 2 * tol:
            continue
        v.co = co
        moved += 1
    bmesh.ops.remove_doubles(bm, verts=list(seam), dist=sc.get("merge", 5e-4))
    bmesh.ops.dissolve_degenerate(bm, edges=bm.edges, dist=1e-6)
    bm.to_mesh(shell.data)
    bm.free()
    print(f"[details] straightened {moved} seam vertices", flush=True)


def cut_shell(shell, geo, cfg, mats, mi):
    geo.set_shell(shell)
    straighten_seams(shell, geo, cfg)
    d = cfg["details"]
    cutters = []
    P = bodymesh.P
    # Headlight pockets: cylinders along the lamp axis (forward, pitched up),
    # into the front fenders.
    hl = d["headlight"]
    axis = Vector((0.0, math.cos(math.radians(hl["pitch_deg"])), math.sin(math.radians(hl["pitch_deg"]))))
    geo.headlight_axis = axis
    for side in (1, -1):
        aim = geo.b(hl["s"], side * hl["y"], hl["z"])
        hit, _ = geo.ray(aim + axis * 2.0, -axis)
        geo.__dict__.setdefault("headlight", {})["r" if side > 0 else "l"] = hit
        bm = _cylinder_y(Vector((0, 0, 0)), hl["radius"], 0.4, -hl["depth"])
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0),
                         matrix=Matrix.Rotation(math.radians(hl["pitch_deg"]), 3, "X"))
        bmesh.ops.translate(bm, vec=hit, verts=bm.verts)
        cutters.append(_cutter_obj("cut_hl", bm, P["body"], mi["trim_black"]))
    # Front intakes.
    for it in d["intakes"]:
        y0, y1 = it["y"]
        z0, z1 = it["z"]
        for side in ((1, -1) if y0 > 0 else (1,)):
            yc = side * (y0 + y1) / 2
            hit, _ = geo.surface_from_front(yc, (z0 + z1) / 2)
            xa, xb = sorted((side * y0, side * y1))
            bm = _box(xa, xb, hit.y + 0.4, hit.y - it["depth"], z0, z1, bevel=it.get("radius", 0.02))
            cutters.append(_cutter_obj("cut_intake", bm, P["bumper_front"], mi["trim_black"]))
    # Exhaust openings.
    ex = d["exhaust"]
    for side in (1, -1):
        hit, _ = geo.surface_from_rear(side * ex["y"], ex["z"])
        geo.__dict__.setdefault("exhaust", {})["r" if side > 0 else "l"] = hit
        bm = _cylinder_y(Vector((side * ex["y"], 0, ex["z"])), ex["size"][1] / 2, hit.y - 0.4,
                         hit.y + ex["depth"], seg=48, scale_x=ex["size"][0] / ex["size"][1])
        cutters.append(_cutter_obj("cut_exhaust", bm, P["bumper_rear"], mi["trim_black"]))
    # Every cutter carries the shell's material slots so the join and the
    # boolean (index mode) keep the pocket material.
    for c in cutters:
        for m in shell.data.materials:
            c.data.materials.append(m)
    cutter = meshkit.join(cutters, "cutters")
    meshkit.boolean(shell, cutter, "DIFFERENCE")
    bpy.data.objects.remove(cutter)


def panel_gaps(objs, cfg):
    """Rolls every open panel edge into the car, giving a V-shaped shut line
    where two panels meet (both edges roll in by the same amount)."""
    g = cfg["details"]["panel_gap"]
    for name in PANELS:
        ob = objs.get(name)
        if ob is None:
            continue
        bm = bmesh.new()
        bm.from_mesh(ob.data)
        boundary = {v for e in bm.edges if e.is_boundary for v in e.verts}
        if not boundary:
            bm.free()
            continue
        bm.normal_update()
        # Roll the first row of faces in: boundary vertices move into the car
        # and slightly back across the panel. (An inset with even offset can
        # spike at the sliver corners where seams meet, so it is not used.)
        moves = {}
        for v in boundary:
            inward = Vector()
            for f in v.link_faces:
                inward += f.calc_center_median() - v.co
            n = v.normal
            inward -= n * inward.dot(n)
            if inward.length > 1e-9:
                inward.normalize()
            moves[v] = -n * g["depth"] + inward * (g["width"] / 2)
        for v, dv in moves.items():
            v.co += dv
        bm.to_mesh(ob.data)
        bm.free()
    glass = objs.get("glass")
    if glass is not None:
        bm = bmesh.new()
        bm.from_mesh(glass.data)
        bm.normal_update()
        for v in bm.verts:
            v.co -= v.normal * g["glass_inset"]
        bm.to_mesh(glass.data)
        bm.free()
    for ob in objs.values():
        meshkit.set_sharp_by_angle(ob, cfg["details"]["sharp_angle_deg"])


# ---------------------------------------------------------------------------
# Overlays


class Builder:
    """Accumulates geometry into one bmesh with material slots."""

    def __init__(self, slots):
        self.bm = bmesh.new()
        self.uv = self.bm.loops.layers.uv.new("UVMap")
        self.slots = slots

    def quad_strip(self, rows, mat, close=False):
        """rows: list of lists of Vectors (same length) -> grid of quads."""
        vs = [[self.bm.verts.new(p) for p in r] for r in rows]
        n = len(rows[0])
        for i in range(len(rows) - 1):
            for j in range(n - 1 + (1 if close else 0)):
                j2 = (j + 1) % n
                try:
                    f = self.bm.faces.new((vs[i][j], vs[i][j2], vs[i + 1][j2], vs[i + 1][j]))
                except ValueError:
                    continue
                f.material_index = mat
                for l, (a, b) in zip(f.loops, ((i, j), (i, j2), (i + 1, j2), (i + 1, j))):
                    l[self.uv].uv = (b / max(n - 1, 1), a / max(len(rows) - 1, 1))
        return vs

    def add_bm(self, other, mat=None, matrix=None):
        me = bpy.data.meshes.new("tmp")
        if matrix is not None:
            bmesh.ops.transform(other, matrix=matrix, verts=other.verts)
        if mat is not None:
            for f in other.faces:
                f.material_index = mat
        other.to_mesh(me)
        other.free()
        self.bm.from_mesh(me)
        bpy.data.meshes.remove(me)

    def box(self, center, size, mat, frame=None, bevel=0.0):
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0)
        bmesh.ops.scale(bm, vec=Vector(size), verts=bm.verts)
        if bevel > 0:
            bmesh.ops.bevel(bm, geom=list(bm.edges), offset=min(bevel, min(size) * 0.45), segments=2,
                            affect="EDGES")
        m = Matrix.Translation(Vector(center))
        if frame is not None:
            m = m @ frame.to_4x4()
        self.add_bm(bm, mat, m)

    def finish(self, name, smooth_angle=40):
        bmesh.ops.recalc_face_normals(self.bm, faces=self.bm.faces)
        ob = meshkit.bm_to_object(name, self.bm, self.slots)
        self.bm.free()
        meshkit.set_sharp_by_angle(ob, smooth_angle)
        return ob


def _frame(normal, tangent):
    """3x3 matrix with columns (tangent, bitangent, normal)."""
    n = Vector(normal).normalized()
    t = (Vector(tangent) - n * n.dot(Vector(tangent))).normalized()
    b = n.cross(t)
    return Matrix((t, b, n)).transposed()


def add_details(geo, cfg, mats, mi, objs):
    d = cfg["details"]
    out = {}
    slots = [mats["trim_black"], mats["glass"], mats["chrome"], mats["light_emissive_front"],
             mats["light_emissive_rear"], mats["car_paint"]]
    BLACK, GLASS, CHROME, EMF, EMR, PAINT = range(6)

    # Headlights: lens following the original fender surface, DRL four-point
    # signature and projector inside the pocket.
    hl = d["headlight"]
    fb = Builder(slots)
    a = geo.headlight_axis
    e1 = Vector((1.0, 0.0, 0.0))
    e2 = a.cross(e1) * -1.0
    lamp = Matrix((e1, e2, a)).transposed()  # local X, Y, Z(axis)
    for key, side in (("r", 1), ("l", -1)):
        c = geo.headlight[key]
        r = hl["radius"] - 0.001

        def disc_pt(rr, t, depth=0.0):
            return c + e1 * (rr * math.cos(t)) + e2 * (rr * math.sin(t)) - a * depth

        rings = []
        for i in range(13):
            rr = r * i / 12
            row = []
            for k in range(64):
                t = 2 * math.pi * k / 64
                p = disc_pt(rr, t)
                hit, _ = geo.ray(p + a * 0.5, -a)
                # Stay on the fender: ignore hits far behind the lamp face.
                q = hit if hit is not None and (hit - p).dot(a) > -0.04 else p
                dome = hl["lens_dome"] * (1 - (rr / r) ** 2)
                row.append(q + a * (0.002 + dome))
            rings.append(row)
        fb.quad_strip(rings, GLASS, close=True)
        back = hl["depth"] - 0.004
        disc = [[disc_pt(rr, 2 * math.pi * k / 48, back) for k in range(48)] for rr in (0.0, 0.05, r * 0.98)]
        fb.quad_strip(disc[:2], BLACK, close=True)
        fb.quad_strip(disc[1:], CHROME, close=True)
        # Chrome reflector cone from the back plate to just behind the lens.
        cone = [[disc_pt(rr, 2 * math.pi * k / 48, dd) for k in range(48)]
                for rr, dd in ((r * 0.98, back), (r * 0.98, 0.02))]
        fb.quad_strip(cone, CHROME, close=True)
        # Four-point DRL: tangential bars at 45 deg intervals around the projector.
        for k in range(4):
            t = math.radians(45 + 90 * k)
            p = disc_pt(0.074, t, back - 0.012)
            tang = -e1 * math.sin(t) + e2 * math.cos(t)
            rad = e1 * math.cos(t) + e2 * math.sin(t)
            fr = Matrix((tang, rad, a)).transposed()
            fb.box(p, (0.05, 0.011, 0.012), EMF, frame=fr, bevel=0.003)
        # Projector: chrome cup and dark lens.
        pv, pf, puv = meshkit.revolve([(0.0, 0.0), (0.03, 0.0), (0.036, 0.004), (0.036, 0.03), (0.042, 0.032)],
                                      40, axis="Z")
        bm = bmesh.new()
        vs = [bm.verts.new(v) for v in pv]
        for f in pf:
            try:
                bm.faces.new([vs[i] for i in f])
            except ValueError:
                pass
        fb.add_bm(bm, CHROME, Matrix.Translation(disc_pt(0, 0, back)) @ lamp.to_4x4())
        lv, lf, luv = meshkit.ellipsoid((0, 0, 0), (0.03, 0.03, 0.015), 32, 12)
        bm = bmesh.new()
        vs = [bm.verts.new(v) for v in lv]
        for f in lf:
            try:
                bm.faces.new([vs[i] for i in f])
            except ValueError:
                pass
        fb.add_bm(bm, GLASS, Matrix.Translation(disc_pt(0, 0, back - 0.03)) @ lamp.to_4x4())
    # Slim lamp strips above the outer intakes (position lights / indicators).
    for st in d.get("front_strips", []):
        for side in (1, -1):
            row0, row1 = [], []
            for t in np.linspace(0, 1, 16):
                y = side * (st["y"][0] + (st["y"][1] - st["y"][0]) * t)
                for z, row in ((st["z"], row0), (st["z"] + st["h"], row1)):
                    hit, n = geo.surface_from_front(y, z)
                    if hit is not None:
                        row.append(hit + n * 0.003)
            if len(row0) == len(row1) == 16:
                fb.quad_strip([row0, row1] if side > 0 else [row1, row0], EMF)
    # Intake slats.
    for it in d["intakes"]:
        y0, y1 = it["y"]
        z0, z1 = it["z"]
        for side in ((1, -1) if y0 > 0 else (1,)):
            for k in range(it.get("slats", 0)):
                z = z0 + (z1 - z0) * (k + 1) / (it["slats"] + 1)
                # Behind the most recessed point of the opening (corners curve back).
                ys = [geo.surface_from_front(side * yy, z)[0] for yy in np.linspace(y0, y1, 7)]
                back = min(h.y for h in ys if h is not None)
                fb.box(Vector((side * (y0 + y1) / 2, back - 0.025, z)),
                       (abs(y1 - y0) - 0.01, 0.03, 0.008), BLACK)
    out["lights_front"] = fb.finish("lights_front")

    # Rear light bar across the tail, plus the third brake light.
    lb = d["light_bar"]
    rb = Builder(slots)
    ys = np.linspace(-lb["y"], lb["y"], 160)
    rows = []
    for z in np.linspace(lb["z"][0], lb["z"][1], 4):
        row = []
        for y in ys:
            hit, n = geo.surface_from_rear(y, z)
            row.append(hit + n * lb["proud"] if hit is not None else None)
        rows.append(row)
    if all(p is not None for r in rows for p in r):
        rb.quad_strip(rows[::-1], EMR)
    # Dark band under the light bar, wrapping the lamp ends.
    rows = []
    for z in (lb["z"][0] - 0.012, lb["z"][0]):
        row = []
        for y in ys:
            hit, n = geo.surface_from_rear(y, z)
            row.append(hit + n * (lb["proud"] * 0.5) if hit is not None else None)
        rows.append(row)
    if all(p is not None for r in rows for p in r):
        rb.quad_strip(rows[::-1], BLACK)
    tb = d["third_brake"]
    rows = []
    for s in (tb["s"][0], tb["s"][1]):
        row = []
        for y in np.linspace(-tb["half_width"], tb["half_width"], 12):
            hit, n = geo.surface_from_top(s, y)
            row.append(hit + n * tb["height"])
        rows.append(row)
    rb.quad_strip(rows, EMR)
    out["lights_rear"] = rb.finish("lights_rear")

    # Grille slats on the engine lid (merged into the body).
    gr = d["grille"]
    gb = Builder(slots)
    for y in np.arange(-gr["half_width"], gr["half_width"] + 1e-6, gr["pitch"]):
        lo, hi = [], []
        for s in np.linspace(gr["s"][0], gr["s"][1], 14):
            hit, n = geo.surface_from_top(s, y)
            lo.append(hit - n * 0.01)
            hi.append(hit + n * gr["height"])
        for dx in (-gr["thickness"] / 2, gr["thickness"] / 2):
            pass
        left = [p + Vector((-gr["thickness"] / 2, 0, 0)) for p in lo], [p + Vector((-gr["thickness"] / 2, 0, 0)) for p in hi]
        right = [p + Vector((gr["thickness"] / 2, 0, 0)) for p in lo], [p + Vector((gr["thickness"] / 2, 0, 0)) for p in hi]
        gb.quad_strip([left[0], left[1]], BLACK)
        gb.quad_strip([right[1], right[0]], BLACK)
        gb.quad_strip([left[1], right[1]], BLACK)
    slats = gb.finish("grille_slats")
    _merge_into(objs["body"], slats)

    # Door handles (flush, body colour with a dark outline).
    dh = d["door_handle"]
    for side, name in ((1, "door_r"), (-1, "door_l")):
        hb = Builder(slots)
        s0, s1 = dh["s"]
        rows = []
        for z in (dh["z"] - dh["h"] / 2, dh["z"] + dh["h"] / 2):
            row = []
            for s in np.linspace(s0, s1, 10):
                hit, n = geo.surface_from_side(s, z, side)
                row.append(hit + n * 0.002)
            rows.append(row)
        rows2 = [[p + (p - sum(r, Vector()) / len(r)) * 0 for p in r] for r in rows]
        hb.quad_strip(rows if side > 0 else rows[::-1], BLACK)
        rows = []
        for z in (dh["z"] - dh["h"] / 2 + 0.004, dh["z"] + dh["h"] / 2 - 0.004):
            row = []
            for s in np.linspace(s0 + 0.006, s1 - 0.006, 10):
                hit, n = geo.surface_from_side(s, z, side)
                row.append(hit + n * 0.0035)
            rows.append(row)
        hb.quad_strip(rows if side > 0 else rows[::-1], PAINT)
        _merge_into(objs[name], hb.finish("handle"))

    # Tailpipes (merged into the rear bumper).
    ex = d["exhaust"]
    tp = Builder(slots)
    for key, side in (("r", 1), ("l", -1)):
        hit = geo.exhaust[key]
        a, b = ex["size"][0] / 2 - 0.006, ex["size"][1] / 2 - 0.006
        rings = []
        for yo, sc in ((hit.y + ex["depth"] - 0.01, 1.0), (hit.y - 0.004, 1.0), (hit.y - 0.004, 0.86),
                       (hit.y + 0.05, 0.86)):
            rings.append([Vector((side * ex["y"] + a * sc * math.cos(t), yo, ex["z"] + b * sc * math.sin(t)))
                          for t in np.linspace(0, 2 * math.pi, 40, endpoint=False)])
        tp.quad_strip(rings[:3], CHROME, close=True)
        tp.quad_strip(rings[2:], BLACK, close=True)
    _merge_into(objs["bumper_rear"], tp.finish("tailpipes"))

    # Crest badge on the hood.
    bd = d["badge"]
    bb = Builder(slots)
    hit, n = geo.surface_from_top(bd["s"], 0.0)
    fr = _frame(n, Vector((1, 0, 0)))
    bb.box(hit + n * 0.003, (bd["size"][0], bd["size"][1], 0.006), CHROME, frame=fr, bevel=0.002)
    _merge_into(objs["hood"], bb.finish("badge"))

    # Spoiler: hinge at its front edge, and a dark well under it in the body.
    sp = objs.get("spoiler")
    if sp is not None:
        well = sp.copy()
        well.data = sp.data.copy()
        bpy.context.scene.collection.objects.link(well)
        well.data.transform(Matrix.Translation((0, 0, -d["spoiler_well_depth"])))
        for p in well.data.polygons:
            p.material_index = sp.data.materials.find("trim_black") if "trim_black" in sp.data.materials else 0
        if "trim_black" not in well.data.materials:
            well.data.materials.append(mats["trim_black"])
            idx = len(well.data.materials) - 1
            for p in well.data.polygons:
                p.material_index = idx
        _merge_into(objs["body"], well)
        a = np.zeros(len(sp.data.vertices) * 3)
        sp.data.vertices.foreach_get("co", a)
        a = a.reshape(-1, 3)
        hinge = Vector((0.0, a[:, 1].max(), a[a[:, 1] > a[:, 1].max() - 0.02, 2].mean()))
        sp.data.transform(Matrix.Translation(-hinge))
        sp.location = hinge
    return out


def _merge_into(target, ob):
    """Joins ob into target (Blender's join maps material slots by material)."""
    meshkit.join([target, ob], target.name)
