"""Extract metric silhouettes and feature lines from a 4-view car blueprint SVG.

    python tools/car_refs/extract_blueprint.py <blueprint.svg> <car.toml> <out.json>

Needs numpy, pillow, svgpathtools and `rsvg-convert` (librsvg). The SVG is the
dimensions.com drawing set: four filled outline paths (side, plan, front,
rear) plus stroked detail lines. Views are scaled so the side and plan
outlines span `length_m` and the side/front/rear outlines span `height_m`
(both from car.toml). Coordinates in the output:

- side:  (x forward from the length midpoint, z up from the ground)
- plan:  (x forward, y left)
- front, rear: (y to the viewer's right, z up)
"""

import json
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

import numpy as np
from PIL import Image
from svgpathtools import parse_path

Image.MAX_IMAGE_PIXELS = None
ZOOM = 12  # raster zoom for silhouettes (1 svg unit = 12 px)


def main(svg_path, toml_path, out_path):
    spec = tomllib.loads(Path(toml_path).read_text())
    L, H = spec["chassis"]["length_m"], spec["chassis"]["height_m"]
    svg = Path(svg_path).read_text()
    outline_at = svg.index('<g id="_x32_D_x24_AG-OUTLINE"')
    details_at = svg.index('<g id="_x32_D_x24_AG-DETAILS"')
    outlines = re.findall(r'<path d="([^"]+)"', svg[outline_at:])
    assert len(outlines) == 4, len(outlines)

    masks, boxes = [], []
    with tempfile.TemporaryDirectory() as tmp:
        for k, d in enumerate(outlines):
            one = (
                '<svg xmlns="http://www.w3.org/2000/svg" width="1000" height="650" '
                'viewBox="0 0 1000 650"><rect width="1000" height="650" fill="#fff"/>'
                f'<path d="{d}" fill="#000"/></svg>'
            )
            src, png = Path(tmp, f"o{k}.svg"), Path(tmp, f"o{k}.png")
            src.write_text(one)
            subprocess.run(["rsvg-convert", "-z", str(ZOOM), src, "-o", png], check=True)
            m = np.array(Image.open(png).convert("L")) < 128
            ys, xs = np.nonzero(m)
            masks.append(m)
            boxes.append((xs.min(), xs.max(), ys.min(), ys.max()))
    # Identify views: the two wide outlines are side (top) and plan (bottom),
    # the narrow ones front (top) and rear (bottom).
    wide = sorted([k for k in range(4) if boxes[k][1] - boxes[k][0] > 4000], key=lambda k: boxes[k][2])
    narrow = sorted([k for k in range(4) if k not in wide], key=lambda k: boxes[k][2])
    ids = {"side": wide[0], "plan": wide[1], "front": narrow[0], "rear": narrow[1]}

    frames = {}
    out = {"length_m": L, "height_m": H, "source": str(svg_path)}
    # Side: top and bottom silhouette per column.
    m, (x0, x1, y0, y1) = masks[ids["side"]], boxes[ids["side"]]
    sx, sz = L / (x1 - x0), H / (y1 - y0)
    frames["side"] = (x0 / ZOOM, x1 / ZOOM, y0 / ZOOM, y1 / ZOOM)
    cols = np.arange(x0, x1 + 1, 3)
    top = np.array([np.nonzero(m[:, c])[0].min() for c in cols])
    bot = np.array([np.nonzero(m[:, c])[0].max() for c in cols])
    out["side"] = {
        "x": ((cols - x0) * sx - L / 2).round(5).tolist(),
        "z_top": ((y1 - top) * sz).round(5).tolist(),
        "z_bottom": ((y1 - bot) * sz).round(5).tolist(),
    }
    # Plan: half width per column (includes mirrors).
    m, (x0, x1, y0, y1) = masks[ids["plan"]], boxes[ids["plan"]]
    sp = L / (x1 - x0)
    frames["plan"] = (x0 / ZOOM, x1 / ZOOM, y0 / ZOOM, y1 / ZOOM)
    cols = np.arange(x0, x1 + 1, 3)
    cy = (y0 + y1) / 2
    left = np.array([cy - np.nonzero(m[:, c])[0].min() for c in cols])
    right = np.array([np.nonzero(m[:, c])[0].max() - cy for c in cols])
    out["plan"] = {
        "x": ((cols - x0) * sp - L / 2).round(5).tolist(),
        "half_width": (0.5 * (left + right) * sp).round(5).tolist(),
    }
    # Front / rear: half width per row, scaled by height.
    for view in ("front", "rear"):
        m, (x0, x1, y0, y1) = masks[ids[view]], boxes[ids[view]]
        s = H / (y1 - y0)
        frames[view] = (x0 / ZOOM, x1 / ZOOM, y0 / ZOOM, y1 / ZOOM)
        rows = np.arange(y0, y1 + 1, 3)
        cx = (x0 + x1) / 2
        hw = [0.5 * (np.nonzero(m[r])[0].max() - np.nonzero(m[r])[0].min()) for r in rows]
        out[view] = {"z": ((y1 - rows) * s).round(5).tolist(), "half_width": (np.array(hw) * s).round(5).tolist()}

    def to_metric(view, x, y):
        X0, X1, Y0, Y1 = frames[view]
        if view == "side":
            s = (X1 - X0) / L
            return (x - X0) / s - L / 2, (Y1 - y) / ((Y1 - Y0) / H)
        if view == "plan":
            s = (X1 - X0) / L
            return (x - X0) / s - L / 2, ((Y0 + Y1) / 2 - y) / s
        s = (Y1 - Y0) / H
        return (x - (X0 + X1) / 2) / s, (Y1 - y) / s

    lines = []
    for mt in re.finditer(r"<(path|polyline|line)([^>]*)/>", svg[details_at:outline_at]):
        tag, attrs = mt.groups()
        if tag == "path":
            pts = []
            for seg in parse_path(re.search(r' d="([^"]+)"', attrs).group(1)):
                n = max(2, int(seg.length() / 0.5))
                pts += [(z.real, z.imag) for z in (seg.point(t) for t in np.linspace(0, 1, n))]
        elif tag == "polyline":
            v = list(map(float, re.search(r'points="([^"]+)"', attrs).group(1).split()))
            pts = list(zip(v[0::2], v[1::2]))
        else:
            g = lambda k: float(re.search(k + r'="([^"]+)"', attrs).group(1))
            pts = [(g("x1"), g("y1")), (g("x2"), g("y2"))]
        cx, cy = np.mean(pts, axis=0)
        view = next(
            (v for v, (X0, X1, Y0, Y1) in frames.items() if X0 - 5 <= cx <= X1 + 5 and Y0 - 15 <= cy <= Y1 + 15),
            None,
        )
        lines.append(
            {"view": view, "pts": [[round(c, 5) for c in to_metric(view, x, y)] for x, y in pts] if view else []}
        )
    out["lines"] = lines
    Path(out_path).write_text(json.dumps(out))
    print(f"{out_path}: {sum(l['view'] is not None for l in lines)} lines")


if __name__ == "__main__":
    main(*sys.argv[1:4])
