"""Derive orthographic silhouettes and profiles from a 4-view car blueprint.

usage: python3 pipelines/refs/blueprint_profiles.py data/cars/<id>/asset.toml OUT_DIR

Reads [blueprint] from asset.toml (image path or URL, one pixel box per view and
the scale in mm/px), segments the car from the light background, and writes
  OUT_DIR/silhouette_{side,top,front,rear}.png   binary masks, 1 px = 5 mm,
                                                 centred like the model's ortho renders
  OUT_DIR/profiles.json                          per-x top/bottom (side), half width
                                                 (top) and per-z half width (front/rear)
Only numpy and Pillow are needed, so it runs outside Blender.
"""
import json
import os
import sys
import tomllib
import urllib.request

import numpy as np
from PIL import Image, ImageDraw

MM_PER_PX_OUT = 5.0


def load_image(src, cache_dir):
    if src.startswith("http"):
        os.makedirs(cache_dir, exist_ok=True)
        path = os.path.join(cache_dir, os.path.basename(src))
        if not os.path.exists(path):
            req = urllib.request.Request(src, headers={"User-Agent": "Mozilla/5.0"})
            with urllib.request.urlopen(req, timeout=60) as r, open(path, "wb") as f:
                f.write(r.read())
        src = path
    return np.asarray(Image.open(src).convert("RGB")).astype(np.int32)


def shift(m, dy, dx):
    out = np.zeros_like(m)
    h, w = m.shape
    out[max(dy, 0):h + min(dy, 0), max(dx, 0):w + min(dx, 0)] = \
        m[max(-dy, 0):h + min(-dy, 0), max(-dx, 0):w + min(-dx, 0)]
    return out


def dilate(m, r):
    out = m.copy()
    for dy in range(-r, r + 1):
        for dx in range(-r, r + 1):
            if dx * dx + dy * dy <= r * r:
                out |= shift(m, dy, dx)
    return out


def erode(m, r):
    return ~dilate(~m, r)


def fill_holes(m):
    """Fills regions not connected to the border (iterative flood fill)."""
    outside = np.zeros_like(m)
    outside[0, :] = ~m[0, :]
    outside[-1, :] = ~m[-1, :]
    outside[:, 0] = ~m[:, 0]
    outside[:, -1] = ~m[:, -1]
    free = ~m
    while True:
        grown = (outside | shift(outside, 1, 0) | shift(outside, -1, 0)
                 | shift(outside, 0, 1) | shift(outside, 0, -1)) & free
        if (grown == outside).all():
            return ~outside
        outside = grown


def segment(rgb, cfg):
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    paint = (b - np.maximum(r, g) > cfg.get("paint_blue_margin", 60))
    dark = (r + g + b) < cfg.get("dark_sum", 420)
    m = paint | dark
    r = cfg.get("close_px", 3)
    m = erode(dilate(m, r), r)
    m = fill_holes(m)
    return erode(dilate(m, 1), 1)


def keep_component(m, seed):
    """Keeps the 4-connected component of m containing seed (row, col)."""
    comp = np.zeros_like(m)
    comp[seed] = True
    while True:
        grown = (comp | shift(comp, 1, 0) | shift(comp, -1, 0)
                 | shift(comp, 0, 1) | shift(comp, 0, -1)) & m
        if (grown == comp).all():
            return comp
        comp = grown


def largest_blob_bbox(m):
    ys, xs = np.nonzero(m)
    return xs.min(), xs.max(), ys.min(), ys.max()


def main():
    cfg_path, out = sys.argv[1], sys.argv[2]
    with open(cfg_path, "rb") as f:
        asset = tomllib.load(f)
    bp = asset["blueprint"]
    car_dir = os.path.dirname(cfg_path)
    with open(os.path.join(car_dir, "car.toml"), "rb") as f:
        car = tomllib.load(f)["chassis"]
    rgb = load_image(bp["image"], os.path.join(car_dir, "refs", "cache"))
    os.makedirs(out, exist_ok=True)
    profiles = {}
    for view in ("side", "top", "front", "rear"):
        x0, y0, x1, y1 = bp["views"][view]
        crop = rgb[y0:y1, x0:x1]
        m = segment(crop, bp)
        for poly in bp.get("fill", {}).get(view, []):
            img = Image.fromarray(m.astype(np.uint8) * 255)
            ImageDraw.Draw(img).polygon([(x - x0, y - y0) for x, y in poly], fill=255)
            m = np.asarray(img) > 0
        m = keep_component(m, (m.shape[0] // 2, m.shape[1] // 2))
        for (ex0, ey0, ex1, ey1) in bp.get("erase", {}).get(view, []):
            m[ey0 - y0:ey1 - y0, ex0 - x0:ex1 - x0] = False
        bx0, bx1, by0, by1 = largest_blob_bbox(m)
        m = m[by0:by1 + 1, bx0:bx1 + 1]
        h, w = m.shape
        # Scale from the known overall dimension of each view (car.toml is the
        # hard constraint, the drawing only gives shape).
        if view == "side":
            sx = car["length_m"] * 1000 / w
            sy = car["height_m"] * 1000 / h
        elif view == "top":
            sx = car["length_m"] * 1000 / w
            sy = None  # mirrors make the top view wider than the body
        else:
            sx = None
            sy = car["height_m"] * 1000 / h
        profiles[view] = {"px": [int(w), int(h)], "mm_per_px": [sx, sy],
                          "bbox_src": [int(x0 + bx0), int(y0 + by0), int(x0 + bx1 + 1), int(y0 + by1 + 1)]}
        if view == "side":
            # Car faces left in the drawing: x from the nose (0) to the tail.
            xs = np.arange(w)
            top = np.array([np.argmax(m[:, i]) for i in xs])
            bottom = np.array([h - 1 - np.argmax(m[::-1, i]) for i in xs])
            profiles["side"]["x_mm"] = (xs * sx).round(1).tolist()
            profiles["side"]["top_mm"] = ((h - top) * sy).round(1).tolist()
            profiles["side"]["bottom_mm"] = ((h - 1 - bottom) * sy).round(1).tolist()
        elif view == "top":
            xs = np.arange(w)
            cy = h / 2
            half = []
            for i in xs:
                col = np.nonzero(m[:, i])[0]
                half.append(((col.max() - col.min() + 1) / 2) if len(col) else 0)
            profiles["top"]["x_mm"] = (xs * sx).round(1).tolist()
            profiles["top"]["half_width_px"] = [float(v) for v in half]
        else:
            zs = np.arange(h)
            half = []
            for j in zs:
                row = np.nonzero(m[j, :])[0]
                half.append(((row.max() - row.min() + 1) / 2) if len(row) else 0)
            profiles[view]["z_mm"] = ((h - zs) * sy).round(1).tolist()
            profiles[view]["half_width_px"] = [float(v) for v in half]
        Image.fromarray((m * 255).astype(np.uint8)).save(os.path.join(out, f"mask_{view}.png"))
    with open(os.path.join(out, "profiles.json"), "w") as f:
        json.dump(profiles, f)
    print("wrote", out)


if __name__ == "__main__":
    main()
