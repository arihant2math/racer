"""Beauty renders of a car GLB for the asset QA review pack (gate 5).

Run headless:

    blender -b --factory-startup --python pipelines/blender/qa/review_render.py -- \
        <car.glb> <out-dir> [--size 1280x720] [--samples 48]

Imports the GLB, hides LOD1+ and collision objects, lights the car in a neutral
studio (grey ground, sun + soft fill), renders the fixed cameras below with
Cycles on the CPU and writes <out-dir>/<camera>.png plus review.json (camera
list, scene statistics as Blender sees them).

Blender's glTF importer converts +Y-up glTF to +Z-up: the car's front (-Z in
glTF) points to +Y, its right side (+X) stays +X.
"""

import json
import math
import re
import sys
import time

import bpy
from mathutils import Vector

argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
if len(argv) < 2:
    sys.exit("usage: review_render.py -- <car.glb> <out-dir> [--size WxH] [--samples N]")
glb, out_dir = argv[0], argv[1]
size, samples = (1280, 720), 48
rest = argv[2:]
while rest:
    flag, value = rest[0], rest[1]
    rest = rest[2:]
    if flag == "--size":
        size = tuple(int(v) for v in value.split("x"))
    elif flag == "--samples":
        samples = int(value)
    else:
        sys.exit(f"unknown flag {flag}")

bpy.ops.wm.read_factory_settings(use_empty=True)
t0 = time.time()
bpy.ops.import_scene.gltf(filepath=glb)
import_seconds = time.time() - t0
scene = bpy.context.scene


def chain(obj):
    while obj is not None:
        yield obj
        obj = obj.parent


def lod_of(obj):
    for o in chain(obj):
        m = re.search(r"(?:^|[^a-z0-9])lod(\d+)(?:$|[^a-z0-9])", o.name.lower())
        if m:
            return int(m.group(1))
    return 0


def is_collision(obj):
    return any(o.name.lower().startswith("collision") for o in chain(obj))


def base(name):
    return re.sub(r"[_.\-]lod\d+$", "", name, flags=re.I)


stats = {"objects": 0, "lod0_triangles": 0, "per_lod_triangles": {}, "hidden": []}
visible = []
for obj in scene.objects:
    if obj.type != "MESH":
        continue
    stats["objects"] += 1
    tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
    lod = lod_of(obj)
    if is_collision(obj):
        obj.hide_render = True
        stats["hidden"].append(obj.name)
        continue
    stats["per_lod_triangles"][str(lod)] = stats["per_lod_triangles"].get(str(lod), 0) + tris
    if lod != 0:
        obj.hide_render = True
        continue
    stats["lod0_triangles"] += tris
    visible.append(obj)

if not visible:
    sys.exit("no LOD0 mesh objects to render")

lo = Vector((math.inf,) * 3)
hi = Vector((-math.inf,) * 3)
for obj in visible:
    for c in obj.bound_box:
        w = obj.matrix_world @ Vector(c)
        lo = Vector(map(min, lo, w))
        hi = Vector(map(max, hi, w))
centre = (lo + hi) / 2
extent = hi - lo
radius = extent.length / 2
stats["bounds_blender"] = {"min": list(lo), "max": list(hi)}


def named_centre(name):
    for obj in scene.objects:
        if base(obj.name) == name and lod_of(obj) == 0 and not is_collision(obj):
            objs = [obj] + [c for c in obj.children_recursive if c.type == "MESH"]
            pts = [o.matrix_world @ Vector(c) for o in objs if o.type == "MESH" for c in o.bound_box]
            if not pts:
                return obj.matrix_world.translation.copy()
            a = Vector([min(p[i] for p in pts) for i in range(3)])
            b = Vector([max(p[i] for p in pts) for i in range(3)])
            return (a + b) / 2
    return None


# Studio: neutral grey world, ground plane, sun and a large soft fill.
world = bpy.data.worlds.new("studio")
world.use_nodes = True
bg = world.node_tree.nodes["Background"]
bg.inputs["Color"].default_value = (0.62, 0.66, 0.72, 1.0)
bg.inputs["Strength"].default_value = 0.9
scene.world = world

bpy.ops.mesh.primitive_plane_add(size=60, location=(centre.x, centre.y, lo.z))
ground = bpy.context.object
mat = bpy.data.materials.new("qa_ground")
mat.use_nodes = True
bsdf = mat.node_tree.nodes["Principled BSDF"]
bsdf.inputs["Base Color"].default_value = (0.42, 0.42, 0.42, 1.0)
bsdf.inputs["Roughness"].default_value = 0.65
ground.data.materials.append(mat)

sun_data = bpy.data.lights.new("qa_sun", "SUN")
sun_data.energy = 3.5
sun_data.angle = math.radians(8)
sun = bpy.data.objects.new("qa_sun", sun_data)
sun.rotation_euler = (math.radians(50), 0, math.radians(-140))
scene.collection.objects.link(sun)

fill_data = bpy.data.lights.new("qa_fill", "AREA")
fill_data.energy = 1500
fill_data.size = 8
fill = bpy.data.objects.new("qa_fill", fill_data)
fill.location = centre + Vector((4, -3, 6))
fill.rotation_euler = (centre - fill.location).to_track_quat("-Z", "Y").to_euler()
scene.collection.objects.link(fill)

scene.render.engine = "CYCLES"
scene.cycles.device = "CPU"
scene.cycles.samples = samples
scene.cycles.use_denoising = True
try:
    scene.cycles.denoiser = "OPENIMAGEDENOISE"
except TypeError:
    pass
scene.render.resolution_x, scene.render.resolution_y = size
scene.render.film_transparent = False
scene.render.image_settings.file_format = "PNG"

cam_data = bpy.data.cameras.new("qa_cam")
cam = bpy.data.objects.new("qa_cam", cam_data)
scene.collection.objects.link(cam)
scene.camera = cam
aspect = size[0] / size[1]


def perspective(direction, target, lens=50.0, fit=radius, margin=1.15):
    cam_data.type = "PERSP"
    cam_data.lens = lens
    fov = 2 * math.atan(cam_data.sensor_width / 2 / lens)
    vfov = 2 * math.atan(math.tan(fov / 2) / aspect)
    dist = fit * margin / math.sin(min(fov, vfov) / 2)
    d = Vector(direction).normalized()
    cam.location = target + d * dist
    cam.rotation_euler = (-d).to_track_quat("-Z", "Y").to_euler()


def ortho(direction, width, height, rotation=None):
    cam_data.type = "ORTHO"
    cam_data.ortho_scale = max(width, height * aspect) * 1.08
    d = Vector(direction).normalized()
    cam.location = centre + d * (radius * 4)
    cam.rotation_euler = rotation if rotation else (-d).to_track_quat("-Z", "Z" if abs(d.z) < 0.9 else "Y").to_euler()
    cam_data.clip_end = radius * 10


L, W, H = extent.y, extent.x, extent.z
cameras = {
    "front-three-quarter": lambda: perspective((-1.0, 1.35, 0.45), centre),
    "rear-three-quarter": lambda: perspective((-1.0, -1.35, 0.5), centre),
    "side": lambda: ortho((-1, 0, 0), L, H),
    "front": lambda: ortho((0, 1, 0), W, H),
    "rear": lambda: ortho((0, -1, 0), W, H),
    "top": lambda: ortho((0, 0, 1), L, W, rotation=(0, 0, -math.pi / 2)),
    "front-low": lambda: perspective((-0.35, 1.0, 0.12), centre, lens=35.0),
}
for name, part, direction, fit in [
    ("wheel-front-left", "wheel_fl", (-1.0, 0.25, 0.12), 0.55),
    ("wheel-rear-left", "wheel_rl", (-1.0, -0.25, 0.12), 0.55),
    ("lights-front", "lights_front", (-0.55, 1.0, 0.3), None),
    ("lights-rear", "lights_rear", (0.45, -1.0, 0.35), None),
    ("interior", "interior", (-1.0, 0.1, 0.45), None),
]:
    c = named_centre(part)
    if c is None:
        continue
    f = fit if fit else 0.6 * max(W, 1.0)
    cameras[name] = (lambda c=c, d=direction, f=f: perspective(d, c, lens=50.0, fit=f))

rendered = {}
for name, setup in cameras.items():
    setup()
    scene.render.filepath = f"{out_dir}/{name}.png"
    t = time.time()
    bpy.ops.render.render(write_still=True)
    rendered[name] = round(time.time() - t, 1)
    print(f"review_render: {name} in {rendered[name]} s", flush=True)

stats["import_seconds"] = round(import_seconds, 1)
stats["render_seconds"] = rendered
stats["blender"] = bpy.app.version_string
with open(f"{out_dir}/review.json", "w") as f:
    json.dump(stats, f, indent=2)
print("review_render: done", flush=True)
