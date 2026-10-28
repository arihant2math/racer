# Racer

A multiplayer street racing game built in Rust on Bevy 0.19.1. See [`PLAN.md`](PLAN.md) for
the full plan and [`docs/milestones/`](docs/milestones/) for milestone handoff notes.

## Layout

| Crate | Purpose |
|---|---|
| `racer-game` | Game client: menus, race setup, race scene with HUD and results, greybox driving scene (test track, skidpad), input, cameras. |
| `racer-sim` | Gameplay shared by client and server (fixed 240 Hz step): Avian vehicle plugin, tracks, race rules, AI drivers, test autopilot. |
| `racer-dynamics` | Vehicle physics core (tires, suspension, drivetrain, aero, assists), pure Rust, no Bevy. |
| `racer-render` | Custom materials and rendering (greybox car paint for now). |
| `racer-server` | Headless dedicated server (Bevy `MinimalPlugins`, no renderer). |
| `racer-backend` | axum backend service (`GET /health`). |
| `racer-trackc` | Track compiler (library + CLI): validates `track.toml`, builds road/curb/apron/wall geometry, generates the racing line and per-car speed profiles. |
| `racer-capture` | Headless screenshot and telemetry harness, image checks and golden diffs. |

Car data lives in `data/cars/<id>/car.toml`: published specs with sources, estimated
parameters under `[estimated]` and the reference performance figures the tests check.
Tracks live in `data/tracks/<id>/track.toml` (route nodes with width, elevation, banking,
aprons and curbs; start, grid, sectors, checkpoints, sprint finish) with the generated
racing line, car models and speed profiles in `data/tracks/<id>/generated/`.

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
cargo run -p racer-game         # main menu: race setup, races against the AI, results
cargo run -p racer-game -- race --car 2 --ai 7 --difficulty pro --laps 3   # straight into a race
cargo run -p racer-game -- race --autopilot          # ... with the AI driving your car
cargo run -p racer-game -- drive                     # M1 driving scene (see controls below)
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
C clutch, V chase/hood camera, R reset (in a race: back to the last valid track position).
In a race, Esc (gamepad Start) pauses. Driving scene only: 1/2/3 Golf GTI/M3/911, T track,
K skidpad. Gamepad: left stick, right/left triggers, south button handbrake, bumpers shift,
north button reset, select/back button chase/hood camera; unplugging the pad pauses a race.
Menus: arrows or d-pad to select and change, Enter/Space or south button to confirm, Esc or
east button to go back (the main menu only quits from its Quit item); mouse clicks work too.

## Tracks

```sh
cargo run -p racer-trackc -- validate data/tracks/downtown-loop   # checks the data and geometry
cargo run -p racer-trackc -- compile data/tracks/downtown-loop    # rewrites generated/
```

`compile` generates the minimum-curvature racing line and measures each car's point-mass
model on the racer-dynamics testbed (grip, braking, drive per speed), then writes the line,
the models and the speed profiles. `cargo test -p racer-trackc` fails if the generated files
no longer match the track or the cars.

## AI verification

```sh
tools/ai-check.sh captures/ai              # 100 solo laps per car and difficulty, 12 8-car races
tools/ai-check.sh captures/ai 5 4 5        # quick: 5 solo laps, 4 races of 5 laps
```

Runs headless AI laps and races on the city track in a release build (all cores), counts
laps, resets and stalls, checks the lap-time spread between difficulties against the
optimal-line time, writes `solo.md` and `mixed.md` reports and plots the AI against the
optimal speed profile with `tools/plot_race.py`. The underlying commands are
`racer-capture ai-laps <out> [--laps N] [--car ID]` and
`racer-capture ai-races <out> [--races N] [--laps N]`.

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
tools/capture.sh menu captures/menu                              # main menu, race setup
CAPTURE_PROFILE=release tools/capture.sh race captures/race --timeout 2400
                                                                 # grid, pack, HUD, results
```

The race scene runs an 8-car, one-lap race with the player's car on autopilot and
fast-forwards (one second of race time per rendered frame) to each capture.

This builds `racer-capture`, runs the scene under `xvfb-run` on lavapipe, waits until the
render pipeline cache has settled (shader compilation on llvmpipe takes tens of seconds),
saves one PNG per scripted camera pose and rejects blank or single-colour frames. It exits
non-zero on failure or timeout (`--timeout SECS`, default 600; `--size WxH`, default 1280x720).

Compare against the committed goldens, or check arbitrary PNGs:

```sh
target/debug/racer-capture diff captures/greybox crates/racer-capture/goldens/greybox
target/debug/racer-capture diff captures/driving crates/racer-capture/goldens/driving
target/debug/racer-capture diff captures/menu crates/racer-capture/goldens/menu
target/release/racer-capture diff captures/race crates/racer-capture/goldens/race
target/debug/racer-capture check captures/greybox/*.png
```

`diff` accepts two files or two directories and the tolerances `--pixel N` (per-channel
difference that counts a pixel as changed, default 24), `--max-fraction F` (allowed fraction
of changed pixels, default 0.01) and `--max-mean E` (allowed mean absolute error, default 2.0).
To update goldens after an intended visual change, copy the new captures over
`crates/racer-capture/goldens/<scene>/` and look at them before committing.

## CI

`.github/workflows/ci.yml` runs fmt, clippy (`-D warnings`) and tests on Linux, Windows and
macOS, plus a Linux job that runs `tools/setup.sh`, validates the tracks, captures the
greybox, driving, menu and race scenes, diffs them against the goldens, runs the telemetry
and a short AI check, and uploads everything as the `captures` artifact.
