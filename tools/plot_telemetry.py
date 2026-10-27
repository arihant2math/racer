#!/usr/bin/env python3
"""Plot racer-capture telemetry CSVs to PNG, one figure per CSV.

    tools/plot_telemetry.py <csv>... [--out DIR]

Panels share the time axis: speed, engine rpm and gear, longitudinal and
lateral g, driver inputs, wheel loads, slip angles and slip ratios. The last
panel depends on the maneuver: a speed-coloured track map for laps, and for
the skidpad the handling balance (front minus rear slip angle against
lateral g: rising = understeer, falling = oversteer).
"""

import argparse
import csv
import os
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

# Categorical slots 1-4 of the validated default palette (dataviz skill).
WHEEL_COLORS = {"fl": "#2a78d6", "fr": "#eb6834", "rl": "#1baf7a", "rr": "#eda100"}
INK, MUTED, GRID, SURFACE = "#0b0b0b", "#52514e", "#e4e3df", "#fcfcfb"


def load(path):
    with open(path, newline="") as f:
        rows = list(csv.DictReader(f))
    if not rows:
        sys.exit(f"{path}: no samples")
    return {k: [float(r[k]) for r in rows] for k in rows[0]}


def style(ax, title, unit):
    ax.set_title(title, loc="left", fontsize=10, color=INK)
    ax.set_ylabel(unit, fontsize=8, color=MUTED)
    ax.tick_params(labelsize=8, colors=MUTED, length=0)
    ax.grid(True, color=GRID, linewidth=0.8)
    for s in ax.spines.values():
        s.set_visible(False)
    ax.set_facecolor(SURFACE)


def series(ax, t, d, cols, labels, colors):
    for c, label, color in zip(cols, labels, colors):
        ax.plot(t, d[c], color=color, linewidth=1.4, label=label)
    ax.legend(fontsize=7, frameon=False, ncol=len(cols), loc="upper right")


def wheels(ax, t, d, prefix):
    series(ax, t, d, [f"{prefix}_{w}" for w in WHEEL_COLORS], [w.upper() for w in WHEEL_COLORS], WHEEL_COLORS.values())


def plot(path, out_dir):
    d = load(path)
    t = d["t"]
    name = os.path.splitext(os.path.basename(path))[0]
    fig, axes = plt.subplots(5, 2, figsize=(14, 15), facecolor=SURFACE)
    ax = axes.flat
    series(ax[0], t, d, ["speed_kmh"], ["speed"], ["#2a78d6"])
    style(ax[0], "Speed", "km/h")
    series(ax[1], t, d, ["rpm"], ["rpm"], ["#2a78d6"])
    style(ax[1], "Engine speed", "rpm")
    ax[2].step(t, d["gear"], where="post", color="#2a78d6", linewidth=1.4)
    style(ax[2], "Gear", "gear")
    series(ax[3], t, d, ["long_g", "lat_g"], ["longitudinal", "lateral (+ right)"], ["#2a78d6", "#eb6834"])
    style(ax[3], "Acceleration", "g")
    series(ax[4], t, d, ["throttle", "brake", "steer"], ["throttle", "brake", "steer (+ right)"], ["#1baf7a", "#e34948", "#6250d6"])
    style(ax[4], "Driver inputs", "fraction")
    series(ax[5], t, d, ["yaw_rate", "sideslip_deg"], ["yaw rate (rad/s, + left)", "sideslip (deg)"], ["#2a78d6", "#eb6834"])
    style(ax[5], "Yaw rate and body sideslip", "")
    wheels(ax[6], t, d, "load")
    style(ax[6], "Wheel loads", "N")
    wheels(ax[7], t, d, "slip_angle")
    style(ax[7], "Slip angles", "deg")
    wheels(ax[8], t, d, "slip_ratio")
    style(ax[8], "Slip ratios", "")
    last = ax[9]
    if name.endswith("-skidpad"):
        front = [(a + b) / 2 for a, b in zip(d["slip_angle_fl"], d["slip_angle_fr"])]
        rear = [(a + b) / 2 for a, b in zip(d["slip_angle_rl"], d["slip_angle_rr"])]
        # Skip the first 5 s: pulling onto the circle is not steady state.
        steady = [i for i, x in enumerate(t) if x > 5.0]
        balance = [abs(front[i]) - abs(rear[i]) for i in steady]
        lat = [abs(d["lat_g"][i]) for i in steady]
        last.scatter(lat, balance, s=8, color="#2a78d6")
        style(last, "Balance: |front| - |rear| slip angle vs lateral g (> 0 understeer)", "deg")
        last.set_xlabel("lateral g", fontsize=8, color=MUTED)
    else:
        sc = last.scatter(d["x"], d["z"], c=d["speed_kmh"], cmap="Blues", s=6)
        last.set_aspect("equal", adjustable="datalim")
        last.invert_yaxis()
        fig.colorbar(sc, ax=last, label="km/h")
        style(last, "Path (x, z), coloured by speed", "z (m)")
    for a in axes[-1]:
        if not a.get_xlabel():
            a.set_xlabel("time (s)" if a is not last else "x (m)", fontsize=8, color=MUTED)
    fig.suptitle(name, fontsize=13, color=INK, x=0.01, ha="left")
    fig.tight_layout()
    out = os.path.join(out_dir or os.path.dirname(path), name + ".png")
    fig.savefig(out, dpi=80)
    plt.close(fig)
    print(f"wrote {out}")


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument("csv", nargs="+")
    p.add_argument("--out", help="output directory (default: next to each CSV)")
    args = p.parse_args()
    if args.out:
        os.makedirs(args.out, exist_ok=True)
    for path in args.csv:
        plot(path, args.out)


if __name__ == "__main__":
    main()
