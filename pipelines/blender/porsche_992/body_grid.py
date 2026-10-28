"""Build the body surface grid: blueprint silhouettes x AI cross-section shapes.

    python body_grid.py <blueprint.json> <ai_raw.json> <car.toml> <out.npz>

Frame (metres): x forward from the length midpoint, y left, z up from the
ground. The body is a closed tube of stations along x; each station is a half
contour from the top centre (theta = 0) round the side to the bottom centre
(theta = pi), stored as a radial function r(x, theta) in normalized section
coordinates (y / half_width, (z - bottom) / (top - bottom)) around (0, 0.5).

- Silhouettes from the blueprint: side top/bottom, plan half width.
- Section shape from the image-to-3D base mesh (wheels and mirrors masked and
  bridged from neighbouring stations), smoothed along x and theta.
"""

import json
import sys
import tomllib
from pathlib import Path

import numpy as np


def gaussian_filter1d(a, sigma, axis=-1, mode="nearest"):
    """Gaussian smoothing along one axis with edge-value padding (numpy only)."""
    a = np.moveaxis(np.asarray(a, dtype=float), axis, -1)
    r = int(4 * sigma + 0.5)
    k = np.exp(-0.5 * (np.arange(-r, r + 1) / sigma) ** 2)
    k /= k.sum()
    pad = np.concatenate([np.repeat(a[..., :1], r, -1), a, np.repeat(a[..., -1:], r, -1)], -1)
    out = np.apply_along_axis(lambda v: np.convolve(v, k, mode="valid"), -1, pad)
    return np.moveaxis(out, -1, axis)

NS, NT = 330, 151  # stations, half-contour angles
SIG_X, SIG_T = 10.0, 1.5
N_HARM = 18  # cosine harmonics kept per section  # smoothing (uniform-x samples of 5 mm, angle steps of 1 deg)


def interp(x, xp, fp):
    return np.interp(x, xp, fp)


def bridge(x, xs, f, lo, hi):
    """Replace f over [lo, hi] with a smooth cubic Hermite bridge."""
    f = f.copy()
    m = (xs > lo) & (xs < hi)
    i0, i1 = np.argmax(xs > lo) - 1, len(xs) - np.argmax((xs < hi)[::-1])
    d0 = (f[i0] - f[i0 - 3]) / (xs[i0] - xs[i0 - 3])
    d1 = (f[i1 + 3] - f[i1]) / (xs[i1 + 3] - xs[i1])
    h = xs[i1] - xs[i0]
    t = (xs[m] - xs[i0]) / h
    h00, h10, h01, h11 = 2 * t**3 - 3 * t**2 + 1, t**3 - 2 * t**2 + t, -2 * t**3 + 3 * t**2, t**3 - t**2
    f[m] = h00 * f[i0] + h10 * h * d0 + h01 * f[i1] + h11 * h * d1
    return f


def main(bp_path, ai_path, toml_path, out_path):
    bp = json.loads(Path(bp_path).read_text())
    ai = json.loads(Path(ai_path).read_text())
    spec = tomllib.loads(Path(toml_path).read_text())
    wb = spec["chassis"]["wheelbase_m"]
    L = spec["chassis"]["length_m"]
    axle = np.array([-wb / 2, wb / 2])  # rear, front (blueprint overhangs are symmetric)

    # --- Silhouettes -----------------------------------------------------
    sx = np.array(bp["side"]["x"])
    zt_raw, zb_raw = np.array(bp["side"]["z_top"]), np.array(bp["side"]["z_bottom"])
    px, hw_raw = np.array(bp["plan"]["x"]), np.array(bp["plan"]["half_width"])
    # Wheels in the side bottom, mirrors in the plan width: bridge them.
    arch_r = 0.43
    zb_body = zb_raw.copy()
    for a in axle:
        zb_body = bridge(None, sx, zb_body, a - arch_r - 0.02, a + arch_r + 0.02)
    hw_body = bridge(None, px, hw_raw, 0.12, 0.50)

    # Stations: cosine clustering at both ends.
    u = np.linspace(0, 1, NS)
    x0, x1 = sx[0] + 0.0015, sx[-1] - 0.0015
    xs = x0 + (x1 - x0) * (0.5 - 0.5 * np.cos(np.pi * (0.06 + 0.88 * u)) - 0.5 + 0.5 * np.cos(np.pi * 0.06)) / (
        np.cos(np.pi * 0.06)
    )
    zt = interp(xs, sx, zt_raw)
    zb = interp(xs, sx, zb_body)
    hw = interp(xs, px, hw_body)

    # --- AI sections -> normalized radial functions -------------------------
    wx = ai["wheel_x"]
    k_x = wb / (wx[1] - wx[0])  # AI units -> metres
    mid = 0.5 * (wx[0] + wx[1])
    st = [s for s in ai["stations"] if s]
    ai_x = np.array([(s["x"] - mid) * k_x for s in st])
    theta = np.linspace(0, np.pi, NT)
    ai_h = (max(s["zmax"] for s in st) - ai["ground"]) / spec["chassis"]["height_m"]  # AI units per metre (z)
    # AI plan half-width with the mirrors bridged and smoothed: the width normaliser.
    ai_w = np.array([max((p[0] for p in s["pts"] if p), default=np.nan) for s in st])
    ai_w = bridge(None, ai_x, ai_w, 0.10, 0.55)
    ai_w = gaussian_filter1d(ai_w, 3.0, mode="nearest")
    R = np.full((len(st), NT), np.nan)
    for j, s in enumerate(st):
        pts = np.array([p if p else (np.nan, np.nan) for p in s["pts"]])
        ok = ~np.isnan(pts[:, 0])
        if ok.sum() < 20:
            continue
        xb = ai_x[j]
        top = np.nanmax(pts[:, 1])
        h_m = interp(xb, xs, zt - zb)
        bot = top - h_m * ai_h
        p = pts[ok]
        p = p[p[:, 1] >= bot]
        if len(p) < 10:
            continue
        yn, zn = np.minimum(p[:, 0] / ai_w[j], 1.0), (p[:, 1] - bot) / (top - bot)
        low = np.argmin(zn)
        yb = np.linspace(yn[low], 0, 25)[1:]
        yn = np.concatenate([yn[: low + 1], yb])
        zn = np.concatenate([zn[: low + 1], np.zeros_like(yb)])
        th = np.arctan2(yn, zn - 0.5)
        r = np.hypot(yn, zn - 0.5)
        th = np.maximum.accumulate(th)  # contour order is monotone in angle
        th, idx = np.unique(th, return_index=True)
        R[j] = np.interp(theta, th, r[idx], left=r[idx][0], right=r[idx][-1])

    keep = ~np.isnan(R).any(axis=1)
    ai_x, R = ai_x[keep], R[keep]

    def mask_rows(lo, hi, th_lo, th_hi):
        m = (ai_x > lo) & (ai_x < hi)
        i0, i1 = np.argmax(ai_x > lo) - 1, len(ai_x) - np.argmax((ai_x < hi)[::-1])
        t = (ai_x[m] - ai_x[i0]) / (ai_x[i1] - ai_x[i0])
        tm = (theta >= th_lo) & (theta <= th_hi)
        R[np.ix_(m, tm)] = (1 - t)[:, None] * R[i0, tm] + t[:, None] * R[i1, tm]

    # Mirrors only; wheel arches are cut later, the tyre hits lie inside the cut.
    mask_rows(0.05, 0.50, np.radians(15), np.radians(75))

    # Low-pass each section in angle: least-squares cosine series (even in
    # theta about the top and bottom centre lines). Keeps the fender crowns,
    # drops AI surface noise and baked-in panel gaps.
    basis = np.cos(np.outer(theta, np.arange(N_HARM)))
    coef = np.linalg.lstsq(basis, R.T, rcond=None)[0]
    R = (basis @ coef).T
    Rs = np.array([np.interp(xs, ai_x, R[:, k]) for k in range(NT)]).T
    xu = np.linspace(xs[0], xs[-1], 900)
    Ru = np.array([np.interp(xu, xs, Rs[:, k]) for k in range(NT)]).T
    Ru = gaussian_filter1d(Ru, sigma=SIG_X, axis=0, mode="nearest")
    Ru = gaussian_filter1d(Ru, sigma=SIG_T, axis=1, mode="nearest")
    Rs = np.array([np.interp(xs, xu, Ru[:, k]) for k in range(NT)]).T

    # Normalized contour -> metric.
    yn = Rs * np.sin(theta)[None, :]
    zn = 0.5 + Rs * np.cos(theta)[None, :]
    # Renormalize each station so silhouettes are hit exactly.
    yn /= yn.max(axis=1, keepdims=True)
    zmin, zmax = zn.min(axis=1, keepdims=True), zn.max(axis=1, keepdims=True)
    zn = (zn - zmin) / (zmax - zmin)
    Y = yn * hw[:, None]
    Z = zb[:, None] + zn * (zt - zb)[:, None]

    # Greenhouse width from the blueprint front (cabin ahead of the B pillar)
    # and rear views: scale y above the shoulder so that the widest point at
    # each height matches. Mirrors (front view 0.8-1.0 m) are bridged.
    for view, sel in (("front", xs > -0.55), ("rear", xs <= -0.55)):
        bz, bw = np.array(bp[view]["z"]), np.array(bp[view]["half_width"])
        o = np.argsort(bz)
        bz, bw = bz[o], bw[o]
        bw = bridge(None, bz, bw, 0.78, 1.0)
        zz = np.linspace(0.95, 1.29, 35)
        mw = np.array([np.max(np.where(np.abs(Z[sel] - z) < 0.01, Y[sel], 0)) for z in zz])
        f = np.clip(np.interp(zz, bz, bw) / np.maximum(mw, 1e-3), 0.85, 1.15)
        f = gaussian_filter1d(f, 2.0, mode="nearest")
        w = np.clip((Z[sel] - 0.92) / 0.06, 0, 1)  # blend in above the shoulder
        Y[sel] *= 1 + w * (np.interp(Z[sel], zz, f) - 1)
        print(view, "greenhouse width factors", np.round(f[::5], 3))
    X = np.repeat(xs[:, None], NT, axis=1)
    np.savez_compressed(out_path, X=X, Y=Y, Z=Z, xs=xs, theta=theta, zt=zt, zb=zb, hw=hw, axle=axle)
    print(f"{out_path}: {NS}x{NT} grid, length {xs[-1]-xs[0]:.3f} m (spec {L}), width {2*hw.max():.3f} m")


if __name__ == "__main__":
    main(*sys.argv[1:5])
