"""Side-by-side sheet of the orthographic renders and the blueprint views.

usage: python3 pipelines/refs/compare_blueprint.py data/cars/<id> RENDER_DIR OUT.png

RENDER_DIR holds ortho_{side,front,rear,top}.png and ortho.json from
render_views.py. Each render is cropped to the car's extents (from car.toml)
and shown above the blueprint view at the same scale, with the blueprint
silhouette outline drawn over the render in red.
"""
import json
import os
import sys
import tomllib

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(__file__))
from blueprint_profiles import load_image  # noqa: E402


def outline(mask):
    m = mask
    e = m & ~(np.roll(m, 1, 0) & np.roll(m, -1, 0) & np.roll(m, 1, 1) & np.roll(m, -1, 1))
    return e


def main():
    car_dir, rdir, out = sys.argv[1:4]
    with open(os.path.join(car_dir, "asset.toml"), "rb") as f:
        cfg = tomllib.load(f)
    with open(os.path.join(car_dir, "car.toml"), "rb") as f:
        ch = tomllib.load(f)["chassis"]
    with open(os.path.join(car_dir, "refs/blueprint/profiles.json")) as f:
        prof = json.load(f)
    with open(os.path.join(rdir, "ortho.json")) as f:
        meta = json.load(f)
    bp = load_image(cfg["blueprint"]["image"], os.path.join(car_dir, "refs", "cache"))
    arches = cfg["body"]["arches"]
    s_mid = (arches[0]["s"] + arches[1]["s"]) / 2
    L, W, H = ch["length_m"], ch["width_m"], ch["height_m"]
    tiles = []
    for view in ("side", "front", "rear", "top"):
        m = meta[f"ortho_{view}"]
        img = Image.open(os.path.join(rdir, f"ortho_{view}.png")).convert("RGB")
        k = m["m_per_px"]
        cx, cy = img.width / 2, img.height / 2
        p = prof[view]
        x0, y0, x1, y1 = p["bbox_src"]
        crop = Image.fromarray(bp[y0:y1, x0:x1].astype(np.uint8))
        mask = np.asarray(Image.open(os.path.join(car_dir, f"refs/blueprint/mask_{view}.png"))) > 127
        if view == "side":
            # camera from the left: nose (world +Y = s_mid ahead) on the left
            left = cx - s_mid / k
            top = cy - (H - m["center"][2]) / k
            w_px, h_px = L / k, H / k
        elif view == "top":
            left = cx - s_mid / k
            top = cy - (p["px"][1] * L * 1000 / p["px"][0] / 1000) / 2 / k
            w_px, h_px = L / k, p["px"][1] * (L * 1000 / p["px"][0]) / 1000 / k
        else:
            scale = H * 1000 / p["px"][1] / 1000  # m per blueprint px
            w_m = p["px"][0] * scale
            left = cx - w_m / 2 / k
            top = cy - (H - m["center"][2]) / k
            w_px, h_px = w_m / k, H / k
        size = (int(round(w_px)), int(round(h_px)))
        crop = crop.resize(size, Image.LANCZOS)
        maskr = np.asarray(Image.fromarray(mask.astype(np.uint8) * 255).resize(size, Image.NEAREST)) > 127
        rend = img.crop((int(round(left)), int(round(top)), int(round(left)) + size[0], int(round(top)) + size[1]))
        r = np.asarray(rend).copy()
        e = outline(maskr)
        r[e] = (255, 0, 0)
        pair = Image.new("RGB", (size[0], size[1] * 2 + 6), (255, 255, 255))
        pair.paste(Image.fromarray(r), (0, 0))
        pair.paste(crop, (0, size[1] + 6))
        tiles.append(pair)
    wtot = sum(t.width for t in tiles) + 10 * (len(tiles) - 1)
    sheet = Image.new("RGB", (wtot, max(t.height for t in tiles)), (255, 255, 255))
    x = 0
    for t in tiles:
        sheet.paste(t, (x, 0))
        x += t.width + 10
    sheet.save(out)
    print("wrote", out)


if __name__ == "__main__":
    main()
