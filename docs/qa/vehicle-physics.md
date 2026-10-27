# QA: vehicle physics

Scope: `crates/racer-dynamics` (vehicle model, testbed), the Avian integration in
`crates/racer-sim/src/vehicle.rs` (wheel shape casts, force application) and car data loading
(`data/cars/*/car.toml`). Done on 2026-10-27 against `main` at `4253183`, in a fresh container
after `tools/setup.sh`. Known gaps from the M1/M2 reviews (camber, ride-height aero, 64 Hz limit
cycle, crest landing loads, the M3 fit) are not repeated here.

Repro tests are `#[ignore]`d with the bug id as the reason. Test names can't contain `-`, so
filter on `qa_phys`:

```
cargo test -p racer-dynamics --test qa_vehicle_physics -- --ignored qa_phys   # QA-PHYS-1,2,3,6,7,8,9 (< 0.1 s)
cargo test -p racer-sim      --test qa_vehicle_physics -- --ignored qa_phys   # QA-PHYS-4,5 (< 1 s)
```

All 10 fail on the current code. The normal suite stays green, and fmt and clippy
(`-D warnings`) are clean.

## Summary

| Id | Severity | Area | One line |
|---|---|---|---|
| QA-PHYS-1 | major | data / model | `Vehicle::from_spec` doesn't validate anything: degenerate values (and `nan`/`inf` in TOML) load fine, then panic or go NaN in `step` |
| QA-PHYS-2 | major | data / drivetrain | An empty `ratios`, `final_drive` or `torque_curve` loads fine, then panics on the first step |
| QA-PHYS-3 | major | tire model | A braked car that slid sideways to a stop creeps 1.43 m back sideways over about 10 s |
| QA-PHYS-4 | major | Avian contact | On the test track's grass (a 4000 m box) about 25 % of wheel casts miss while parked, and loads swing from 0 to 16.6 kN |
| QA-PHYS-5 | major | Avian contact | Rolling onto an 8 cm step edge at 1 m/s snaps the suspension 6 cm in one step, with a 34 kN load spike (7.5× static) |
| QA-PHYS-6 | minor | model API | `Vehicle::step` panics when `dt = 0` or NaN; a negative `dt` silently produces forces |
| QA-PHYS-7 | minor | model API | NaN in `Controls` panics inside `Vehicle::step` |
| QA-PHYS-8 | minor | drivetrain | In neutral, full throttle holds about 3270 rpm; the engine can't be revved |
| QA-PHYS-9 | minor | drivetrain | The automatic gearbox never leaves neutral: one shift-down at rest and the throttle does nothing |

**Totals: 0 critical, 5 major, 4 minor.**

## Bugs

### QA-PHYS-1 — no validation of car parameters (major, high confidence)

**Repro:** `qa_phys_1_degenerate_parameters_are_rejected` and
`qa_phys_1_nan_and_inf_in_car_toml_are_rejected`.

**Expected:** `Vehicle::from_spec` (it already returns `Result`) or `CarSpec::parse` rejects
out-of-range or non-finite values with a message that names the field.

**Actual:** every case returns `Ok`. Starting from the GTI and changing one field each time,
then driving for 2 s:

| Change | Outcome |
|---|---|
| mass 0, negative or NaN | panic `min > max, or either was NaN` (vehicle.rs:386, load clamp) |
| wheel inertia 0, engine inertia 0, relaxation length 0, peak slip ratio 0, front track 0, ride frequency 0 | panic, same message (vehicle.rs:386 or drivetrain.rs:423, LSD lock clamp) |
| yaw gyration 0 | no panic; position NaN after the first step (singular inertia in the integrator; Avian would get a zero `AngularInertia`) |
| tire `shape_long` < 1 | silently zero grip: `Curve::new` (tire.rs:77) finds B = 0 and the car never moves (0.00 m/s) |
| `curvature` ≥ 1 | the curve is no longer monotonic and the bisection picks a wrong B: 2.5 m/s after 2 s instead of about 15 |
| `front_weight_fraction` 1.2, limiter below idle, efficiency 0, steering ratio 0 | silently broken car (rear axle ahead of the CoM; no drive at all; no steering) |
| `curb_mass_kg = nan` / `inf` / `-inf` in car.toml | accepted by `CarSpec::parse` (TOML allows these floats), then as above |

Unknown fields, missing fields and integer-for-float values are handled well (`deny_unknown_fields`
gives a clear error message). Only the ranges go unchecked. An LSD with no `preload_nm`/ramps
also loads silently as a zero-capacity LSD, because of the `#[serde(default)]` fields.

**Location:** `crates/racer-dynamics/src/vehicle.rs:188` (`from_spec`) and
`crates/racer-dynamics/src/spec.rs:309` (`parse`). Suggested fix: a `CarSpec::validate` that runs
on load (positive finite masses, inertias, lengths, rates and slips; C > 1; E < 1;
0 < fraction < 1; idle < launch < limiter; 0 < efficiency ≤ 1; non-empty, sorted tables).

### QA-PHYS-2 — empty gear and torque tables panic (major, high confidence)

**Repro:** `qa_phys_2_empty_gear_and_torque_tables_are_rejected`.

**Expected:** a load error. **Actual:** `from_spec` returns `Ok`, and the first step panics:

| Case | Panic | Location |
|---|---|---|
| `ratios = []` | `attempt to subtract with overflow` | drivetrain.rs:74 `self.ratios.len() - 1` |
| `final_drive = []` | `attempt to subtract with overflow` | drivetrain.rs:69 `self.finals.len() - 1` |
| `torque_curve = []` | `index out of bounds` | drivetrain.rs:28 `c[0]` |

In release builds the subtraction wraps, and the panic comes from the index instead. A torque curve
that isn't sorted by rpm is also accepted: `full_torque` interpolates and gives wrong torque
silently, and a duplicate rpm point divides by zero (no panic here only because of the `<=` order).
A single-gear box works fine.

### QA-PHYS-3 — sideways creep after a slide to rest (major, high confidence)

**Repro:** `qa_phys_3_car_stopped_after_a_sideways_slide_stays_put` (testbed, GTI, flat dry
asphalt, starting at 12 m/s purely sideways, as after a spin, with full brake and handbrake).

**Expected:** the car stops and stays put (well under 0.1 m of springback).

**Actual:** it stops at x = 6.51 m after about 1.1 s, then crawls back sideways at a steady
0.16 m/s and only comes to rest about 11 s in, **1.43 m** back from where it stopped. Brakes
and handbrake are on throughout.

**Cause:** the relaxation filter decays the lateral slip with |vx| only (vehicle.rs:480-484).
With vx ≈ 0, `slip_lat` integrates vy/σ without decay, reaching about 10 (tan α) during the slide.
At rest this unwinds as a spring of travel σ·tan α ≈ 0.35 × 4 m. The low-speed damper
(vehicle.rs:487-501) then balances that spring at a constant creep speed, rather than stopping it.
The same effect shows longitudinally: after a locked-wheel stop with ABS off the car rolls back
0.10 m (minor on its own). Suggested fix: clamp the relaxed slips to the kinematic value, or
decay with max(|vx|, |vy|, some floor).

### QA-PHYS-4 — wheel casts miss on the grass box (major, high confidence on symptom, medium on cause)

**Repro:** `qa_phys_4_wheels_keep_contact_on_the_grass_box`, which spawns the real `test_track()`
and puts the car on grass at (-400, 0, -400): 2 s parked on the brakes, then 2 s rolling at
1 m/s.

**Expected:** all four wheels in contact on flat ground.

**Actual:** the GTI has 977 and the M3 1008 of 3840 wheel-steps with no contact (about 25 %).
Loads while parked jump between 0 and 16.6 kN (static is about 3.7–4.6 kN), and the parked
body visibly jitters. Measured on plain boxes at the origin, the rate grows with box size:
100 m box 0/2880, 1000 m box 27–36/2880, test-track grass (4000 m) about 1000/3840. A trimesh
ground of the same size has 0 misses.

**Location:** `crates/racer-sim/src/vehicle.rs:173`. `spatial.cast_shape` with a cylinder against
the huge cuboid returns `None` intermittently (probably parry GJK precision against the very
large support extents). Grass is the test track's run-off. City tracks also put their ground
on one big box (`city_track`, bounds + 800 m); there the aprons and walls usually keep cars off
it, but a reset or crash can land a car on it. Suggested fix: tile the ground or make it a
trimesh or heightfield. Cross-area note: the tooling/track QA may want the same check on the
skidpad pad box.

### QA-PHYS-5 — the step-edge contact snaps to the bump stop (major, high confidence)

**Repro:** `qa_phys_5_wheel_climbs_a_step_edge_smoothly` (Avian, GTI rolling at 1 m/s onto an
8 cm box step with a vertical edge).

**Expected:** the cylinder cast's contact with the edge raises the wheel gradually over the
last about 0.21 m of approach, with a load rise of a few kN.

**Actual:** the wheel's travel drops 0.060 m in one step, the moment the cylinder first touches
the edge (wheel centre still 0.21 m before it). The load goes 4.6 kN → **34.3 kN (7.5×)**, followed
by about 15 steps of chatter between travel 0 and 0.12 with loads between 0 and 33 kN.

**Cause:** `crates/racer-sim/src/vehicle.rs:179-193`. After a hit, a ray from `hit.point1 + up*0.05`
finds the edge's top face, and the distance is re-solved against that **infinite plane**, which
puts the wheel bottom at full step height as soon as the cylinder grazes the corner. The cylinder
cast distance is ignored, though it was the correct one. A ray that slips off the vertical face
instead lands on the lower ground, which explains the chatter. This affects every box with a
vertical edge (test-track curbs and pad) and the 2 cm road-to-grass step (`ROAD_TOP`). It goes
against PLAN §4 "shape-casts so curbs read correctly". Suggested fix: never return a distance
smaller than `hit.distance` (use the plane only to refine the normal, or take
`max(plane_distance, hit.distance)`).

### QA-PHYS-6 — `dt = 0` panics (minor, high confidence)

**Repro:** `qa_phys_6_zero_dt_step_is_a_no_op`. `Vehicle::step(.., dt = 0.0)` panics with
`min > max, or either was NaN` at drivetrain.rs:423, from 0/0 in the clutch solve (drivetrain.rs:337)
feeding the LSD capacity. `dt = NaN` panics the same way. A negative `dt` returns forces and
moves the wheel speeds backwards with no error. The Avian path guards `dt <= 0`
(racer-sim vehicle.rs), but the crate docs say `step` takes any `dt`, and the rollback/replay
code is a likely future caller. Expected: early return (or assert with a clear message) for
`dt <= 0` or a non-finite `dt`.

### QA-PHYS-7 — NaN controls panic (minor, high confidence)

**Repro:** `qa_phys_7_nan_controls_do_not_panic_or_poison_the_state`. NaN `throttle`, `brake` and
`steer` (`clamp` passes NaN through) make the wheel speeds NaN, then the next substep panics in the
LSD clamp (drivetrain.rs:423). An AI bug or a malformed network input would take down the client or
server. Expected: sanitize controls (non-finite → 0) at the top of `step`. Cross-area: the
adversarial and netcode QA sessions may also want to check the server's input path.

### QA-PHYS-8 — the engine can't be revved in neutral (minor, high confidence)

**Repro:** `qa_phys_8_engine_revs_freely_in_neutral` (manual, one shift down from 1st to
neutral, full throttle for 2 s). **Expected:** the engine revs to the limiter (6700 rpm).
**Actual:** it holds 3273 rpm. In neutral `g = 0`, so `driveline = 0 < launch` and
`pulling_away` is true (drivetrain.rs:284). The launch-speed throttle cap then applies with no
clutch to slip. The same cap applies at standstill with the clutch pedal pressed. This makes rev
matching, and the engine-sound range, impossible below the launch speed. Expected: apply
`pulling_away` only with a gear engaged and the clutch not fully pressed.

### QA-PHYS-9 — the automatic gearbox stays in neutral (minor, medium confidence on intent)

**Repro:** `qa_phys_9_automatic_gearbox_engages_drive_from_neutral` (automatic, one shift-down
at rest, then full throttle for 3 s). **Expected:** the automatic engages 1st when the throttle
is pressed in neutral, as a D selection would. **Actual:** it stays in gear 0 at 0.00 m/s.
`auto_shift` returns early for `gear < 1` (drivetrain.rs:232). Players in automatic mode reach
reverse by pressing shift-down twice (Q in `racer-game/src/input.rs`), so a single tap, or an
extra tap while slowing in 1st, leaves the car dead until they press shift-up. Cross-area:
input/UI QA may prefer to fix this as a reverse key in automatic mode.

## Checked and fine

**Data loading.** Missing fields, unknown fields (top level, nested tables, inline diff tables)
and integers where floats are expected all give clear TOML errors. A single forward gear,
a one-point torque curve and a reversed torque curve drive without panicking.

**Determinism.** Testbed: the same inputs twice give bit-identical body and state after 5 s.
Avian: 4 cars × 6 s of full throttle with steering into a wall, run twice in separate apps,
give bit-identical positions. The M1 replay test holds.

**Rollover and orientation.** Upside down and resting on the side: the contacts drop out
(normal · up < 0.3), the car rests still with full throttle and steering held, and every
value stays finite. Free flight: rotational energy and |L| change by under 0.1 % over 10 s of
tumbling (the explicit gyroscopic term is fine at 240 Hz).

**Airborne and landing.** Full throttle airborne hits the limiter (6699 rpm), and the landing
from 2 m at 30 m/s stays finite. Landing loads reach about 48 kN, the same as the known M1 crest
artefact.

**Reversing and gear changes.**
- Reverse at full throttle reaches 18.5 m/s at the limiter.
- Shifting into reverse is refused above 1 m/s forward.
- Shifting into 1st while reversing at 18 m/s decelerates through clutch slip (1.2 g peak) without
  overrevving.
- Downshift spam from 5th at 200 km/h down to neutral is clamped at 1.1× the limiter, with no
  yaw upset.
- Dumping the clutch at the launch rpm works.

**Holding and pinning.**
- On a 10–30 % grade, brake or handbrake holds the car once it has settled; the testbed's own
  placement on slopes is approximate and accounts for the first metre.
- Pinned against a 0.4 m trimesh wall at full throttle for 20 s (M3, Avian), the car is still
  (|v| < 0.03 m/s), doesn't climb the wall, and TCS holds the rear wheels at about 4 rad/s.

**Walls and car contact (Avian).**
- High-speed wall impacts into a wall built like racer-trackc's (0.4 m thick open trimesh,
  1 m tall) at 30, 60, 90 and 120 m/s: no tunnelling. The car never passes the wall face; at
  120 m/s it vaults upward, as expected for a 1 m wall.
- Car-on-car stacking: a GTI dropped onto another rests on its body box, the wheels correctly
  ignore the car layer, and it stays stable (|ω| < 0.08 rad/s).

**Far from the origin (Avian, f32).** At 2, 10 and 50 km from the origin, the car at rest and
the 4 s full-throttle speed (20.1–20.3 m/s) match the origin within noise, on a 2 km box so
QA-PHYS-4 doesn't interfere.

**Assists, surfaces and dt.**
- Assist toggles mid-maneuver: the ABS factor resets to 1 when off or not braking, the TCS
  factor resets when off, and ESC is stateless.
- Surface grip is applied per patch.
- `dt` from 1e-9 to 0.5 s returns finite forces. The plugin uses the fixed 240 Hz step, and
  `drive_cars` guards `dt <= 0`.
- Car resets restore `initial_state()` (race.rs `apply_resets`), so stale wheel spin doesn't
  survive a reset.
