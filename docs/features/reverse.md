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

RESULTS_PLACEHOLDER

## Known gaps

GAPS_PLACEHOLDER
