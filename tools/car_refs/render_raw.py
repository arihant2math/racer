"""Quick look at a raw image-to-3D GLB (any frame): 4 workbench views.

    blender -b --python tools/car_refs/render_raw.py -- <in.glb> <out_prefix> [textured]

Writes <out_prefix>_{q_fl,side,front,top}.png. The mesh is framed by its
bounding box; the longest horizontal extent is taken as the car length.
"""
import math
import sys

import bpy
from mathutils import Vector

argv = sys.argv[sys.argv.index("--") + 1:]
src, prefix = argv[0], argv[1]
textured = len(argv) > 2 and argv[2] == "textured"
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
obs = [o for o in bpy.context.scene.objects if o.type == "MESH"]
pts = [o.matrix_world @ Vector(c) for o in obs for c in o.bound_box]
lo = Vector((min(p.x for p in pts), min(p.y for p in pts), min(p.z for p in pts)))
hi = Vector((max(p.x for p in pts), max(p.y for p in pts), max(p.z for p in pts)))
c, size = (lo + hi) / 2, (hi - lo)
R = max(size) * 1.6
sc = bpy.context.scene
sc.render.engine = "BLENDER_WORKBENCH"
sc.display.shading.light = "STUDIO"
sc.display.shading.color_type = "TEXTURE" if textured else "SINGLE"
sc.display.shading.single_color = (0.75, 0.75, 0.78)
sc.display.shading.show_cavity = True
sc.render.resolution_x, sc.render.resolution_y = 1400, 900
cam = bpy.data.objects.new("cam", bpy.data.cameras.new("cam"))
sc.collection.objects.link(cam)
sc.camera = cam
cam.data.type = "ORTHO"
cam.data.ortho_scale = max(size) * 1.15
long_x = size.x >= size.y  # length axis in the Blender (Z-up) frame
views = {
    "q_fl": Vector((1, -1, 0.5)) if long_x else Vector((-1, 1, 0.5)),
    "side": Vector((0, -1, 0)) if long_x else Vector((-1, 0, 0)),
    "front": Vector((1, 0, 0)) if long_x else Vector((0, 1, 0)),
    "top": Vector((0, 0.001, 1)),
}
for name, d in views.items():
    d = d.normalized()
    cam.location = c + d * R
    cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()
    cam.data.type = "PERSP" if name == "q_fl" else "ORTHO"
    cam.data.lens = 50
    sc.render.filepath = f"{prefix}_{name}.png"
    bpy.ops.render.render(write_still=True)
