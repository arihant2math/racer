"""Renders a car GLB: orthographic views, beauty shots and silhouette masks.

usage: blender -b --factory-startup --python pipelines/blender/render_views.py -- \
           --glb assets/cars/<id>/car.glb --out docs/assets/<id> [--masks DIR] [--size 1280]
           [--only name,name] [--samples 32]

Renders with Eevee from the exported GLB (what the game loads), lit by a CC0
studio HDRI from Poly Haven (downloaded once into .cache/hdri, falls back to a
plain sky). --masks writes black/white orthographic silhouettes at 5 mm/px
for pipelines/refs/silhouette_iou.py.
"""

import argparse
import math
import os
import sys
import urllib.request

import bpy
from mathutils import Vector

HDRI = {
    "studio": "https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/1k/studio_small_09_1k.hdr",
    "sky": "https://dl.polyhaven.org/file/ph-assets/HDRIs/hdr/1k/kloofendal_48d_partly_cloudy_puresky_1k.hdr",
}
MASK_PX = 0.005  # metres per pixel


def hdri(name):
    root = os.path.join(os.path.dirname(__file__), "..", "..", ".cache", "hdri")
    os.makedirs(root, exist_ok=True)
    path = os.path.join(root, os.path.basename(HDRI[name]))
    if not os.path.exists(path):
        try:
            urllib.request.urlretrieve(HDRI[name], path)
        except Exception as e:  # offline: plain sky
            print("hdri download failed:", e)
            return None
    return path


def world(kind, strength=1.0, rotation=0.0):
    w = bpy.data.worlds.new("world")
    bpy.context.scene.world = w
    w.use_nodes = True
    nt = w.node_tree
    bg = nt.nodes["Background"]
    bg.inputs["Strength"].default_value = strength
    path = hdri(kind) if kind else None
    if path:
        env = nt.nodes.new("ShaderNodeTexEnvironment")
        env.image = bpy.data.images.load(path)
        mapping = nt.nodes.new("ShaderNodeMapping")
        coord = nt.nodes.new("ShaderNodeTexCoord")
        mapping.inputs["Rotation"].default_value[2] = rotation
        nt.links.new(coord.outputs["Generated"], mapping.inputs["Vector"])
        nt.links.new(mapping.outputs["Vector"], env.inputs["Vector"])
        nt.links.new(env.outputs["Color"], bg.inputs["Color"])
    else:
        bg.inputs["Color"].default_value = (0.6, 0.65, 0.75, 1)


def look_at(ob, target, up=(0, 0, 1)):
    d = Vector(target) - ob.location
    ob.rotation_euler = d.to_track_quat("-Z", "Y" if abs(Vector(up).z) > 0.5 else "X").to_euler()


def camera(name, loc, target, lens=50, ortho=None, up=(0, 0, 1)):
    cam = bpy.data.cameras.new(name)
    ob = bpy.data.objects.new(name, cam)
    bpy.context.scene.collection.objects.link(ob)
    ob.location = loc
    if ortho:
        cam.type = "ORTHO"
        cam.ortho_scale = ortho
    else:
        cam.lens = lens
    cam.clip_end = 200
    d = Vector(target) - Vector(loc)
    if abs(d.normalized().z) > 0.99:
        ob.rotation_euler = d.to_track_quat("-Z", "X").to_euler()
        ob.rotation_euler.rotate_axis("Z", math.radians(90 if up[0] >= 0 else -90))
    else:
        ob.rotation_euler = d.to_track_quat("-Z", "Y").to_euler()
    return ob


def ground(size=60, shadow_only=False):
    bpy.ops.mesh.primitive_plane_add(size=size, location=(0, 0, 0))
    g = bpy.context.active_object
    g.name = "ground"
    m = bpy.data.materials.new("ground")
    m.use_nodes = True
    b = m.node_tree.nodes["Principled BSDF"]
    b.inputs["Base Color"].default_value = (0.32, 0.32, 0.33, 1)
    b.inputs["Roughness"].default_value = 0.6
    g.data.materials.append(m)
    return g


def import_glb(path):
    bpy.ops.import_scene.gltf(filepath=path)
    for ob in list(bpy.context.scene.objects):
        if ob.name.startswith("collision"):
            ob.hide_render = True
        if ob.type == "EMPTY":
            ob.hide_render = True


def render(path, w, h):
    sc = bpy.context.scene
    sc.render.resolution_x = w
    sc.render.resolution_y = h
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True)
    print("wrote", path, flush=True)


def setup_eevee(samples):
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_EEVEE"
    ee = sc.eevee
    ee.taa_render_samples = samples
    for attr, val in (("use_raytracing", True), ("use_shadows", True), ("use_gtao", True),
                      ("use_ssr", True)):
        if hasattr(ee, attr):
            setattr(ee, attr, val)
    sc.view_settings.view_transform = "AgX" if "AgX" in [i.identifier for i in
                                                          sc.view_settings.bl_rna.properties["view_transform"].enum_items] else "Filmic"
    sc.view_settings.look = "None"
    sc.render.film_transparent = False


def masks(out, glb):
    """Silhouettes: workbench, flat white on black, orthographic, 5 mm/px."""
    sc = bpy.context.scene
    sc.render.engine = "BLENDER_WORKBENCH"
    sh = sc.display.shading
    sh.light = "FLAT"
    sh.color_type = "SINGLE"
    sh.single_color = (1, 1, 1)
    sh.background_type = "VIEWPORT"
    sh.background_color = (0, 0, 0)
    sc.display_settings.display_device = "sRGB"
    sc.view_settings.view_transform = "Standard"
    os.makedirs(out, exist_ok=True)
    # name: (camera location, target, image w x h in metres, up)
    W, H = 5.0, 1.5
    views = {
        "side": ((-20, 0, H / 2), (0, 0, H / 2), (W, H), (0, 0, 1)),
        "top": ((0, 0, 20), (0, 0, 0), (W, 2.5), (1, 0, 0)),
        "front": ((0, 20, H / 2), (0, 0, H / 2), (2.5, H), (0, 0, 1)),
        "rear": ((0, -20, H / 2), (0, 0, H / 2), (2.5, H), (0, 0, 1)),
    }
    for name, (loc, tgt, (w, h), up) in views.items():
        cam = camera(f"mask_{name}", loc, tgt, ortho=max(w, h), up=up)
        sc.camera = cam
        render(os.path.join(out, f"model_{name}.png"), int(round(w / MASK_PX)), int(round(h / MASK_PX)))


def beauty(out, size, samples, only):
    setup_eevee(samples)
    ground()
    os.makedirs(out, exist_ok=True)
    sc = bpy.context.scene
    W, H = 5.0, 1.5
    shots = {
        # orthographic reference views (match the blueprint layout)
        "ortho_side": dict(loc=(-20, 0, 0.65), tgt=(0, 0, 0.65), ortho=5.0, aspect=(1, 0.36), hdr="studio"),
        "ortho_front": dict(loc=(0, 20, 0.7), tgt=(0, 0, 0.7), ortho=2.3, aspect=(1, 0.66), hdr="studio"),
        "ortho_rear": dict(loc=(0, -20, 0.7), tgt=(0, 0, 0.7), ortho=2.3, aspect=(1, 0.66), hdr="studio"),
        "ortho_top": dict(loc=(0, 0, 20), tgt=(0, 0, 0), ortho=5.0, aspect=(1, 0.5), hdr="studio", up=(1, 0, 0)),
        # beauty
        "front_three_quarter": dict(loc=(4.6, 5.4, 1.35), tgt=(0, 0.2, 0.55), lens=55, aspect=(1, 0.5625), hdr="sky"),
        "rear_three_quarter": dict(loc=(-4.4, -5.6, 1.6), tgt=(0, -0.3, 0.6), lens=55, aspect=(1, 0.5625), hdr="sky"),
        "front_low": dict(loc=(1.2, 6.5, 0.55), tgt=(0, 0, 0.55), lens=50, aspect=(1, 0.5625), hdr="studio"),
        "rear_high": dict(loc=(2.2, -6.0, 2.6), tgt=(0, -0.4, 0.6), lens=50, aspect=(1, 0.5625), hdr="studio"),
        "side_profile": dict(loc=(7.5, 0.0, 0.9), tgt=(0, 0, 0.6), lens=50, aspect=(1, 0.5625), hdr="sky"),
        "detail_headlight": dict(loc=(1.6, 3.6, 0.95), tgt=(0.7, 2.0, 0.6), lens=50, aspect=(1, 0.5625), hdr="studio"),
        "detail_rear": dict(loc=(-1.4, -4.4, 1.25), tgt=(-0.2, -2.1, 0.75), lens=45, aspect=(1, 0.5625), hdr="studio"),
        "detail_wheel": dict(loc=(2.6, -1.2, 0.5), tgt=(0.8, -1.2, 0.36), lens=40, aspect=(1, 0.5625), hdr="studio"),
    }
    meta = {}
    for name, s in shots.items():
        if s.get("ortho"):
            # metres per pixel and the world point at the image centre, for
            # pipelines/refs/compare_blueprint.py
            meta[name] = {"m_per_px": s["ortho"] / size, "center": list(s["tgt"]),
                          "size": [size, int(size * s["aspect"][1])]}
        if only and name not in only:
            continue
        world(s["hdr"], 1.0 if s["hdr"] == "sky" else 1.2, rotation=math.radians(60))
        cam = camera(name, s["loc"], s["tgt"], lens=s.get("lens", 50), ortho=s.get("ortho"), up=s.get("up", (0, 0, 1)))
        sc.camera = cam
        render(os.path.join(out, f"{name}.png"), size, int(size * s["aspect"][1]))
    import json
    with open(os.path.join(out, "ortho.json"), "w") as f:
        json.dump(meta, f, indent=1)


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser()
    ap.add_argument("--glb", required=True)
    ap.add_argument("--out")
    ap.add_argument("--masks")
    ap.add_argument("--size", type=int, default=1280)
    ap.add_argument("--samples", type=int, default=32)
    ap.add_argument("--only", default="")
    args = ap.parse_args(argv)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    import_glb(args.glb)
    if args.masks:
        masks(args.masks, args.glb)
    if args.out:
        beauty(args.out, args.size, args.samples, [s for s in args.only.split(",") if s])


if __name__ == "__main__":
    main()
