# Racer

A multiplayer street racing game built in Rust on Bevy 0.19.1. See [`PLAN.md`](PLAN.md) for
the full plan and [`docs/milestones/`](docs/milestones/) for milestone handoff notes.

## Layout

| Crate | Purpose |
|---|---|
| `racer-game` | Game client: greybox driving scene (test track, skidpad), input, cameras, HUD. |
| `racer-sim` | Gameplay shared by client and server (fixed 240 Hz step): Avian vehicle plugin, track, test autopilot. |
| `racer-dynamics` | Vehicle physics core (tires, suspension, drivetrain, aero, assists), pure Rust, no Bevy. |
| `racer-render` | Custom materials and rendering (greybox car paint for now). |
| `racer-server` | Headless dedicated server (Bevy `MinimalPlugins`, no renderer). |
| `racer-backend` | axum backend service (`GET /health`). |
| `racer-trackc` | City/track compiler (CLI skeleton). |
| `racer-capture` | Headless screenshot and telemetry harness, image checks and golden diffs. |
| `racer-citykit` | Procedural track-side asset kit (roads, racing and street furniture, buildings, textures, validator) and the `downtown-kit` showcase; see [`docs/assets/track-kit.md`](docs/assets/track-kit.md). |

Car data lives in `data/cars/<id>/car.toml`: published specs with sources, estimated
parameters under `[estimated]` and the reference performance figures the tests check.

## Setup

On Ubuntu 24.04 (a fresh container or a GitHub Actions runner), one non-interactive,
idempotent command installs the Rust toolchain, Bevy's build dependencies and the
software rendering stack (Mesa lavapipe + Xvfb):

```sh
tools/setup.sh                  # add --with-blender for headless Blender in .tools/blender
```

On Windows and macOS, install Rust with [rustup](https://rustup.rs); nothing else is needed
to build. The toolchain version is pinned in `rust-toolchain.toml`.

## Build, run, test

```sh
cargo build --workspace
cargo run -p racer-game         # driving scene (see controls below)
cargo run -p racer-game -- drive --car 3 --skidpad   # 911 on the skidpad
cargo run -p racer-game -- greybox                   # M0 scene, free camera
cargo run -p racer-game --features wheel             # + SDL3 steering wheel/FFB (needs cmake)
cargo run -p racer-server -- --max-ticks 2400   # headless server for 10 s of simulation
cargo run -p racer-backend      # http://127.0.0.1:8080/health (RACER_BACKEND_ADDR to change)
cargo test --workspace
cargo test -p racer-dynamics    # reference maneuvers and fuzz tests for every car, ~3 s
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
```

Driving controls: WASD or arrows, Space handbrake, Q/E shift down/up (automatic by default),
C clutch, V chase/hood camera, 1/2/3 Golf GTI/M3/911, T track, K skidpad, R reset. Gamepad:
left stick, right/left triggers, south button handbrake, bumpers shift.

## Telemetry

```sh
tools/telemetry.sh captures/telemetry          # all cars; or append car ids
```

Drives each car through scripted maneuvers on the Avian test track headlessly (a lap, a
skidpad speed ramp, a launch and ABS stop), writes one CSV per car and maneuver and plots them
to PNG with `tools/plot_telemetry.py` (speed, rpm, gear, g, inputs, yaw and sideslip, wheel
loads, slip angles and ratios, plus a track map or the skidpad balance curve).

## Headless captures

```sh
tools/capture.sh greybox captures/greybox
tools/capture.sh driving captures/driving
```

This builds `racer-capture`, runs the scene under `xvfb-run` on lavapipe, waits until the
render pipeline cache has settled (shader compilation on llvmpipe takes tens of seconds),
saves one PNG per scripted camera pose and rejects blank or single-colour frames. It exits
non-zero on failure or timeout (`--timeout SECS`, default 600; `--size WxH`, default 1280x720).

Compare against the committed goldens, or check arbitrary PNGs:

```sh
target/debug/racer-capture diff captures/greybox crates/racer-capture/goldens/greybox
target/debug/racer-capture diff captures/driving crates/racer-capture/goldens/driving
target/debug/racer-capture check captures/greybox/*.png
```

`diff` accepts two files or two directories and the tolerances `--pixel N` (per-channel
difference that counts a pixel as changed, default 24), `--max-fraction F` (allowed fraction
of changed pixels, default 0.01) and `--max-mean E` (allowed mean absolute error, default 2.0).
To update goldens after an intended visual change, copy the new captures over
`crates/racer-capture/goldens/<scene>/` and look at them before committing.

## CI

`.github/workflows/ci.yml` runs fmt, clippy (`-D warnings`) and tests on Linux, Windows and
macOS, plus a Linux job that runs `tools/setup.sh`, captures the greybox and driving scenes,
diffs them against the goldens, runs the telemetry and uploads everything as the `captures`
artifact.
