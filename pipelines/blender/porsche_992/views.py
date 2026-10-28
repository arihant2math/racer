"""Render review views of a .blend or .glb (car frame: X right, Y forward, Z up).

blender -b --python views.py -- <file> <out_prefix> [workbench|eevee|cycles] [views...]
Views: side, top, front, rear, q_fl, q_rr, q_fr, low_fl, close_front, close_rear,
wheel. Default: side top front rear q_fl q_rr.
"""
import bpy, sys, math
from mathutils import Vector

argv = sys.argv[sys.argv.index("--") + 1:]
src, out = argv[0], argv[1]
mode = argv[2] if len(argv) > 2 else "workbench"
names = argv[3:] or ["side", "top", "front", "rear", "q_fl", "q_rr"]

if src.endswith(".blend"):
    bpy.ops.wm.open_mainfile(filepath=src)
else:
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=src)
    # glTF: +Y up, -Z forward -> importer gives Z up, +Y forward already.
sc = bpy.context.scene
for o in list(sc.objects):
    if o.type in {"CAMERA", "LIGHT"} and o.name.startswith("_rv"):
        bpy.data.objects.remove(o)
meshes = [o for o in sc.objects if o.type == "MESH" and o.visible_get() and not o.name.startswith("LOD") and not o.name.startswith("COL")]
import re
for o in sc.objects:
    if o.type == "MESH" and (o.name.startswith("LOD") or o.name.startswith("COL") or re.search(r"_LOD[1-3]", o.name, re.I)):
        o.hide_render = True
mn = Vector((1e9,) * 3); mx = -mn
for o in meshes:
    if o.hide_render:
        continue
    for c in o.bound_box:
        w = o.matrix_world @ Vector(c)
        mn = Vector(map(min, mn, w)); mx = Vector(map(max, mx, w))
ctr = (mn + mx) / 2
size = max(mx - mn)

cam = bpy.data.objects.new("_rv_cam", bpy.data.cameras.new("_rv_cam"))
sc.collection.objects.link(cam)
sc.camera = cam
sc.render.resolution_x, sc.render.resolution_y = 1600, 900
if mode == "workbench":
    sc.render.engine = "BLENDER_WORKBENCH"
    sc.display.shading.light = "STUDIO"
    sc.display.shading.color_type = "MATERIAL"
    sc.display.shading.show_specular_highlight = True
    sc.display.shading.show_cavity = True
else:
    sc.render.engine = "CYCLES" if mode == "cycles" else "BLENDER_EEVEE"
    if mode == "cycles":
        sc.cycles.samples = 96
        sc.cycles.use_denoising = True
        try:
            sc.cycles.device = "GPU"
            prefs = bpy.context.preferences.addons["cycles"].preferences
            prefs.compute_device_type = "METAL"
            prefs.get_devices()
            for d in prefs.devices:
                d.use = True
        except Exception:
            pass
    sc.view_settings.view_transform = "AgX"
    sc.view_settings.look = "AgX - Medium High Contrast"
    # Lookdev: Blender's bundled studio HDRI + a ground plane.
    import os
    hdri = os.path.join(os.path.dirname(bpy.app.binary_path), "../Resources", bpy.app.version_string[:3], "datafiles/studiolights/world", os.environ.get("RV_HDRI", "sunset.exr"))
    w = bpy.data.worlds.new("_rv_world")
    sc.world = w
    w.use_nodes = True
    nt = w.node_tree
    env = nt.nodes.new("ShaderNodeTexEnvironment")
    env.image = bpy.data.images.load(os.path.normpath(hdri))
    mapping = nt.nodes.new("ShaderNodeMapping")
    tc = nt.nodes.new("ShaderNodeTexCoord")
    mapping.inputs["Rotation"].default_value[2] = float(os.environ.get("RV_ROT", "1.2"))
    nt.links.new(tc.outputs["Generated"], mapping.inputs["Vector"])
    nt.links.new(mapping.outputs["Vector"], env.inputs["Vector"])
    nt.links.new(env.outputs["Color"], nt.nodes["Background"].inputs["Color"])
    nt.nodes["Background"].inputs["Strength"].default_value = 1.0
    bpy.ops.mesh.primitive_plane_add(size=60)
    gp = bpy.context.active_object
    gp.name = "_rv_ground"
    gm = bpy.data.materials.new("_rv_ground")
    gm.use_nodes = True
    gm.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.18, 0.18, 0.19, 1)
    gm.node_tree.nodes["Principled BSDF"].inputs["Roughness"].default_value = 0.6
    gp.data.materials.append(gm)

V = {
    "side": ((-1, 0, 0), "ORTHO", 0),
    "side_r": ((1, 0, 0), "ORTHO", 0),
    "top": ((0, 0, 1), "ORTHO", 0),
    "front": ((0, 1, 0), "ORTHO", 0),
    "rear": ((0, -1, 0), "ORTHO", 0),
    "q_fl": ((-1.0, 1.25, 0.42), "PERSP", 60),
    "q_fr": ((1.0, 1.25, 0.42), "PERSP", 60),
    "q_rr": ((0.95, -1.2, 0.5), "PERSP", 60),
    "q_rl": ((-0.95, -1.2, 0.5), "PERSP", 60),
    "low_fl": ((-1.0, 1.0, 0.12), "PERSP", 45),
    "close_front": ((-0.35, 1.0, 0.25), "CLOSE_F", 85),
    "close_rear": ((0.35, -1.0, 0.35), "CLOSE_R", 85),
    "wheel": ((-1.0, 0.25, 0.08), "WHEEL", 85),
    "interior": ((-0.9, 0.15, 0.9), "INT", 35),
}
for n in names:
    d, kind, lens = V[n]
    d = Vector(d).normalized()
    target = ctr.copy()
    dist = size * 1.9
    if kind == "CLOSE_F":
        target = Vector((0, mx.y - 0.5, 0.55)); dist = 3.2
    elif kind == "CLOSE_R":
        target = Vector((0, mn.y + 0.5, 0.75)); dist = 3.2
    elif kind == "WHEEL":
        target = Vector((mn.x + 0.1, 1.225, 0.36)); dist = 1.9
    elif kind == "INT":
        target = Vector((0.0, 0.2, 0.75)); dist = 4.0
    cam.data.type = "ORTHO" if kind == "ORTHO" else "PERSP"
    cam.data.ortho_scale = size * 1.04
    cam.data.lens = lens or 50
    cam.data.clip_end = 100
    cam.location = target + d * dist
    up = "Y" if n == "top" else "Z"
    if n == "top":
        cam.rotation_euler = (0, 0, -math.pi / 2)
    else:
        cam.rotation_euler = (target - cam.location).to_track_quat("-Z", "Y").to_euler()
    sc.render.filepath = f"{out}_{n}.png"
    bpy.ops.render.render(write_still=True)
print("BBOX", tuple(round(v, 3) for v in mn), tuple(round(v, 3) for v in mx))
