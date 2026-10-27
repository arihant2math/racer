"""Fast silhouette check of the lofted body against the blueprint, without Blender.

usage: python3 pipelines/blender/preview_profile.py data/cars/<id> OUT.png

Rasterises the body grid (and wheel discs in the side view) orthographically at
5 mm/px, overlays it on the blueprint masks (white = both, red = blueprint only,
blue = model only) and prints the IoU per view.
"""
import os
import sys
import tomllib

import numpy as np
from PIL import Image, ImageDraw

sys.path.insert(0, os.path.dirname(__file__))
from carlib import profile  # noqa: E402

PX = 0.005


def raster(polys, w, h):
    img = Image.new("L", (w, h), 0)
    d = ImageDraw.Draw(img)
    for p in polys:
        d.polygon([tuple(q) for q in p], fill=255)
    return np.asarray(img) > 0


def quads(g):
    a, b = g[:-1, :-1], g[1:, :-1]
    c, d = g[1:, 1:], g[:-1, 1:]
    return np.stack([a, b, c, d], axis=2).reshape(-1, 4, g.shape[-1])


def bp_mask(path, w, h, mm_per_px):
    """Blueprint mask at PX scale, centred on a (h, w) canvas, bottom aligned."""
    m = Image.open(path)
    k = mm_per_px / 1000 / PX
    m = m.resize((max(1, round(m.width * k)), max(1, round(m.height * k))), Image.BILINEAR)
    a = np.asarray(m) > 127
    out = np.zeros((h, w), bool)
    x0 = (w - a.shape[1]) // 2
    y0 = h - a.shape[0]
    xs0, ys0 = max(0, -x0), max(0, -y0)
    xd0, yd0 = max(0, x0), max(0, y0)
    cw = min(a.shape[1] - xs0, w - xd0)
    chh = min(a.shape[0] - ys0, h - yd0)
    out[yd0:yd0 + chh, xd0:xd0 + cw] = a[ys0:ys0 + chh, xs0:xs0 + cw]
    return out


def bp_mask_top(path, shape, mm_per_px):
    """Top view: vertically centred rather than bottom aligned."""
    h, w = shape
    m = Image.open(path)
    k = mm_per_px / 1000 / PX
    m = m.resize((max(1, round(m.width * k)), max(1, round(m.height * k))), Image.BILINEAR)
    a = np.asarray(m) > 127
    out = np.zeros((h, w), bool)
    y0 = (h - a.shape[0]) // 2
    cw = min(a.shape[1], w)
    out[y0:y0 + a.shape[0], :cw] = a[:, :cw]
    return out


def main():
    car_dir, out = sys.argv[1], sys.argv[2]
    with open(os.path.join(car_dir, "asset.toml"), "rb") as f:
        cfg = tomllib.load(f)
    with open(os.path.join(car_dir, "car.toml"), "rb") as f:
        car = tomllib.load(f)
    ch = car["chassis"]
    body = profile.load(cfg, ch, os.path.join(car_dir, "refs/blueprint/profiles.json"))
    st, g, bands = body.grid()
    L, W, H = ch["length_m"], ch["width_m"], ch["height_m"]
    q = quads(g)  # (n, 4, 3) s, y, z
    views = {}
    # Side: s right-to-left as in the drawing (nose left), z up.
    wl, hl = int(round(L / PX)), int(round(H / PX))
    side = [np.column_stack([p[:, 0] / PX, (H - p[:, 2]) / PX]) for p in q]
    for a in body.arches:
        pass
    m = raster(side, wl, hl)
    wheels = cfg.get("preview_wheels", [])
    img = Image.fromarray(m.astype(np.uint8) * 255)
    d = ImageDraw.Draw(img)
    for s, r in wheels:
        d.ellipse([(s - r) / PX, (H - 2 * r) / PX, (s + r) / PX, H / PX], fill=255)
    views["side"] = np.asarray(img) > 0
    # Top: nose left, y down; mirror the half body.
    ww = int(round(W / PX))
    top = []
    for p in q:
        top.append(np.column_stack([p[:, 0] / PX, (W / 2 + p[:, 1]) / PX]))
        top.append(np.column_stack([p[:, 0] / PX, (W / 2 - p[:, 1]) / PX]))
    views["top"] = raster(top, wl, ww)
    # Front: y across, z up (rear view is the same silhouette mirrored; split by s).
    for name, sel in (("front", lambda p: p[:, 0].mean() < L / 2), ("rear", lambda p: p[:, 0].mean() >= L / 2)):
        polys = []
        for p in q:
            polys.append(np.column_stack([(W / 2 + p[:, 1]) / PX, (H - p[:, 2]) / PX]))
            polys.append(np.column_stack([(W / 2 - p[:, 1]) / PX, (H - p[:, 2]) / PX]))
        views[name] = raster(polys, ww, hl)
    import json
    with open(os.path.join(car_dir, "refs/blueprint/profiles.json")) as f:
        bpp = json.load(f)
    rows = []
    for name, m in views.items():
        # Blueprint masks keep their own scale (mirrors make the top, front
        # and rear views wider than the body); length and height are exact.
        scale = {"side": L * 1000 / bpp["side"]["px"][0], "top": L * 1000 / bpp["top"]["px"][0],
                 "front": H * 1000 / bpp["front"]["px"][1], "rear": H * 1000 / bpp["rear"]["px"][1]}[name]
        pad = 40 if name != "side" else 0
        if pad:
            if name == "top":
                m = np.pad(m, ((pad, pad), (0, 0)))
            else:
                m = np.pad(m, ((0, 0), (pad, pad)))
        bp = bp_mask(os.path.join(car_dir, f"refs/blueprint/mask_{name}.png"), m.shape[1], m.shape[0], scale)
        if name == "top":
            bp = bp_mask_top(os.path.join(car_dir, "refs/blueprint/mask_top.png"), m.shape, scale)
        inter = (m & bp).sum()
        union = (m | bp).sum()
        print(f"{name}: IoU {inter / union:.3f}")
        rgb = np.zeros(m.shape + (3,), np.uint8)
        rgb[m & bp] = 255
        rgb[bp & ~m] = (255, 60, 60)
        rgb[m & ~bp] = (60, 120, 255)
        rows.append(Image.fromarray(rgb))
    wtot = max(r.width for r in rows[:2]) + max(rows[2].width, rows[3].width) + 20
    canvas = Image.new("RGB", (wtot, rows[0].height + rows[1].height + 20), (40, 40, 40))
    canvas.paste(rows[0], (0, 0))
    canvas.paste(rows[1], (0, rows[0].height + 10))
    canvas.paste(rows[2], (rows[0].width + 10, 0))
    canvas.paste(rows[3], (rows[0].width + 10, rows[2].height + 10))
    canvas.save(out)


if __name__ == "__main__":
    main()
