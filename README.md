# Racer

A multiplayer street racing game built in Rust on Bevy 0.19.1. See [`PLAN.md`](PLAN.md) for
the full plan and [`docs/milestones/`](docs/milestones/) for milestone handoff notes.

## Layout

| Crate | Purpose |
|---|---|
| `racer-game` | Game client: menus, race setup, race scene with HUD and results, online play (quick match, lobbies, direct connect), greybox driving scene (test track, skidpad), input, cameras. |
| `racer-sim` | Gameplay shared by client and server (fixed 240 Hz step): Avian vehicle plugin, tracks, race rules, AI drivers, test autopilot, the net protocol and the online client (prediction, rollback, remote-car extrapolation). |
| `racer-dynamics` | Vehicle physics core (tires, suspension, drivetrain, aero, assists), pure Rust, no Bevy. |
| `racer-render` | Custom materials and rendering (greybox car paint for now). |
| `racer-server` | Headless dedicated race server: lobby, countdown, race, results, exit; authoritative physics and rules at 240 Hz, 60 Hz snapshots, AI fill. |
| `racer-backend` | axum backend: device-key identity, invite-code lobbies, quick match, server allocation (`Fleet`, local process spawner). |
| `racer-trackc` | Track compiler (library + CLI): validates `track.toml`, builds road/curb/apron/wall geometry, generates the racing line and per-car speed profiles. |
| `racer-capture` | Headless screenshot and telemetry harness, image checks and golden diffs. |

Car data lives in `data/cars/<id>/car.toml`: published specs with sources, estimated
parameters under `[estimated]` and the reference performance figures the tests check.
Tracks live in `data/tracks/<id>/track.toml` (route nodes with width, elevation, banking,
aprons and curbs; start, grid, sectors, checkpoints, sprint finish) with the generated
racing line, car models and speed profiles in `data/tracks/<id>/generated/`.

Binaries find `data/` at run time, in this order: the `RACER_DATA_DIR` environment variable
(the directory that holds `cars/` and `tracks/`), a `data/` directory next to the executable
(as in a release archive), and finally the checkout the binary was built from (so `cargo run`
and `cargo test` need nothing set). To run a built binary elsewhere, copy `data/` next to it
or point `RACER_DATA_DIR` at it:

```sh
cp -r target/release/racer-game data /opt/racer/        # /opt/racer/racer-game finds /opt/racer/data
RACER_DATA_DIR=/path/to/racer/data /somewhere/racer-trackc compile /path/to/my-track
```

Built car models (`assets/`, from `tools/build_car_992.sh`; not committed) are found the same
way: `RACER_ASSETS_DIR`, then `assets/` next to the executable, then the checkout. Without
them `racer-game` and `racer-capture` draw the greybox car bodies.

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
cargo run -p racer-server -- --max-ticks 2400   # headless server, 10 s of an AI-only race
cargo run -p racer-backend      # http://127.0.0.1:8080/health (see "Online play")
RACER_BACKEND_ADDR=localhost:9000 cargo run -p racer-backend   # any host:port (names resolved)
cargo test --workspace
cargo test -p racer-dynamics    # reference maneuvers and fuzz tests for every car, ~3 s
cargo fmt --all --check && cargo clippy --workspace --all-targets -- -D warnings
```

`cargo build --workspace`, `cargo build -p racer-game` and `cargo build -p racer-capture` (as
`tools/capture.sh` runs) resolve the same features, so they share one compiled Bevy.
`racer-server` and `racer-sim` on their own build a smaller, headless Bevy (no renderer,
windowing or audio), which is compiled separately the first time.

Driving controls: WASD or arrows, Space handbrake, Q/E shift down/up (automatic by default),
C clutch, V chase/hood camera, hold B to look back, R reset (in a race: back to the last valid
track position). Reverse: in the automatic, hold the brake at a standstill (0.6 s), then the brake
pedal drives backwards and the throttle brakes and goes back to 1st; in the manual, shift down
from neutral at a standstill (see `docs/features/reverse.md`).
In a race, Esc (gamepad Start) pauses. Driving scene only: 1/2/3 Golf GTI/M3/911, T track,
K skidpad. Gamepad: left stick, right/left triggers, south button handbrake, bumpers shift,
north button reset, select/back button chase/hood camera, right stick button look back;
unplugging the pad pauses a race.
Menus: arrows or d-pad to select and change, Enter/Space or south button to confirm, Esc or
east button to go back (the main menu only quits from its Quit item); mouse clicks work too.

## Online play

Races online run on a dedicated `racer-server` (one process per race). Clients send their
inputs; the server simulates every car and sends snapshots; each client predicts its own car and
extrapolates the others (see [`docs/milestones/M3.md`](docs/milestones/M3.md)).

With the backend (lobbies and quick match):

```sh
cargo build -p racer-server -p racer-backend    # the backend starts racer-server from its own directory
cargo run -p racer-backend                      # http://127.0.0.1:8080
RACER_NAME=Ana cargo run -p racer-game          # Quick match, Create lobby, or Join lobby with a code
```

The host of a lobby picks mode, laps, collisions and AI fill and starts the race once the others
are ready; a quick match starts by itself after 15 s (or when 8 players are in), with AI cars in
the empty slots. Backend settings: `RACER_BACKEND_ADDR` (listen address), `RACER_SERVER_BIN`
(server binary, default: next to the backend), `RACER_FLEET_BIND` / `RACER_FLEET_PUBLIC` (the
address servers listen on and the one clients are told, default `127.0.0.1`), and
`RACER_BACKEND_STATE` (a JSON file that keeps the players across restarts). Game settings:
`RACER_BACKEND` (default `http://127.0.0.1:8080`), `RACER_NAME`, and `RACER_CONFIG_DIR` (where
the device key is kept; default the platform's config directory, e.g. `~/.config/racer`).

Without the backend (development): start a server and use Direct connect in the menu.

```sh
cargo run -p racer-server -- --bind 0.0.0.0:7777 --players 2 --laps 3   # waits for 2 players (60 s)
cargo run -p racer-game                          # Direct connect, type 127.0.0.1:7777
```

`racer-server --help` lists the options (mode, laps, collisions, grid size, AI fill and difficulty,
lobby timeout, simulated network conditions, and a JSON report at the end).

## Net check

```sh
tools/net-check.sh captures/net          # 8 bots, 2 races of 3 laps at 0, 150 and 250 ms RTT (~35 min)
tools/net-check.sh captures/net 1 1      # quick: 1 race of 1 lap per condition
```

Starts one `racer-server` and 8 headless bot clients (the AI drives each client's car through its
controls, as a player would) over UDP on localhost with lightyear's link conditioner, runs whole
races and checks: the race completes and every client agrees with the server on finishing order,
laps and times; prediction corrections and desyncs stay under the limits; and every car-to-car
contact on the server was predicted by both cars' clients. It writes `net.md` per condition. It
first runs the backend path once: `racer-capture net-lobby` starts `racer-backend`, creates and
joins a lobby with two device keys, starts it (the local fleet starts a server) and races two bots
on it for a lap. The underlying commands are `racer-capture net-race <out> [--bots N] [--races N] [--laps N]
[--conditions RTT_MS,JITTER_MS,LOSS_PERCENT]` and `racer-capture net-cost` (CPU cost of a
simulation tick and of a rollback).

## Tracks

```sh
cargo run -p racer-trackc -- validate data/tracks/downtown-loop   # checks the data and geometry
cargo run -p racer-trackc -- compile data/tracks/downtown-loop    # rewrites generated/
```

`compile` generates the minimum-curvature racing line and measures each car's point-mass
model on the racer-dynamics testbed (grip, braking, drive per speed), then writes the line,
the models and the speed profiles. `cargo test -p racer-trackc` fails if the generated files
no longer match the track or the cars. `compile` also removes speed profiles of cars it no
longer compiles. Validation bounds what it will work on (at most 500 nodes, coordinates within
50 km of the origin, a 25 km lap, 16 grid slots, 16 sectors) and checks every value before
building anything, so malformed data is always an error, never a hang or a crash.

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
`racer-capture ai-races <out> [--races N] [--laps N] [--first I]` (`--first` is the index of
the first race, which picks its grid, cars and difficulties; races `I..I+N` are run).

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
tools/capture.sh lobby captures/lobby                            # join-code entry, lobby (host)
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
target/debug/racer-capture diff captures/lobby crates/racer-capture/goldens/lobby
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
greybox, driving, menu, lobby and race scenes, diffs them against the goldens, runs the telemetry,
a short AI check and a short net check, and uploads everything as the `captures` artifact.
