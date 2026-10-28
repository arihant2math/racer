#!/usr/bin/env python3
"""Draws the temporary app icon: a bold italic "R" on a rounded orange tile.

Writes assets/icon/icon-<N>.png for N in SIZES, icon.ico (Windows, 16-256)
and icon.icns (macOS, 16-1024). Needs Pillow and a bold display font
(Inter Display Black Italic by default; pass another .otf/.ttf as argv[1]).
Run from anywhere: tools/make_icon.py [FONT]
"""

import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

SIZES = [16, 24, 32, 48, 64, 128, 256, 512, 1024]
FONTS = [
    "/usr/share/fonts/opentype/inter/InterDisplay-BlackItalic.otf",
    "/usr/share/fonts/opentype/inter/InterDisplay-ExtraBoldItalic.otf",
    "/usr/share/fonts/truetype/dejavu/DejaVuSans-BoldOblique.ttf",
]
# The menu highlight colour (flow.rs HIGHLIGHT), lighter at the top.
TOP = (238, 112, 34)
BOTTOM = (196, 70, 12)
INK = (255, 255, 255)
SHADOW = (90, 28, 4, 150)
OUT = Path(__file__).resolve().parent.parent / "assets" / "icon"


def font_path() -> str:
    if len(sys.argv) > 1:
        return sys.argv[1]
    for f in FONTS:
        if Path(f).exists():
            return f
    sys.exit("no bold font found; pass one as the first argument")


def master(size: int, font: str) -> Image.Image:
    """The icon at `size` px, drawn 4x larger and scaled down (anti-aliased)."""
    s = size * 4
    # Small icons use the whole canvas; larger ones keep a margin like the
    # platform templates (macOS's ~10 %).
    margin = 0 if size <= 32 else round(s * 0.06)
    tile = s - 2 * margin
    radius = round(tile * 0.22)

    gradient = Image.new("RGB", (1, tile))
    for y in range(tile):
        t = y / max(tile - 1, 1)
        gradient.putpixel((0, y), tuple(round(a + (b - a) * t) for a, b in zip(TOP, BOTTOM)))
    gradient = gradient.resize((tile, tile))
    mask = Image.new("L", (tile, tile), 0)
    ImageDraw.Draw(mask).rounded_rectangle((0, 0, tile - 1, tile - 1), radius, fill=255)

    img = Image.new("RGBA", (s, s), (0, 0, 0, 0))
    img.paste(gradient, (margin, margin), mask)

    # The letter: fitted to ~68 % of the tile height, centred on its ink box.
    target = tile * (0.74 if size <= 24 else 0.68)
    f = ImageFont.truetype(font, round(target * 1.4))
    box = f.getbbox("R")
    f = ImageFont.truetype(font, round(target * 1.4 * target / (box[3] - box[1])))
    box = f.getbbox("R")
    x = margin + (tile - (box[2] - box[0])) / 2 - box[0]
    y = margin + (tile - (box[3] - box[1])) / 2 - box[1]
    if size > 32:
        shadow = Image.new("RGBA", (s, s), (0, 0, 0, 0))
        off = tile * 0.025
        ImageDraw.Draw(shadow).text((x + off, y + off), "R", font=f, fill=SHADOW)
        shadow = shadow.filter(ImageFilter.GaussianBlur(tile * 0.015))
        clipped = Image.new("RGBA", (s, s), (0, 0, 0, 0))
        full_mask = Image.new("L", (s, s), 0)
        full_mask.paste(mask, (margin, margin))
        clipped.paste(shadow, (0, 0), Image.composite(shadow.getchannel("A"), full_mask, full_mask))
        img = Image.alpha_composite(img, clipped)
    ImageDraw.Draw(img).text((x, y), "R", font=f, fill=INK)
    return img.resize((size, size), Image.LANCZOS)


# icns element types holding PNG data, by pixel size. Pillow's writer leaves
# out icp4/icp5 (16 and 32 px at 1x), which would lose the hand-tuned small sizes.
ICNS_TYPES = [
    (b"icp4", 16), (b"icp5", 32), (b"icp6", 64), (b"ic07", 128), (b"ic08", 256),
    (b"ic09", 512), (b"ic10", 1024), (b"ic11", 32), (b"ic12", 64), (b"ic13", 256),
    (b"ic14", 512),
]


def write_icns(path: Path, pngs: dict) -> None:
    elements = [(t, pngs[n]) for t, n in ICNS_TYPES]
    toc = b"TOC " + struct.pack(">I", 8 + 8 * len(elements))
    toc += b"".join(t + struct.pack(">I", 8 + len(d)) for t, d in elements)
    body = toc + b"".join(t + struct.pack(">I", 8 + len(d)) + d for t, d in elements)
    path.write_bytes(b"icns" + struct.pack(">I", 8 + len(body)) + body)


def main() -> None:
    font = font_path()
    OUT.mkdir(parents=True, exist_ok=True)
    images = {n: master(n, font) for n in SIZES}
    for n, img in images.items():
        img.save(OUT / f"icon-{n}.png", optimize=True)
    # Each size drawn on its own (hinted for small sizes), not one image resampled.
    ico = [images[n] for n in SIZES if n <= 256]
    ico[-1].save(OUT / "icon.ico", sizes=[(i.width, i.height) for i in ico], append_images=ico[:-1])
    write_icns(OUT / "icon.icns", {n: (OUT / f"icon-{n}.png").read_bytes() for n in SIZES})
    print(f"wrote {len(SIZES)} PNGs, icon.ico and icon.icns to {OUT} (font {font})")


if __name__ == "__main__":
    main()
