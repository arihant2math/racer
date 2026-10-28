"""Build a game car asset from its aligned image-to-3D base mesh.

    blender -b --python pipelines/blender/cars/build.py -- <repo_root> <car_id> [out_dir]

Inputs (all in the repo):
  data/cars/<id>/car.toml                    dimensions, tyres, discs (hard constraints)
  data/cars/<id>/refs/base_mesh/base.glb     TRELLIS.2 mesh + PBR texture, aligned to the
                                             blueprint by tools/car_refs/normalize_base.py
  pipelines/blender/cars/configs/<id>.toml   wheel design, caliper colour, tyre text, exhausts

Output in <out_dir> (default assets/cars/<id>/): <out>.glb and <out>.blend, stats.json.

Steps:
1. Import the base (metres, X right, Y forward, Z up), find its axles from the
   tyre contact patches and put the origin on the ground at the wheelbase midpoint.
2. Delete the generated wheels (cylinders at the axles), add arch liners.
3. Classify faces from the generated base colour: body paint (close to the
   bonnet colour) gets `car_paint` (the game swaps it for the grid colour),
   dark greenhouse faces get `glass`, red rear lamps get `light_tail`; the
   rest keeps the generated PBR texture (`body_detail`: grilles, lamps,
   badges, trim). Paint regions are relaxed to clean up reflections.
4. Wheels and brakes from the tyre/disc sizes (wheels.py).
5. LOD1-3, convex collision hull, sockets, glTF export (same contract as
   pipelines/blender/porsche_992, see MODEL_GEN.md).
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
sys.path.insert(0, os.path.join(HERE, "../porsche_992"))
import materials  # noqa: E402
import parts  # noqa: E402
import textures  # noqa: E402
import wheels  # noqa: E402

argv = sys.argv[sys.argv.index("--") + 1:]
ROOT, CAR_ID = argv[0], argv[1]
OUT = argv[2] if len(argv) > 2 else os.path.join(ROOT, "assets/cars", CAR_ID)
CAR = os.path.join(ROOT, "data/cars", CAR_ID)
os.makedirs(OUT, exist_ok=True)
SPEC = tomllib.load(open(os.path.join(CAR, "car.toml"), "rb"))
CFG = tomllib.load(open(os.path.join(HERE, "configs", CAR_ID + ".toml"), "rb"))
C = SPEC["chassis"]
L, WIDTH, H = C["length_m"], C["width_m"], C["height_m"]
WB, TF, TR = C["wheelbase_m"], C["track_front_m"], C["track_rear_m"]
AF, AR = WB / 2, -WB / 2
BODY_TRIS = 320_000


def tyre_dims(s):
    w, ar, rim = s.replace("ZR", " ").replace("R", " ").replace("/", " ").split()[:3]
    w, ar, rim = float(w) / 1000, float(ar) / 100, float(rim) * 0.0254
    return w, rim / 2, rim / 2 + w * ar


TYRE_F = tyre_dims(SPEC["tires"]["front"])
TYRE_R = tyre_dims(SPEC["tires"]["rear"])

# ---------------------------------------------------------------------------
bpy.ops.wm.read_factory_settings(use_empty=True)
textures.TYRE_TEXT = CFG.get("tyre_text", textures.TYRE_TEXT)
TEX = textures.build_all()
MAT = materials.build(TEX)
MAT["caliper"].node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (*CFG["caliper"], 1)
MAT["caliper"].diffuse_color = (*CFG["caliper"], 1)
MAT["rim"].node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (*CFG["rim"]["colour"], 1)
MAT["rim"].diffuse_color = (*CFG["rim"]["colour"], 1)
if not CFG.get("badge", True):
    # Hub cap: plain gloss black instead of the Porsche crest texture.
    bc = MAT["badge"].node_tree
    for n in [n for n in bc.nodes if n.type == "TEX_IMAGE"]:
        bc.nodes.remove(n)
    bc.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.02, 0.02, 0.022, 1)
    MAT["badge"].diffuse_color = (0.02, 0.02, 0.022, 1)
# materials.build() already made a transmissive "glass"; the generated shell
# is closed (no interior), so the body glass is opaque, dark and glossy.
g = bpy.data.materials["glass"]
b = g.node_tree.nodes["Principled BSDF"]
b.inputs["Base Color"].default_value = (0.015, 0.018, 0.022, 1)
b.inputs["Transmission Weight"].default_value = 0.0
b.inputs["Alpha"].default_value = 1.0
b.inputs["Roughness"].default_value = 0.03
b.inputs["Coat Weight"].default_value = 1.0
b.inputs["Coat Roughness"].default_value = 0.0
if hasattr(g, "surface_render_method"):
    g.surface_render_method = "DITHERED"
g.diffuse_color = (0.03, 0.035, 0.04, 1)
MAT["glass_body"] = g

scene = bpy.context.scene
coll = {}
for name in ("LOD0", "LOD1", "LOD2", "LOD3", "COLLISION", "SOCKETS", "_work"):
    c = bpy.data.collections.new(name)
    scene.collection.children.link(c)
    coll[name] = c
W = coll["_work"]
wheels.setup(W, MAT)

# ---------------------------------------------------------------------------
# 1. Base mesh, axles, origin
# ---------------------------------------------------------------------------
before = set(bpy.data.objects)
bpy.ops.import_scene.gltf(filepath=os.path.join(CAR, "refs/base_mesh/base.glb"), merge_vertices=True)
new = [o for o in bpy.data.objects if o not in before]
meshes = [o for o in new if o.type == "MESH"]
for o in meshes:
    o.data.transform(o.matrix_world)
    o.matrix_world = Matrix.Identity(4)
    o.parent = None
if len(meshes) > 1:
    bpy.ops.object.select_all(action="DESELECT")
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    bpy.ops.object.join()
shell = meshes[0]
for o in new:
    if o is not shell and o.name in bpy.data.objects:
        bpy.data.objects.remove(o)
for c in list(shell.users_collection):
    c.objects.unlink(shell)
W.objects.link(shell)
shell.name = "body"
me = shell.data
bm = bmesh.new()
bm.from_mesh(me)
nv0 = len(bm.verts)
bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)  # UV seams split vertices; UVs live on loops
print("welded", nv0, "->", len(bm.verts), "verts")
bm.to_mesh(me)
bm.free()
BASE_MAT = me.materials[0]
BASE_MAT.name = "body_detail"

V = np.empty(len(me.vertices) * 3)
me.vertices.foreach_get("co", V)
V = V.reshape(-1, 3)
low = V[V[:, 2] < 0.02]
wf, wr = low[low[:, 1] > 0, 1].mean(), low[low[:, 1] < 0, 1].mean()
mid = 0.5 * (wf + wr)
print(f"generated axles {wr:.3f} {wf:.3f} (wheelbase {wf - wr:.3f}, spec {WB:.3f}), shift {-mid:.3f}")
me.transform(Matrix.Translation((0, -mid, -V[:, 2].min())))

# ---------------------------------------------------------------------------
# 2. Remove the generated wheels
# ---------------------------------------------------------------------------
bm = bmesh.new()
bm.from_mesh(me)
kill = []
for f in bm.faces:
    c = f.calc_center_median()
    for ay, (tw, rr, ro), tr in ((AF, TYRE_F, TF), (AR, TYRE_R, TR)):
        if math.hypot(c.y - ay, c.z - ro) < ro + 0.008 and abs(c.x) > tr / 2 - tw / 2 - 0.12:
            kill.append(f)
            break
bmesh.ops.delete(bm, geom=kill, context="FACES")
# Debris left by the cut.
bm.verts.ensure_lookup_table()
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
            o = e.other_vert(a)
            if o.index not in seen:
                seen.add(o.index)
                st.append(o)
    comps.append(comp)
big = max(len(c) for c in comps)
bmesh.ops.delete(bm, geom=[v for c in comps if len(c) < 0.01 * big for v in c], context="VERTS")
bm.to_mesh(me)
bm.free()
print("removed generated wheel faces", len(kill))


def arch_liner(ay, r, half_track, width, name):
    bm = bmesh.new()
    segs = 48
    x_in, x_out = max(half_track - width * 0.5 - 0.1, 0.35), WIDTH / 2 + 0.01
    rows = []
    for k in range(segs + 1):
        t = math.pi * (-0.05 + 1.1 * k / segs)
        y, z = ay + r * math.cos(t), r * 0.92 + r * math.sin(t)
        rows.append((bm.verts.new((-x_out, y, z)), bm.verts.new((-x_in, y, z))))
    for k in range(segs):
        a, b2 = rows[k], rows[k + 1]
        bm.faces.new((a[0], b2[0], b2[1], a[1]))
    # Inner wall (closes the wheel well towards the engine bay / cabin).
    wall = [bm.verts.new((-x_in, ay + r * math.cos(math.pi * k / segs), r * 0.92 + r * math.sin(math.pi * k / segs))) for k in range(segs + 1)]
    ctr = bm.verts.new((-x_in, ay, r * 0.92))
    for k in range(segs):
        bm.faces.new((ctr, wall[k + 1], wall[k]))
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    for f in bm.faces:
        f.normal_flip()
    ob = parts.bm_to_object(bm, name, W, MAT["plastic"])
    m = ob.modifiers.new("mirror", "MIRROR")
    m.use_axis[0] = True
    parts.apply_modifiers(ob)
    return ob


liners = [
    arch_liner(AF, TYRE_F[2] + 0.035, TF / 2, TYRE_F[0], "arch_liner_front"),
    arch_liner(AR, TYRE_R[2] + 0.035, TR / 2, TYRE_R[0], "arch_liner_rear"),
]

# ---------------------------------------------------------------------------
# 3. Face classes from the generated texture
# ---------------------------------------------------------------------------
def tex(colourspace):
    im = next(n.image for n in BASE_MAT.node_tree.nodes if n.type == "TEX_IMAGE" and n.image.colorspace_settings.name == colourspace)
    w_, h_ = im.size
    a_ = np.empty(w_ * h_ * 4, np.float32)
    im.pixels.foreach_get(a_)  # byte images: stored (sRGB-encoded) values
    return a_.reshape(h_, w_, 4)


BC = tex("sRGB")  # base colour + alpha
MR = tex("Non-Color")  # glTF: G roughness, B metallic
TH, TW = BC.shape[:2]
nf = len(me.polygons)
uvs = np.empty(len(me.loops) * 2)
me.uv_layers.active.data.foreach_get("uv", uvs)
uvs = uvs.reshape(-1, 2)
ls = np.empty(nf, int)
lt = np.empty(nf, int)
me.polygons.foreach_get("loop_start", ls)
me.polygons.foreach_get("loop_total", lt)
fuv = np.add.reduceat(uvs, ls) / lt[:, None]
ij = (np.clip((fuv[:, 1] * (TH - 1)).astype(int), 0, TH - 1), np.clip((fuv[:, 0] * (TW - 1)).astype(int), 0, TW - 1))
col, alpha = BC[..., :3][ij], BC[..., 3][ij]
rough, metal = MR[..., 1][ij], MR[..., 2][ij]
cen = np.empty(nf * 3)
nor = np.empty(nf * 3)
me.polygons.foreach_get("center", cen)
me.polygons.foreach_get("normal", nor)
cen, nor = cen.reshape(-1, 3), nor.reshape(-1, 3)
luma = col @ [0.2126, 0.7152, 0.0722]

# Paint reference: the bonnet seen from above (the generated mesh also has
# cabin and under-bonnet surfaces, so only faces hit first by a vertical ray).
from mathutils.bvhtree import BVHTree
bvh = BVHTree.FromPolygons([v.co[:] for v in me.vertices], [p.vertices[:] for p in me.polygons])
hits = []
for x in np.linspace(-0.4, 0.4, 25):
    for y in np.linspace(AF - 0.2, AF + 0.6, 25):
        loc, n, fi, _ = bvh.ray_cast(Vector((x, y, 3.0)), Vector((0, 0, -1)), 5.0)
        if fi is not None and n.z > 0.5:
            hits.append(fi)
ref = np.median(col[hits], axis=0)
bonnet = np.zeros(nf, bool)
bonnet[hits] = True
ref_l = float(ref @ [0.2126, 0.7152, 0.0722])
a_ref, m_ref = float(np.median(alpha[hits])), float(np.median(metal[hits]))
metal_ok = metal < m_ref + 0.25  # paint is as (non)metallic as the bonnet
# Paint: same chroma as the bonnet, any brightness from baked shadow to
# highlight, dielectric. Glass: TRELLIS.2 marks it metallic + translucent.
chroma = np.linalg.norm(col / (luma[:, None] + 1e-3) - ref / (ref_l + 1e-3), axis=1)
ratio = luma / max(ref_l, 1e-3)
sat = (col.max(1) - col.min(1)) / (col.max(1) + 1e-3)
ref_sat = float((ref.max() - ref.min()) / (ref.max() + 1e-3))
def hue(c):
    c = np.atleast_2d(c)
    r, g, b = c[:, 0], c[:, 1], c[:, 2]
    return np.degrees(np.arctan2(np.sqrt(3) * (g - b), 2 * r - g - b))


if ref_sat > 0.3:  # coloured paint: hue angle, tolerant of baked highlights
    dh = np.abs((hue(col) - hue(ref)[0] + 180) % 360 - 180)
    chroma = dh / 175.0  # 35 degrees -> 0.2
paint = (chroma < 0.2) & (((ratio > 0.45) & (ratio < 1.35)) | ((ratio > 0.2) & (rough < 0.45))) & metal_ok
if ref_sat > 0.3:
    # Shadowed panels are darker but keep their hue; require some colour.
    paint = (chroma < 0.2) & (ratio > 0.15) & (ratio < 2.2) & (sat > 0.3 * ref_sat) & metal_ok
if ref_l > 0.45:  # light paint: any bright, unsaturated dielectric face is paint
    paint |= (sat < 0.12) & (luma > 0.5 * ref_l) & metal_ok & (alpha > 0.9 * a_ref)
if ref_l < 0.15:  # dark paint: brightness carries little, rely on roughness
    paint = (luma < 0.2) & (rough < 0.45) & metal_ok
print("paint reference", np.round(ref, 3), "from", len(hits), "faces")
cls = np.full(nf, 1)  # 0 paint, 1 detail (generated texture), 2 glass, 3 tail lamp
cls[paint] = 0
belt = 0.55 * H
# TRELLIS.2 marks glass as more translucent and/or more metallic than the
# paint; the absolute levels vary per run, so compare with the bonnet.
glass = (cen[:, 2] > belt) & ((alpha < a_ref - 0.12) | (metal > m_ref + 0.3))
print(f"bonnet alpha {a_ref:.2f} metal {m_ref:.2f}")
cls[glass] = 2
red = (col[:, 0] > 0.25) & (col[:, 0] > 2.2 * col[:, 1]) & (col[:, 0] > 2.0 * col[:, 2])
cls[red & (cen[:, 1] < AR - 0.2) & (cen[:, 2] > 0.4) & (nor[:, 1] < 0.3)] = 3

# Majority filter over face adjacency (removes speckle at baked highlights).
bm = bmesh.new()
bm.from_mesh(me)
bm.faces.ensure_lookup_table()
nbr = [[g.index for e in f.edges for g in e.link_faces if g.index != f.index] for f in bm.faces]
bm.free()
for _ in range(6):
    new_cls = cls.copy()
    for i, nb in enumerate(nbr):
        if not nb:
            continue
        cnt = np.bincount(cls[nb], minlength=4)
        k = int(cnt.argmax())
        if cnt[k] * 2 > len(nb) and k != cls[i]:
            new_cls[i] = k
    cls = new_cls
# Greenhouse x range from the blueprint side top line: from the windscreen
# base (top line rises above the bonnet) to the rear screen base (above the
# boot lid; hatchbacks have no boot).
BP = json.load(open(os.path.join(CAR, "refs/blueprint.json")))
bx, bz = np.array(BP["side"]["x"]), np.array(BP["side"]["z_top"])
off = float(cen[:, 1].max()) - L / 2  # blueprint x (length midpoint) -> model y
zt = lambda x: float(np.interp(x - off, bx, bz))
ys_ = np.linspace(-L / 2, L / 2, 400)
tall = np.array([zt(y) for y in ys_]) > 0.8 * H  # roof / greenhouse stations
x_front, x_rear = ys_[tall].max() + 0.12, ys_[tall].min() - 0.05
print(f"greenhouse y {x_rear:.2f}..{x_front + 0.05:.2f}")
in_gh = (cen[:, 1] < x_front + 0.05) & (cen[:, 1] > x_rear)
cls[(cls == 2) & ~in_gh] = 1
# Glass: grow the detected glass through non-paint faces inside the greenhouse
# (seals, pillar trim and reflections there read as detail), not into paint.
green = (cen[:, 2] > belt + 0.05) & (np.abs(cen[:, 0]) < WIDTH / 2 - 0.12) & in_gh
front = list(np.nonzero(cls == 2)[0])
while front:
    nxt = []
    for i in front:
        for j in nbr[i]:
            if cls[j] == 1 and green[j]:
                cls[j] = 2
                nxt.append(j)
    front = nxt
# Islands: merge components smaller than MIN_ISLAND faces into their surroundings.
MIN_ISLAND = 400
for _ in range(2):
    lab = np.full(nf, -1)
    sizes = []
    for i in range(nf):
        if lab[i] >= 0:
            continue
        k = len(sizes)
        lab[i] = k
        st, n_ = [i], 0
        while st:
            a_ = st.pop()
            n_ += 1
            for j in nbr[a_]:
                if lab[j] < 0 and cls[j] == cls[i]:
                    lab[j] = k
                    st.append(j)
        sizes.append(n_)
    sizes = np.array(sizes)
    small_f = np.nonzero(sizes[lab] < MIN_ISLAND)[0]
    for _ in range(30):
        changed = 0
        for i in small_f:
            nb = [j for j in nbr[i] if sizes[lab[j]] >= MIN_ISLAND]
            if nb:
                cls[i] = np.bincount(cls[nb], minlength=4).argmax()
                lab[i] = lab[nb[0]]
                changed += 1
        if not changed:
            break
print("classes: paint", (cls == 0).sum(), "detail", (cls == 1).sum(), "glass", (cls == 2).sum(), "tail", (cls == 3).sum())

me.materials.clear()
for m in (MAT["paint"], BASE_MAT, MAT["glass_body"], MAT["tail"]):
    me.materials.append(m)
me.polygons.foreach_set("material_index", cls)

# (No smoothing: corrective smooth tears the non-manifold generated shell.)
dec = shell.modifiers.new("decimate", "DECIMATE")
dec.ratio = min(1.0, BODY_TRIS / max(len(me.polygons), 1))
dec.use_collapse_triangulate = True
parts.apply_modifiers(shell)
shell.vertex_groups.clear()
for p in shell.data.polygons:
    p.use_smooth = True
wn = shell.modifiers.new("wn", "WEIGHTED_NORMAL")
wn.keep_sharp = True
parts.apply_modifiers(shell)
print("body tris", parts.tri_count(shell))

# Split glass and lamps into their own objects (game nodes `glass`, `lights`).
def split(ob, index, name):
    bm = bmesh.new()
    bm.from_mesh(ob.data)
    keep = [f for f in bm.faces if f.material_index == index]
    other = [f for f in bm.faces if f.material_index != index]
    me2 = ob.data.copy()
    bm2 = bmesh.new()
    bm2.from_mesh(me2)
    bmesh.ops.delete(bm2, geom=[f for f in bm2.faces if f.material_index != index], context="FACES")
    bm2.to_mesh(me2)
    bm2.free()
    bmesh.ops.delete(bm, geom=keep, context="FACES")
    bm.to_mesh(ob.data)
    bm.free()
    o = bpy.data.objects.new(name, me2)
    W.objects.link(o)
    return o


glass_o = split(shell, 2, "glass")
lights_o = split(shell, 3, "lights")
for o in (glass_o, lights_o, shell):
    # Drop unused slots so each node only carries its materials.
    used = {p.material_index for p in o.data.polygons}
    for i in reversed(range(len(o.data.materials))):
        if i not in used:
            o.data.materials.pop(index=i)

# ---------------------------------------------------------------------------
# 4. Wheels
# ---------------------------------------------------------------------------
RIM = CFG["rim"]
WHEELS = {}
for key, axle_y, track, tyre in (("FL", AF, TF, TYRE_F), ("FR", AF, TF, TYRE_F), ("RL", AR, TR, TYRE_R), ("RR", AR, TR, TYRE_R)):
    width, r_rim, r_out = tyre
    left = key.endswith("L")
    s = -1 if left else 1
    tyre_o = wheels.tyre_mesh(width, r_rim, r_out, f"wheel_{key}_tyre")
    rim = wheels.rim_mesh(r_rim, width, f"wheel_{key}_rim", spokes=RIM["spokes"], paired=RIM["paired"], spoke_w=RIM["spoke_w"], twist=RIM["twist"])
    br = wheels.brake(SPEC["brakes"]["front_disc_mm" if key[0] == "F" else "rear_disc_mm"], f"brake_{key}", front=key[0] == "F")
    out = []
    for group, nm in ((rim, f"wheel_{key}_rim"), (br, f"brake_{key}")):
        bpy.ops.object.select_all(action="DESELECT")
        for o in group:
            o.select_set(True)
        bpy.context.view_layer.objects.active = group[0]
        bpy.ops.object.join()
        group[0].name = nm
        out.append(group[0])
    rim_o, brake_o = out
    centre = Vector((s * track / 2, axle_y, r_out))
    for o in (tyre_o, rim_o, brake_o):
        if left:
            o.data.transform(Matrix.Scale(-1, 4, (1, 0, 0)))
            o.data.flip_normals()
        o.location = centre
    WHEELS[key] = {"tyre": tyre_o, "rim": rim_o, "brake": brake_o, "centre": centre, "radius": r_out}

# ---------------------------------------------------------------------------
# 5. LOD0 hierarchy, LODs, collision, sockets, export
# ---------------------------------------------------------------------------
def join(objs, name):
    objs = [o for o in objs if o and o.name in bpy.data.objects]
    bpy.ops.object.select_all(action="DESELECT")
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    o = bpy.context.view_layer.objects.active
    o.name = name
    o.data.name = name
    return o


for o in W.objects:
    if o.type == "MESH":
        if not o.data.uv_layers:
            o.data.uv_layers.new(name="UVMap")
        for p in o.data.polygons:
            p.use_smooth = True
body = join([shell] + liners, "body")
keep = [o for o in (body, glass_o, lights_o) if o.data.polygons]
for o in (glass_o, lights_o):
    if o not in keep:
        parts.remove(o)
for o in keep + [w[k] for w in WHEELS.values() for k in ("tyre", "rim", "brake")]:
    for c in list(o.users_collection):
        c.objects.unlink(o)
    coll["LOD0"].objects.link(o)
for key, w in WHEELS.items():
    e = bpy.data.objects.new(f"wheel_{key}", None)
    e.location = w["centre"]
    coll["LOD0"].objects.link(e)
    for part in ("tyre", "rim"):
        w[part].parent = e
        w[part].location = (0, 0, 0)
    eb = bpy.data.objects.new(f"brake_{key}_mount", None)
    eb.location = w["centre"]
    coll["LOD0"].objects.link(eb)
    w["brake"].parent = eb
    w["brake"].location = (0, 0, 0)

LOD_RATIOS = {"LOD1": 0.35, "LOD2": 0.12, "LOD3": 0.035}


def decimate_copy(src, ratio, name, coll_):
    o = src.copy()
    o.data = src.data.copy()
    o.name = name
    coll_.objects.link(o)
    m = o.modifiers.new("dec", "DECIMATE")
    m.ratio = ratio
    m.use_collapse_triangulate = True
    parts.apply_modifiers(o)
    return o


lod_sources = [o for o in coll["LOD0"].objects if o.type == "MESH" and o.parent is None]
wheel_sources = [o for o in coll["LOD0"].objects if o.type == "MESH" and o.parent is not None]
# Body parts: decimate each LOD from the previous one (one big collapse from
# the generated mesh stalls on its non-manifold edges).
prev = {o.name: (o, 1.0) for o in lod_sources}
for lod, ratio in LOD_RATIOS.items():
    for name_, (src, r0) in list(prev.items()):
        target = parts.tri_count(src) * ratio / r0
        o = decimate_copy(src, ratio / r0, f"{name_}_{lod}", coll[lod])
        # Stalled collapse (the generated shell has non-manifold edges, which
        # the collapse never removes): split those edges and decimate again.
        for _attempt in range(3):
            if parts.tri_count(o) <= 1.3 * target:
                break
            bm = bmesh.new()
            bm.from_mesh(o.data)
            bmesh.ops.split_edges(bm, edges=[e for e in bm.edges if len(e.link_faces) > 2])
            # Tiny loose pieces (generated interior debris) cannot collapse further.
            if _attempt:
                seen, small = set(), []
                for f in bm.faces:
                    if f in seen:
                        continue
                    st, comp = [f], []
                    seen.add(f)
                    while st:
                        a_ = st.pop()
                        comp.append(a_)
                        for e in a_.edges:
                            for g_ in e.link_faces:
                                if g_ not in seen:
                                    seen.add(g_)
                                    st.append(g_)
                    if len(comp) < 24:
                        small += comp
                bmesh.ops.delete(bm, geom=small, context="FACES")
            bm.to_mesh(o.data)
            bm.free()
            m = o.modifiers.new("dec", "DECIMATE")
            m.ratio = min(1.0, target / max(parts.tri_count(o), 1))
            m.use_collapse_triangulate = True
            parts.apply_modifiers(o)
        print(lod, name_, parts.tri_count(o), "target", int(target))
        prev[name_] = (o, ratio)
for lod, ratio in LOD_RATIOS.items():
    for src in wheel_sources:
        if src.name.startswith("brake") and lod == "LOD3":
            continue
        decimate_copy(src, min(1.0, ratio * 1.5), f"{src.name}_{lod}", coll[lod])

col_src = decimate_copy(body, 0.01, "COL_body_hull", coll["COLLISION"])
bm = bmesh.new()
bm.from_mesh(col_src.data)
hull = bmesh.ops.convex_hull(bm, input=bm.verts)
bmesh.ops.delete(bm, geom=[v for v in hull["geom_interior"] + hull["geom_unused"] if isinstance(v, bmesh.types.BMVert)], context="VERTS")
bm.to_mesh(col_src.data)
bm.free()
col_src.data.materials.clear()


def socket(name, loc, **extras):
    e = bpy.data.objects.new(name, None)
    e.location = loc
    for k, v in extras.items():
        e[k] = v
    coll["SOCKETS"].objects.link(e)
    return e


Vb = np.array([v.co[:] for v in body.data.vertices])
y_front, y_rear = Vb[:, 1].max(), Vb[:, 1].min()
for key, w in WHEELS.items():
    socket(f"socket_wheel_{key}", w["centre"], radius=w["radius"])
ex = CFG["exhaust"]
for i, x in enumerate(ex["y"]):
    socket(f"socket_exhaust_{i}" if len(ex["y"]) > 2 else f"socket_exhaust_{'L' if x < 0 else 'R'}", (x, y_rear - 0.02, ex["z"]))
for left in (True, False):
    s = -1 if left else 1
    socket(f"socket_headlight_{'L' if left else 'R'}", (s * (WIDTH / 2 - 0.22), y_front - 0.1, 0.68))
    socket(f"socket_taillight_{'L' if left else 'R'}", (s * (WIDTH / 2 - 0.15), y_rear + 0.08, 0.8))
drv = -0.37 if CFG.get("drive_side", "left") == "left" else 0.37
socket("socket_camera_hood", (0.0, 0.9, H - 0.4))
socket("socket_camera_cockpit", (drv, -0.2, H - 0.32))
socket("socket_camera_bumper", (0.0, y_front + 0.05, 0.45))

report = {k: sum(parts.tri_count(o) for o in coll[k].objects if o.type == "MESH") for k in ("LOD0", "LOD1", "LOD2", "LOD3", "COLLISION")}
print("TRIANGLES", report)
for o in list(W.objects):
    parts.remove(o)
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
for im in bpy.data.images:
    if im.size[0] > 2048:
        im.scale(2048, 2048)
        im.pack()
name = CFG.get("out", "model")
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, name + ".blend"))
bpy.ops.export_scene.gltf(
    filepath=os.path.join(OUT, name + ".glb"),
    export_format="GLB",
    export_yup=True,
    export_apply=True,
    export_tangents=True,
    export_extras=True,
    export_image_format="AUTO",
    use_visible=False,
    export_materials="EXPORT",
)
json.dump(report, open(os.path.join(OUT, "stats.json"), "w"), indent=1)
print("exported", os.path.join(OUT, name + ".glb"))
