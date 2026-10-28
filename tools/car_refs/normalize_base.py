"""Write the chosen image-to-3D candidate as the car's base mesh, in metres.

    python tools/car_refs/normalize_base.py <car_dir> <scores.json> <candidate_dir> [file]

Takes the best candidate in <scores.json> (from score_candidates.py), or
[file] if given, applies its `to_car_frame` alignment (rigid + uniform scale,
no warping), drops the generator's base plate if it had one, and writes:

- <car_dir>/refs/base_mesh/base.glb: the game's glTF frame (+Y up, -Z
  forward, +X right), metres, ground at y = 0, z = 0 at the length midpoint.
  Imported into Blender this is the build frame (X right, Y forward, Z up).
  Textures (TRELLIS.2 PBR) are kept.
- <car_dir>/refs/base_mesh/base.json: provenance (generator, version,
  settings, seed, input photo) and the blueprint IoU scores.
"""
import json
import subprocess
import sys
from pathlib import Path

import numpy as np
import trimesh


def main(car_dir, scores, cand_dir, pick=None):
    car = Path(car_dir)
    S = json.loads(Path(scores).read_text())
    best = next(s for s in S if s["file"] == pick) if pick else S[0]
    src = Path(cand_dir) / best["file"]
    meta = json.loads(src.with_suffix(".json").read_text()) if src.with_suffix(".json").exists() else {}
    M = np.array(best["to_car_frame"])
    C = np.eye(4)
    C[:3, :3] = [[0, -1, 0], [0, 0, 1], [-1, 0, 0]]  # car (x fwd, y left, z up) -> glTF (x right, y up, -z fwd)
    T = C @ M
    scene = trimesh.load(src, force="scene")
    geoms = []
    for name in scene.graph.nodes_geometry:
        tf, gname = scene.graph[name]
        g = scene.geometry[gname].copy()
        g.apply_transform(T @ tf)
        if best.get("plate_z") is not None:
            # glTF y is car z: drop faces entirely at or below the plate.
            keep = (g.vertices[g.faces][:, :, 1] > best["plate_z"]).any(1)
            g.update_faces(keep)
            g.remove_unreferenced_vertices()
        geoms.append(g)
    out = car / "refs/base_mesh"
    out.mkdir(parents=True, exist_ok=True)
    trimesh.Scene(geoms).export(out / "base.glb")
    rev = lambda d: subprocess.run(["git", "-C", str(d), "rev-parse", "HEAD"], capture_output=True, text=True).stdout.strip()
    info = {
        **meta,
        "candidate": best["file"],
        "input_photo": f"refs/photos/{meta.get('input', '?')}",
        "aligned": "rigid + uniform scale to car.toml length_m (score_candidates.py), no warp",
        "frame": "glTF +X right, +Y up, -Z forward, metres, ground y=0, z=0 at the length midpoint",
        "blueprint_iou": {k: best[k] for k in ("side", "plan", "front", "rear", "symmetry", "total")},
        "faces": int(sum(len(g.faces) for g in geoms)),
        "selection": "best mean silhouette IoU among the candidates in candidates.json",
    }
    (out / "base.json").write_text(json.dumps(info, indent=1) + "\n")
    print("wrote", out / "base.glb", info["faces"], "faces", info["blueprint_iou"])


if __name__ == "__main__":
    main(*sys.argv[1:])
