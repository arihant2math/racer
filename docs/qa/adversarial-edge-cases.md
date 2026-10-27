# QA: adversarial edge cases (property-based testing and fuzzing)

Branch `qa/adversarial-edge-cases`, on top of the M2 review (`4253183`). Date 2026-10-27.
Scope: the whole codebase, approached with generated inputs. I wrote properties and
invariants, let proptest look for counterexamples and shrink them, and turned risky numeric
sites into targeted tests. I did not change any product code.

## How to run

`proptest = "=1.12.0"` is a dev-dependency of `racer-dynamics`, `racer-trackc`, `racer-sim`
and `racer-game`. It is pinned in the workspace with `default-features = false,
features = ["std"]`, so there is no fork or timeout machinery and no regression files are
written (`failure_persistence: None`).

| File | What it covers |
|---|---|
| `crates/racer-dynamics/tests/properties.rs` | `Vehicle::step` and the testbed: finiteness, the friction ellipse, determinism, mirror symmetry, energy, dt splitting, replay, car specs |
| `crates/racer-trackc/tests/properties.rs` | mutated `track.toml` (struct edits and raw text), geometry soundness, racing line, speed profiles, loader and CLI robustness |
| `crates/racer-sim/tests/rules_properties.rs` | arbitrary drives through `Progress`, `Timing` and `ResetMonitor`, plus `standings` and `reset_pose` |
| `crates/racer-sim/tests/long_run.rs` | an 8-car AI race over a long simulated time (ignored by default) |
| `crates/racer-game/tests/format_properties.rs` | `flow::lap_time` formatting |
| `crates/racer-server/tests/args.rs` | `racer-server` command line |

```sh
cargo test --workspace                                  # properties at small case counts (each < 10 s)
QA_EDGE_CASES=4000 cargo test --release -p racer-dynamics --test properties   # deeper
QA_EDGE_CASES=4000 cargo test --release -p racer-trackc --test properties
QA_EDGE_CASES=2000 cargo test --release -p racer-sim --test rules_properties
cargo test --workspace -- --ignored QA-EDGE             # bug regressions (all fail today)
cargo test -p racer-sim --test rules_properties qa_edge_11 -- --ignored   # one bug
QA_LONG_RUN_MINUTES=10 cargo test --release -p racer-sim --test long_run -- --ignored --nocapture
```

`QA_EDGE_CASES` sets the case count of every property. The defaults are 256 for the dynamics
step properties, 24 to 64 for most others, and 4 for the racing-line solver, which is slow.
Every bug below has an `#[ignore = "QA-EDGE-N"]` regression test with its shrunk
counterexample hard-coded, and the test fails on the current code. Once a bug is fixed, delete
its `ignore` attribute. Where a generator excludes the known-bad region so the property can
keep searching (`known_bad` in `racer-trackc`, the shorter excursions and the reset-after-reverse
skip in `racer-sim`), the code comment names the bug.

`fmt` and `clippy --workspace --all-targets -- -D warnings` are clean.

## Bugs

16 bugs in total: critical 0, major 4, minor 12.

### QA-EDGE-1 (major): a progress update that jumps two gates loses the lap for good

*Shrunk case:* the shipped track with `checkpoint_spacing = 20`, the validator's minimum,
which puts checkpoint 35 6.7 m after the sector end at checkpoint 34. A car leaves the corridor
2 m before the sector end and rejoins just past checkpoint 35. Proptest first found this with a
13.5 m excursion: half a second at 30 m/s, well inside the 3 s off-track reset.

*Expected:* both gates are credited. Failing that, the car can still complete the lap.
*Actual:* the car is credited with gate 34 only. Gate 35 now lies behind it, and
`old < next && new >= next` can never be true for it again, so `next_gate` stays at 35 on every
later lap. After 2 more full laps the car still has `laps = 0`. It can never finish, and is
marked DNF 30 s after the winner.

*Where:* `crates/racer-sim/src/rules.rs:145-155`. `Progress::update` handles at most one gate per
call, but `rules.rs:140` accepts jumps of up to `(WINDOW + 2) * L / n`, about 26 m. Gates closer
together than that can be skipped. `geometry::validate` (`crates/racer-trackc/src/geometry.rs:447`,
`701`) allows gates as close as `spacing / 3`, which is 6.7 m.

*Reachability:* on the shipped track the gates are at least 30 m apart, so this needs track data
that the validator accepts. It is a latent hole for every future track.
*Confidence:* high. Regression test: `racer-sim/tests/rules_properties.rs::qa_edge_1_*`.

### QA-EDGE-2 (major): rejoining the corridor more than ~26 m downstream freezes progress for the rest of the lap

*Shrunk case:* shipped track, from pole. The car spends 33.5 m outside the corridor (about a
second at 30 m/s), comes back onto the road and drives 500 m.

*Expected:* progress picks the car up where it rejoined, or the jump is rejected and the car is
reset. *Actual:* the car is at s = 530 m with progress still at -4.0 m, `next_gate` 0 and
`ResetMonitor::last_valid` = (-4.0, 0). The update that would move progress is discarded as an
impossible jump. `self.sample` follows the car anyway, so every later update produces the same
"impossible" jump. The car drives a whole lap with progress, gates, standings and the reset
target all stuck where it left. A later reset (stuck, flipped, or the R key) teleports it back
there.

*Where:* `crates/racer-sim/src/rules.rs:136-143`. The code sets `self.sample = sample` before the
jump check, then returns without moving `distance`.

*Reachability:* the car must leave the corridor (beyond the wall + 0.8 m, or more than 4 m
above / 1.5 m below the road) and come back more than 26 m further on within 3 s. That is
unlikely with walls everywhere, but a pile-up launch or a ghosted car on a crest can do it.
*Confidence:* high (mechanism), low (frequency). Test: `qa_edge_2_*`.

### QA-EDGE-11 (major): a reset after reversing over a checkpoint skips that checkpoint for good

*Shrunk case:* shipped track. Drive 210.7 m from pole, then reverse 39.7 m at 5 m/s, back over
the checkpoint at about 180 m. Then reset (stuck reset, flipped reset, or the R key).

*Expected:* the car is placed behind the gates it took back, or the gates it is placed past are
credited. *Actual:* the reset moves the car forwards from s = 167 m (`next_gate` 1) to
s = 193.7 m (`next_gate` still 1). The checkpoint at about 180 m is now behind the car and is
never credited, so after 2 more laps the car still has `laps = 0` and can never finish.

*Where:* `ResetMonitor::update` (`rules.rs:265`) keeps `last_valid` from the first second of
reversing, before the wrong-way flag. `reset_pose` (`rules.rs:299`) goes 8 m behind that point.
`Progress::move_back_to` (`rules.rs:179`, called from `race.rs:380-391`) only ever takes credit
back and never credits gates the car is moved past. The reset should go to
`min(last_valid, current)`, or `move_back_to` should credit forward as well.

*Reachability:* realistic for a human driver: spin just after a checkpoint, reverse back over it
to straighten up, then get stuck or press R. The AI does not reverse. *Confidence:* high.
Test: `qa_edge_11_*`.

### QA-EDGE-3 (major, latent): a NaN control value panics `Vehicle::step`

*Shrunk case:* any of `steer`, `throttle`, `brake` or `clutch` set to `f64::NAN` for one step,
then default controls. The shipped GTI is at 20 m/s on flat ground.

*Expected:* the input is rejected or sanitised. At the least, the state stays finite.
*Actual:* `clamp` passes NaN through, the NaN reaches the drivetrain, and `lock`'s
`clamp(-capacity, capacity)` with a NaN capacity panics with "min > max, or either was NaN".
`clamp` asserts its bounds, so this happens in release builds too. Before the panic,
`VehicleState`, and with it any rollback snapshot, is already NaN.

*Where:* `crates/racer-dynamics/src/vehicle.rs:299` and `:445` (no sanitising of `Controls`),
`crates/racer-dynamics/src/drivetrain.rs:338` and `:424` (clamp with data-derived bounds).

*Reachability:* no shipped input path produces NaN today. Pads are finite, and the AI's steer
is NaN only if its pose already is. On the planned authoritative server, though, one malformed
client input would crash the server. *Confidence:* high. Test:
`racer-dynamics/tests/properties.rs::qa_edge_3_*`.

### QA-EDGE-4 (minor): `Vehicle::from_spec` accepts physically invalid car data, then panics or goes NaN

*Shrunk cases:* each is one edit of the shipped GTI `car.toml`. Each is accepted (`Ok`), then
fails within a second of driving:

| Edit | Result |
|---|---|
| `gearbox.ratios = []` | panic: subtract with overflow (`drivetrain.rs:74`) |
| `gearbox.final_drive = []` | panic: subtract with overflow (`drivetrain.rs:69`) |
| `estimated.engine.torque_curve = []` | panic: index out of bounds (`drivetrain.rs:28`) |
| zero total mass (`curb_mass_kg = -payload_kg`) | NaN, then the `clamp` panic from QA-EDGE-3 |
| tire `inertia_kgm2 = 0` | same |
| tire `relaxation_long_m = 0` | same |
| engine `inertia_kgm2 = 0` | same |
| `shape_lat = 0.9` (Magic Formula C < 1) | the curve fit has no peak (`tire.rs:77`), so B = 0 and the tire has no lateral grip at all (0 N at 4 kN load); no error |

Also, `tire_radius` (`spec.rs:337`) parses "nan" and "inf", and more than 127 gears wrap
`forward_gears() as i8`.

*Expected:* `from_spec` returns `Err` naming the field. *Where:* `vehicle.rs:188` (no validation
at all). *Confidence:* high. The data is shipped and covered by tests, so this is minor, but any
modded or new car file goes straight to a panic. Tests: `qa_edge_4_*` (2).

### QA-EDGE-5 (minor): the track validator accepts NaN and degenerate race layout values

*Shrunk cases:* each is one field of the shipped track.

| Edit | Result |
|---|---|
| `defaults.curb_width = nan` | accepted; curb meshes all NaN |
| `race.grid_row_spacing = nan` | accepted; NaN grid poses and markings |
| `race.grid_column_offset = nan` | accepted; NaN grid poses and markings |
| `race.grid_row_spacing = 0` | accepted; slots 0, 2, 4 and 6 at the same spot (4 cars stacked) |
| `race.grid_row_spacing = -1` | accepted; slots 2 m apart, so the cars overlap |

Each NaN edit passes because its check is a comparison that NaN fails silently: `a < nan + 0.3`,
`take(nan as usize)`, `nan + 1.2 > hw`. Negative or small row spacings are never checked.
NaN vertices go into Avian trimesh colliders.

*Where:* `crates/racer-trackc/src/geometry.rs` `validate` (defaults and race fields have no
finiteness or range checks), `:357`, `:410`. *Confidence:* high. Test: `qa_edge_5_*`.

### QA-EDGE-6 (minor): a tiny `checkpoint_spacing` hangs `racer-trackc validate` until it runs out of memory

*Shrunk case:* `checkpoint_spacing = 0.0`. `0.0001` also fails, because on a 2 km lap it is below
half an f32 ulp of `s`.

*Expected:* "checkpoint_spacing must be at least 20 m". *Actual:* `validate` calls
`build(file)` (`geometry.rs:633`) before the spacing check (`:701`). The checkpoint loop
`while s < length - spacing / 2.0 { ...; s += spacing }` (`:447`) never ends and pushes
checkpoints without bound. The test kills it after 5 s under a 4 GB memory cap.
*Confidence:* high. Test: `qa_edge_6_*` (runs the CLI as a subprocess).

### QA-EDGE-7 (minor): a huge grid or a huge track crashes or hangs `validate`

*Shrunk cases:*
- `grid_slots = 9223372036854775807` panics with "capacity overflow" in `build`
  (`geometry.rs:408`), which runs before the `1..=16` check at `:683`.
- Plan coordinates scaled by 1e6 are still running after 30 s, and slow without bound. There is
  no bound on track size. The work is 1 m samples, the O(n²) self-overlap check (`:664`) and a
  0.5 m brute-force `Route::project` (`:272`). Scaled by 1e3, it is still running after 60 s.

*Expected:* an error. *Confidence:* high. Test: `qa_edge_7_*` (subprocess, timeouts).

### QA-EDGE-8 (minor): a short `drive` table in `summary.toml` panics `load_dir`

*Shrunk case:* `drive = [3.0]` for one car in `generated/summary.toml`.

*Expected:* `Err`. *Actual:* a panic, subtract with overflow in `CarModel::drive_at`
(`crates/racer-trackc/src/profile.rs:173`, `self.drive.len() - 2`), called through
`load_dir` → `profile_for` (`lib.rs:124`). The game loads this file at race start
(`race_scene.rs:39` uses `expect`). The same goes for `racing-line.csv` offsets of "NaN", which
`parse_column` accepts. *Confidence:* high. Test: `qa_edge_8_*`.

### QA-EDGE-9 (minor): a second lap taken back restores the wrong lap start

*Shrunk case:* two laps (0 → 100 s, 100 → 200 s), then driving backwards over the line twice
(`LapUndone`, all `CheckpointUndone`, `LapUndone`).

*Expected:* `lap_start = 0`, the race start. *Actual:* 100. `Timing` keeps one level of history
(`previous_lap_start`, `rules.rs:396-407`), so the redone lap is timed from the wrong point.
`last_sectors` and `last_checkpoint_times` are also gone after the first undo. Reaching this
means reversing a whole lap, which is unlikely, but the rules allow it.
*Confidence:* high. Test: `qa_edge_9_*`.

### QA-EDGE-10 (minor): a lap that is taken back keeps its best sectors and stays the delta reference

*Shrunk case:* one lap timed, then `LapUndone` straight away (rocking back over the line).

*Expected:* no lap stands, so there are no best sectors and no delta. *Actual:* `best_lap` is
reset to `None`, but `best_sectors` still holds the undone lap's sectors, and the redone lap's
first checkpoint still gets a delta against the undone lap's checkpoint times. The HUD can show
a best sector from a lap that never counted. *Where:* `rules.rs:381-391` (the `Lap` arm updates
the bests) and `:396-407` (`LapUndone` does not restore them). The property found this with a
plain forward/backward drive on the road. *Confidence:* high. Test: `qa_edge_10_*`.

### QA-EDGE-12 (minor): a banking change in the hairpin builds a 31° apron ramp

*Shrunk case:* node 6 (hairpin entry) `bank_deg = -7.64`, inside the allowed ±10°, instead of +3.

*Expected:* the validator rejects it, or the aprons stay about as flat as the road (the shipped
track is under 12°). *Actual:* accepted, and an inner-apron triangle slopes 31.0°. The bank
blends from -7.6° to +3° across an inner apron edge whose radius is only about 2.5 m, so the
edge rises 1 m within a metre or two of plan. That is a ramp a car can be launched off.
*Where:* `geometry.rs` `validate` checks banking per node and inner-wall folding, but not the
resulting surface slope. *Confidence:* medium on the severity of the effect, high on the
geometry. Test: `qa_edge_12_*`.

### QA-EDGE-13 (minor): a road below the ground plane is accepted

*Shrunk case:* node 4 `elevation = -2.39`.

*Expected:* an error. The city ground is a 1 m collider box spanning -1.05 to -0.05 m
(`racer-sim/src/track.rs:284`). *Actual:* accepted, and the road runs down to -2.39 m, under the
ground. The walls' outer face also folds into degenerate (zero-area) triangles, because it runs
from the wall top down to a fixed `GROUND_Y - 0.5` (`geometry.rs:387`). Elevation has no lower
bound. *Confidence:* high. Test: `qa_edge_13_*`.

### QA-EDGE-14 (minor): `racer-server --max-ticks` with a bad value panics, and with no value runs forever

*Shrunk cases:*
- `--max-ticks abc`, `-1` and `1e3` exit with code 101 and a panic message
  (`crates/racer-server/src/main.rs:7`, `expect`). The other binaries print usage and exit 2.
- `--max-ticks` with no value is silently ignored, and the server runs forever. That is a bad
  failure mode for CI scripts.

*Confidence:* high. Test: `racer-server/tests/args.rs::qa_edge_14_*`.

### QA-EDGE-15 (minor): the results table shows "0:60.000"

*Shrunk case:* `lap_time(59.9996)` returns `"0:60.000"`, and `lap_time(119.9997)` returns
`"1:60.000"`.

*Expected:* `"1:00.000"` and `"2:00.000"`. *Where:* `crates/racer-game/src/flow.rs:663`. It
formats minutes and seconds separately and rounds only the seconds. This affects the finish
time and best-lap columns. *Confidence:* high. Test:
`racer-game/tests/format_properties.rs::qa_edge_15_*`.

### QA-EDGE-16 (minor): `racer-capture ai-races --first` overflows

*Shrunk case:* `ai-races <out> --first 18446744073709551615 --races 2`.

`first..first + races` (`crates/racer-capture/src/main.rs:307`) panics with "attempt to add with
overflow" in debug builds. In release builds the addition wraps to an empty range, so the run
does no races and reports on nothing. `races * laps as usize` (`:314`) has the same problem.

*Confidence:* high, found by reading. No regression test: the binary is heavy to start, and the
fix is a `checked_add`.

## Properties that held

All of these ran at 2000-5000 cases in release with no failure, on top of the default-profile
runs. Ranges are the generators' ranges.

**racer-dynamics** (`tests/properties.rs`), with arbitrary car (all 3), plausible internal state
(wheel spin ±250 rad/s, slips ±1.5, any gear, engine idle..limiter, ABS and TCS factors),
chassis tilt ±0.4 rad, any yaw, velocity -20..75 m/s forward, ±15 lateral, ±5 vertical,
angular velocity up to 3 rad/s, 0-4 wheel contacts at any travel (-0.1..0.6 m), normals tilted
up to 57° on any of the 8 surfaces, and all controls in range:
- `step` output and state are finite, the engine stays inside its clamp, the ABS and TCS
  factors stay in 0..=1, wheel loads stay in 0..=20 mg/4, and **every tire force is inside its
  friction ellipse** (low-speed damping included).
- `step` is deterministic, bit for bit.
- **Mirror symmetry:** mirroring the chassis, contacts, wheel states and steer mirrors the force
  and the torque (pseudovector) to 1e-6 relative, and the wheel spins to 1e-9.
- Airborne with the clutch pressed, wheel spin energy never rises: brakes and LSD locks only
  dissipate.
- Coasting in neutral on flat ground for up to 20 s with random steer, brake, handbrake and
  assists, total mechanical energy never rises above the start (+0.2 % + 500 J for spring
  energy).
- Snapshot and restore, then replay of random inputs, is bit-identical.
- Out-of-range finite controls (±10) keep the state finite.
- **dt splitting:** one step of 2 dt and two steps of dt agree within 2 % on wheel and engine
  speed, for a car on flat ground with wheels near rolling and the clutch locked. Outside that
  envelope the coupled tire, wheel and clutch system is stiff. Examples are a locked wheel at
  28 m/s, a slipping clutch at 6000 rpm on an airborne wheel, and ESC braking a wheel in the air.
  There, dt and 2 dt differ by up to 50 % over one step. That is first-order integration error
  with 4 substeps, consistent with the M1 review's "tuned and validated only at 240 Hz". It is
  not a defect at the fixed 240 Hz step, so I did not file it.

**racer-trackc** (`tests/properties.rs`):
- 1-3 random edits of the shipped track: node moves, radius, width, elevation ≥ 0, bank ±12°,
  aprons, curbs, wall height, grid fields, checkpoint spacing ≥ 10 m, sector and start moves,
  node removal. Each mutated track is either rejected or produces finite meshes, no degenerate
  triangles, a road facing up (normal y > 0.95), no flipped aprons, checkpoints strictly inside
  the lap and ascending, sector ends ascending, the sprint finish inside the lap, and grid slots
  behind the line that do not overlap. `build` is deterministic, bit for bit. This held at 4000
  cases once the QA-EDGE-5/6/7/12/13 regions were excluded.
- Random text mutations of `track.toml` never panic the parser or the validator (3000 cases).
- The racing line on mutated valid tracks is finite, inside `line::bounds`, no longer than the
  centreline, and never more curved than the centreline's tightest corner.
- Speed profiles for arbitrary point-mass models (lateral and braking 4-15 m/s², top speed
  20-100 m/s, any drive table of 2+ entries, limits 0.5-1) are finite, positive, within the top
  speed, within the lateral grip at every point, and never brake harder than the model allows
  between points. Lap time is finite.

**racer-sim rules** (`tests/rules_properties.rs`):
- Driving forwards and backwards on the road (legs of -40..250 m at 5-90 m/s, any lateral
  position on the road), on the shipped track and on the dense-checkpoint variant, from any
  grid slot. Credit `(laps, next_gate)` always equals the gates at or before the distance.
  Progress tracks the car within 2 m. Laps change by at most 1 per update and match the
  `Lap`/`LapUndone` events. `Timing` has one lap per counted lap, positive lap times,
  `best_lap` equal to the minimum, the last sectors summing to the last lap, `lap_start` equal
  to the standing lap's start, and best sectors equal to the minimum over the standing laps
  until a lap is taken back (QA-EDGE-10).
- The same with excursions of up to 20 m off the corridor and resets mixed in, on the shipped
  track. This holds except for resets right after reversing (QA-EDGE-11).
- `standings` is a permutation in a consistent total order: finishers by laps desc then time,
  then racing cars, then DNFs by (laps, gate, distance) desc. Tested on 0-12 arbitrary cars.
- `reset_pose` from any last valid point (-100..6000 m, lateral ±20 m) with 0-8 nearby cars is
  finite, 8-38 m behind the point, at least 1.6 m inside the road edge, on track by `Progress`,
  and facing along the track.

**racer-game:** `lap_time` round-trips within 0.5 ms for 0-7200 s, apart from QA-EDGE-15.

**Long run** (`tests/long_run.rs`, release): an 8-car AI race (all three cars and all five
difficulties, full contact) ran for 10 simulated minutes. Results: LONG_RUN_RESULT.

**Reviewed by reading, no issue:** every `clamp` in `racer-sim`, `racer-game` and
`racer-trackc` has ordered bounds for validated data. The AI's `clear` interval is fixed up
when it inverts. `bounds` has edge > 0 because width is at least 6 m. `RacingLine::nearest` and
`rules::nearest` windows cannot underflow on validated tracks (n ≥ 100). The `drive_cars` system
skips `dt <= 0`; a direct `Vehicle::step` with dt = 0 gives 0/0 in the clutch solve, but
nothing calls it that way. The `racer-capture` flag parser rejects unknown flags and bad values.
`check::check` handles empty images.
