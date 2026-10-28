"""Build the Porsche 911 Carrera S (992.1) game asset.

    blender -b --python pipelines/blender/porsche_992/build.py -- <repo_root> <out_dir>

Inputs (all in the repo):
  data/cars/porsche-911-carrera-s-992/car.toml            dimensions (hard constraints)
  data/cars/porsche-911-carrera-s-992/refs/blueprint.json  silhouettes + feature lines
  data/cars/porsche-911-carrera-s-992/refs/base_mesh/hunyuan3d_sections.json
                                                           cross-sections of the image-to-3D base

Outputs in <out_dir>:
  porsche_992.blend   source scene
  porsche_992.glb     game asset: LOD0..LOD3, collision hull, sockets (empties)

Frame: Blender X right, Y forward, Z up, origin on the ground at the
wheelbase midpoint. glTF export converts to Y up, -Z forward (Bevy).
"""
import json
import math
import os
import sys
import tomllib

import bmesh
import bpy
import numpy as np
from mathutils import Matrix, Vector

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import base_fit  # noqa: E402
import body_grid  # noqa: E402
import materials  # noqa: E402
import parts  # noqa: E402
import textures  # noqa: E402
from grid_to_mesh import grid_mesh  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
ROOT, OUT = argv[0], argv[1]
CAR = os.path.join(ROOT, "data/cars/porsche-911-carrera-s-992")
os.makedirs(OUT, exist_ok=True)

SPEC = tomllib.load(open(os.path.join(CAR, "car.toml"), "rb"))
BP = json.load(open(os.path.join(CAR, "refs/blueprint.json")))
LINES = BP["lines"]
C = SPEC["chassis"]
WB, TF, TR = C["wheelbase_m"], C["track_front_m"], C["track_rear_m"]
AF, AR = WB / 2, -WB / 2  # axle y (forward)
SMOOTH_ITERS = 10
BODY_FACES = 200_000


def tyre_dims(s):
    w, ar, rim = s.replace("ZR", " ").replace("/", " ").split()
    w, ar, rim = float(w) / 1000, float(ar) / 100, float(rim) * 0.0254
    return w, rim / 2, rim / 2 + w * ar


TYRE_F = tyre_dims(SPEC["tires"]["front"])  # width, rim radius, outer radius
TYRE_R = tyre_dims(SPEC["tires"]["rear"])


def line(i):
    return np.array(LINES[i]["pts"])


def side_pt(x, z, left=True):
    """Blueprint side coordinate -> Blender point on the left (-X) or right side."""
    return Vector((-1.0 if left else 1.0, x, z))


# ---------------------------------------------------------------------------
# Scene setup
# ---------------------------------------------------------------------------
bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.unit_settings.system = "METRIC"
TEX = textures.build_all()
MAT = materials.build(TEX)

coll = {}
for name in ("LOD0", "LOD1", "LOD2", "LOD3", "COLLISION", "SOCKETS", "_work"):
    c = bpy.data.collections.new(name)
    scene.collection.children.link(c)
    coll[name] = c
W = coll["_work"]

# ---------------------------------------------------------------------------
# 1. Body shell from blueprint x AI sections
# ---------------------------------------------------------------------------
grid_path = os.path.join(OUT, "body_grid.npz")
body_grid.main(
    os.path.join(CAR, "refs/blueprint.json"),
    os.path.join(CAR, "refs/base_mesh/hunyuan3d_sections.json"),
    os.path.join(CAR, "car.toml"),
    grid_path,
)
G = np.load(grid_path)  # silhouettes for the underbody tray
loft = grid_mesh(grid_path, "loft")
shell = base_fit.fit(os.path.join(CAR, "refs/base_mesh/hunyuan3d_base.glb"), BP, SPEC, "body_shell", smooth_iters=SMOOTH_ITERS, target_faces=10**7)
parts.apply_modifiers(shell)
base_fit.close_and_remesh(shell, voxel=0.005, target_faces=BODY_FACES, loft=loft)
parts.remove(loft)
for c in shell.users_collection:
    c.objects.unlink(shell)
W.objects.link(shell)
SURF = parts.Surface(shell)

# Ride-height detail: the shell bottom is the sill line; leave it.

# ---------------------------------------------------------------------------
# 2. Wheel arches: cut the openings, add liners and lips
# ---------------------------------------------------------------------------
def arch_circle(i):
    p = line(i)
    A = np.c_[2 * p, np.ones(len(p))]
    c = np.linalg.lstsq(A, (p**2).sum(1), rcond=None)[0]
    return c[:2], math.sqrt(c[2] + c[0] ** 2 + c[1] ** 2)


(arf_c, arf_r) = arch_circle(34)  # front arch lip (blueprint side)
(arr_c, arr_r) = arch_circle(39)  # rear arch lip
ARCH = {"front": (AF, arf_c[1], arf_r), "rear": (AR, arr_c[1], arr_r)}
print("arches", ARCH)


def arch_cutter(axle_y, cz, r, name):
    # Cylinder along X through both sides, then trimmed so it does not cut the
    # inner body (only the outer 0.35 m on each side).
    objs = []
    for s in (-1, 1):
        bm = bmesh.new()
        bmesh.ops.create_cone(bm, cap_ends=True, segments=96, radius1=r, radius2=r, depth=0.5)
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(math.pi / 2, 3, "Y"))
        bmesh.ops.translate(bm, verts=bm.verts, vec=(s * 0.95, axle_y, cz))
        objs.append(parts.bm_to_object(bm, f"{name}_{s}", W))
    return objs


for key, (ay, cz, r) in ARCH.items():
    for cut in arch_cutter(ay, cz, r - 0.012, f"archcut_{key}"):
        parts.boolean(shell, cut)
        parts.remove(cut)

def drop_small_islands(ob, keep_frac=0.02):
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    seen, comps = set(), []
    for v in bm.verts:
        if v.index in seen:
            continue
        st, comp = [v], []
        seen.add(v.index)
        while st:
            a = st.pop()
            comp.append(a)
            for e in a.link_edges:
                b = e.other_vert(a)
                if b.index not in seen:
                    seen.add(b.index)
                    st.append(b)
        comps.append(comp)
    big = max(len(c) for c in comps)
    drop = [v for c in comps if len(c) < keep_frac * big for v in c]
    bmesh.ops.delete(bm, geom=drop, context="VERTS")
    bm.to_mesh(ob.data)
    bm.free()
    return len(drop)


print("dropped debris verts", drop_small_islands(shell))
SURF = parts.Surface(shell)

# Inner liners: half-cylinder tubs inside each arch (textured black plastic).
def arch_liner(ay, cz, r, half_track, width, name):
    prof = []
    segs = 48
    bm = bmesh.new()
    rings = []
    x_in, x_out = half_track - width * 0.5 - 0.1, 0.94
    for k in range(segs + 1):
        t = math.pi * (0.08 + 0.84 * k / segs)  # over the top, stays above the sill
        y = ay + (r + 0.005) * math.cos(t)
        z = cz + (r + 0.005) * math.sin(t)
        rings.append((bm.verts.new((-x_out, y, z)), bm.verts.new((-x_in, y, z))))
    for k in range(segs):
        a, b = rings[k], rings[k + 1]
        bm.faces.new((a[0], b[0], b[1], a[1]))
    # inner wall
    wall = [bm.verts.new((-x_in, ay + (r + 0.005) * math.cos(math.pi * (-0.15 + 1.3 * k / segs)) * f, cz + (r + 0.005) * math.sin(math.pi * (-0.15 + 1.3 * k / segs)) * f)) for k in range(segs + 1) for f in (1.0,)]
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    for f in bm.faces:
        f.normal_flip()
    ob = parts.bm_to_object(bm, name, W, MAT["plastic"])
    m = ob.modifiers.new("mirror", "MIRROR")
    m.use_axis[0] = True
    parts.apply_modifiers(ob)
    return ob


liners = [
    arch_liner(AF, ARCH["front"][1], ARCH["front"][2], TF / 2, TYRE_F[0], "arch_liner_front"),
    arch_liner(AR, ARCH["rear"][1], ARCH["rear"][2], TR / 2, TYRE_R[0], "arch_liner_rear"),
]

# ---------------------------------------------------------------------------
# 3. Glass: windscreen, side windows (DLO), rear screen
# ---------------------------------------------------------------------------
def closed_side_outline(i, n=160, shrink=0.0):
    p = parts.resample_closed(line(i), n)
    p = parts.smooth_closed(p, 2)
    if shrink:
        c = p.mean(0)
        p = c + (p - c) * (1 - shrink)
    return p


# Side window opening (DLO) = line 164 (outer frame). Cut a recess and inset glass.
dlo = closed_side_outline(164, 200)
dlo_glass = closed_side_outline(163, 200)
SURF_ORIG = None


def side_window_parts():
    objs = []
    for left in (True, False):
        s = -1 if left else 1
        pts = [Vector((s * 1.5, x, z)) for x, z in dlo_glass]
        g = parts.shell_patch(SURF, pts, (-s, 0, 0), f"glass_side_{'L' if left else 'R'}", offset=0.0015, grid=10, coll=W, mat=MAT["glass_tint"])
        objs.append(g)
        frame = parts.ribbon_on_surface(SURF, [Vector((s * 1.5, x, z)) for x, z in dlo], (-s, 0, 0), 0.022, f"dlo_trim_{'L' if left else 'R'}", offset=0.0025, coll=W, mat=MAT["paint_dark"], closed=True)
        objs.append(frame)
        b = (line(171) + line(172)) / 2
        bp = [Vector((s * 1.5, x, z)) for x, z in parts.resample_open(b, 12)]
        objs.append(parts.ribbon_on_surface(SURF, bp, (-s, 0, 0), 0.03, f"b_pillar_{'L' if left else 'R'}", offset=0.003, coll=W, mat=MAT["paint_dark"]))
    return objs


side_glass = side_window_parts()


def screen_plan(i, name, mat):
    """Glass from a plan-view outline (x fwd, y left), projected from above."""
    p = parts.smooth_closed(parts.resample_closed(line(i), 240), 4)
    pts = [Vector((-y, x, 3.0)) for x, y in p]
    d = (0, 0, -1)
    g = parts.shell_patch(SURF, pts, d, name, offset=0.0015, grid=20, coll=W, mat=mat)
    trim = parts.ribbon_on_surface(SURF, pts, d, 0.035, name + "_seal", offset=0.0025, coll=W, mat=MAT["frit"], closed=True)
    return g, trim, p


windscreen, windscreen_seal, ws_poly = screen_plan(94, "glass_windscreen", MAT["glass"])
rearscreen, rearscreen_seal, rs_poly = screen_plan(85, "glass_rear", MAT["glass_tint"])

# Apertures in the paint shell (shrunk a little so the glass overlaps the cut).
def shrink(poly, f):
    c = poly.mean(0)
    return c + (poly - c) * f


n_side = 0
for s_ in (-1, 1):
    n_side += parts.open_aperture(shell, shrink(dlo_glass, 0.97), lambda c: (c.y, c.z), (s_, 0, 0), 0.35)
n_ws = parts.open_aperture(shell, shrink(ws_poly, 0.96), lambda c: (c.y, -c.x), (0, 0, 1), 0.2)
n_rs = parts.open_aperture(shell, shrink(rs_poly, 0.96), lambda c: (c.y, -c.x), (0, 0, 1), 0.2)
print("apertures (faces removed): side", n_side, "windscreen", n_ws, "rear", n_rs)

# Inner shell: a decimated, inward-offset, flipped copy of the opened body,
# dark trim material; seen only through the glass.
def inner_shell():
    o = shell.copy()
    o.data = shell.data.copy()
    o.name = "body_inner"
    W.objects.link(o)
    d = o.modifiers.new("dec", "DECIMATE")
    d.ratio = 0.12
    parts.apply_modifiers(o)
    bm = bmesh.new()
    bm.from_mesh(o.data)
    bm.normal_update()
    for v in bm.verts:
        v.co -= v.normal * 0.025
    for f in bm.faces:
        f.normal_flip()
    bm.to_mesh(o.data)
    bm.free()
    o.data.materials.clear()
    o.data.materials.append(MAT["leather"])
    return o


inner = inner_shell()

# ---------------------------------------------------------------------------
# 4. Panel gaps: thin dark grooves along door, bonnet, bumpers
# ---------------------------------------------------------------------------
GAP_W = 0.0045


def gap_side(i, name, n=120):
    objs = []
    p = parts.resample_open(line(i), n)
    for left in (True, False):
        s = -1 if left else 1
        pts = [Vector((s * 1.5, x, z)) for x, z in p]
        objs.append(parts.ribbon_on_surface(SURF, pts, (-s, 0, 0), GAP_W, f"{name}_{'L' if left else 'R'}", offset=0.0006, coll=W, mat=MAT["plastic"]))
    return objs


def gap_top(i, name, n=200, closed=False):
    p = parts.resample_closed(line(i), n) if closed else parts.resample_open(line(i), n)
    pts = [Vector((-y, x, 3.0)) for x, y in p]
    return [parts.ribbon_on_surface(SURF, pts, (0, 0, -1), GAP_W, name, offset=0.0006, coll=W, mat=MAT["plastic"], closed=closed)]


gaps = []
gaps += gap_side(168, "gap_door_front")
gaps += gap_side(170, "gap_door_rear_bottom", 200)
gaps += gap_side(38, "gap_sill")
gaps += gap_top(92, "gap_bonnet", 300)
gaps += gap_side(37, "gap_door_crease", 160)

# ---------------------------------------------------------------------------
# 5. Lights
# ---------------------------------------------------------------------------
def headlight(left):
    s = -1 if left else 1
    # Front view: outer ring 143/154, inner lens 27/30 ; centre ~ (0.70, 0.695)
    ring = line(143 if not left else 154)
    c = ring.mean(0)
    rx = (ring[:, 0].max() - ring[:, 0].min()) / 2
    rz = (ring[:, 1].max() - ring[:, 1].min()) / 2
    # Find the surface point and normal at the lamp centre.
    yv = abs(c[0])
    loc, n = SURF.project(Vector((s * yv, 3.0, c[1])), (0, -1, 0))
    n = n.normalized()
    # Lamp axis: mostly forward, tilted with the wing surface.
    axis = (n + Vector((0, 0.6, 0))).normalized()
    objs = []
    # Lens: shallow dome ellipse, 0.0 m proud.
    prof = []
    for k in range(13):
        t = k / 12
        prof.append((t * 1.0, 0.018 * (1 - t * t)))
    lens = parts.lathe(prof[::-1], 64, f"headlight_lens_{'L' if left else 'R'}", axis="Z", coll=W, mat=MAT["lens_clear"])
    # Reflector bowl and the four-point DRL graphic + projector.
    bowl = parts.lathe([(0.0, -0.06), (0.4, -0.055), (0.75, -0.04), (0.95, -0.012), (1.0, 0.0)], 64, f"headlight_bowl_{'L' if left else 'R'}", axis="Z", coll=W, mat=MAT["headlight_inner"])
    for f in bowl.data.polygons:
        pass
    proj = parts.lathe([(0.0, -0.02), (0.22, -0.02), (0.24, -0.045), (0.0, -0.045)], 32, f"headlight_projector_{'L' if left else 'R'}", axis="Z", coll=W, mat=MAT["headlight_chrome"])
    drl = []
    for k in range(4):
        a = math.radians(45 + 90 * k)
        bm = bmesh.new()
        bmesh.ops.create_cube(bm, size=1.0)
        bmesh.ops.scale(bm, vec=(0.30, 0.07, 0.012), verts=bm.verts)
        bmesh.ops.translate(bm, vec=(0.55, 0, -0.012), verts=bm.verts)
        bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(a, 3, "Z"))
        drl.append(parts.bm_to_object(bm, f"drl_{k}", W, MAT["drl"]))
    ring_o = parts.lathe([(1.0, 0.0), (1.06, 0.004), (1.12, -0.01), (1.12, -0.03)], 64, f"headlight_bezel_{'L' if left else 'R'}", axis="Z", coll=W, mat=MAT["chrome"])
    group = [lens, bowl, proj, ring_o] + drl
    # Place: scale unit (radius 1) to the ellipse radii; orient z->axis.
    q = axis.to_track_quat("Z", "Y")
    M = Matrix.Translation(loc - axis * 0.004) @ q.to_matrix().to_4x4() @ Matrix.Diagonal((rx * 0.92, rz * 0.92, 1.0, 1.0))
    for o in group:
        o.data.transform(M)
        o.data.update()
    return group


def recess(center, axis, rx, rz, depth, name):
    """Elliptic cylinder cutter for lamp pockets."""
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=True, segments=64, radius1=1.0, radius2=1.0, depth=1.0)
    ob = parts.bm_to_object(bm, name, W)
    q = axis.to_track_quat("Z", "Y")
    ob.data.transform(Matrix.Translation(center) @ q.to_matrix().to_4x4() @ Matrix.Diagonal((rx, rz, depth, 1.0)))
    return ob


headlights = []
for left in (True, False):
    s = -1 if left else 1
    ring = line(143 if not left else 154)
    c = ring.mean(0)
    rx = (ring[:, 0].max() - ring[:, 0].min()) / 2
    rz = (ring[:, 1].max() - ring[:, 1].min()) / 2
    loc, n = SURF.project(Vector((s * abs(c[0]), 3.0, c[1])), (0, -1, 0))
    axis = (n.normalized() + Vector((0, 0.6, 0))).normalized()
    cut = recess(loc, axis, rx * 0.96, rz * 0.96, 0.1, "hl_cut")
    parts.boolean(shell, cut)
    parts.remove(cut)
SURF = parts.Surface(shell)
for left in (True, False):
    headlights += headlight(left)

# Tail light bar: full-width light strip (rear view lines 1, 49/50) at z ~0.77-0.79.
def tail_lights():
    objs = []
    p_top = line(50)
    p_bot = line(49)
    zt = np.interp(np.linspace(-0.68, 0.68, 120), p_top[np.argsort(p_top[:, 0]), 0], p_top[np.argsort(p_top[:, 0]), 1])
    zb = np.interp(np.linspace(-0.68, 0.68, 120), p_bot[np.argsort(p_bot[:, 0]), 0], p_bot[np.argsort(p_bot[:, 0]), 1])
    ys = np.linspace(-0.68, 0.68, 120)
    bm = bmesh.new()
    rows = []
    for t in np.linspace(0, 1, 6):
        row = []
        for y, a, b in zip(ys, zt, zb):
            z = b + (a - b) * t
            loc, n = SURF.project(Vector((y, -3.0, z)), (0, 1, 0))
            if loc is None or n.y > -0.35:  # wraps round the quarter: clamp to the tail plane
                loc, n = Vector((y, (row[-1].co.y + 0.004) if row else -2.15, z)), Vector((0, -1, 0))
            row.append(bm.verts.new(loc + n * 0.004))
        rows.append(row)
    for r in range(len(rows) - 1):
        for i in range(len(ys) - 1):
            bm.faces.new((rows[r][i], rows[r][i + 1], rows[r + 1][i + 1], rows[r + 1][i]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    bm.normal_update()
    if sum((f.normal for f in bm.faces), Vector()).y > 0:
        for f in bm.faces:
            f.normal_flip()
    bmesh.ops.solidify(bm, geom=list(bm.faces), thickness=0.01)
    bar = parts.bm_to_object(bm, "tail_light_bar", W, MAT["lens_smoked"])
    objs.append(bar)
    # Emissive strip inside the bar (thin line, the 992 light blade).
    pts = [Vector((y, -3.0, (a + b) / 2)) for y, a, b in zip(ys, zt, zb)]
    objs.append(parts.ribbon_on_surface(SURF, pts, (0, 1, 0), 0.006, "tail_light_blade", offset=0.0065, coll=W, mat=MAT["tail"]))
    return objs


tails = tail_lights()

# Rear lettering plate (PORSCHE between the light bar and engine lid).
def rear_lettering():
    w, h = 0.62, 0.045
    z = 0.745
    bm = bmesh.new()
    uv = bm.loops.layers.uv.new("UVMap")
    nx = 30
    vs = []
    for j in range(2):
        row = []
        for i in range(nx + 1):
            y = -w / 2 + w * i / nx
            loc, n = SURF.project(Vector((y, -3.0, z - h / 2 + h * j)), (0, 1, -0.1))
            row.append((bm.verts.new(loc + n * 0.0025), (i / nx, j)))
        vs.append(row)
    for i in range(nx):
        f = bm.faces.new((vs[0][i][0], vs[0][i + 1][0], vs[1][i + 1][0], vs[1][i][0]))
        for l, uvv in zip(f.loops, (vs[0][i][1], vs[0][i + 1][1], vs[1][i + 1][1], vs[1][i][1])):
            l[uv].uv = (1 - uvv[0], uvv[1])
    bm.normal_update()
    if sum((f.normal for f in bm.faces), Vector()).y > 0:
        for f in bm.faces:
            f.normal_flip()
    return parts.bm_to_object(bm, "badge_porsche_rear", W, MAT["lettering"])


lettering = rear_lettering()

# Bonnet crest
def bonnet_crest():
    x, z0 = 2.06, None
    loc, n = SURF.top(2.0, 0.0)
    bm = bmesh.new()
    uv = bm.loops.layers.uv.new("UVMap")
    s = 0.034
    corners = [(-s, -s * 1.2), (s, -s * 1.2), (s, s * 1.2), (-s, s * 1.2)]
    tvec = Vector((0, 1, 0))
    u_ax = Vector((1, 0, 0))
    v_ax = n.cross(u_ax).normalized()
    vs = [bm.verts.new(loc + n * 0.003 + u_ax * a + v_ax * b) for a, b in corners]
    f = bm.faces.new(vs)
    for l, (a, b) in zip(f.loops, ((1, 0), (0, 0), (0, 1), (1, 1))):
        l[uv].uv = (a, b)
    bm.normal_update()
    if f.normal.dot(n) < 0:
        f.normal_flip()
    return parts.bm_to_object(bm, "badge_crest_bonnet", W, MAT["badge"])


crest = bonnet_crest()

# ---------------------------------------------------------------------------
# 6. Front: intakes, grille, splitter; rear: diffuser, exhausts, engine grille
# ---------------------------------------------------------------------------
def front_intakes():
    objs = []
    # Front view outline 130 (bumper intake band, both sides + centre).
    for i, nm in ((141, "intake_R"), (152, "intake_L"), (127, "intake_C")):
        p = parts.resample_closed(line(i), 120)
        p = parts.smooth_closed(p, 2)
        c = Vector((-p[:, 0].mean(), 0, p[:, 1].mean()))
        loc, n = SURF.project(Vector((-p[:, 0].mean(), 3.0, p[:, 1].mean())), (0, -1, 0))
        if loc is None:
            continue
        # Cutter: extrude outline along Y deep into the bumper.
        outline = [(-y, z) for y, z in p]
        cut = parts.prism(outline, loc.y - 0.12, loc.y + 0.4, nm + "_cut", W, axis="Y")
        parts.boolean(shell, cut)
        parts.remove(cut)
        # Back wall: dark grille plane with slats.
        back = parts.prism(outline, loc.y - 0.13, loc.y - 0.1, nm + "_back", W, MAT["grille"], axis="Y")
        objs.append(back)
        if nm != "intake_C":
            zmin, zmax = p[:, 1].min(), p[:, 1].max()
            ymin, ymax = -p[:, 0].max(), -p[:, 0].min()
            for k in range(1, 4):
                z = zmin + (zmax - zmin) * k / 4
                bm = bmesh.new()
                bmesh.ops.create_cube(bm, size=1.0)
                bmesh.ops.scale(bm, vec=((ymax - ymin) * 0.85, 0.05, 0.008), verts=bm.verts)
                bmesh.ops.translate(bm, vec=((ymin + ymax) / 2, loc.y - 0.07, z), verts=bm.verts)
                objs.append(parts.bm_to_object(bm, f"{nm}_slat{k}", W, MAT["trim_satin"]))
            # Front indicator / DRL blade above the intake (992 bumper light strip).
            bm = bmesh.new()
            bmesh.ops.create_cube(bm, size=1.0)
            bmesh.ops.scale(bm, vec=((ymax - ymin) * 0.45, 0.012, 0.01), verts=bm.verts)
            yc = ymin + (ymax - ymin) * (0.62 if ymin > 0 else 0.38)
            lz = zmax - 0.012
            ll, ln = SURF.project(Vector((yc, 3.0, lz)), (0, -1, 0))
            bmesh.ops.translate(bm, vec=(yc, (ll.y if ll else loc.y) - 0.008, lz), verts=bm.verts)
            objs.append(parts.bm_to_object(bm, f"{nm}_indicator", W, MAT["indicator"]))
    return objs


intakes = front_intakes()
SURF = parts.Surface(shell)


def rear_details():
    objs = []
    # Lower rear valance / diffuser band (rear view outline 48) as a black panel.
    p = parts.resample_closed(line(48), 200)
    pts = [Vector((y, -3.0, z)) for y, z in p]
    objs.append(parts.shell_patch(SURF, pts, (0, 1, 0), "rear_valance", offset=0.003, grid=12, coll=W, mat=MAT["plastic"]))
    # Exhaust tips: ellipses around (±0.43, 0.345), radius ~0.07/0.05.
    for s in (-1, 1):
        q = line(65)
        c = q.mean(0)
        rx = (q[:, 0].max() - q[:, 0].min()) / 2
        rz = (q[:, 1].max() - q[:, 1].min()) / 2
        loc, n = SURF.project(Vector((s * abs(c[0]), -3.0, c[1])), (0, 1, 0))
        y0 = (loc.y if loc else -2.2) - 0.02
        tip = parts.lathe([(0.82, 0.12), (0.84, 0.0), (1.0, 0.0), (1.0, 0.13), (0.96, 0.14)], 48, f"exhaust_tip_{s}", axis="Y", coll=W, mat=MAT["exhaust"])
        tip.data.transform(Matrix.Translation((s * abs(c[0]), y0, c[1])) @ Matrix.Diagonal((rx, 1.0, rz, 1.0)) @ Matrix.Rotation(math.pi, 4, "Z"))
        objs.append(tip)
        inner = parts.lathe([(0.0, 0.08), (0.82, 0.08)], 48, f"exhaust_inner_{s}", axis="Y", coll=W, mat=MAT["exhaust_inner"])
        inner.data.transform(Matrix.Translation((s * abs(c[0]), y0, c[1])) @ Matrix.Diagonal((rx, 1.0, rz, 1.0)) @ Matrix.Rotation(math.pi, 4, "Z"))
        objs.append(inner)
    # Licence plate recess + plate (rear view 121).
    p = line(121)
    yc, zc = p[:, 0].mean(), p[:, 1].mean()
    loc, n = SURF.project(Vector((yc, -3.0, zc)), (0, 1, 0))
    bm = bmesh.new()
    uv = bm.loops.layers.uv.new("UVMap")
    w, h = 0.52, 0.11
    vs = [bm.verts.new((yc + a * w / 2, loc.y - 0.006, zc + b * h / 2)) for a, b in ((1, -1), (-1, -1), (-1, 1), (1, 1))]
    f = bm.faces.new(vs)
    for l, uvv in zip(f.loops, ((0, 0), (1, 0), (1, 1), (0, 1))):
        l[uv].uv = uvv
    bm.normal_update()
    if f.normal.y > 0:
        f.normal_flip()
    objs.append(parts.bm_to_object(bm, "licence_plate_rear", W, MAT["plate"]))
    # Engine lid grille: vertical slats over the rear deck (plan lines 85/87).
    # Grille panel between the rear screen and the deck lip (plan lines 86/87):
    # rounded rectangle x -1.93..-1.60, |y| < 0.47.
    t = np.linspace(0, 2 * np.pi, 160, endpoint=False)
    ex, ey = 6.0, 6.0
    cx, cy, ax, ay = -1.765, 0.0, 0.165, 0.47
    p_out = np.c_[cx + ax * np.sign(np.cos(t)) * np.abs(np.cos(t)) ** (2 / ex), cy + ay * np.sign(np.sin(t)) * np.abs(np.sin(t)) ** (2 / ey)]
    xs0, xs1 = p_out[:, 0].min(), p_out[:, 0].max()
    ys0, ys1 = p_out[:, 1].min(), p_out[:, 1].max()
    cut_pts = [(-y, x) for x, y in parts.smooth_closed(p_out, 2)]
    zmax = 1.4
    cut = parts.prism(cut_pts, 0.0, zmax, "engine_grille_cut", W, axis="Z")
    # Only cut a shallow pocket: intersect with a slab just below the surface.
    deck = parts.Surface(shell)
    # Build the grille as slats lying on the deck surface (no cut, raised slats).
    parts.remove(cut)
    nsl = 30
    for k in range(nsl):
        yv = ys0 + 0.03 + (ys1 - ys0 - 0.06) * (k + 0.5) / nsl
        if abs(yv) < 0.04:
            continue  # third brake light strip
        seg = []
        for x in np.linspace(xs0 + 0.015, xs1 - 0.015, 20):
            loc, n = deck.top(x, yv)
            if loc is not None:
                seg.append(loc)
        if len(seg) > 3:
            objs.append(parts.ribbon_on_surface(deck, [p + Vector((0, 0, 1)) for p in seg], (0, 0, -1), 0.012, f"engine_slat_{k}", offset=0.004, coll=W, mat=MAT["engine_grille"]))
    # Black base under the slats.
    pts = [Vector((-y, x, 3.0)) for x, y in parts.smooth_closed(p_out, 2)]
    objs.append(parts.shell_patch(deck, pts, (0, 0, -1), "engine_grille_base", offset=0.0015, grid=10, coll=W, mat=MAT["plastic"]))
    # Third brake light (centre of the engine grille, red stripe).
    seg = []
    for x in np.linspace(xs0 + 0.06, xs1 - 0.05, 16):
        loc, n = deck.top(x, 0.0)
        if loc is not None:
            seg.append(loc + Vector((0, 0, 1)))
    objs.append(parts.ribbon_on_surface(deck, seg, (0, 0, -1), 0.06, "third_brake_light", offset=0.0055, coll=W, mat=MAT["brake"]))
    return objs


rear = rear_details()

# Rear spoiler (deployable): flat blade inside the engine-lid line at the tail.
def rear_spoiler():
    p = line(84)  # plan, rear deck trailing edge arc
    pts = parts.resample_open(p[np.argsort(p[:, 1])], 60)
    seg = []
    for x, y in pts:
        loc, n = SURF.top(x + 0.03, y)
        if loc is not None:
            seg.append(loc + Vector((0, 0, 1)))
    return parts.ribbon_on_surface(SURF, seg, (0, 0, -1), 0.05, "rear_spoiler_lip", offset=0.003, coll=W, mat=MAT["paint_dark"])


spoiler = None  # deployable spoiler sits flush under the grille line; omitted

# ---------------------------------------------------------------------------
# 7. Mirrors, door handles
# ---------------------------------------------------------------------------
def mirror(left):
    s = -1 if left else 1
    # Plan: mirror head outline 105/117 (x, y), side outline 166 (x, z).
    plan = line(117 if left else 105)
    sidep = line(166)
    zc = (sidep[:, 1].min() + sidep[:, 1].max()) / 2
    hz = (sidep[:, 1].max() - sidep[:, 1].min()) / 2
    outline = parts.smooth_closed(parts.resample_closed(plan, 64), 3)
    bm = bmesh.new()
    rings = []
    nz = 9
    for k in range(nz):
        t = -1 + 2 * k / (nz - 1)
        f = math.sqrt(max(1 - t * t, 0.0)) ** 0.6
        c = outline.mean(0)
        ring = [bm.verts.new((-s * 0 + (-(c[1] + (y - c[1]) * f)) if left else -(c[1] + (y - c[1]) * f), c[0] + (x - c[0]) * f, zc + t * hz)) for x, y in outline]
        rings.append(ring)
    for k in range(nz - 1):
        for i in range(len(outline)):
            j = (i + 1) % len(outline)
            bm.faces.new((rings[k][i], rings[k][j], rings[k + 1][j], rings[k + 1][i]))
    bm.faces.new(rings[0][::-1])
    bm.faces.new(rings[-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    head = parts.bm_to_object(bm, f"mirror_{'L' if left else 'R'}", W, MAT["paint"])
    sub = head.modifiers.new("sub", "SUBSURF")
    sub.levels = 1
    parts.apply_modifiers(head)
    # Mirror glass on the rear face.
    yv = -np.array(outline)[:, 1] if left else -np.array(outline)[:, 1]
    gx = float(np.mean(-outline[:, 1]))
    gy = float(outline[:, 0].min()) - 0.002
    bm = bmesh.new()
    w2, h2 = (outline[:, 1].max() - outline[:, 1].min()) * 0.42, hz * 0.75
    vs = [bm.verts.new((gx + a * w2, gy, zc + b * h2)) for a, b in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
    bm.faces.new(vs)
    glass = parts.bm_to_object(bm, f"mirror_glass_{'L' if left else 'R'}", W, MAT["chrome"])
    # Stalk from door to head (side line 162).
    st = line(162)
    xs_, zs_ = st[:, 0].mean(), st[:, 1].mean()
    loc, n = SURF.side(xs_, zs_, left)
    a = loc
    b = Vector((-(np.mean(outline[:, 1]) * 0.85), xs_, zc - hz * 0.6))
    stalk = parts.tube_along([a - n * 0.01, (a + b) / 2 + Vector((0, 0, 0.01)), b], 0.014, f"mirror_stalk_{'L' if left else 'R'}", sides=10, coll=W, mat=MAT["trim_gloss" if False else "paint_dark"])
    return [head, glass, stalk]


mirrors = mirror(True) + mirror(False)


def door_handles():
    objs = []
    p = parts.resample_closed(line(184), 64)
    for left in (True, False):
        s = -1 if left else 1
        pts = [Vector((s * 1.5, x, z)) for x, z in p]
        objs.append(parts.shell_patch(SURF, pts, (-s, 0, 0), f"door_handle_{'L' if left else 'R'}", offset=0.003, grid=6, coll=W, mat=MAT["paint"]))
        objs.append(parts.ribbon_on_surface(SURF, pts, (-s, 0, 0), 0.003, f"door_handle_gap_{'L' if left else 'R'}", offset=0.0005, coll=W, mat=MAT["plastic"], closed=True))
    return objs


handles = door_handles()

# Side skirts: black lower sill strip along the bottom of the body between arches.
def sills():
    objs = []
    for left in (True, False):
        s = -1 if left else 1
        pts = [Vector((s * 1.5, x, 0.175)) for x in np.linspace(AR + ARCH["rear"][2] + 0.01, AF - ARCH["front"][2] - 0.01, 60)]
        objs.append(parts.ribbon_on_surface(SURF, pts, (-s, 0, 0), 0.07, f"sill_{'L' if left else 'R'}", offset=0.002, coll=W, mat=MAT["plastic"]))
    return objs


sill_strips = sills()

# Underbody tray
def underbody():
    bm = bmesh.new()
    ys = G["xs"]
    hw = G["hw"]
    zb = G["zb"]
    a = [bm.verts.new((-w * 0.97, y, z + 0.004)) for y, w, z in zip(ys, hw, zb)]
    b = [bm.verts.new((w * 0.97, y, z + 0.004)) for y, w, z in zip(ys, hw, zb)]
    for i in range(len(ys) - 1):
        bm.faces.new((a[i], b[i], b[i + 1], a[i + 1]))
    bm.normal_update()
    for f in bm.faces:
        if f.normal.z > 0:
            f.normal_flip()
    return parts.bm_to_object(bm, "underbody", W, MAT["under"])


under = underbody()

# ---------------------------------------------------------------------------
# 8. Wheels (5 parts each): rim, tyre, disc, caliper, hub cap; separate objects
# ---------------------------------------------------------------------------
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


def rim_mesh(r_rim, width, name, spokes=10):
    """Carrera S 20/21-inch: 10 thin twin spokes (5 pairs), concave face."""
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
        pair = k // 2
        base = 2 * math.pi * pair / (n // 2)
        a = base + (0.11 if k % 2 else -0.11)
        bm = bmesh.new()
        prof_r = np.linspace(0.085, r_rim * 0.93, 9)
        rows = []
        for j, r in enumerate(prof_r):
            t = j / (len(prof_r) - 1)
            x = hw * (0.62 - 0.28 * math.sin(math.pi * t * 0.9) + 0.25 * t)  # concave dish
            wdt = 0.022 * (1.15 - 0.45 * t)
            dep = 0.03 * (1.0 - 0.4 * t)
            ang = a + (0.12 if k % 2 else -0.12) * t * 0.2  # slight splay
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
    for k in range(5):
        t = 2 * math.pi * k / 5 + math.pi / 5
        bolt = parts.lathe([(0.0, 0.0), (0.011, 0.0), (0.011, 0.018), (0.0, 0.02)], 12, f"{name}_lug{k}", axis="X", coll=W, mat=MAT["chrome"])
        bolt.data.transform(Matrix.Translation((hw * 0.5, 0.06 * math.cos(t), 0.06 * math.sin(t))))
        objs.append(bolt)
    return objs


def brake(r_out_tyre, name, front=True):
    objs = []
    rd = SPEC["brakes"]["front_disc_mm" if front else "rear_disc_mm"] / 2000
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


WHEELS = {}
for key, axle_y, track, tyre in (("FL", AF, TF, TYRE_F), ("FR", AF, TF, TYRE_F), ("RL", AR, TR, TYRE_R), ("RR", AR, TR, TYRE_R)):
    width, r_rim, r_out = tyre
    left = key.endswith("L")
    s = -1 if left else 1
    tyre_o = tyre_mesh(width, r_rim, r_out, f"wheel_{key}_tyre")
    rim = rim_mesh(r_rim, width, f"wheel_{key}_rim")
    br = brake(r_out, f"brake_{key}", front=key.startswith("F"))
    # Join rim parts, keep tyre separate (different material), brake separate (does not spin).
    for group in (rim,):
        bpy.ops.object.select_all(action="DESELECT")
        for o in group:
            o.select_set(True)
        bpy.context.view_layer.objects.active = group[0]
        bpy.ops.object.join()
    rim_o = group[0]
    rim_o.name = f"wheel_{key}_rim"
    bpy.ops.object.select_all(action="DESELECT")
    for o in br:
        o.select_set(True)
    bpy.context.view_layer.objects.active = br[0]
    bpy.ops.object.join()
    brake_o = br[0]
    brake_o.name = f"brake_{key}"
    # Mirror for the left side (outer face must point outward: +x right, -x left).
    centre = Vector((s * track / 2, axle_y, r_out))
    for o in (tyre_o, rim_o, brake_o):
        if left:
            o.data.transform(Matrix.Scale(-1, 4, (1, 0, 0)))
            o.data.flip_normals()
        o.location = centre
    # Caliper sits behind the axle for front, ahead for rear? 992: front caliper at the trailing edge (rear).
    WHEELS[key] = {"tyre": tyre_o, "rim": rim_o, "brake": brake_o, "centre": centre, "radius": r_out}

# ---------------------------------------------------------------------------
# 9. Interior (visible through glass): tub, seats, dash, wheel
# ---------------------------------------------------------------------------
def interior():
    objs = []
    # Floor/tub: box inside the cabin
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=(1.5, 2.1, 0.3), verts=bm.verts)
    bmesh.ops.translate(bm, vec=(0, -0.15, 0.35), verts=bm.verts)
    objs.append(parts.bm_to_object(bm, "interior_tub", W, MAT["leather"]))
    # Seats (2): bucket shapes from boxes with bevel.
    for s in (-1, 1):
        for (sz, loc, nm) in (((0.5, 0.5, 0.12), (s * 0.37, -0.15, 0.48), "cushion"), ((0.5, 0.12, 0.62), (s * 0.37, -0.45, 0.78), "back"), ((0.3, 0.1, 0.18), (s * 0.37, -0.5, 1.12), "head")):
            bm = bmesh.new()
            bmesh.ops.create_cube(bm, size=1.0)
            bmesh.ops.scale(bm, vec=sz, verts=bm.verts)
            if nm == "back":
                bmesh.ops.rotate(bm, verts=bm.verts, cent=(0, 0, 0), matrix=Matrix.Rotation(math.radians(-14), 3, "X"))
            bmesh.ops.translate(bm, vec=loc, verts=bm.verts)
            o = parts.bm_to_object(bm, f"seat_{nm}_{s}", W, MAT["leather"])
            b = o.modifiers.new("bev", "BEVEL")
            b.width, b.segments = 0.04, 3
            parts.apply_modifiers(o)
            objs.append(o)
    # Dashboard: swept slab under the windscreen.
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=(1.45, 0.45, 0.16), verts=bm.verts)
    bmesh.ops.translate(bm, vec=(0, 0.45, 0.86), verts=bm.verts)
    o = parts.bm_to_object(bm, "dashboard", W, MAT["leather"])
    b = o.modifiers.new("bev", "BEVEL")
    b.width, b.segments = 0.05, 3
    parts.apply_modifiers(o)
    objs.append(o)
    # Centre screen
    bm = bmesh.new()
    vs = [bm.verts.new(v) for v in ((-0.14, 0.24, 0.84), (0.14, 0.24, 0.84), (0.14, 0.22, 0.94), (-0.14, 0.22, 0.94))]
    bm.faces.new(vs)
    objs.append(parts.bm_to_object(bm, "centre_screen", W, MAT["screen"]))
    # Steering wheel (left-hand drive): torus at the driver position
    sw = parts.lathe([(0.17, 0.0), (0.185, 0.012), (0.2, 0.0), (0.185, -0.012), (0.17, 0.0)], 48, "steering_wheel", axis="Y", coll=W, mat=MAT["leather"])
    sw.data.transform(Matrix.Translation((-0.37, 0.18, 0.88)) @ Matrix.Rotation(math.radians(-22), 4, "X"))
    objs.append(sw)
    return objs


cabin = interior()

# ---------------------------------------------------------------------------
# 10. Assign paint to the shell, UV unwrap, smooth shading, final body object
# ---------------------------------------------------------------------------
shell.data.materials.clear()  # drop the base mesh's own material slot
shell.data.materials.append(MAT["paint"])
for poly in shell.data.polygons:
    poly.material_index = 0
# Bonnet seam / door shut lines are separate ribbons. Unwrap with smart project
# so a dirt/wear mask or decals can be layered by the game.
for o in [shell]:
    bpy.context.view_layer.objects.active = o
    o.select_set(True)
    bpy.ops.object.mode_set(mode="EDIT")
    bpy.ops.mesh.select_all(action="SELECT")
    bpy.ops.uv.smart_project(angle_limit=math.radians(60), island_margin=0.004)
    bpy.ops.object.mode_set(mode="OBJECT")
    o.select_set(False)
# Weighted normals keep panel edges crisp after booleans.
wn = shell.modifiers.new("wn", "WEIGHTED_NORMAL")
wn.keep_sharp = True
parts.apply_modifiers(shell)
shell.name = "body"

# ---------------------------------------------------------------------------
# 11. Collect LOD0: join static parts by material group for draw-call budget
# ---------------------------------------------------------------------------
def join(objs, name):
    objs = [o for o in objs if o and o.name in bpy.data.objects]
    if not objs:
        return None
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    o = bpy.context.view_layer.objects.active
    o.name = name
    o.data.name = name
    return o


static_parts = [o for o in W.objects if not (o.name.startswith("wheel_") or o.name.startswith("brake_") or o.name in ("body",))]
# Ensure every mesh has a UV map (glTF tangents need one when normal maps are used).
for o in W.objects:
    if o.type == "MESH" and not o.data.uv_layers:
        o.data.uv_layers.new(name="UVMap")
for o in W.objects:
    if o.type == "MESH":
        for p in o.data.polygons:
            p.use_smooth = True

lights_objs = [o for o in static_parts if any(k in o.name for k in ("headlight", "drl", "tail", "intake_R_indicator", "intake_L_indicator", "third_brake"))]
glass_objs = [o for o in static_parts if o.name.startswith("glass_") or o.name.startswith("mirror_glass")]
interior_objs = [o for o in static_parts if o in cabin or o.name == "body_inner"]
detail_objs = [o for o in static_parts if o not in lights_objs + glass_objs + interior_objs]

body_detail = join(detail_objs + [shell], "body")
lights = join(lights_objs, "lights")
glass = join(glass_objs, "glass")
cabin_o = join(interior_objs, "interior")

for o in [body_detail, lights, glass, cabin_o] + [w[k] for w in WHEELS.values() for k in ("tyre", "rim", "brake")]:
    if o is None:
        continue
    for c in list(o.users_collection):
        c.objects.unlink(o)
    coll["LOD0"].objects.link(o)

# Wheel hierarchy: an empty per wheel at the hub centre; rim+tyre parented (they spin),
# brake parented to a separate non-spinning empty.
for key, w in WHEELS.items():
    e = bpy.data.objects.new(f"wheel_{key}", None)
    e.empty_display_type = "ARROWS"
    e.empty_display_size = 0.2
    e.location = w["centre"]
    coll["LOD0"].objects.link(e)
    w["empty"] = e
    for part in ("tyre", "rim"):
        o = w[part]
        o.parent = e
        o.location = (0, 0, 0)
    eb = bpy.data.objects.new(f"brake_{key}_mount", None)
    eb.location = w["centre"]
    coll["LOD0"].objects.link(eb)
    w["brake"].parent = eb
    w["brake"].location = (0, 0, 0)

for o in coll["LOD0"].objects:
    o.name = o.name if o.name.endswith("_LOD0") or o.type == "EMPTY" else o.name

# ---------------------------------------------------------------------------
# 12. LODs (decimate copies), collision hull, sockets
# ---------------------------------------------------------------------------
LOD_RATIOS = {"LOD1": 0.35, "LOD2": 0.12, "LOD3": 0.035}


def decimate_copy(src, ratio, name, coll_):
    o = src.copy()
    o.data = src.data.copy()
    o.name = name
    coll_.objects.link(o)
    # Wheel parts keep their parent (wheel_XX / brake_XX_mount empties) so
    # every LOD spins and steers with the same node.
    m = o.modifiers.new("dec", "DECIMATE")
    m.ratio = ratio
    m.use_collapse_triangulate = True
    parts.apply_modifiers(o)
    return o


lod_sources = [o for o in coll["LOD0"].objects if o.type == "MESH" and o.parent is None]
wheel_sources = [o for o in coll["LOD0"].objects if o.type == "MESH" and o.parent is not None]
for lod, ratio in LOD_RATIOS.items():
    for src in lod_sources:
        if lod == "LOD3" and src.name in ("interior",):
            continue
        decimate_copy(src, ratio, f"{src.name}_{lod}", coll[lod])
    for src in wheel_sources:
        if src.name.startswith("brake") and lod == "LOD3":
            continue
        decimate_copy(src, min(1.0, ratio * 1.5), f"{src.name}_{lod}", coll[lod])

# Collision: convex hull of the decimated body.
col_src = decimate_copy(body_detail, 0.02, "COL_body_hull", coll["COLLISION"])
col_src.parent = None
bm = bmesh.new()
bm.from_mesh(col_src.data)
hull = bmesh.ops.convex_hull(bm, input=bm.verts)
drop = {v for v in hull["geom_interior"] + hull["geom_unused"] if isinstance(v, bmesh.types.BMVert)}
bmesh.ops.delete(bm, geom=list(drop), context="VERTS")
bm.to_mesh(col_src.data)
bm.free()
col_src.data.materials.clear()
col_src.display_type = "WIRE"

# Sockets (empties, exported as nodes with extras for the game).
def socket(name, loc, **extras):
    e = bpy.data.objects.new(name, None)
    e.empty_display_type = "SPHERE"
    e.empty_display_size = 0.05
    e.location = loc
    for k, v in extras.items():
        e[k] = v
    coll["SOCKETS"].objects.link(e)
    return e


for key, w in WHEELS.items():
    socket(f"socket_wheel_{key}", w["centre"], radius=w["radius"])
for s in (-1, 1):
    p = line(65)
    socket(f"socket_exhaust_{'L' if s < 0 else 'R'}", (s * abs(p[:, 0].mean()), -2.24, p[:, 1].mean()))
for left in (True, False):
    ring = line(143 if not left else 154)
    s = -1 if left else 1
    socket(f"socket_headlight_{'L' if left else 'R'}", (s * abs(ring.mean(0)[0]), 2.15, ring.mean(0)[1]))
    socket(f"socket_taillight_{'L' if left else 'R'}", (s * 0.78, -2.22, 0.76))
socket("socket_camera_hood", (0.0, 0.9, 1.05))
socket("socket_camera_cockpit", (-0.37, -0.2, 1.1))
socket("socket_camera_bumper", (0.0, 2.3, 0.45))

# ---------------------------------------------------------------------------
# 13. Report + save + export
# ---------------------------------------------------------------------------
def tris(c):
    return sum(parts.tri_count(o) for o in c.objects if o.type == "MESH")


report = {k: tris(coll[k]) for k in ("LOD0", "LOD1", "LOD2", "LOD3", "COLLISION")}
print("TRIANGLES", report)
for o in list(W.objects):
    parts.remove(o)
# Export hygiene: triangulate, merge doubles, drop degenerate faces, validate.
for o in bpy.data.objects:
    if o.type != "MESH":
        continue
    bm = bmesh.new()
    bm.from_mesh(o.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-6)
    bmesh.ops.dissolve_degenerate(bm, edges=bm.edges, dist=1e-7)
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if not v.link_faces], context="VERTS")
    bmesh.ops.triangulate(bm, faces=bm.faces, quad_method="BEAUTY", ngon_method="BEAUTY")
    bm.to_mesh(o.data)
    bm.free()
    o.data.validate(clean_customdata=False)
    if not o.data.uv_layers:
        o.data.uv_layers.new(name="UVMap")
bpy.data.collections.remove(W)
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, "porsche_992.blend"))

# glTF/GLB for Bevy: Y up, -Z forward. Collections become top-level nodes
# (LOD0..LOD3, COLLISION, SOCKETS); the game picks nodes by name.
bpy.ops.export_scene.gltf(
    filepath=os.path.join(OUT, "porsche_992.glb"),
    export_format="GLB",
    export_yup=True,
    export_apply=True,
    export_tangents=True,
    export_extras=True,
    export_image_format="AUTO",
    use_visible=False,
    export_materials="EXPORT",
)
print("exported", os.path.join(OUT, "porsche_992.glb"))
json.dump(report, open(os.path.join(OUT, "stats.json"), "w"), indent=1)
