"""Sample an image-to-3D base mesh into normalized half cross-sections.

blender -b --python sample_ai.py -- <base.glb> <out.json>

The base mesh (Hunyuan3D-2.1, AI frame: +X front, +Z up, sits on a plate)
is cut into stations along its length. Each station is ray cast in the
lateral plane from a centre inside the section; hits give the outer half
contour from the top centre down to the lowest side point. Output curves
are normalized per station: s / half-width, (z - zmin) / (zmax - zmin),
so the body builder can rescale them to the blueprint silhouettes.
"""
import bpy, sys, json, math
import numpy as np
from mathutils import Vector
from mathutils.bvhtree import BVHTree

argv = sys.argv[sys.argv.index("--") + 1:]
src, out = argv[0], argv[1]
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=src)
o = [o for o in bpy.context.scene.objects if o.type == "MESH"][0]
V = np.array([o.matrix_world @ v.co for v in o.data.vertices])
bvh = BVHTree.FromPolygons([tuple(v) for v in V], [tuple(p.vertices) for p in o.data.polygons])
# The base plate extends past the car; its top is the highest vertex beyond the body ends.
xr = V[:, 0].max() - V[:, 0].min()
edge = V[np.abs(V[:, 0] - 0.5 * (V[:, 0].max() + V[:, 0].min())) > 0.48 * xr]
plate = float(edge[:, 2].max()) + 0.006
ground = float(V[:, 2].min())  # tire contact patches below the plate
# Tire bottoms poke through the plate: two clusters along X give the axles.
low = V[V[:, 2] < float(edge[:, 2].min()) - 0.005]
wheel_x = sorted({round(float(low[low[:, 0] < 0][:, 0].mean()), 5), round(float(low[low[:, 0] > 0][:, 0].mean()), 5)})
body = V[V[:, 2] > plate]
x0, x1 = float(body[:, 0].min()), float(body[:, 0].max())
NS, NA = 240, 360
stations = []
for xi in np.linspace(x0 + 0.002, x1 - 0.002, NS):
    slab = body[np.abs(body[:, 0] - xi) < 0.004]
    if len(slab) < 10:
        stations.append(None)
        continue
    zc = 0.5 * (slab[:, 2].min() + slab[:, 2].max())
    halves = []
    for side in (1.0, -1.0):
        pts = []
        for k in range(NA + 1):
            th = math.pi * k / NA  # 0 = up, pi = down
            d = Vector((0.0, side * math.sin(th), math.cos(th)))
            c = Vector((xi, 0.0, zc))
            hit = bvh.ray_cast(c + d * 2.0, -d, 2.0)
            if hit[0] is not None and hit[0].z > plate:
                pts.append((abs(hit[0].y), hit[0].z))
            else:
                pts.append(None)
        halves.append(pts)
    # Mirror-average the two halves (AI mesh is near symmetric).
    pts = []
    for a, b in zip(*halves):
        if a and b:
            pts.append(((a[0] + b[0]) / 2, (a[1] + b[1]) / 2))
        else:
            pts.append(a or b)
    stations.append({"x": float(xi), "pts": pts, "zmin": float(slab[:, 2].min()), "zmax": float(slab[:, 2].max())})
json.dump({"x0": x0, "x1": x1, "plate": plate, "ground": ground, "wheel_x": wheel_x, "stations": stations}, open(out, "w"))
print("wheels", wheel_x, "ground", ground)
print("sampled", sum(s is not None for s in stations), "stations")
