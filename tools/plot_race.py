#!/usr/bin/env python3
"""Plot AI laps against the generated optimal speed profile.

    tools/plot_race.py <ai-laps-out-dir> [track-id]

Reads the fastest-lap traces written by `racer-capture ai-laps`
(trace-<car>-<difficulty>.csv) and the track's generated racing line and
speed profiles, and writes into the same directory:

- speed-<car>.png: speed against distance along the line for every
  difficulty on top of the car's optimal profile, plus the speed deficit
  of the Alien against the profile;
- line.png: the racing line on the track map, coloured by the GTI's
  optimal speed.
"""

import csv
import glob
import os
import sys

import matplotlib

matplotlib.use("Agg")
import matplotlib.pyplot as plt  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
DIFFICULTIES = ["novice", "amateur", "pro", "expert", "alien"]
COLOURS = {"novice": "#4daf4a", "amateur": "#984ea3", "pro": "#ff7f00", "expert": "#377eb8", "alien": "#e41a1c"}


def read(path):
    with open(path) as f:
        return list(csv.DictReader(f))


def column(rows, key, scale=1.0):
    return [float(r[key]) * scale for r in rows]


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(2)
    out = sys.argv[1]
    track = sys.argv[2] if len(sys.argv) > 2 else "downtown-loop"
    generated = os.path.join(ROOT, "data", "tracks", track, "generated")
    line = read(os.path.join(generated, "racing-line.csv"))
    cars = sorted({os.path.basename(p)[6:].rsplit("-", 1)[0] for p in glob.glob(os.path.join(out, "trace-*.csv"))})
    for car in cars:
        profile = read(os.path.join(generated, f"speed-{car}.csv"))
        px, pv = column(profile, "distance"), column(profile, "speed", 3.6)
        fig, (ax, deficit) = plt.subplots(2, 1, figsize=(16, 9), sharex=True, gridspec_kw={"height_ratios": [3, 1]})
        ax.plot(px, pv, color="black", lw=1.6, label="optimal profile")
        for d in DIFFICULTIES:
            path = os.path.join(out, f"trace-{car}-{d}.csv")
            if not os.path.exists(path):
                continue
            t = [r for r in read(path)]
            # A lap's trace can wrap past the line; keep it in order.
            t.sort(key=lambda r: float(r["distance"]))
            ax.plot(column(t, "distance"), column(t, "speed", 3.6), color=COLOURS[d], lw=0.9, label=d.capitalize())
            if d == "alien":
                # Deficit against the profile at the same distance.
                j, diff = 0, []
                for r in t:
                    x = float(r["distance"])
                    while j + 1 < len(px) and px[j + 1] < x:
                        j += 1
                    diff.append(float(r["speed"]) * 3.6 - pv[j])
                deficit.plot(column(t, "distance"), diff, color=COLOURS[d], lw=0.8)
        ax.set_ylabel("speed (km/h)")
        ax.set_title(f"{car}: fastest flying lap per difficulty against the optimal-line profile")
        ax.legend(loc="lower right", ncol=6)
        ax.grid(alpha=0.3)
        deficit.axhline(0, color="black", lw=0.8)
        deficit.set_ylabel("Alien - optimal (km/h)")
        deficit.set_xlabel("distance along the racing line (m)")
        deficit.grid(alpha=0.3)
        fig.tight_layout()
        fig.savefig(os.path.join(out, f"speed-{car}.png"), dpi=80)
        plt.close(fig)
        print(f"wrote {os.path.join(out, f'speed-{car}.png')}")

    gti = read(os.path.join(generated, "speed-vw-golf-gti-mk8.csv"))
    fig, ax = plt.subplots(figsize=(9, 10))
    sc = ax.scatter(column(line, "x"), column(line, "z"), c=column(gti, "speed", 3.6), s=3, cmap="viridis")
    ax.plot([float(line[0]["x"])], [float(line[0]["z"])], "r^", ms=10, label="start line")
    ax.invert_yaxis()
    ax.set_aspect("equal")
    ax.set_xlabel("x (m, east)")
    ax.set_ylabel("z (m, south)")
    ax.set_title(f"{track}: racing line, coloured by the GTI's optimal speed (km/h)")
    ax.legend()
    fig.colorbar(sc, ax=ax, shrink=0.7)
    fig.tight_layout()
    fig.savefig(os.path.join(out, "line.png"), dpi=80)
    print(f"wrote {os.path.join(out, 'line.png')}")


if __name__ == "__main__":
    main()
