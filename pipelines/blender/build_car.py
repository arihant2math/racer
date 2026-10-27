"""Builds a game-ready car GLB from data/cars/<id>/{car.toml,asset.toml}.

usage: blender -b --factory-startup --python pipelines/blender/build_car.py -- \
           --car data/cars/porsche-911-carrera-s-992 \
           --out assets/cars/porsche-911-carrera-s-992 [--blend out.blend]

Writes car.glb (LOD0), car_lod1.glb, car_lod2.glb and stats.json into --out.
Contract (docs/assets/<id>.md): metres, +Y up, car facing -Z, origin on the
ground at the wheelbase midpoint; named part nodes, sockets and materials.
"""

import argparse
import json
import math
import os
import sys
import time
import tomllib

import bpy

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from carlib import bodymesh, details, meshkit, parts, profile  # noqa: E402

MATERIALS = ["car_paint", "glass", "rubber", "rim", "chrome", "light_emissive_front",
             "light_emissive_rear", "trim_black", "interior", "brake_caliper"]


def make_materials(cfg):
    pc = cfg.get("paint", {}).get("color", [0.75, 0.75, 0.73])
    defs = {
        "car_paint": dict(color=pc, metallic=0.6, roughness=0.35, coat=1.0),
        "glass": dict(color=(0.02, 0.025, 0.03), metallic=0.0, roughness=0.05, alpha=0.35),
        "rubber": dict(color=(0.025, 0.025, 0.025), roughness=0.85),
        "rim": dict(color=(0.42, 0.43, 0.45), metallic=1.0, roughness=0.28),
        "chrome": dict(color=(0.9, 0.9, 0.92), metallic=1.0, roughness=0.08),
        "light_emissive_front": dict(color=(0.9, 0.92, 1.0), roughness=0.1,
                                     emission=(0.9, 0.95, 1.0), emission_strength=4.0),
        "light_emissive_rear": dict(color=(0.45, 0.0, 0.0), roughness=0.15,
                                    emission=(1.0, 0.02, 0.01), emission_strength=3.0),
        "trim_black": dict(color=(0.015, 0.015, 0.016), roughness=0.45),
        "interior": dict(color=(0.05, 0.045, 0.04), roughness=0.7),
        "brake_caliper": dict(color=(0.55, 0.02, 0.02), roughness=0.35),
    }
    mats = {n: meshkit.material(n, **defs[n]) for n in MATERIALS}
    return mats


def split_by_part(shell, names):
    """Separates the shell into one object per part id (face attribute 'part')."""
    import bmesh
    out = {}
    for pid, name in enumerate(names):
        bm = bmesh.new()
        bm.from_mesh(shell.data)
        layer = bm.faces.layers.int.get("part")
        kill = [f for f in bm.faces if f[layer] != pid]
        bmesh.ops.delete(bm, geom=kill, context="FACES")
        if not bm.faces:
            bm.free()
            continue
        ob = meshkit.bm_to_object(name, bm, shell.data.materials)
        bm.free()
        out[name] = ob
    return out


def main():
    argv = sys.argv[sys.argv.index("--") + 1:] if "--" in sys.argv else []
    ap = argparse.ArgumentParser()
    ap.add_argument("--car", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--blend")
    ap.add_argument("--no-lods", action="store_true")
    args = ap.parse_args(argv)
    t0 = time.time()
    with open(os.path.join(args.car, "car.toml"), "rb") as f:
        car = tomllib.load(f)
    with open(os.path.join(args.car, "asset.toml"), "rb") as f:
        cfg = tomllib.load(f)
    ch = car["chassis"]
    meshkit.clear_scene()
    mats = make_materials(cfg)
    mat_list = [mats[n] for n in MATERIALS]
    mat_idx = {n: i for i, n in enumerate(MATERIALS)}
    body = profile.load(cfg, ch, os.path.join(args.car, "refs/blueprint/profiles.json"))
    front, rear = body.arches[0]["s"], body.arches[1]["s"]
    if abs((rear - front) - ch["wheelbase_m"]) > 1e-6:
        raise SystemExit(f"arch spacing {rear - front} != wheelbase {ch['wheelbase_m']}")
    s_mid = (front + rear) / 2
    geo = parts.Frame(body, s_mid, ch, car["tires"], cfg)

    shell = bodymesh.build(body, cfg, s_mid, mat_idx, mat_list)
    print(f"[build] shell {meshkit.tri_count(shell)} tris, {time.time() - t0:.1f}s", flush=True)
    details.cut_shell(shell, geo, cfg, mats, mat_idx)
    print(f"[build] cuts done {time.time() - t0:.1f}s", flush=True)
    objs = split_by_part(shell, bodymesh.PARTS)
    bpy.data.objects.remove(shell)
    details.panel_gaps(objs, cfg)
    objs.update(details.add_details(geo, cfg, mats, mat_idx, objs))
    objs.update(parts.wheels(geo, mats))
    objs["interior"] = parts.interior(geo, mats)
    objs.update(parts.mirrors(geo, mats))
    parts.finish(objs, geo, cfg, mats)
    sockets = parts.sockets(geo, cfg)
    collision = parts.collision(objs, geo)
    print(f"[build] parts done {time.time() - t0:.1f}s", flush=True)

    os.makedirs(args.out, exist_ok=True)
    stats = parts.export_all(objs, sockets, collision, args.out, lods=not args.no_lods)
    stats["build_seconds"] = round(time.time() - t0, 1)
    stats["dimensions"] = parts.measure(objs)
    with open(os.path.join(args.out, "stats.json"), "w") as f:
        json.dump(stats, f, indent=2, sort_keys=True)
    if args.blend:
        bpy.ops.wm.save_as_mainfile(filepath=os.path.abspath(args.blend))
    print(json.dumps(stats, indent=2), flush=True)


if __name__ == "__main__":
    main()
