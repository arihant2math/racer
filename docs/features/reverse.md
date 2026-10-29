# Reversing

Branch `feat/reverse`. Players can now drive backwards the way racing games do it: in the
automatic, hold the brake at a standstill; in the manual, shift down from neutral.

## What it does

### Automatic gearbox (the default)

| Situation | What happens |
|---|---|
| Brake held (≥ 20 % pedal, throttle ≤ 5 %) with the car below 0.3 m/s either way | After **0.6 s** (`REVERSE_HOLD`) reverse engages. |
| In that reverse | The pedals swap: the brake pedal is the reverse throttle (analog, proportional), the throttle pedal brakes. |
| In reverse, throttle pressed while the car is below 0.3 m/s backwards (or rolling forwards) | Straight back to **1st**. Holding the throttle while reversing faster first brakes the car, then selects 1st and drives off. |
| Any gear, reversing faster than **30 km/h** (`REVERSE_TOP_SPEED`) | The engine is cut (1 m/s taper), in the manual too. Without it the gearing alone allows 69 to 87 km/h backwards. |
| Countdown on the grid | Race control holds the cars with `Controls::hold`; the held brake never engages reverse. |

The hold time and speed thresholds were picked so a reverse never fights a stop: a full ABS stop
from 100 km/h followed by the brake held a moment longer stays in 1st, a light brake rolling to a
stop stays in 1st, brake and throttle together (launch control) never selects reverse, and the
countdown hold never does. 0.6 s is long enough that a stab at the brake while stopped does
nothing, and short enough to feel immediate when you mean it (the 0.6 s overlaps the time it takes
to move your foot anyway).

The logic is all in `racer-dynamics` (`Drivetrain::auto_reverse` in `drivetrain.rs`, the pedal
swap, the reverse speed cap and the reverse-aware traction-control slip in `vehicle.rs`). The
hold timer and the brake-reverse flag are new `DriveState` fields (`reverse_hold`,
`brake_reverse`), so they are part of `VehicleState`: rollback
copies it and the snapshot's `own` state carries it, so the client predicts reverse exactly from
the pedals.

### Manual gearbox

Unchanged and checked: from 1st, shift down twice at a standstill (1 → N → R; `shift` refuses
reverse above 1 m/s forwards). Q, the gamepad's left bumper and the wheel's left paddle all do
it (tests below). The pedals are normal: throttle drives backwards, brake brakes. The 30 km/h
reverse cap applies.

In the automatic, selecting R with the lever (two shift-downs at rest) still works as before
(QA-PHYS-9): the throttle drives backwards and reverse stays until the driver shifts out. Only a
reverse engaged by holding the brake swaps the pedals and returns to 1st on the throttle;
`DriveState::brake_reverse` tells the two apart (any lever shift clears it).

### Interactions checked

| | |
|---|---|
| Launch control | Needs gear > 0, so it never runs in reverse. Brake held into reverse and then throttle added: back to 1st on the throttle, launch control takes over (test). |
| Countdown hold | `Controls::hold` set by `race::hold` offline and on the server, and by the client's `inputs` before the start tick. Not in the input message: both sides derive it from the race clock. |
| ABS | Works on the wheel speed sign already; in reverse the brake is the throttle pedal and ABS acts as forwards. |
| TCS | Driven-wheel slip is now measured in the drive direction in reverse (before, a wheel spinning backwards read as braking slip and TCS never trimmed). Forward gears unchanged. |
| ESC | Off below 5 m/s forwards as before, so it does nothing in reverse. |
| Rev limiter | Only reached in reverse if a car's gearing tops out below the 30 km/h cap (none of the three does). |
| AI | Always drives the manual gearbox (`automatic: false`), so the automatic's reverse never touches it. Its reverse recoveries run below 3 m/s, far under the cap. The TCS slip change applies to its reverse recoveries. |

### Input

Nothing changed in the device mapping: reverse comes from the brake pedal, which is already analog
on the gamepad trigger and the wheel pedal, and digital on S/↓. `Controls` gained `hold`, which
`apply_to_car` sets to false. The input message (`NetControls`) is unchanged: reverse is derived
from the pedals, the hold from the race clock.

### HUD, camera, lights

The race HUD already showed `R` and the absolute speed; the driving scene's debug HUD now shows the
speed as a positive number too (it was negative in reverse). I left its help line alone so the
driving goldens stay as they are. The chase camera follows the car's heading, not its velocity, so it stays behind
the car when reversing (a test drives the camera through a reverse). New: **look back**, held B on
the keyboard or the right stick button on a gamepad, turns the chase or hood camera round.
The greybox car body has two small white **reverse lights** in the tail, lit while the car is in
reverse (hidden otherwise, so the goldens do not change). Built car models get none.

### Race rules

No rules code changed. Checked with real car physics through the race plugin:

| Case | Result |
|---|---|
| 1.5 m reverse mid-lap at full pedal, then drive on | No wrong-way warning, no reset, no gate lost. |
| Over the line, brake into reverse, back over it, throttle over it again | Lap taken back, then counted once; the re-credited lap is timed from the race start, as the QA-RULES-4 fix intends; no reset. |
| Rules level: 2 m back over the line at 2.5 m/s | `LapUndone`, no warning; driving over again gives one `Lap`. |
| Rules level: 10 m back at 2.5 m/s (4 s) | Warning comes on (the rule as intended), no events, lap count unchanged. |

**The wrong-way warning is triggered by reversing longer than about 1 s.** The rule (unchanged)
flags 1 s of moving backwards along the track above 1 m/s, whichever way the car faces. At full
pedal that is about 2 m of reversing; a 1.5 m reverse stays under it. I left the rule alone: the
QA-RULES-1 regression test (`reset_after_reversing_over_a_gate_must_not_strand_the_car`) relies
on reversing facing forwards to freeze `last_valid` after a second. See Known gaps.

## Verification

### Tests added

`crates/racer-dynamics/tests/reverse.rs` (all three cars each):
`holding_the_brake_at_a_standstill_engages_reverse_and_drives_backwards` (0.48 s held: still
1st; 0.6 s: reverse; reverse top speed between 28.8 and 30.5 km/h),
`the_throttle_brakes_a_reversing_car_then_goes_back_to_first` (never speeds up backwards on the
throttle, leaves reverse only below the standstill threshold, then > 5 m/s forwards),
`no_reverse_when_stopping_hard_or_while_rolling_forward` (ABS stop from 100 km/h plus a hold, light
braking to a stop, launch control), `race_control_hold_never_engages_reverse`,
`launch_control_works_after_reverse_was_engaged_on_the_spot`,
`analog_brake_in_reverse_is_proportional_throttle` (25 %, 50 %, 100 % pedal give increasing
speeds), `manual_reverse_needs_a_standstill_and_the_throttle_drives_it`.

`crates/racer-game/src/input.rs`: keyboard S held → reverse, W → 1st and forwards; manual Q twice
→ R and W drives it; gamepad left trigger at 35 % and 100 % gives proportional reverse; gamepad
left bumper twice and wheel paddle twice select R in the manual; wheel brake pedal at 30 % and
100 % gives proportional reverse. Gamepads are simulated with bevy's raw gamepad events, the
wheel with a scripted `WheelDevice`.

`crates/racer-game/src/driving.rs`: the chase camera stays behind a reversing car.

`crates/racer-sim/tests/reverse.rs`: the race-rules cases in the table above.

`crates/racer-server/tests/net_race.rs`:
`reversing_in_the_automatic_is_predicted_like_the_server`: a server and one client over UDP
(60 ms RTT); the client's scripted driver throttles, holds the brake through the stop into reverse
and backs up, then throttles out. Both the client's prediction and the server's car go into R,
and the prediction needs at most one correction over 5 cm.

### Existing tests touched

Five existing tests held the brake at a standstill in the automatic for several seconds as "a
parked car", which is now exactly the reverse gesture. Their checks are unchanged:
`fuzz.rs` `rest_is_quiet` and `jump_landing_settles`, and `qa_vehicle_physics.rs`'s three QA-PHYS-3
tests, now hold the car with `hold: true` (race control's hold: bit-identical physics, the
automatic just does not engage reverse). `qa_ai.rs`'s `park` (a human waiting on the brakes) now
uses the manual gearbox, where the brake only brakes. `fuzz.rs`/`properties.rs` random controls set
`hold: false` (the new field; the random streams are unchanged).

### Forward driving and AI unchanged

Same release binary build settings, same seeds, run from untouched `e973a3f` (before) and this
branch (after):

`racer-capture telemetry` (what `tools/telemetry.sh` runs; scripted lap, skidpad ramp, launch and
ABS stop for all three cars): **all 9 CSVs are byte-identical** before and after. For reference:
GTI lap 51.08 s (max 119.3 km/h), launch-brake 12.60 s; M3 lap 48.62 s (133.2 km/h),
launch-brake 9.70 s; 911 lap 47.93 s (136.3 km/h), launch-brake 8.93 s. The launch-brake
maneuver holds the brake 0.5 s after stopping, under the 0.6 s reverse hold, so it is unchanged
too.

`racer-capture ai-laps --laps 5` (the solo half of `tools/ai-check.sh captures/ai 5 4 5`): the
`solo.md` report and all 15 per-car, per-difficulty trace CSVs are **byte-identical** before and
after. Mean flying laps, before = after: GTI 103.53 / 98.44 / 94.17 / 90.48 / 87.87 s (Novice to
Alien), M3 95.80 / 91.09 / 87.25 / 84.22 / 82.41 s, 911 94.97 / 90.08 / 86.34 / 82.88 / 80.93 s;
0 resets and 0 stalls everywhere; every difficulty inside its target band.

`racer-capture ai-races --races 4 --laps 5` (the mixed half): `mixed.md` is **byte-identical**
before and after: 144 laps, 32 contacts, 0 resets in traffic or alone, 0 stalls over 10 s,
longest stall 3.0 s, nothing permanently stuck, every difficulty ok. (Identical output means
the reverse-only TCS change did not alter these races either.)

Captures: `racer-capture scene driving` with this branch's release binary under Xvfb/lavapipe
diffs within tolerance against the committed driving goldens on all six poses (mean error
0.03-0.16, 0.04-0.31 % differing pixels). I looked at `chase.png`: same scene and HUD; the only
visible difference is the HUD reading `gear N, 0 rpm` because the release build rendered before
the first physics tick, which has nothing to do with this branch. No goldens were regenerated.

Checks before pushing: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
warnings`, cross-target clippy for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` with
`CARGO_FEATURE_PURE=1`, and `cargo test --workspace` (all passing, exit 0).

## Known gaps

- **Wrong-way warning on longer reverses.** Reversing for more than about 1 s above 1 m/s (about
  2 m at full pedal) shows "wrong way" for a moment, because the rule counts any backwards motion
  along the track, whichever way the car faces. A better rule would ignore backwards motion while
  the car faces along the track, but QA-RULES-1's regression test relies on that motion freezing
  `last_valid`, so the change needs the reset logic revisited with it. I left the rules alone.
- **Remote cars reversing** are extrapolated between snapshots as if braking (the extrapolation
  model in `net/extrapolate.rs` does not know the gear or the pedal swap). Snapshots come at 60 Hz
  and reversing is slow, so the error is small; the local car's prediction is exact.
- **Reverse lights** exist only on the greybox body; built car models (`assets/`) have none. No
  capture shows them lit (the scripted scenes never reverse); a unit test checks the camera, not
  the lights.
- **No golden or screenshot of reversing** (the game runs at about 1 fps here and the capture
  scenes are scripted forwards). The HUD's `R` and positive speed come from code that already
  existed in the race HUD.
- **Help text**: the driving scene's help line does not mention "hold S to reverse" or "B look
  back", to keep the driving goldens unchanged; the README's controls line does.
- The no-progress backstop (10 m per 7 s) still applies: a driver who spends 7 s reversing and
  dithering is reset, as before.

## Review

Independent review of this branch, adversarial, against the brief (gearbox and pedals, netcode,
AI and race rules, camera/HUD/lights, gate).

### Checked, found sound

| Area | What I checked |
|---|---|
| Hard stops, slow corners | `auto_reverse` needs the car below 0.3 m/s *and* 0.6 s of brake ≥ 20 % with throttle ≤ 5 %; any motion above 0.3 m/s restarts the hold. A corner at walking pace never stands still long enough. |
| Shunt from behind, walls | A push above 0.3 m/s while waiting on the brake restarts the hold (new test). Nose in a wall with the brake held is the reverse gesture, as intended. |
| Slopes | Rolling back down a 12 % grade in a forward gear, the brake first stops the car and reverse only comes after a full 0.6 s standing; off the pedals in R the car rolls back and stays in R; the throttle brakes it to a stop, selects 1st and climbs (new test). Side note, not this branch: the automatic upshifts on wheel speed while rolling backwards. |
| R/1st oscillation | Each change needs the other pedal (R → 1st on the throttle, 1st → R on the brake with the throttle off), so there is no flicker at the 0.3 m/s threshold. Both pedals in a brake reverse: 1st, and no gear change over 3 s (new test). A brake hovering at 18–22 % only restarts the hold (new test). |
| Handbrake | Not an input to `auto_reverse`; the brake with the handbrake engages reverse as the brake alone does. |
| Reset, restart, pause, finish | A reset puts `initial_state()` back (1st, timer 0, no swap); a restart spawns a new car; pause stops the fixed steps, so the timer stops too; after the finish nothing takes over the player's car (autopilot is only inserted at the start). |
| Automatic → manual mid-reverse | The swap needs `automatic`, so in the manual the brake brakes and the throttle drives back; a lever shift clears `brake_reverse` (new test). |
| Countdown `hold` | The server's `race::hold` runs after `apply_inputs` in the same tick and only while `RacePhase::Countdown`; the client's `inputs` sets it for `tick < start`. That is the same boundary the existing `brake = 1.0` hold already used, and `check_start` logs if the server's start tick ever differs. Every controls writer (keyboard/pad/wheel, AI, net decode, autopilot) replaces the whole `Controls`, so `hold` never lingers past the start. |
| Snapshots | `DriveState` is serde-derived inside `VehicleState`; the snapshot's `own` carries it whole and rollback restores it. There is no separate hash. |
| Bindings | B and the right stick button are bound nowhere else (keyboard: WASD/arrows, Space, C, E/Q, R, V, T, K, 1–3; pad: South, North, Select, Start, bumpers, triggers, D-pad, East). |
| HUD | Both HUDs show `R` for any `gear < 0`, which covers both gearbox modes. |
| Camera | The chase camera follows the heading, so it stays behind the nose while reversing at the 30 km/h cap (about 0.8 m of lag). A fast spin still snaps through the heading lerp as before; nothing new. |

### Found and fixed

1. **Rollback missed reverse-state mispredictions.** The client compared only the gear and the
   wheel speeds with the server's `own` state, so a `reverse_hold` timer a tick out, or a
   different `brake_reverse`, went uncorrected until reverse engaged a tick apart (a jolt later).
   `state_off` in `net/client.rs` now compares both fields too.
2. **A longer reverse got the car reset.** The no-progress backstop (10 m of progress in 7 s)
   counted from the last 10 m mark, so backing up more than a few metres and driving on was reset
   as Stuck: 12 m back over the line and forward again was reset at 7.4 s (8 m too). While the car
   is in the automatic's brake-engaged reverse, `ResetMonitor::backing_up` now holds the clock
   and moves the anchor back with the car, so the 10 m count starts from the furthest point back.
   Rocking in place is still caught (the clock runs whenever the car is not reversing; caught at
   15 s instead of 7 s with half the time spent reversing). The AI drives the manual and never
   sets `brake_reverse`, so AI resets are unchanged. A lever reverse in the manual does not
   get this. Tests: `a_longer_reverse_over_the_line_counts_the_lap_once` (12 m with real physics,
   no reset, lap counted once), `a_longer_reverse_warns_then_clears_without_a_reset`, and
   `backing_up_holds_the_no_progress_clock_but_rocking_is_still_stuck` in `rules.rs`.
3. **A resting right trigger blocked reverse on a gamepad.** Triggers shared one 5 % deadzone
   that only gated *whether* the pad was read: with the left trigger down, the right trigger's
   raw value went straight through, so a trigger that rests at 6–8 % read as throttle above
   `PEDAL_OFF` and the automatic never engaged reverse (or dropped straight back to 1st). Each
   trigger now has its own 10 % deadzone (values above it pass through unchanged, as
   QA-UI-4's test expects). Test:
   `a_right_trigger_resting_above_zero_does_not_block_reverse`.
4. **No reverse lights on built car models.** Models now get the same two lights just behind
   the tail (`len / 2 + 1 cm`, the greybox's height), hidden until reverse. Built models are not
   in the repo (`/assets` is ignored), so I could not view them; the placement comes from the spec
   dimensions and may want a tweak against the real meshes.

### Gate

`cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, cross-target
clippy for `x86_64-pc-windows-msvc` and `aarch64-apple-darwin` with `CARGO_FEATURE_PURE=1`, and
`cargo test --workspace --no-fail-fast`: all exit 0 on the pushed tree, after merging `main`
(`feat/options`, c1444c5) in. The one conflict was the driving HUD's speed: it keeps main's
`units.format` with this branch's absolute speed. `flow::release_controls_in_menus` zeroes the
pedals while a pause menu is open, so a reverse hold restarts and nothing engages behind a menu.
Look-back reads B and the right stick button directly, and neither is a menu key.
The first gate run caught my first trigger fix (a rescaled deadzone) breaking QA-UI-4's
`keyboard_steering_survives_a_gamepad_trigger`, which expects 0.6 to pass through exactly; the
cutoff deadzone keeps it.

### Not done, remains

- **Not re-run: `tools/ai-check.sh`, `tools/net-check.sh`, `tools/capture.sh`.** Each needs a cold
  release build, and with 4 cores that did not fit in the 2 h budget next to two debug gates. Why
  I expect no change: the AI drives the manual, so it never sets `brake_reverse`, and the
  reset-monitor change only acts on that flag. The net-check bots are the same AI. The rollback
  check only adds conditions to `state_off`, so it can only add corrections where the reverse
  state really differed. The reverse lights are hidden unless the car is in reverse, and captures
  never reverse. The TCS claim (byte-identical AI reports) is the author's measurement; I
  did not repeat it.
- No scripted reversing in `net-check.sh`. `reversing_in_the_automatic_is_predicted_like_the_server`
  (UDP, 60 ms RTT) is still the netcode coverage for reverse.
- Remote cars' extrapolation still ignores reverse (author's known gap).
- The wrong-way warning still flashes on a reverse longer than about 1 s (author's known gap; my
  longer-reverse test checks that it clears once the car drives on).
- A lever reverse in the manual does not get the reset monitor's `backing_up`: a manual-gearbox
  human who reverses far can still be reset by the no-progress backstop.
- Hood look-back turns the hood camera round in place, so it looks back through the car body
  (back faces are culled, so the cabin is see-through). A camera at the tail would be nicer.

### Follow-up: the 0 ms net-check failure

The orchestrator's `tools/net-check.sh out 1 2` on 84339c8 failed at 0 ms RTT: bots 0 and 1 both
corrected by about 1.4 m at the same tick (race time 150.04 s, rollback depth 12 and 13). That was
outside every contact (the last server contact was at race tick 4364) and every reset (none). 150 and
250 ms passed. **It is not this branch: `main` fails the same way.**

| Build (both with the same print-only instrumentation) | 0 ms, 2 races × 2 laps, 8 bots | Result |
|---|---|---|
| feat/reverse 84339c8 | position p95 0.010 m, p99 0.010 m, max 0.010 m; 0 desyncs | PASS |
| main c1444c5 | position p95 0.199 m, **p99 0.744 m**, max 0.744 m; 1 desync (race 1, bot 1, 118.4 s, 0.74 m, no car within 17 m, last server step over 20 ms 24 s earlier) | **FAIL** |

Why the branch cannot cause it. The bots are the AI, which drives the manual gearbox. So
`auto_reverse` never runs for them, `reverse_hold` stays 0 and `brake_reverse` stays false, and
`ResetMonitor::backing_up` (server rules only, and only on `brake_reverse`) never fires. The
`state_off` change can only add rollbacks. Client and server run the same `racer-dynamics`, and
the new fields travel in `own`, so the TCS slip change applies identically on both sides. On the
client, remote car bodies have collision filter `NONE`, so a client-only contact is impossible
too.

What it looks like (from main's instrumented desync): server and prediction agree on gear and
wheel speeds and differ in velocity by 0.12 m/s, but sit 0.74 m apart along the direction of travel
at 29.6 m/s. That is about 6 ticks of motion at depth 7. The orchestrator's case (1.4 m at about
35 m/s, depth 12) fits the same pattern. A time shift like this, not different physics or inputs,
suggests the client's tick clock (`advance_tick` follows lightyear's `LocalTimeline`) advanced
without simulating some steps, on two bot processes at once (CPU starvation on the shared
machine). **Unconfirmed**: I ran out of budget before a starved run with a tick-jump print. It is
intermittent (3 races passed on M3's main for the orchestrator, my 2 branch races passed) and
belongs to the M3 netcode, not this branch, so I did not widen the branch for it.
