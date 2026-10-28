# QA fixes: fix/dynamics

Fixes for the `fix/dynamics` group of `docs/qa/TRIAGE.md`: QA-PHYS-1 to QA-PHYS-9, plus the
duplicates QA-EDGE-4 (covered by PHYS-1/2) and QA-EDGE-3 (covered by PHYS-7). Every repro test
reproduced on `fix/dynamics` before its fix and failed for the reason given in the report. All of
them now pass with their `#[ignore]` removed, and they stay in the suite as regression tests.

```
cargo test -p racer-dynamics --test qa_vehicle_physics   # PHYS-1,2,3,6,7,8,9 (+ regressions)
cargo test -p racer-dynamics --test properties qa_edge   # EDGE-3, EDGE-4
cargo test -p racer-sim      --test qa_vehicle_physics   # PHYS-4, PHYS-5
```

Files changed: `crates/racer-dynamics/src/{spec,tire,vehicle,drivetrain}.rs`,
`crates/racer-sim/src/{vehicle,track}.rs` and the QA test files of those two crates. No car data
changed: the three shipped cars pass the new validation as they are.

Two sets of derived files outside the group's scope were regenerated because the physics changed
(see [Regression checks](#regression-checks)):

- `data/tracks/downtown-loop/generated/` (summary and speed profiles), with
  `cargo run --release -p racer-trackc -- compile data/tracks/downtown-loop` (commit `64c478b`).
- `crates/racer-capture/goldens/race/*.png`, with
  `CAPTURE_PROFILE=release tools/capture.sh race <dir>`.

## QA-PHYS-1 (+ QA-EDGE-4): no validation of car parameters

**Root cause.** `Vehicle::from_spec` and `CarSpec::parse` checked nothing. TOML accepts
`nan`/`inf`, and `tire_radius` parses them too. A degenerate value then reached a division or a
`clamp` with NaN bounds inside `Vehicle::step`.

**Fix.** A new `CarSpec::validate` checks every number the model uses and returns an error that
names the field, e.g. `estimated.tires.front.shape_lat = 0.9: must be in (1, 2]`. `CarSpec::parse`
(and with it `load`/`load_car`) and `Vehicle::from_spec` both run it, so a bad file fails at load
time, and so does a spec built in code. The checks:

- Masses, inertias, lengths, tire widths, rates, slips and relaxation lengths must be positive
  and finite (payload, damping ratios, brake torques and drags may be zero).
- `0 < front_weight_fraction < 1`, and `idle < launch < limiter`.
- `0 < efficiency <= 1`, and the road-wheel lock must be between 0 and 90°.
- Peak slip angle in (0, 90)°. Magic Formula `1 < C <= 2` (it has no peak at C ≤ 1 and goes
  negative past the peak above 2), and `E < 1` (the curve stops being monotonic).
- An LSD must have some preload or ramp: QA's "loads silently as a zero-capacity LSD". The centre
  `front_split` must be in (0, 1] (it defaults to 0 when omitted, so 0 is rejected).

The bisection in `tire::Curve::new` assumed B < 100. For C just above 1 the peak lies beyond that,
and the fit came out silently wrong. It now brackets the root first.

**Tests.** `qa_phys_1_*` (2), `qa_edge_4_invalid_car_specs_are_accepted`, and
`qa_edge_4_shape_factor_below_one_is_rejected`. That last one is rewritten: it used to `expect`
that C = 0.9 loads and then check its grip, and now it expects an error naming `shape_lat`. New
unit tests: `spec::tests::shipped_cars_validate`, `spec::tests::validation_names_the_bad_field`
(unsorted or NaN torque curve, wrong final-drive count, negative ratio, 200 gears, `Rnan` tire
size, launch below idle, zero-capacity LSD, infinite curvature) and
`tire::tests::curve_peaks_at_one_for_shape_factors_close_to_one`.

## QA-PHYS-2: empty gear and torque tables panic

**Root cause.** Same as PHYS-1: `Gearbox::ratio` does `len() - 1` and `Engine::full_torque` reads
`c[0]` on tables that were never checked.

**Fix.** Part of `validate`:

- 1 to 127 forward gears (`forward_gears()` is an `i8`, so more than 127 wrapped, as EDGE-4
  notes), all ratios positive.
- `final_drive` has 1 entry or one per gear, as its doc comment says. Other lengths used to be
  silently clamped.
- The torque curve is non-empty with rpm strictly increasing. Unsorted curves interpolated
  wrongly, and a duplicate rpm divided by zero.

**Test.** `qa_phys_2_empty_gear_and_torque_tables_are_rejected`.

## QA-PHYS-3: sideways creep after a slide to rest

**Root cause.** As QA found, the relaxation filter decays the relaxed slip at a rate set by |vx|.
In a pure side slide (vx ≈ 0) `slip_lat` integrates vy/σ with nothing to stop it and reaches
about 10 (tan 84°). At rest that is a spring of σ · 10 ≈ 3.5 m of tread travel. The low-speed
damper is limited by grip and can't hold it, so the two balance at a steady 0.16 m/s creep
against the brakes. The underlying error is physical: past the peak, slip is the contact patch
sliding, not carcass deflection, so it stores no elastic energy. When it unwound, the "spring" did
positive work on the car.

**Fix.** In `Vehicle::step`, when the patch velocity opposes the stored slip (the spring is
unwinding), the slip is first limited to the peak slip: `peak_slip_tan` laterally,
`peak_slip_ratio` longitudinally. Only the elastic part springs back. The rule never acts while
slip builds up, so a steady slide at speed still sits past the peak at the kinematic slip, and the
steady-state Magic Formula behaviour is unchanged. I didn't use the two remedies QA suggested.
Clamping to the kinematic slip removes the standstill spring that holds a car on a slope. Decaying
at max(|vx|, |vy|) still leaves σ · 1 ≈ 0.35 m to unwind.

**Result.**

| Case | Before | After |
|---|---|---|
| Sideways springback after the 12 m/s side slide | 1.43 m | 0.062 m |
| Rollback after a locked-wheel stop, ABS off (the longitudinal case QA mentioned) | 0.104 m | 0.030 m |

The 0.030 m is about 1.3 cm of elastic tire deflection at the peak, plus the pitch rebound. The
reference maneuvers are unchanged (M3 0-100 4.06 → 4.05 s, all others identical).

**Tests.** `qa_phys_3_car_stopped_after_a_sideways_slide_stays_put` (repro, < 0.1 m) and two new
regressions. `qa_phys_3_car_stopped_on_locked_wheels_stays_put` checks the ABS-off rollback stays
under 5 cm. `qa_phys_3_braked_car_holds_on_a_grade` checks a braked car moves under 1 cm in 10 s on
a 20 % grade, so the standstill spring still holds.

## QA-PHYS-4: wheel casts miss on the grass box

**Root cause.** I confirmed QA's guess, and it is precision. parry casts a cylinder against a
cuboid with a GJK ray cast on their Minkowski difference (`gjk::minkowski_ray_cast`), which stops
on an absolute tolerance of `eps_tol()` = 10 f32 epsilons (≈ 1.2e-6). The 4000 m box's support
points are 2-3 km from the wheel, where f32 resolves only about 2e-4 m. The iteration can't meet
its tolerance, takes the "upper bounds inconsistency" exit and reports no hit.

Measured with the GTI at (-400, 0, -400), parked then rolling:

| Ground | Dropped wheel-steps | Peak load |
|---|---|---|
| 1000 m box | 5 | |
| 4000 m box | 1246 | 14.3 kN |
| 4000 m box, 100 m thick | 1228 | |
| 4000 m two-triangle trimesh | 822 | 36 kN |
| half-space | 0 | 4.85 kN |
| 130 m skidpad pad box | 0 | |

The same effect makes cast distances on a 60 m box a steady 3.8 mm short (see PHYS-5).

**Fix.** In `spawn_track`, the ground piece (`PieceKind::Grass`) collides as a
`Collider::half_space(Y)` under the box's top face. It is a child collider, so the piece and its
rendering (`Shape::Box`) don't change. parry casts, ray casts and contacts against a half-space
analytically. With it, wheels have 0 misses and steady loads, car bodies rest on it (checked
upside down: the roof box rests at the right height), and broad-phase pairs are the same as with
the box, which overlapped every car anyway.

**Behaviour change.** The ground no longer ends at the box edge. On the test track it used to end
2 km out. The city ground box (`city_track`, bounds + 800 m) goes through the same `spawn_track`
code, so it is fixed too, as QA's cross-area note asked. This is in `track.rs`, next to the M1
colliders. The skidpad pad (130 m) and the curbs stay boxes: they are small enough (0 misses on
the pad, including near its edge).

**Test.** `qa_phys_4_wheels_keep_contact_on_the_grass_box`, now also asserting that the peak
wheel load stays under 6 kN (static is 3.7-4.6 kN; the misses swung it to 16.6 kN).

## QA-PHYS-5: the step-edge contact snaps to the bump stop

**Root cause.** As QA described: after a hit, a ray 5 cm above the contact point finds a face,
and the distance is re-solved against that face's infinite plane. At an edge the ray finds the
top face beyond the corner, and the plane puts the wheel on top of the step as soon as the
cylinder grazes it. When the ray slips off the vertical face instead, the old code fell back to
the cast distance, hence the chatter.

QA's suggested `max(plane, cast)` doesn't work here. The cast distance is not exact either: it is
typically 0.1-1 mm short of the plane on trimesh roads (p99.9 2.6 mm), and a steady 3.8 mm short
on a 60 m box (PHYS-4's precision issue). And at a 2 cm road-to-grass edge the slipping ray gives
a plane that is too *deep*, so any distance-agreement threshold sits right in that noise.

**Fix.** The plane is used only if its face really goes on under the wheel, checked
geometrically. A second ray at the foot of the wheel centre on the plane must land on the plane
(within 5 mm). Otherwise the contact is the cast's own: its distance, and its normal
(`normal1`, pointing from the corner to the wheel centre; 37° at first touch on the 8 cm step
against 41° geometric). The edge normal tilts the tire force back, so a step now also resists the
car.

On faces nothing changes, so the face code path is the same as before. Over a city race (255k
casts) the face test rejected 1345 casts on the curb meshes and 4 elsewhere. On the test track,
all its rejections were at road-to-grass edges.

**Result.** At 1 m/s onto the 8 cm step, the travel changes at most 4.8 mm per step (was 60 mm)
and the load peaks at 7.1 kN (1.5× static, was 34 kN, 7.5×). The climb is gradual over the last
0.21 m, followed by a damped body rebound, with no chatter.

**Test.** `qa_phys_5_wheel_climbs_a_step_edge_smoothly` (now prints its figures).

## QA-PHYS-6: `dt = 0` panics

**Root cause.** 0/0 in the clutch solve fed the LSD clamp. A negative `dt` ran the integrators
backwards.

**Fix.** `Vehicle::step` returns early for a `dt` that is not positive and finite: the state is
left alone, with zero force and torque and default telemetry. This is documented on `step`.

**Test.** `qa_phys_6_zero_dt_step_is_a_no_op`, extended to dt = -1/240, NaN and ∞. Each must leave
the state bit-identical and give zero force.

## QA-PHYS-7 (+ QA-EDGE-3): NaN controls panic

**Root cause.** `clamp` passes NaN through, and the NaN reached the drivetrain.

**Fix.** A new `Controls::sanitized` clamps every analogue control to its range and reads
non-finite values as 0. `step` applies it on entry.

**Tests.** `qa_phys_7_nan_controls_do_not_panic_or_poison_the_state` and
`qa_edge_3_nan_control_poisons_the_vehicle_state`. The proptest
`out_of_range_finite_controls_keep_the_state_finite` still passes.

## QA-PHYS-8: the engine can't be revved in neutral

**Root cause.** `pulling_away` was true whenever the driveline turned slower than the launch
speed. That includes neutral (g = 0) and the clutch pedal fully down. The launch-speed throttle
cap then held the engine at about 3270 rpm with nothing to pull.

**Fix.** `pulling_away` also requires a gear (`g != 0`) and the pedal not fully pressed
(`clutch_pedal < 1`).

**Tests.** `qa_phys_8_engine_revs_freely_in_neutral` and a new
`qa_phys_8_engine_revs_freely_with_the_clutch_pressed` (1st gear, pedal down, full throttle: above
6000 rpm and the car stays put). Both failed before the fix.

## QA-PHYS-9: the automatic gearbox stays in neutral

**Root cause.** `auto_shift` returned early for any gear below 1.

**Fix.** In neutral, throttle above 5 % engages 1st, as selecting D would. This runs after the
usual post-shift hold (2 × shift time), so a quick double tap of shift-down still reaches reverse.
Reverse stays engaged under throttle. The AI's reverse manoeuvre switches to manual and is
unaffected.

**Tests.** `qa_phys_9_automatic_gearbox_engages_drive_from_neutral` and a new
`qa_phys_9_automatic_gearbox_keeps_reverse`.

## Regression checks

Run on `fix/dynamics` at `64c478b`, after all the fixes. The race-golden regeneration (last item)
was committed after that.

**Lints.**

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean.
- `cargo clippy -p racer-game --all-targets --features wheel --locked -- -D warnings`: clean.
- `CARGO_FEATURE_PURE=1 cargo clippy --workspace --all-targets --locked --target <t> -- -D warnings`
  for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin`: both clean.

**`cargo test --workspace --locked`.** All pass. The other groups' QA repro tests are still
ignored, as they should be.

The first run had one failure: `racer-trackc`'s `generated_data_matches_the_track` reported
`bmw-m3-competition-g80: car model changed`. The track's car models are fitted from the vehicle
dynamics. The PHYS-3 slip change acts during launch wheelspin, so only the low-speed bins of the
fitted `drive` tables moved (M3 first bin 6.50 → 6.73). Lap times moved by at most 0.001 s (GTI
88.413 → 88.414, M3 81.296 → 81.295, 911 unchanged).

I regenerated with exactly `cargo run --release -p racer-trackc -- compile data/tracks/downtown-loop`
(commit `64c478b`, generated files only). **Note for fix/ai and fix/tooling:** this data is
derived. If either branch regenerates it too, the merge conflict resolves by re-running that
command on the merged tree, not by picking a side.

**M1 reference maneuvers** (`cargo test -p racer-dynamics --release --test maneuvers`). All
three cars stay within the M1 tolerances, and the figures are identical to before the fixes
except the M3 0-100 (4.06 → 4.05 s):

| Car | 0-100 s (±8 %) | 100-0 m (±8 %) | Skidpad g (±0.08) | Top km/h (±4 %) |
|---|---|---|---|---|
| Golf GTI | 6.00 / 6.30 | 36.66 / 36.40 | 0.96 / 0.97 | 250.0 / 250 |
| M3 Competition | 4.05 / 3.90 | 34.89 / 36.00 | 1.04 / 1.03 | 250.0 / 250 |
| 911 Carrera S | 3.57 / 3.70 | 31.26 / 31.40 | 1.05 / 1.06 | 313.1 / 308 |

(sim / real.) The step-steer stability tests pass unchanged. The Avian side doesn't feed these
figures: they come from the testbed.

**`tools/telemetry.sh`.** All 9 runs (3 cars × lap, skidpad, launch-brake) and the plots were
written. In the GTI launch-brake plot, the car stops at 12.2 s with a short damped settle and no
creep afterwards; slip ratios stay within ±0.25 except on the stop itself. The M3 lap looks as in
M1. The single ~50 kN load spike is the crest landing, a known M1 artefact (QA measured 48 kN).

**Capture goldens** (`tools/capture.sh <scene>`, then `racer-capture diff`):

| Scene | Result |
|---|---|
| greybox | 0.000 mean error on all 3 poses |
| menu | 0.000 on both poses |
| driving | all 6 pass (≤ 0.159 mean error) |
| race | **changed**, goldens regenerated (below) |

At the branch base, the race goldens match bit for bit; with the fixes, `hud`, `pack` and
`results` differ. I bisected the cause. With only the old `racer-sim` contact code restored,
`pack` drops from 10.3 to 1.9 mean error, and restoring the old `track.rs` alone brings back the
old behaviour. So almost all of it is **QA-PHYS-4**, and it was bigger than the report thought.

A GTI launched from the back of the city grid on the old code had 3090 wheel-travel jumps over
1 cm in 8.5 s, with loads to 6.5 kN, while driving on the road trimesh. Wheel casts gather
candidates from every ground collider, and the GJK failure against the ~2 km city ground box
under the road produced bogus hits as well as misses. With the half-space ground there are 0
jumps, the peak load is static (4.6 kN), and the GTI reaches 100 km/h in 6.0 s from the grid, as
in the testbed, instead of about 6.5 s.

The new race captures are the correct physics, so I regenerated `crates/racer-capture/goldens/race/`
from `CAPTURE_PROFILE=release tools/capture.sh race`. A fresh capture matches them bit for bit.

**AI races** (`racer-capture ai-races --races 12 --laps 3`, the M2 review's run). This is for the
AI fixer's before/after baseline, since the contact fixes change the races:

| | Contacts | Resets in traffic | Resets alone | Rows |
|---|---|---|---|---|
| branch base | 458 | 15 (6.1 % of 246 laps) | 0 | all ok |
| fix/dynamics | 345 | 8 | 3 | Pro and Expert **FAIL** |

I replayed all three "alone" resets (race 5 Pro 911 twice at ~1450 m, race 11 Expert 911 at
1875 m). In each, the AI brakes to a standstill 3.5-8 m behind a stopped car, with all wheels in
contact, normal loads and no wall contact. It waits and is reset as Stuck. That is QA-AI-1 (AI
waits behind a stopped car instead of driving round it) showing up in a reshuffled race, not a
vehicle-physics problem, so I left it to fix/ai. Their exit check should compare against the
fix/dynamics numbers above (or merge this branch first).

## Left open

- **Input path.** Sanitizing controls in `Vehicle::step` protects the model. A server that wants to
  reject or log malformed client input still needs its own check (cross-area note in QA-PHYS-7).
- **PHYS-5 at the pad edge.** The face test uses a 5 mm tolerance and the edge contact uses the cast
  distance, which on big boxes is a few mm short (PHYS-4's precision issue, at smaller scale). On
  the 130 m pad's own edge this is at most a few mm; I didn't make the pad a trimesh.
- **Spec fields the model doesn't use.** These are not validated: published peaks, reference
  figures, brake disc sizes, and the ordering of `upshift_rpm`/`downshift_rpm`. None of them can
  panic.
