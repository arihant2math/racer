# QA triage (2026-10-27)

Six QA sessions ran against `main` at `4253183` (M2 merged): vehicle physics, race rules, AI
drivers, game flow/UI/input, tooling/data/build, and adversarial edge cases (property tests and
fuzzing). Their reports are next to this file, and every repro test is in the tree, marked
`#[ignore = "QA-…"]` so the suite stays green. List them with `cargo test --workspace -- --ignored`.

The reports filed 70 findings. After deduplication there are 60 distinct bugs: 1 critical, 19
major and 40 minor. They are grouped below by the code that owns them, so that the fixers
never edit the same files. A fixer that finds a bug is not real (or is working as intended)
says so in its fixes note instead of changing code.

## Duplicates

| Kept | Duplicate(s) |
|---|---|
| QA-RULES-1 | QA-EDGE-11 |
| QA-RULES-3 | QA-EDGE-2 |
| QA-PHYS-1 | QA-EDGE-4 |
| QA-PHYS-7 | QA-EDGE-3 |
| QA-UI-6 | QA-EDGE-15 |
| QA-TOOL-3 | QA-EDGE-5 |
| QA-TOOL-5 | QA-EDGE-6, QA-EDGE-7 |
| QA-TOOL-6, QA-TOOL-7 | QA-EDGE-14 |
| QA-TOOL-9 | QA-EDGE-16 |

## Groups

### fix/dynamics: vehicle model, car data loading, Avian vehicle plugin

Owns `crates/racer-dynamics`, `data/cars/`, `crates/racer-sim/src/vehicle.rs` and the M1 test
track/skidpad colliders in `crates/racer-sim/src/track.rs`.

| Bug | Sev | Summary |
|---|---|---|
| QA-PHYS-1 (+EDGE-4) | major | `Vehicle::from_spec` validates nothing; degenerate/NaN/inf values load, then panic or go NaN |
| QA-PHYS-2 | major | Empty `ratios`, `final_drive` or `torque_curve` panic on the first step |
| QA-PHYS-3 | major | A braked car that slid to rest creeps 1.43 m sideways over ~10 s |
| QA-PHYS-4 | major | ~25 % of wheel casts miss on the 4000 m grass box while parked; loads swing 0–16.6 kN |
| QA-PHYS-5 | major | Rolling onto an 8 cm step snaps the suspension 6 cm in one step (34 kN, 7.5× static) |
| QA-PHYS-6 | minor | `step` panics on `dt = 0`/NaN; negative `dt` produces forces |
| QA-PHYS-7 (+EDGE-3) | minor | NaN in `Controls` panics `step` |
| QA-PHYS-8 | minor | Engine can't be revved in neutral (holds ~3270 rpm) |
| QA-PHYS-9 | minor | Automatic gearbox stays in neutral after a shift-down at rest |

### fix/race-rules: race loop, progress, timing, standings, resets

Owns `crates/racer-sim/src/rules.rs`, `race.rs` and the reset monitor. Includes QA-AI-4,
because stuck detection lives in the reset monitor.

| Bug | Sev | Summary |
|---|---|---|
| QA-RULES-1 (+EDGE-11) | **critical** | A reset after reversing over a gate strands the car: it never laps again |
| QA-RULES-3 (+EDGE-2) | major | Rejoining the road > ~26 m further on freezes progress, with no reset |
| QA-RULES-4 | major | Driving a full lap backwards corrupts lap and sector times |
| QA-RULES-5 | major | Same-step finishers ordered by entity, not by who crossed first |
| QA-EDGE-1 | major | A progress update jumping two gates loses the lap for good (`Progress::update` credits one gate per call) |
| QA-AI-4 | major | A car pushing against a wall/car at 0–3 m/s isn't reset for 11–12 s |
| QA-RULES-2 | minor | Wrong-way timer has no cap; the warning outlives the wrong-way driving |
| QA-RULES-6 | minor | Cars reset in the same step land on the same spot, then turn solid inside each other |
| QA-RULES-7 | minor | After the winner is home, rocking over the line finishes a car a lap early |
| QA-RULES-8 | minor | `laps: 0` finishes everyone on the first running step |
| QA-EDGE-9 | minor | A second lap taken back restores the wrong lap start |
| QA-EDGE-10 | minor | A lap taken back keeps its best sectors and stays the delta reference |

### fix/ai: AI driver and racecraft

Owns `crates/racer-sim/src/ai.rs` (and, only if a fix needs it, the racing line/speed profile
code in `crates/racer-trackc/src/profile.rs` and the line solver; not `data.rs`/`geometry.rs`
validation, which belong to fix/tooling).

| Bug | Sev | Summary |
|---|---|---|
| QA-AI-2 | major | Inside passes at corner entry hit the inner wall (turn 1 ~369 m, ~946 m): the root cause of the first-lap pile-up resets |
| QA-AI-1 | major | AI waits behind a stopped car on an open road and gets reset instead of driving round it |
| QA-AI-3 | major | AI ignores ghosted cars; when one turns solid ahead it hits it at ~33 m/s |
| QA-AI-5 | minor | A finished car on its cool-down lap still defends against racing cars |
| QA-AI-6 | minor | A car right behind the leader counts as "alongside", so the leader slows 8 % |
| QA-AI-7 | minor | A car reversing towards the AI reads as moving forward (code reading; verify first) |

Exit check: the 8-car race reset rate from the M2 review (6.1 % of laps in traffic) should
drop clearly; report before/after with `racer-capture ai-races` on the same seeds.

### fix/game-ui: menus, HUD, pause, cameras, input

Owns `crates/racer-game`.

| Bug | Sev | Summary |
|---|---|---|
| QA-UI-3 | major | Force feedback keeps the last race force while paused |
| QA-UI-11 | major | `--features wheel` opens any gamepad as a wheel, with 50 % throttle and brake |
| QA-UI-13 | major | Setup and results screens don't fit the window (clipped even at 1280×720) |
| QA-UI-1 | minor | Esc/Backspace/pad East on the main menu quits at once |
| QA-UI-2 | minor | R, Q, E pressed while paused act on the car after resuming |
| QA-UI-4 | minor | Any gamepad trigger/stick input discards keyboard steering |
| QA-UI-5 | minor | Results hide steering assist, racing line and gearbox |
| QA-UI-6 (+EDGE-15) | minor | `lap_time` shows `0:60.000` instead of `1:00.000` |
| QA-UI-7 | minor | Chase camera flies across the city after a restart or reset |
| QA-UI-8 | minor | No gamepad button for reset or camera |
| QA-UI-9 | minor | Unplugging the gamepad mid-race doesn't pause |
| QA-UI-10 | minor | Stick deadzone applied twice (~12.6 % dead travel) |
| QA-UI-12 | minor | Pausing on the frame the results come up leaves the results paused |

### fix/tooling: track compiler, CLIs, server/backend, scripts, docs

Owns `crates/racer-trackc` except the line/profile solver, `crates/racer-capture` CLI parsing,
`crates/racer-server`, `crates/racer-backend`, `tools/`, `README.md` and CI.

| Bug | Sev | Summary |
|---|---|---|
| QA-TOOL-2 | major | Grid slots may overlap or sit ahead of the start line |
| QA-TOOL-3 (+EDGE-5) | major | Non-finite race layout values pass validation |
| QA-TOOL-12 | major | Data paths are baked in at compile time (`CARGO_MANIFEST_DIR`); binaries don't work outside the checkout |
| QA-TOOL-1 | minor | `curb_width` never validated |
| QA-TOOL-4 | minor | `compile` writes the track id unescaped into `summary.toml` |
| QA-TOOL-5 (+EDGE-6, EDGE-7) | minor | `validate` is quadratic with no size bound; tiny `checkpoint_spacing` or huge tracks/grids hang or run out of memory |
| QA-TOOL-6, QA-TOOL-7 (+EDGE-14) | minor | `racer-server` args: `--max-ticks 0` runs a tick; bad values panic; missing value runs forever |
| QA-TOOL-8 | minor | `ai-laps --car <unknown>` and `ai-races --races 0` pass |
| QA-TOOL-9 (+EDGE-16) | minor | `ai-races --first` overflow panics |
| QA-TOOL-10 | minor | `diff` accepts NaN and negative tolerances |
| QA-TOOL-11 | minor | `RACER_BACKEND_ADDR=localhost:8080` panics |
| QA-TOOL-13 | minor | `telemetry.sh`/`ai-check.sh` resolve `<out-dir>` against the repo root and ignore `CARGO_TARGET_DIR` |
| QA-TOOL-14 | minor | `setup.sh` accepts any `cargo` without the pinned toolchain |
| QA-TOOL-15 | minor | `racer-capture scene` validates its arguments late |
| QA-TOOL-16 | minor | Each `-p` build compiles its own Bevy (feature unification; build time) |
| QA-TOOL-17 | minor | `compile` leaves stale generated files behind |
| QA-EDGE-8 | minor | A short `drive` table in `summary.toml` panics `load_dir` |
| QA-EDGE-12 | minor | A banking change in the hairpin builds a 31° apron ramp |
| QA-EDGE-13 | minor | A road below the ground plane is accepted |

QA-TOOL-12 touches every crate that loads data. Keep the fix to one small shared path helper
and its call sites, so merges with the other groups stay trivial.
