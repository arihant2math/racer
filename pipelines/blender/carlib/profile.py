"""Parametric car body: per-station cross-sections lofted along the length.

Pure numpy (no bpy), so it can be tested and previewed outside Blender.

Body coordinates used here: s = distance from the nose (m, 0 at the front
bumper, growing rearwards), y = lateral (m, >= 0 for the half section), z = up
(m, 0 on the ground). build_car.py maps them to Blender space.

Each cross-section is a smooth curve through seven control points, bottom
centre to top centre:
    P0 floor centre, P1 floor corner, P2 rocker, P3 widest point,
    P4 shoulder (fender crest, or beltline along the cabin),
    P5 top edge (hood / roof / deck edge, A- and C-pillars),
    P6 top centre.
Every control value is a table over s in asset.toml ([body] section). Lateral
values are fractions of the half width w(s) from the blueprint top view, so the
sections shrink cleanly into the rounded nose and tail. Wheel arches clip the
section below the arch circle and add a wheel well (lip + inner wall).
"""

import json

import numpy as np


def pchip(xs, ys):
    """Monotone piecewise-cubic interpolant (no overshoot), clamped at the ends."""
    xs = np.asarray(xs, float)
    ys = np.asarray(ys, float)
    if len(xs) == 1:
        return lambda x: np.full_like(np.asarray(x, float), ys[0])
    h = np.diff(xs)
    d = np.diff(ys) / h
    m = np.zeros_like(ys)
    for k in range(1, len(xs) - 1):
        if d[k - 1] * d[k] > 0:
            w1, w2 = 2 * h[k] + h[k - 1], h[k] + 2 * h[k - 1]
            m[k] = (w1 + w2) / (w1 / d[k - 1] + w2 / d[k])
    m[0], m[-1] = d[0], d[-1]
    if len(xs) > 2:
        m[0] = 0.5 * d[0] if d[0] * d[1] > 0 else 0.0
        m[-1] = 0.5 * d[-1] if d[-1] * d[-2] > 0 else 0.0

    def f(x):
        x = np.clip(np.asarray(x, float), xs[0], xs[-1])
        k = np.clip(np.searchsorted(xs, x) - 1, 0, len(xs) - 2)
        t = (x - xs[k]) / h[k]
        h00 = (1 + 2 * t) * (1 - t) ** 2
        h10 = t * (1 - t) ** 2
        h01 = t * t * (3 - 2 * t)
        h11 = t * t * (t - 1)
        return h00 * ys[k] + h10 * h[k] * m[k] + h01 * ys[k + 1] + h11 * h[k] * m[k + 1]

    return f


def smooth(v, n):
    if n <= 1:
        return v
    k = np.ones(n) / n
    pad = np.pad(v, n, mode="edge")
    return np.convolve(pad, k, mode="same")[n:-n]


def gauss(x, v, sigma, keep=()):
    """Gaussian smoothing of samples v(x) (x increasing) with width sigma (m).

    Samples within 2 sigma of a position in `keep` are blended back toward the
    raw data so deliberate kinks (windscreen base) stay crisp.
    """
    x = np.asarray(x, float)
    v = np.asarray(v, float)
    out = np.empty_like(v)
    for i, xi in enumerate(x):
        w = np.exp(-0.5 * ((x - xi) / sigma) ** 2)
        out[i] = (w * v).sum() / w.sum()
    for k in keep:
        f = np.clip(np.abs(x - k) / (2 * sigma), 0, 1)
        out = out * f + v * (1 - f)
    # Keep the end samples (nose and tail faces) from shrinking.
    return out


def table(spec):
    """[[s, v], ...] -> callable."""
    a = np.asarray(spec, float)
    return pchip(a[:, 0], a[:, 1])


def catmull_rom(points, n_per_seg, alpha=0.5):
    """Centripetal Catmull-Rom through points[1:-1] (first/last are phantoms).

    Returns the dense polyline and the index of each interior control point.
    """
    p = np.asarray(points, float)
    out = []
    knots = []
    for i in range(1, len(p) - 2):
        p0, p1, p2, p3 = p[i - 1], p[i], p[i + 1], p[i + 2]
        t0 = 0.0
        t1 = t0 + max(np.linalg.norm(p1 - p0), 1e-6) ** alpha
        t2 = t1 + max(np.linalg.norm(p2 - p1), 1e-6) ** alpha
        t3 = t2 + max(np.linalg.norm(p3 - p2), 1e-6) ** alpha
        t = np.linspace(t1, t2, n_per_seg, endpoint=False)[:, None]
        a1 = (t1 - t) / (t1 - t0) * p0 + (t - t0) / (t1 - t0) * p1
        a2 = (t2 - t) / (t2 - t1) * p1 + (t - t1) / (t2 - t1) * p2
        a3 = (t3 - t) / (t3 - t2) * p2 + (t - t2) / (t3 - t2) * p3
        b1 = (t2 - t) / (t2 - t0) * a1 + (t - t0) / (t2 - t0) * a2
        b2 = (t3 - t) / (t3 - t1) * a2 + (t - t1) / (t3 - t1) * a3
        c = (t2 - t) / (t2 - t1) * b1 + (t - t1) / (t2 - t1) * b2
        knots.append(len(out))
        out.extend(c.tolist())
    knots.append(len(out))
    out.append(p[-2].tolist())
    return np.asarray(out), knots


def resample(poly, n):
    """n points evenly spaced by arc length along poly (n >= 2), endpoints kept."""
    if len(poly) < 2 or n < 2:
        return np.repeat(poly[:1], max(n, 1), axis=0)
    seg = np.linalg.norm(np.diff(poly, axis=0), axis=1)
    acc = np.concatenate([[0], np.cumsum(seg)])
    if acc[-1] < 1e-9:
        return np.repeat(poly[:1], n, axis=0)
    t = np.linspace(0, acc[-1], n)
    return np.stack([np.interp(t, acc, poly[:, 0]), np.interp(t, acc, poly[:, 1])], axis=1)


# Row bands of the half section, bottom centre to top centre. The names are
# used by build_car.py to classify faces (glass, panels, wheel wells).
BANDS = ("floor", "well", "lip", "lower", "upper", "side_top", "top")


class Body:
    def __init__(self, cfg, chassis, profiles):
        self.cfg = cfg
        b = cfg["body"]
        self.length = chassis["length_m"]
        self.half_width = chassis["width_m"] / 2
        self.height = chassis["height_m"]
        self.rows = b["rows"]  # per band
        self._blueprint(profiles, b)
        self.t = {k: table(v) for k, v in b["tables"].items()}
        self.arches = b["arches"]

    def _blueprint(self, prof, b):
        L = self.length
        side = prof["side"]
        xs = np.asarray(side["x_mm"]) / 1000
        xs = xs * L / xs.max()
        top = smooth(np.asarray(side["top_mm"]) / 1000, 3)
        top = top * self.height / top.max()
        bottom = np.asarray(side["bottom_mm"]) / 1000
        tp = prof["top"]
        xt = np.asarray(tp["x_mm"]) / 1000
        xt = xt * L / xt.max()
        hw = np.asarray(tp["half_width_px"]) * tp["mm_per_px"][0] / 1000
        # Patch ranges where the drawing shows something other than the body
        # (mirrors in the top view, watermark specks), then rescale to car.toml.
        for s0, s1 in b.get("patch_width", []):
            sel = (xt > s0) & (xt < s1)
            hw[sel] = np.interp(xt[sel], [s0, s1], np.interp([s0, s1], xt, hw))
        for s0, s1 in b.get("patch_top", []):
            sel = (xs > s0) & (xs < s1)
            top[sel] = np.interp(xs[sel], [s0, s1], np.interp([s0, s1], xs, top))
        # Hand-read ends of the side profile where the drawing's outline is a
        # one-pixel sliver (nose and tail faces).
        for spec in b.get("top_override", []):
            a = np.asarray(spec, float)
            sel = (xs >= a[0, 0]) & (xs <= a[-1, 0])
            top[sel] = pchip(a[:, 0], a[:, 1])(xs[sel])
        sg = b.get("smooth_sigma", 0.03)
        hw = gauss(xt, hw, b.get("smooth_sigma_width", sg))
        top = gauss(xs, top, sg, keep=b.get("smooth_keep", []))
        hw = hw * self.half_width / hw.max()
        self.bp_side_s, self.bp_top, self.bp_bottom = xs, top, bottom
        self.bp_top_s, self.bp_w = xt, hw

    def w(self, s):
        return np.interp(s, self.bp_top_s, self.bp_w)

    def side_top(self, s):
        return np.interp(s, self.bp_side_s, self.bp_top)

    def arch(self, s):
        """(z_lip, y_well, active) of the wheel arch at s."""
        for a in self.arches:
            ds = s - a["s"]
            if abs(ds) < a["radius"]:
                z = a["z"] + np.sqrt(a["radius"] ** 2 - ds * ds)
                return z, a["well_y"], True
        return -1.0, 0.0, False

    def controls(self, s):
        t = self.t
        w = max(float(self.w(s)), 1e-3)
        top = float(self.side_top(s))
        zb = float(t["z_floor"](s))
        dip = float(t["center_dip"](s))
        z6 = top - dip
        z5 = z6 - float(t["edge_drop"](s))
        z4 = min(float(t["z_shoulder"](s)), top)
        if dip > 0:
            # Fender crest is the silhouette top where the hood dips below it.
            z4 = max(z4, top - 0.002)
        z3 = min(float(t["z_widest"](s)), z4 - 0.03)
        z2 = min(float(t["z_rocker"](s)), z3 - 0.03)
        zb = min(zb, z2 - 0.02)
        y3 = w
        y2 = w * float(t["f_rocker"](s))
        y1 = y2 * 0.9
        y4 = w * float(t["f_shoulder"](s))
        y5 = w * float(t["f_edge"](s))
        return np.array([
            [0.0, zb], [y1, zb], [y2, z2], [y3, z3], [y4, z4], [y5, z5], [0.0, z6]])

    def section(self, s):
        """Half section at s: (points[N, 2], band index per point)."""
        P = self.controls(s)
        pts = np.vstack([[-P[1, 0], P[1, 1]], P, [-P[5, 0], P[5, 1]]])
        dense, knots = catmull_rom(pts, 120)
        # Keep the widest point exactly at w(s) and the top at the silhouette.
        ymax = dense[:, 0].max()
        if ymax > 1e-6:
            dense[:, 0] *= P[3, 0] / ymax
        r = self.rows
        z_lip, y_well, active = self.arch(s)
        i1, i2, i3, i4, i5 = knots[1], knots[2], knots[3], knots[4], knots[5]
        # Outer curve from the floor corner up to the shoulder.
        outer = dense[i1:i4 + 1]
        k3 = i3 - i1
        if active and z_lip > outer[0, 1]:
            # Clip below the arch: first point at or above z_lip on the rising side.
            k = int(np.argmax(outer[:, 1] >= z_lip)) if (outer[:, 1] >= z_lip).any() else len(outer) - 1
            if k > 0:
                a, b = outer[k - 1], outer[k]
                f = (z_lip - a[1]) / max(b[1] - a[1], 1e-9)
                start = a + f * (b - a)
                outer = np.vstack([start, outer[k:]])
                k3 = max(1, k3 - k + 1)
            # The arch removed the widest part of the section: widen what is
            # left so the fender lip still reaches the top-view half width.
            k = P[3, 0] / max(outer[:, 0].max(), 1e-6)
            outer[:, 0] *= k
            dense[i4:, 0] *= k
            y_well = min(y_well, outer[0, 0] - 0.01)
        else:
            z_lip = outer[0, 1]
            y_well = outer[0, 0]
        zb = P[0, 1]
        floor = resample(np.array([[0.0, zb], [y_well, zb]]), r["floor"])
        well = resample(np.array([[y_well, zb], [y_well, z_lip]]), r["well"] + 1)[1:]
        lip = resample(np.array([[y_well, z_lip], outer[0]]), r["lip"] + 1)[1:]
        # Split the outer curve at the widest point (P3) for the lower/upper bands.
        k3 = max(1, min(len(outer) - 2, k3))
        lower = resample(outer[:k3 + 1], r["lower"] + 1)[1:]
        upper = resample(outer[k3:], r["upper"] + 1)[1:]
        side_top = resample(dense[i4:i5 + 1], r["side_top"] + 1)[1:]
        topc = resample(dense[i5:], r["top"] + 1)[1:]
        parts = [floor, well, lip, lower, upper, side_top, topc]
        bands = np.concatenate([np.full(len(p), i) for i, p in enumerate(parts)])
        return np.vstack(parts), bands

    def stations(self):
        """Station positions along s: denser at the nose and tail, with the
        configured key stations (panel edges) snapped in exactly."""
        n = self.cfg["body"]["stations"]
        t = np.linspace(0, 1, n)
        s = self.length * (0.6 * t + 0.4 * (1 - np.cos(np.pi * t)) / 2)
        for k in self.cfg["body"].get("key_stations", []):
            i = int(np.argmin(abs(s - k)))
            if 0 < i < n - 1:
                s[i] = k
        return np.sort(s)

    def grid(self):
        """Full grid: stations[S], points[S, N, 3] (s, y, z) half body, bands[N]."""
        st = self.stations()
        secs = []
        bands = None
        for s in st:
            p, b = self.section(s)
            secs.append(np.column_stack([np.full(len(p), s), p]))
            bands = b
        g = np.asarray(secs)
        # Fair the loft along the length: the section construction is per
        # station, so tiny station-to-station differences show as ripples in
        # reflections. Rows above the widest point are smoothed over stations
        # (the wheel-arch rows below are left alone).
        sig = self.cfg["body"].get("fair_sigma_stations", 0.0)
        if sig > 0:
            rows = np.nonzero(bands >= self.cfg["body"].get("fair_min_band", 4))[0]
            n = len(st)
            k = np.arange(-int(3 * sig), int(3 * sig) + 1)
            w = np.exp(-0.5 * (k / sig) ** 2)
            for _ in range(self.cfg["body"].get("fair_passes", 1)):
                sm = g.copy()
                for i in range(2, n - 2):
                    j = np.clip(i + k, 0, n - 1)
                    ww = w * (np.abs(st[j] - st[i]) < 0.2)
                    sm[i, rows, 1:] = (ww[:, None, None] * g[j][:, rows, 1:]).sum(0) / ww.sum()
                g = sm
        # Close the nose and tail: the end stations collapse onto the centre plane.
        g[0, :, 1] = 0.0
        g[-1, :, 1] = 0.0
        return st, g, bands

    def surface_z(self, s, y):
        """Height of the top surface at (s, |y|) from the section (top bands)."""
        p, b = self.section(s)
        top = p[b >= 4]
        order = np.argsort(top[:, 0])
        return float(np.interp(abs(y), top[order, 0], top[order, 1]))


def load(cfg, chassis, profiles_path):
    with open(profiles_path) as f:
        prof = json.load(f)
    return Body(cfg, chassis, prof)
