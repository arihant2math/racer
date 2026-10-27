"""Headless Blender review of the racer-citykit asset kit.

Imports every `<asset>_lod0.obj` written by `racer-citykit obj <dir>`, lays
the assets out on a grid with name labels, renders a Cycles (CPU) contact
sheet and writes an independent mesh report (triangles, dimensions,
non-manifold edges after welding) as JSON. Run through tools/kit_review.sh:

    blender -b --factory-startup --python pipelines/blender/kit_review.py -- \
        <obj-dir> <out-png> <out-json> [--samples N]
"""

import json
import math
import sys
from pathlib import Path

import bmesh
import bpy


def args():
    argv = sys.argv[sys.argv.index("--") + 1 :] if "--" in sys.argv else []
    if len(argv) < 3:
        raise SystemExit("usage: kit_review.py -- <obj-dir> <out-png> <out-json> [--samples N]")
    samples = 32
    if "--samples" in argv:
        samples = int(argv[argv.index("--samples") + 1])
    return Path(argv[0]), Path(argv[1]), Path(argv[2]), samples


def reset_scene():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    scene = bpy.context.scene
    scene.render.engine = "CYCLES"
    scene.cycles.device = "CPU"
    return scene


def import_obj(path):
    before = set(bpy.data.objects)
    # OBJ files are Y-up like Bevy.
    bpy.ops.wm.obj_import(filepath=str(path), forward_axis="NEGATIVE_Z", up_axis="Y")
    return [o for o in bpy.data.objects if o not in before]


def mesh_report(obj):
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-4)
    bm.edges.ensure_lookup_table()
    boundary = sum(1 for e in bm.edges if len(e.link_faces) == 1)
    non_manifold = sum(1 for e in bm.edges if len(e.link_faces) > 2)
    tris = sum(len(f.verts) - 2 for f in bm.faces)
    bm.free()
    d = obj.dimensions
    return {
        "triangles": tris,
        "size_m": [round(d.x, 3), round(d.y, 3), round(d.z, 3)],
        "boundary_edges": boundary,
        "non_manifold_edges": non_manifold,
        "materials": sorted(m.name for m in obj.data.materials if m),
    }


def label(text, location, size):
    curve = bpy.data.curves.new(name=f"label_{text}", type="FONT")
    curve.body = text
    curve.size = size
    curve.align_x = "CENTER"
    obj = bpy.data.objects.new(curve.name, curve)
    obj.location = location
    bpy.context.collection.objects.link(obj)
    mat = bpy.data.materials.get("label") or bpy.data.materials.new("label")
    mat.diffuse_color = (0.02, 0.02, 0.02, 1.0)
    obj.data.materials.append(mat)
    return obj


def main():
    obj_dir, out_png, out_json, samples = args()
    scene = reset_scene()
    files = sorted(obj_dir.glob("*_lod0.obj"))
    if not files:
        raise SystemExit(f"no *_lod0.obj in {obj_dir}")
    # Big assets (roads, buildings, grandstand) get their own row at the back.
    report = {}
    placed = []
    for f in files:
        objs = import_obj(f)
        if not objs:
            continue
        obj = objs[0]
        if len(objs) > 1:
            bpy.ops.object.select_all(action="DESELECT")
            for o in objs:
                o.select_set(True)
            bpy.context.view_layer.objects.active = obj
            bpy.ops.object.join()
        name = f.stem.removesuffix("_lod0")
        # Use the material colours from kit.mtl in the render.
        for m in obj.data.materials:
            if m and m.use_nodes:
                bsdf = m.node_tree.nodes.get("Principled BSDF")
                if bsdf:
                    bsdf.inputs["Base Color"].default_value = m.diffuse_color
        report[name] = mesh_report(obj)
        placed.append((name, obj))

    small = [(n, o) for n, o in placed if max(o.dimensions) < 12.0]
    large = [(n, o) for n, o in placed if max(o.dimensions) >= 12.0]
    cols = 8
    pitch = 7.0
    for i, (name, obj) in enumerate(small):
        r, c = divmod(i, cols)
        x, y = (c - cols / 2 + 0.5) * pitch, -r * pitch
        bb = [obj.matrix_world @ v.co for v in obj.data.vertices]
        minx = min(v.x for v in bb); maxx = max(v.x for v in bb)
        miny = min(v.y for v in bb); maxy = max(v.y for v in bb)
        minz = min(v.z for v in bb)
        obj.location.x += x - (minx + maxx) / 2
        obj.location.y += y - (miny + maxy) / 2
        obj.location.z -= minz
        label(name.replace("_", " "), (x, y - 3.0, 0.01), 0.45)
    rows = math.ceil(len(small) / cols)
    x = -cols / 2 * pitch
    for name, obj in large:
        bb = [obj.matrix_world @ v.co for v in obj.data.vertices]
        minx = min(v.x for v in bb); maxx = max(v.x for v in bb)
        miny = min(v.y for v in bb); maxy = max(v.y for v in bb)
        minz = min(v.z for v in bb)
        w = maxx - minx
        obj.location.x += x - minx
        obj.location.y += 25.0 - (miny + maxy) / 2
        obj.location.z -= minz
        label(name.replace("_", " "), (x + w / 2, 25.0 - (maxy - miny) / 2 - 4.0, 0.01), 1.6)
        x += w + 8.0

    # Ground, sun, sky, camera.
    bpy.ops.mesh.primitive_plane_add(size=600, location=(0, 0, 0))
    ground = bpy.context.object
    gmat = bpy.data.materials.new("ground")
    gmat.diffuse_color = (0.75, 0.75, 0.73, 1.0)
    gmat.use_nodes = True
    gmat.node_tree.nodes["Principled BSDF"].inputs["Base Color"].default_value = (0.75, 0.75, 0.73, 1.0)
    ground.data.materials.append(gmat)
    bpy.ops.object.light_add(type="SUN", rotation=(math.radians(50), 0, math.radians(30)))
    bpy.context.object.data.energy = 3.5
    world = bpy.data.worlds.new("sky")
    world.use_nodes = True
    world.node_tree.nodes["Background"].inputs[0].default_value = (0.75, 0.82, 0.95, 1.0)
    world.node_tree.nodes["Background"].inputs[1].default_value = 0.8
    scene.world = world
    span = max(cols * pitch, x + cols * pitch / 2)
    bpy.ops.object.camera_add(location=(0, -rows * pitch - 70, 95), rotation=(math.radians(52), 0, 0))
    cam = bpy.context.object
    cam.data.type = "ORTHO"
    cam.data.ortho_scale = span * 1.05
    cam.location.y = -rows * pitch / 2 - 60
    cam.location.x = (x - cols * pitch / 2) / 2 - 2
    scene.camera = cam
    scene.render.resolution_x, scene.render.resolution_y = 1920, 1080
    scene.cycles.samples = samples
    scene.cycles.use_denoising = False
    scene.render.filepath = str(out_png)
    bpy.ops.render.render(write_still=True)
    out_json.write_text(json.dumps(report, indent=2, sort_keys=True))
    print(f"kit-review: {len(report)} assets, contact sheet {out_png}, report {out_json}")


main()
