# QA: AI drivers

Branch `qa/ai-drivers`, on `4253183` (M2 review). Scope: the AI controller and racecraft
(`crates/racer-sim/src/ai.rs`, `drive_ai` in `race.rs`), the racing line and speed profiles
(`crates/racer-trackc`), difficulties, catch-up, and the AI verification tooling. Tested headless
in release builds on the city track, 2026-10-27, same kind of container as M2 (4 cores, no GPU).

Repro tests live in `crates/racer-sim/tests/qa_ai.rs`. Every bug test is `#[ignore = "QA-AI-N"]`,
so the normal suite stays green. The passing checks are ignored as `QA check` because they are
slow in a debug build. Run all of them in about 25 s:

```text
cargo test --release -p racer-sim --test qa_ai -- --ignored
```

All six bug tests fail on `4253183`. The three checks pass. `diag_first_lap_resets` is the trace
tool used for the pile-up analysis (`QA_RACE=7 QA_SECS=56 cargo test --release -p racer-sim --test
qa_ai -- --ignored --nocapture diag_first_lap_resets`). It runs the same field as
`racer-capture ai-races` race N and prints the last 10 s of every car near each reset. Races are
deterministic, so the seeds below reproduce exactly.

## Summary

| ID | Severity | Bug |
|---|---|---|
| QA-AI-1 | major | AI stops behind a stopped car on an open road, waits, and is reset as `Stuck` instead of driving round it |
| QA-AI-2 | major | Inside passes at corner entry steer the AI into the inner wall (turn 1 at ~369 m, right-hander at ~946 m). This starts the first-lap pile-ups |
| QA-AI-3 | major | AI ignores ghosted cars completely; when a reset car turns solid ahead, the AI hits it at ~33 m/s |
| QA-AI-4 | major | A car pushing against a wall or another car at 0-3 m/s is not reset for 11-12 s and blocks the corner |
| QA-AI-5 | minor | A finished car on its cool-down lap still defends its line against racing cars |
| QA-AI-6 | minor | A car right behind the leader counts as "alongside": the leader is marked squeezed and slows 8 % in front of it |
| QA-AI-7 | minor | A car reversing towards the AI is read as moving forward (code reading, no repro) |

Totals: **4 major, 3 minor, 0 critical.**

## Root cause of the first-lap pile-up resets (M2 review gap)

I re-ran the 12 `ai-races` fields (indices 0-11) for 70 s each with the trace tool. The resets
match the review exactly: 14 `Stuck` resets in 6 of the 12 races, all at 370-383 m (20-25 s) or
948-962 m (50-55 s). Each one is a chain of three distinct defects.

1. **Trigger: an inside pass puts a car into the inner wall (QA-AI-2).** Races 0 and 2 (turn 1)
   and race 7 (~946 m) each start with an Alien in `overtaking` on the inside of a right-hander.
   The car's body touches the `Wall` piece at a lateral offset of 5.3-6.2 m. Speed drops from
   16-22 m/s to ~5 m/s in 0.05 s. For example, race 0 has the Alien M3 at 369.7 m at 15.45 s,
   and race 7 has the Alien GTI at 945.9 m at 40.65 s. Race 8's Alien ends up pinned at lateral
   6.7 m the same way. Both corners are right-handers whose racing line crosses the road from the
   far left to the apex just before the corner. That is why the trouble is at these two places
   and not at the left-handers.
2. **Second car: contact with the stopped car.** The next car arrives in the braking zone,
   often itself mid-pass. In race 7 the Expert GTI was overtaking and ran into the Alien. It is
   then pinned too.
3. **Amplifier: everyone queued behind waits for 3 s and is reset (QA-AI-1, QA-AI-4).** Cars
   behind brake to `target 0` from the traffic cap and sit there with the throttle closed. They
   cannot reach the pass lane at standstill, and the AI's own stuck check never runs because it
   requires the throttle above 0.3. `ResetMonitor` then resets each of them as `Stuck` 3 s after
   it stopped. About 8 of the 14 resets were cars in exactly this state. Race 7 resets three
   cars within 0.15 s, and race 10 resets three within 0.15 s. The crashed cars themselves keep
   moving at 0.5-3 m/s against the wall. That defeats both stuck timers (QA-AI-4), so they block
   the corner for up to 12 s while the queue grows.

The first lap is where this happens because the pack is still dense. The 5 s no-passing window
ends before turn 1 (which comes at ~15 s), and the pass logic picks the inside of the next corner
first. In an all-Alien 8-car field the same turn-1 jam costs the pole car 8 s without any reset:
it is passed, joins a 6-car knot at 380-395 m, and crawls for ~8 s.

Fixing only the reset rate (for example, a longer `RESET_AFTER`) would hide this. The fixes
that remove the chain are, in order of payoff: keep pass paths inside the bounds *at the corner
ahead* and never past `hi`/`lo` (QA-AI-2); let a car held at `target 0` behind a stopped car
pick a lane and creep round it, or reverse (QA-AI-1); and detect "pushing but going nowhere" by
progress rather than instantaneous speed (QA-AI-4).

## Bugs

### QA-AI-1 (major): AI does not drive round a stopped car; it waits and is reset

- **Repro:** `qa_ai_1_a_stopped_car_on_the_line_is_driven_round_without_a_reset`. A player
  stands on the racing line at 200 m on the 13 m wide main avenue, with nothing else around. A
  Pro GTI arrives from the grid.
- **Expected:** it drives round on the free right side of the road and carries on.
- **Actual:** it brakes to a stop 6.5 m behind (193.5 m) with its lateral offset at -3.14 m, just
  short of clear. It waits with the throttle closed and is reset as `Stuck` at 16.3 s. After the
  reset it is ghosted and gets past. In a real race the same thing happens to every car queued
  behind a crash (see the root cause above).
- **Why:** `traffic_cap` (`ai.rs:704-734`) drives the cap to `o.speed + gap`, which reaches 0 at
  the stopped car, before the car has moved sideways. The "obstacle" pass starts only at
  `d < 20 m` (`ai.rs:743`), and the path moves sideways at 1.2-2.6 m/s through `shift`
  (`ai.rs:682-683`). Once the car has stopped it cannot move sideways at all, so the pass can never
  finish. The blocker test (`ai.rs:712-713`) still counts the car as in the way at 2.1 m lateral
  separation. The AI's stuck/reverse recovery requires `controls.throttle > 0.3` (`ai.rs:813`),
  so a car held by the cap never tries anything. `ResetMonitor` (`rules.rs:263`) then resets it
  after 3 s below 1 m/s.
- **Confidence:** high (deterministic repro, traced).

### QA-AI-2 (major): inside passes at corner entry hit the inner wall

- **Repro:** `qa_ai_2_an_inside_pass_at_turn_1_does_not_hit_the_wall`. This is `ai-races`
  race 0, first 17 s, full contact. The Alien M3 touches the `Wall` at 369.5 m, lateral 6.15 m,
  15.4 s, with `overtaking: Some(..)`. Second instance: race 7, the Alien GTI at 945.9 m,
  lateral 5.30 m, 40.65 s (`diag_first_lap_resets` with `QA_RACE=7`).
- **Expected:** a pass takes a lane that stays on the road through the corner, or is abandoned.
- **Actual:** the car's path sweeps to the inside edge at corner entry and the body hits the
  inner wall at 16-22 m/s. In race 7 the lateral offset went from 4.57 m to 5.30 m in 0.1 s at
  22 m/s, a heading about 18° towards the wall. The walls themselves are where the geometry says
  (8.0 m and 7.0 m from the centre; I checked the wall mesh vertices).
- **Why:** several things combine.
  - The pass lane is an absolute offset (`ai.rs:627`). It is converted to a shift *relative to
    the racing line* (`target = want - my_line`, `ai.rs:629`), and that shift is rate-limited
    (`ai.rs:682-683`). Approaching these right-handers the line itself crosses the road at
    ~0.3 m per metre (turn 1: -4.3 m at 355 m to +4.3 m at 379 m). The resulting path offset
    therefore jumps towards the inside much faster than the shift rate intends.
  - The path is clamped only to `hi + 0.3` (`ai.rs:386-389`), which is already 0.5 m onto the
    curb. The lane test checks the bounds at the car's current sample (`ai.rs:628`, `755-766`),
    not at the corner it is driving into.
  - Passes are started and kept up inside the braking zone. A pass is only abandoned there if
    the car is still behind by 0.6 car lengths (`ai.rs:621-622`).
  - The car arrives at the clamp boundary at a steep angle, overshoots it (6.15-6.2 m against a
    clamp at 6.03 m), and its front corner reaches the wall.
- **Confidence:** high that inside passes cause the wall hits (contact logs). Medium on the exact
  weighting between the rate-limit artefact and the clamp margin.

### QA-AI-3 (major): ghosted cars are invisible to the AI until they turn solid

- **Repro:** `qa_ai_3_a_car_turning_solid_ahead_is_not_rammed`. A car stands on the racing line
  at 250 m with 2 s of ghosting left, as after a reset. A Pro M3 arrives at 38 m/s from 150 m.
- **Expected:** the AI anticipates a car that will become solid, or the rules keep it ghosted
  while a car is closing fast. Either way, no high-speed impact.
- **Actual:** the AI drives straight at it. Ghosting ends when the AI is ~27 m away, because
  `update_collisions` only checks for a car within 5.5 m. The AI brakes fully at ~10 m/s² from
  39 m/s but hits the standing car at ~33 m/s and knocks it to 20.9 m/s. After a real reset the
  car is placed 8-38 m *behind* its last valid point, in the path of the following traffic, so
  this is the "reset, then immediately crashed again" loop.
- **Why:** every AI query skips non-solid cars (`ai.rs:704`, `664`, `642-644`). The rules side
  (`race.rs:424-427`, cross-area: race rules) decides solidity from distance only, not closing
  speed.
- **Confidence:** high.

### QA-AI-4 (major): a car pushing against an obstacle is not reset for 11-12 s

- **Repro:** `qa_ai_4_a_car_pushing_against_an_obstacle_is_reset_within_a_few_seconds`. This is
  `ai-races` race 7, first 56 s. The Alien GTI (wall hit at ~946 m) makes no 10 m of progress for
  12.4 s without a reset. The Expert GTI that ran into it is stuck for ~11 s (43.5-54.9 s), and
  the two cars queued behind are reset as well.
- **Expected:** a car that cannot get going is reset after the rules' 3 s, or recovers.
- **Actual:** the crashed cars alternate between full throttle at 0.4-3 m/s and short
  reverse/pull manoeuvres that go nowhere.
  - The rules' timer (`rules.rs:263`) resets to 0 whenever the speed passes 1 m/s even for a
    tick.
  - The AI's stuck check needs 0.8 s *continuously* below 0.8 m/s with the throttle open
    (`ai.rs:813-819`).
  - Its progress check counts 4 m of motion either way, reversing included (`ai.rs:805-808`).
  - Only the 12 s progress timeout (`ai.rs:878`) eventually asks for a reset. That fits the
    13-14 s "longest stall" in the M2 review tables.
- **Confidence:** high for the 11-12 s block. Medium on which of the three checks should change
  (the rules timer is cross-area).

### QA-AI-5 (minor): a finished car still defends on its cool-down lap

- **Repro:** `qa_ai_5_a_finished_car_does_not_defend_against_racing_cars`. An Expert GTI with
  `Status::Finished` is at 290 m, and a racing Alien 911 closes from 280 m before turn 1.
- **Expected:** a car on its cool-down lap yields, or at least holds its line.
- **Actual:** it starts `defending` (3 s) and moves 3 m across to the inside of turn 1, in front
  of a car that is still racing. Cool-down cars also drive the racing line at 70 % speed
  (`race.rs:542`), so the field finishing behind the winner has to pass them at turn 1.
- **Why:** nothing in the AI knows the car has finished. The defending (`ai.rs:641`), passing and
  "rams back" logic all stay on, and `drive_ai` only lowers `catch_up` (`race.rs:536-543`).
- **Confidence:** high.

### QA-AI-6 (minor): a car right behind squeezes the leader

- **Repro:** `qa_ai_6_a_car_close_behind_does_not_squeeze_the_leader`. A Pro leader at 150 m
  (lateral -4.6 m) has a car 5.6 m behind it (0.8 m bumper gap, lateral -4.4 m) and a car
  alongside at lateral -0.9 m with 3.7 m between them.
- **Expected:** the car behind is the follower's problem. The leader keeps its speed and line.
- **Actual:** the leader is `squeezed`, with its clear range collapsed to its own offset. Its
  target drops from 38.7 to 35.5 m/s (-8 %) in front of a car 0.8 m behind it. In braking zones
  that invites the rear-end contacts seen in the pile-ups.
- **Why:** the keep-clear pass treats any car within `CAR_LENGTH + 1.5 m` *ahead or behind* as
  alongside (`ai.rs:664`, `rel(o).abs()`). A follower nose-to-tail sets one lateral bound, the
  car beside the leader sets the other, and they cross (`ai.rs:671-680`). Then `ai.rs:449-451`
  slows the leader.
- **Confidence:** high for the behaviour. Medium for how much it contributes to contact.

### QA-AI-7 (minor): a reversing car reads as moving forward

- **Where:** `race.rs:506` sets `Other::speed` to `vel.length()`, so it is never negative.
  `traffic_cap` (`ai.rs:727-729`) plans the following distance as if the car ahead keeps going
  forward at that speed. A car ahead reversing out of a wall at 3 m/s (the AI's own `Reverse`
  mode) is therefore treated as pulling away at 3 m/s instead of closing.
- **Expected:** use the speed along the track (`vel · tangent`).
- **Confidence:** medium. This is from code reading only and was not reproduced in a scenario;
  it is low speed and rare outside pile-ups.

## Checked and fine

- **Determinism:** the same field and seed give bit-identical positions after 40 s
  (`check_races_are_deterministic`). Each AI's random stream is seeded from its slot and
  difficulty.
- **Backwards start:** a Pro standing still facing backwards turns round in three reverse/pull
  cycles (~7 s) and drives on without a reset (`check_an_ai_facing_backwards_turns_round`).
- **Catch-up:** at its full +3 % an Alien M3, GTI and 911 stay on the road with no resets over
  3-5 laps and gain 0.2-0.3 % a lap. The full-limit profile caps the gain
  (`check_full_catch_up_keeps_an_alien_on_the_road`). With no human, or catch-up off, the factor
  is exactly 1.0.
- **Uniform fields, 1 lap, full contact:** an all-Alien and an all-Novice 8-car field had 0
  resets. All-Pro had 1 (`Stuck` at 953 m, the same corner as QA-AI-2). The all-Alien field
  still loses ~8 s at turn 1 in a jam (see the root cause above).
- **Spin and slide catching, and wrong-way recovery:** the backwards start covers the reverse/pull
  path. `Recover` steers into the slide with the right sign: I checked the sideslip and steer
  sign conventions.
- **Honesty:** the AI writes only `Controls`, with the player's assist set and no physics
  changes. It reads its own telemetry plus the other cars' track distance, lateral offset, speed
  and ghost state, which a player can see. `Other::position` is never read. No advantage found.
- **Steering feed-forward:** `ai.rs:405` corrects the line's curvature by the offset from the
  *centreline* (`path_offset`) instead of the offset from the *line* (`path_offset - lp.offset`).
  This looks wrong, so I measured a flying lap per car at Alien with and without the correction.
  Mean tracking error was unchanged (0.26-0.29 m). Peak error got worse with the "corrected"
  form (0.89 → 1.16 m on the GTI). The gains are tuned around the current form, so I'm not
  filing it as a bug, but it is worth a comment in the code.
- **Racing line and profile:** spot-checked the `bounds` corridor, curvature sign, `corner_speed`
  with lift, and the forward/backward passes. These agree with the M2 review. The forward pass
  clamps a negative uphill acceleration to 0, so the profile never shows the car slowing on a
  climb. That only matters at top speed on a steep grade, which this track does not have.

## Not covered / cross-area notes

- I did not run the full `tools/ai-check.sh` (about 2 h) or re-check the Golf Expert/Pro
  lap-time band edges. The solo runs here (Alien laps of 88.1 s GTI, 82.9 s M3 and 81.2 s 911)
  match the review's figures.
- I did not script a player ramming the AI repeatedly or test airborne/curb starts. The
  `place` helper in the test file supports these (lateral, yaw, speed) for a follow-up.
- Cross-area, race rules: `update_collisions` turns a car solid by distance only (QA-AI-3).
  `ResetMonitor` resets cars that are merely queued, and its stuck timer is defeated by brief
  speed spikes (QA-AI-1, QA-AI-4).
- Cross-area, tooling: in `racer-capture`, `Watch::observe` skips cars that are no longer
  `Racing` before counting resets, so resets on a cool-down lap are never counted.
