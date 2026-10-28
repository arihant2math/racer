"""Procedural textures (numpy -> bpy images, packed as PNG in the GLB).

All textures are generated from code so the asset is reproducible; nothing is
copied from photos. Text is rasterised with Blender's bundled font through a
temporary text object render, which keeps the pipeline dependency-free.
"""
import math

import bpy
import numpy as np


# Sidewall lettering (cars/build.py overrides it per car).
TYRE_TEXT = "PIRELLI      P ZERO      305/30 ZR21 (104Y)      N0      PIRELLI      P ZERO      "


def _img(name, arr, alpha=False):
    h, w = arr.shape[:2]
    if arr.ndim == 2:
        arr = np.repeat(arr[..., None], 3, axis=2)
    if arr.shape[2] == 3:
        arr = np.concatenate([arr, np.ones((h, w, 1))], axis=2)
    img = bpy.data.images.get(name)
    if img:
        bpy.data.images.remove(img)
    img = bpy.data.images.new(name, w, h, alpha=alpha)
    img.pixels.foreach_set(np.ascontiguousarray(arr[::-1], dtype=np.float32).ravel())
    img.file_format = "PNG"
    img.pack()
    return img


def _noise(h, w, scale, seed):
    rng = np.random.default_rng(seed)
    gh, gw = max(2, int(h / scale)) + 2, max(2, int(w / scale)) + 2
    g = rng.random((gh, gw))
    y = np.linspace(0, gh - 2.0001, h)
    x = np.linspace(0, gw - 2.0001, w)
    yi, xi = y.astype(int), x.astype(int)
    fy, fx = (y - yi)[:, None], (x - xi)[None, :]
    fy, fx = fy * fy * (3 - 2 * fy), fx * fx * (3 - 2 * fx)
    a = g[yi][:, xi]
    b = g[yi][:, xi + 1]
    c = g[yi + 1][:, xi]
    d = g[yi + 1][:, xi + 1]
    return a * (1 - fx) * (1 - fy) + b * fx * (1 - fy) + c * (1 - fx) * fy + d * fx * fy


def _height_to_normal(hgt, strength):
    gy, gx = np.gradient(hgt)
    n = np.stack([-gx * strength, gy * strength, np.ones_like(hgt)], axis=2)
    n /= np.linalg.norm(n, axis=2, keepdims=True)
    return n * 0.5 + 0.5


def _text_mask(text, w, h, bold=False, extend=1.0):
    """Rasterise text to an (h, w) mask using Blender's font + workbench render."""
    sc = bpy.data.scenes.new("_txt")
    cu = bpy.data.curves.new("_txt", "FONT")
    cu.body = text
    cu.align_x = "CENTER"
    cu.align_y = "CENTER"
    cu.space_character = extend
    ob = bpy.data.objects.new("_txt", cu)
    sc.collection.objects.link(ob)
    mat = bpy.data.materials.new("_txtm")
    mat.diffuse_color = (1, 1, 1, 1)
    cu.materials.append(mat)
    if bold:
        cu.offset = 0.012
    cam = bpy.data.objects.new("_txtc", bpy.data.cameras.new("_txtc"))
    sc.collection.objects.link(cam)
    sc.camera = cam
    cam.data.type = "ORTHO"
    bpy.context.view_layer.update()
    dims = ob.dimensions
    cam.data.ortho_scale = max(dims.x * 1.08, dims.y * 1.08 * w / h)
    cam.location = (0, 0, 5)
    sc.render.engine = "BLENDER_WORKBENCH"
    sc.display.shading.light = "FLAT"
    sc.display.shading.color_type = "MATERIAL"
    sc.render.resolution_x, sc.render.resolution_y = w, h
    sc.render.film_transparent = True
    sc.view_settings.view_transform = "Standard"
    path = bpy.app.tempdir + "_txt.png"
    sc.render.filepath = path
    bpy.ops.render.render(write_still=True, scene=sc.name)
    img = bpy.data.images.load(path)
    a = np.array(img.pixels[:]).reshape(h, w, 4)[::-1, :, 3]
    bpy.data.images.remove(img)
    bpy.data.objects.remove(ob)
    bpy.data.objects.remove(cam)
    bpy.data.curves.remove(cu)
    bpy.data.materials.remove(mat)
    bpy.data.scenes.remove(sc)
    return a


def tyre_sidewall(w=2048, h=256):
    """Sidewall strip: U around the tyre, V from rim (0) to tread shoulder (1)."""
    v = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    base = 0.022 + 0.006 * _noise(h, w, 6, 1)
    hgt = 0.15 * _noise(h, w, 3, 2)
    # Rim protector rib and concentric rings.
    hgt += 0.6 * np.exp(-((v - 0.12) / 0.04) ** 2) + 0.25 * np.exp(-((v - 0.9) / 0.03) ** 2)
    col = np.repeat(base[..., None], 3, axis=2)
    # Raised lettering band.
    m = _text_mask(TYRE_TEXT, w, int(h * 0.42))
    band = np.zeros((h, w))
    o = int(h * 0.32)
    band[o : o + m.shape[0]] = m
    hgt += 0.7 * band
    col += 0.03 * band[..., None]
    return col, _height_to_normal(hgt, 6.0)


def tread(w=1024, h=1024):
    v = np.linspace(0, 1, h)[:, None] * np.ones((1, w))
    u = np.linspace(0, 1, w)[None, :] * np.ones((h, 1))
    hgt = np.ones((h, w))
    # Four circumferential grooves (V across the tread), sipes across.
    for c in (0.2, 0.4, 0.6, 0.8):
        hgt -= 0.9 * (np.abs(v - c) < 0.018)
    sip = (np.abs(((u * 64 + 0.6 * np.sin(v * 12)) % 1.0) - 0.5) < 0.04) & ((v < 0.18) | (v > 0.82))
    hgt -= 0.6 * sip
    return hgt


def tyre_textures():
    side_col, side_n = tyre_sidewall()
    th = tread(1024, 512)
    tn = _height_to_normal(th, 4.0)
    # Combine: atlas top half sidewall (outer), lower half tread. Tyre UVs map accordingly.
    h = 1024
    col = np.zeros((h, 2048, 3)) + 0.02
    nrm = np.zeros((h, 2048, 3)) + np.array([0.5, 0.5, 1.0])
    col[: 256] = side_col
    nrm[: 256] = side_n
    col[256:512] = side_col[::-1]  # inner sidewall (mirrored)
    nrm[256:512] = side_n[::-1] * np.array([1, -1, 1]) + np.array([0, 1, 0])
    tcol = 0.02 + 0.01 * (th[..., None] > 0.5)
    tt = np.tile(tcol, (1, 2, 1))
    col[512:] = tt[:512, :2048]
    nrm[512:] = np.tile(tn, (1, 2, 1))[:512, :2048]
    return _img("tx_tyre_col", col), _img("tx_tyre_n", nrm)


def grain_normal(n=512):
    hgt = 0.6 * _noise(n, n, 2, 7) + 0.4 * _noise(n, n, 5, 8)
    return _img("tx_grain_n", _height_to_normal(hgt, 3.0))


def brake_disc(n=1024):
    y, x = np.mgrid[0:n, 0:n] / (n - 1) * 2 - 1
    r = np.hypot(x, y)
    a = np.arctan2(y, x)
    col = 0.38 + 0.08 * np.sin(r * 400) * 0.3 + 0.05 * _noise(n, n, 10, 3)
    holes = np.zeros_like(r)
    for ring, count in ((0.72, 24), (0.8, 24), (0.88, 24)):
        for k in range(count):
            t = 2 * math.pi * k / count + ring * 7
            cx, cy = ring * math.cos(t), ring * math.sin(t)
            holes = np.maximum(holes, (np.hypot(x - cx, y - cy) < 0.018).astype(float))
    col = col * (1 - holes) + 0.03 * holes
    hub = r < 0.55
    col = np.where(hub, 0.18, col)
    return _img("tx_disc", np.clip(col, 0, 1))


def crest(n=512):
    """Stylised shield crest (generic, not a trademark reproduction)."""
    y, x = np.mgrid[0:n, 0:n] / (n - 1)
    x = x * 2 - 1
    y = 1 - y
    shield = (np.abs(x) < 0.82 - 0.25 * np.clip(0.35 - y, 0, 1) ** 1.5 * 4) & (y > 0.04) & (y < 0.96)
    shield &= ~((y < 0.25) & (np.abs(x) > 0.82 * (y / 0.25) ** 0.6))
    col = np.zeros((n, n, 3)) + np.array([0.8, 0.62, 0.2])  # gold frame
    inner = shield & (np.abs(x) < 0.72) & (y > 0.12) & (y < 0.82)
    quad = ((x < 0) ^ (y > 0.47))
    stripes = (np.sin(y * 40) > 0)
    col[inner & quad & stripes] = (0.55, 0.05, 0.05)
    col[inner & quad & ~stripes] = (0.05, 0.05, 0.05)
    col[inner & ~quad] = (0.85, 0.65, 0.18)
    centre = (np.abs(x) < 0.22) & (np.abs(y - 0.47) < 0.2)
    col[centre] = (0.9, 0.75, 0.25)
    col[~shield] = (0.75, 0.75, 0.75)
    banner = shield & (y > 0.82)
    col[banner] = (0.06, 0.06, 0.06)
    return _img("tx_crest", col)


def lettering():
    m = _text_mask("P O R S C H E", 2048, 256, extend=1.4)
    col = np.zeros((256, 2048, 4))
    col[..., :3] = 0.85
    col[..., 3] = m
    return _img("tx_lettering", col, alpha=True)


def model_script():
    m = _text_mask("911 Carrera S", 1024, 256)
    col = np.zeros((256, 1024, 4))
    col[..., :3] = 0.8
    col[..., 3] = m
    return _img("tx_script", col, alpha=True)


def plate():
    w, h = 1040, 220
    col = np.ones((h, w, 3)) * 0.95
    col[:8] = col[-8:] = 0.1
    col[:, :8] = col[:, -8:] = 0.1
    col[:, 8:90] = (0.05, 0.15, 0.6)
    m = _text_mask("RCR 911", 900, 180)
    col[20:200, 120:1020] = col[20:200, 120:1020] * (1 - m[..., None]) + 0.05 * m[..., None]
    return _img("tx_plate", col)


def grille_mask(n=512):
    y, x = np.mgrid[0:n, 0:n] / n
    # Hexagonal-ish honeycomb mesh: alpha 1 on the webs.
    a = (np.abs(((x * 24 + 0.5 * (np.floor(y * 16) % 2)) % 1) - 0.5) > 0.36) | (np.abs((y * 16 % 1) - 0.5) > 0.38)
    return _img("tx_grille", a.astype(float))


def build_all():
    tyre_col, tyre_n = tyre_textures()
    return {
        "tyre_col": tyre_col,
        "tyre_n": tyre_n,
        "grain_n": grain_normal(),
        "disc_col": brake_disc(),
        "crest": crest(),
        "lettering": lettering(),
        "script": model_script(),
        "plate": plate(),
        "grille_mask": grille_mask(),
    }
