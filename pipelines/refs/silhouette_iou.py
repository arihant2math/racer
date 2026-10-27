"""IoU of rendered silhouette masks against the blueprint silhouettes.

usage: python3 pipelines/refs/silhouette_iou.py data/cars/<id> MASK_DIR [--json OUT]

MASK_DIR holds model_{side,top,front,rear}.png from render_views.py --masks
(5 mm/px, orthographic: side from the left with the nose left, top with the
nose left, front and rear centred, ground on the bottom edge of side/front/rear).
The blueprint masks keep their own scale (length from car.toml for side/top,
height for front/rear) and are placed the same way. Mirrors are part of both.
"""
import json
import os
import sys
import tomllib

import numpy as np
from PIL import Image

PX = 0.005


def place(mask, scale_m_per_px, shape, anchor):
    """Resizes a blueprint mask to PX and places it on a canvas of `shape`.

    anchor: ("x", value_px) left edge, or "center"; vertical: bottom or center.
    """
    h, w = shape
    k = scale_m_per_px / PX
    m = Image.fromarray(mask.astype(np.uint8) * 255)
    m = np.asarray(m.resize((max(1, round(m.width * k)), max(1, round(m.height * k))), Image.BILINEAR)) > 127
    out = np.zeros(shape, bool)
    hx, vy = anchor
    x0 = int(round(hx if hx is not None else (w - m.shape[1]) / 2))
    y0 = int(round(h - m.shape[0] if vy == "bottom" else (h - m.shape[0]) / 2))
    xs0, ys0 = max(0, -x0), max(0, -y0)
    xd0, yd0 = max(0, x0), max(0, y0)
    cw = min(m.shape[1] - xs0, w - xd0)
    ch = min(m.shape[0] - ys0, h - yd0)
    out[yd0:yd0 + ch, xd0:xd0 + cw] = m[ys0:ys0 + ch, xs0:xs0 + cw]
    return out


def main():
    car_dir, mask_dir = sys.argv[1], sys.argv[2]
    out_json = sys.argv[sys.argv.index("--json") + 1] if "--json" in sys.argv else None
    with open(os.path.join(car_dir, "car.toml"), "rb") as f:
        ch = tomllib.load(f)["chassis"]
    with open(os.path.join(car_dir, "asset.toml"), "rb") as f:
        cfg = tomllib.load(f)
    with open(os.path.join(car_dir, "refs/blueprint/profiles.json")) as f:
        prof = json.load(f)
    arches = cfg["body"]["arches"]
    s_mid = (arches[0]["s"] + arches[1]["s"]) / 2
    L, H = ch["length_m"], ch["height_m"]
    res = {}
    for view in ("side", "top", "front", "rear"):
        model = np.asarray(Image.open(os.path.join(mask_dir, f"model_{view}.png")).convert("L")) > 127
        bp = np.asarray(Image.open(os.path.join(car_dir, f"refs/blueprint/mask_{view}.png"))) > 127
        h, w = model.shape
        if view in ("side", "top"):
            scale = L / prof[view]["px"][0]
            # image centre is the wheelbase midpoint; the nose is s_mid ahead (left)
            anchor = (w / 2 - s_mid / PX, "bottom" if view == "side" else "center")
        else:
            scale = H / prof[view]["px"][1]
            anchor = (None, "bottom")
        bpm = place(bp, scale, (h, w), anchor)
        inter = (model & bpm).sum()
        union = (model | bpm).sum()
        res[view] = round(float(inter / union), 4)
        rgb = np.zeros((h, w, 3), np.uint8)
        rgb[model & bpm] = 255
        rgb[bpm & ~model] = (255, 60, 60)
        rgb[model & ~bpm] = (60, 120, 255)
        Image.fromarray(rgb).save(os.path.join(mask_dir, f"iou_{view}.png"))
    res["mean"] = round(sum(res[v] for v in ("side", "top", "front", "rear")) / 4, 4)
    print(json.dumps(res))
    if out_json:
        with open(out_json, "w") as f:
            json.dump(res, f, indent=2)


if __name__ == "__main__":
    main()
