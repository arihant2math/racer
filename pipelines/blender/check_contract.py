"""Checks a car GLB against the asset contract (node, socket and material names,
UVs, triangle budget, frame) by importing it into Blender.

usage: blender -b --factory-startup --python pipelines/blender/check_contract.py -- \
           assets/cars/<id>/car.glb [--car data/cars/<id>]

Prints one line per check and exits non-zero on failure. This is the pipeline's
own smoke test; the independent QA checker lives on its own branch.
"""

import os
import sys
import tomllib

import bpy
import numpy as np

PARTS = ["body", "hood", "bumper_front", "bumper_rear", "door_l", "door_r", "mirror_l", "mirror_r",
         "spoiler", "glass", "lights_front", "lights_rear", "interior",
         "wheel_fl", "wheel_fr", "wheel_rl", "wheel_rr", "collision"]
SOCKETS = ["socket_wheel_fl", "socket_wheel_fr", "socket_wheel_rl", "socket_wheel_rr",
           "socket_exhaust_l", "socket_exhaust_r", "socket_camera_hood", "socket_camera_cockpit"]
MATERIALS = ["car_paint", "glass", "rubber", "rim", "chrome", "light_emissive_front",
             "light_emissive_rear", "trim_black", "interior"]


def main():
    argv = sys.argv[sys.argv.index("--") + 1:]
    glb = argv[0]
    car_dir = argv[argv.index("--car") + 1] if "--car" in argv else None
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=glb)
    obs = {o.name: o for o in bpy.context.scene.objects}
    fails = []

    def check(ok, msg):
        print(("ok   " if ok else "FAIL ") + msg)
        if not ok:
            fails.append(msg)

    for n in PARTS:
        check(n in obs and obs[n].type == "MESH", f"mesh node {n}")
    for n in SOCKETS:
        check(n in obs and obs[n].type == "EMPTY", f"socket {n}")
    check(any(n.startswith("socket_light_") for n in obs), "socket_light_*")
    used = {m.name for o in obs.values() if o.type == "MESH" for m in o.data.materials if m}
    for m in MATERIALS:
        check(m in used, f"material {m}")
    tris = 0
    for o in obs.values():
        if o.type != "MESH" or o.name == "collision":
            continue
        check(len(o.data.uv_layers) > 0, f"UVs on {o.name}")
        tris += sum(len(p.vertices) - 2 for p in o.data.polygons)
    check(150_000 <= tris <= 500_000, f"LOD0 triangles {tris} in 150k..500k")
    pts = []
    for n in ("body", "bumper_front", "bumper_rear", "hood", "glass"):
        o = obs.get(n)
        if o:
            pts += [o.matrix_world @ v.co for v in o.data.vertices]
    a = np.array(pts)
    lo, hi = a.min(0), a.max(0)
    # Blender import is Z-up with the car facing +Y (glTF -Z).
    print(f"     extents x {lo[0]:.3f}..{hi[0]:.3f}  y {lo[1]:.3f}..{hi[1]:.3f}  z {lo[2]:.3f}..{hi[2]:.3f}")
    check(abs(lo[0] + hi[0]) < 0.005, "centred laterally")
    check(lo[2] > 0.0, "body above the ground plane")
    if car_dir:
        with open(os.path.join(car_dir, "car.toml"), "rb") as f:
            car = tomllib.load(f)
        ch = car["chassis"]
        check(abs((hi[1] - lo[1]) - ch["length_m"]) < 0.01, f"length {hi[1] - lo[1]:.3f} vs {ch['length_m']}")
        check(abs((hi[0] - lo[0]) - ch["width_m"]) < 0.01, f"width {hi[0] - lo[0]:.3f} vs {ch['width_m']}")
        zmax = max(float(np.max(np.array([o.matrix_world @ v.co for v in o.data.vertices])[:, 2]))
                   for o in obs.values() if o.type == "MESH" and o.name != "collision")
        check(abs(zmax - ch["height_m"]) < 0.01, f"height {zmax:.3f} vs {ch['height_m']}")
        fl, rl = obs["wheel_fl"].location, obs["wheel_rl"].location
        check(abs((fl.y - rl.y) - ch["wheelbase_m"]) < 0.002, f"wheelbase {fl.y - rl.y:.3f}")
        check(abs(fl.y + rl.y) < 0.002, "origin at the wheelbase midpoint")
        fr, rr = obs["wheel_fr"].location, obs["wheel_rr"].location
        check(abs((fr.x - fl.x) - ch["track_front_m"]) < 0.002, f"front track {fr.x - fl.x:.3f}")
        check(abs((rr.x - rl.x) - ch["track_rear_m"]) < 0.002, f"rear track {rr.x - rl.x:.3f}")
        check(rl.z > fl.z, f"staggered wheel radii {fl.z:.4f} / {rl.z:.4f}")
    if fails:
        print(f"{len(fails)} check(s) failed")
        sys.exit(1)
    print("all checks passed")


if __name__ == "__main__":
    main()
