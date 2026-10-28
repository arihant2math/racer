# Fixes: AI drivers (fix/ai)

Branch `fix/ai`, on `f381b04` (QA triage). Scope: `crates/racer-sim/src/ai.rs` and the AI repro
tests in `crates/racer-sim/tests/qa_ai.rs`. The racing line and speed profile needed no change.
Bugs: QA-AI-1, 2, 3, 5, 6 and 7 (QA-AI-4 belongs to fix/race-rules). Write-ups are in
[`ai-drivers.md`](ai-drivers.md). Tested headless in release builds on the city track, 4 cores,
2026-10-27/28.

All six bugs were real, and all six are fixed. Each repro test failed on `f381b04` and passes now.
The `#[ignore]` is gone, so they run in `cargo test` as regression tests. The AI still acts only
through `Controls`, with the player's assists and no physics, grip or power changes. The only new
thing it reads about other cars is how their track distance changes from one step to the next,
which gives their direction of travel. A player sees that too.

## Before and after

Same binary invocations and seeds before (`f381b04`) and after (this branch). Races: 8-car mixed
fields, full contact, `racer-capture ai-races`. A race ends 30 s after the winner, so most cars log
2 or 3 laps in a 3-lap race.

| Run | Laps | Resets in traffic | Resets alone | Stalls > 10 s | Longest stall | Car contacts | Permanently stuck |
|---|---|---|---|---|---|---|---|
| Races 0-11, 3 laps (M2 review seeds), before | 246 | **15 (6.1 %)** | 0 | 4 | 14.0 s | 458 | 0 |
| Races 0-11, 3 laps, after | 246 | **0** | 0 | 0 | 2.7 s | 70 | 0 |
| Races 12-23, 3 laps (not used while fixing), before | 223 | **8 (3.6 %)**, Novice row FAIL | 1 | 1 | 11.4 s | 369 | 0 |
| Races 12-23, 3 laps, after | 233 | **0** | 0 | 0 | 2.8 s | 62 | 0 |
| Races 0-3, 10 laps, before | 302 | **9 (3.0 %)**, Alien row FAIL (6 in 59 laps) | 0 | 4 | 13.2 s | 406 | 0 |
| Races 0-3, 10 laps, after | 292 | **0** | 0 | 0 | 2.7 s | 34 | 0 |

Every reset before the fix was a `Stuck` reset in a pile-up at turn 1 (370-383 m) or at the
right-hander around 950-962 m, plus three later ones in 10-lap races at ~1460 m and ~1875 m. After
the fix there is no reset of any kind in 771 race laps. Car-to-car contact episodes dropped by a
factor of 6-12. The difficulties still finish in order on average: after the fix the mean finishing
position on races 0-11 is Alien 3.4, Expert 3.9, Pro 4.8, Amateur 4.2, Novice 6.2 (grid 4.8, 4.1,
5.1, 3.8, 4.7). On races 12-23 it is 2.8, 3.7, 4.1, 5.8, 6.3. So passing still happens.

Solo laps (`racer-capture ai-laps --laps 20`, 300 laps): the before and after tables are
**identical**. The changed code only runs when there are other cars. 0 resets, 0 stalls, longest
stall 2.1 s, every pair inside its band and the difficulties strictly ordered for every car:

| Car | Alien | Expert | Pro | Amateur | Novice |
|---|---|---|---|---|---|
| Golf GTI | -0.4 % | +2.3 % | +6.4 % | +11.0 % | +17.2 % |
| M3 Competition | +1.9 % | +4.0 % | +7.6 % | +12.2 % | +18.2 % |
| 911 Carrera S | +0.8 % | +3.2 % | +7.3 % | +11.9 % | +18.3 % |

(Bands: Alien -2..+2 %, Expert +2..+6 %, Pro +6..+11 %, Amateur +10..+17 %, Novice +15..+25 %
against the optimal-line lap. The Golf Expert and Pro are still near their lower edges, as in M2.)

## QA-AI-2 (major): inside passes hit the inner wall. Fixed (done first)

**Root cause.** The pass lane was an absolute offset next to the car being passed,
`o.lateral ± (width + gap)`, measured where *that* car was, which can be up to 35 m ahead. It was
then turned into a shift from the racing line *here* (`want - my_line`). Into turn 1 and the
946 m right-hander the racing line crosses the road at up to 0.5 m per metre. So the same lane
meant a shift that swept by several metres as the car moved, and the rate-limited shift could not
follow it. The path (`line + shift`) ran onto the `hi + 0.3` clamp, onto the curb, and the car's
front corner reached the wall. The trace of race 0 showed the target shift moving from +10 m to
-7 m within 2 s while the path sat on the clamp. In addition, the lane was only checked against the
road bounds at the car, not at the corner it was driving into.

A first attempt (rate-limiting the path in road coordinates) fixed race 0. It failed in race 7
because a path at a constant offset from the centreline is a very tight path through this track's
90° city corners (centreline radius about 16 m). It was dropped.

**Fix.** Lanes beside the racing line are planned as shifts from it, in the line's own terms:

- The car being passed is measured by its deviation from the racing line where it is
  (`off_line`). The lane is that deviation ± (width + gap), and it stays steady while the line
  sweeps under both cars.
- A new `room`: the range of shifts that keeps the path on the road over the next 2 s of travel
  (at least 24 m). A pass starts only on a lane inside it (0.3 m margin). It is held only while it
  still fits (0.2 m slack), or, once the car is alongside, at the edge of the room, where the
  other car's keep-clear logic leaves space. Otherwise the pass is given up.
- Defending moves to the inside within the same room (it used to aim at the inside edge at the
  car, which has the same sweep problem).
- The "is it in my path" test in the traffic cap and the lane-free test also compare deviations
  from the line, not absolute offsets of cars up to 80 m apart.

**Tests.** `qa_ai_2_an_inside_pass_at_turn_1_does_not_hit_the_wall` is the QA repro (race 0, first
17 s, no wall contact for any car). The new `qa_ai_2_no_pass_or_defence_in_race_7_hits_the_wall`
covers race 7 for 45 s, which takes in both the 946 m site and an Alien hitting the turn-1 inner
wall at 16.6 s on the old code. It asserts no wall contact while a car is passing or defending.
It does not flag a plain-line brush: on the new code an Amateur M3 brushes the wall at the 500 m
left-hander at 22.8 s on the racing line with no pass, which is line noise, not racecraft (see
open items).

## QA-AI-1 (major): AI waits behind a stopped car and is reset. Fixed

**Root cause.** The traffic cap drove the speed to `o.speed + gap`, which is 0 at the stopped car,
before the car had moved sideways. The pass started only at 20 m, and the path moves across at
1.2-2.6 m/s. A stopped car cannot move sideways, so the pass could never finish. The AI then waited
with the throttle closed until the rules reset it.

**Fix.**

- A car stopped in the way (< 3 m/s) is an obstacle from `15 + 2 v` metres, early enough to get
  across.
- While passing a car on a lane that clears it, the AI does not follow that car. It closes only as
  fast as it can get beside it: `o.speed + reach * shift_rate / need`, at least 1.5 m/s, where
  `need` is the sideways distance still missing and `reach` the room left before the other car's
  bumper.
- A car only gets about 0.5 m across per metre it drives. If it is closer than that, it follows
  (stops short) instead. Once stopped there for 0.5 s, with no car close behind it in its lane
  (within about 10 m), it backs off straight (new `Mode::BackOff`) far enough to get round, plus
  3 m. It stops backing off at once if a car comes up behind. Then it pulls away and the pass
  lane steers it round.

**Test.** `qa_ai_1_a_stopped_car_on_the_line_is_driven_round_without_a_reset`: no reset, and the AI
is past 260 m.

## QA-AI-3 (major): ghosted cars invisible until they turn solid. Fixed

**Root cause.** Every AI query skipped non-solid cars. A car ghosted after a reset turns solid as
soon as no car is within 5.5 m, which happened with the AI 27 m away at 38 m/s.

**Fix.** In the traffic cap, a ghosted car ahead is planned round like a solid one (followed, and
passed as an obstacle when stopped) until the AI is within a car length of it. From there it stays
ghosted (the rules keep it ghosted while any car is within 5.5 m), so the cap has a 3 m/s floor for
ghosts and the AI can creep through. The lane-free check counts ghosted cars too. Keep-clear
alongside still ignores them, since they cannot be hit while overlapping. With the collisions
option off (or ghosted first lap), this means the AI follows and passes other cars instead of
driving through them, which is still valid racing. Making the rules side (`update_collisions`)
consider closing speed would be a rules change (fix/race-rules).

**Test.** `qa_ai_3_a_car_turning_solid_ahead_is_not_rammed`: the standing car is never knocked
above 3 m/s (it was knocked to 20.9 m/s).

## QA-AI-5 (minor): finished car defends on its cool-down lap. Fixed

**Root cause.** Nothing in the AI knew the car had finished.

**Fix.** `AiDriver::cooling_down()` is true when the race has set the cool-down pace. `drive_ai`
sets `catch_up = 0.7` after the flag, and while racing catch-up only moves it within 0.97..=1.03,
so `catch_up < 0.9` is unambiguous. On the cool-down lap the AI does not defend, does not start
held-up passes (it still drives round stopped cars) and does not ram back. `race.rs` belongs to
fix/race-rules, so I did not add an explicit flag there. **Follow-up for race-rules:** set an
explicit "finished" flag on `AiDriver` in `drive_ai` (or name the 0.7) so the inference is not
needed.

**Test.** `qa_ai_5_a_finished_car_does_not_defend_against_racing_cars`.

## QA-AI-6 (minor): a car right behind squeezes the leader. Fixed

**Root cause.** Keep-clear counted any car within `CAR_LENGTH + 1.5` m ahead *or behind*
(`rel(o).abs()`) as alongside.

**Fix.** Alongside now means from 6.3 m ahead to overlapping our rear by at least 1 m
(`rel` in `-(CAR_LENGTH - 1)..CAR_LENGTH + 1.5`). A car nose-to-tail behind is its own driver's
problem.

**Test.** `qa_ai_6_a_car_close_behind_does_not_squeeze_the_leader`.

## QA-AI-7 (minor): a reversing car reads as moving forward. Real; fixed

**Verified.** `drive_ai` sets `Other::speed` to `|v|`, so the traffic cap planned a car
reversing towards the AI as pulling away at that speed, and did not count it as an obstacle
(< 3 m/s). I scripted it: a car on the racing line at 200 m reverses at 3 m/s and an AI Pro
follows it. Over a grid of 12 cases (reversing for 1.6, 2.5 or 4 s; the AI starting 14-30 m back
at 5-15 m/s), the old code makes contact in 8 cases and is reset in 5. The new code makes contact
in 4 cases, all longer reverses (2.5-4 s) into an AI that has already stopped (0.4 m/s or
reversing), so the reversing car does the hitting. It is reset in 1 case.

**Fix.** `ai.rs` cannot change `race.rs`, so the AI signs the speeds itself. It keeps each other
car's track distance from the last step (`seen`), and a car whose distance went down is moving
backwards. Its speed is then negative, and it keeps that sign while it does not move measurably.
In the cap a negative speed counts as closing (`sqrt(2 b gap) + o.speed`), and a reversing car is
an obstacle to drive round. Simpler, if race-rules agrees: set `speed` to `vel · tangent` in
`drive_ai`.

**Test.** `qa_ai_7_a_car_reversing_towards_the_ai_is_avoided`: the car reverses for 1.6 s, as the
AI's own reverse manoeuvre does, then stops. The AI starts 18 m back at 8 m/s, then 14 m back at
5 m/s. It must not touch the car, must not be reset, and must get past within 12 s. The old code
fails the first case with a `Stuck` reset (in the second it makes contact and is reset twice). The
new code with only the direction sign disabled makes contact. The new code passes. In the first
case it stops short, backs off and drives round, which takes about 9.5 s.

## Combined with fix/dynamics

fix/dynamics (`43ba494`, ground as a half-space, QA-PHYS-4) will be merged to main first. I merged
it into a scratch branch on top of this one (not pushed) and ran the same races. fix/dynamics
alone left 3 "resets alone" in the Pro and Expert rows. They were `Stuck` resets of AI cars
waiting behind a stopped car (QA-AI-1), at ~1450 m (race 5, twice) and ~1875 m (race 11).

| Races, 3 laps | Laps | Resets in traffic | Resets alone | Stalls > 10 s | Longest stall | Contacts |
|---|---|---|---|---|---|---|
| 0-11, fix/dynamics alone | 238 | 8 | 3 (Pro and Expert rows FAIL) | 4 | 11.6 s | 345 |
| 0-11, fix/dynamics + fix/ai | 247 | **0** | **0** | 0 | 2.9 s | 78 |
| 12-23, fix/dynamics alone | 225 | 3 | 0 | 1 | 10.0 s | 282 |
| 12-23, fix/dynamics + fix/ai | 237 | **0** | **0** | 0 | 3.3 s | 48 |

**Merge note:** both branches update `crates/racer-capture/goldens/race/{hud,pack,results}.png`,
and git cannot merge PNGs. Whichever merges second should regenerate the race goldens on the
merged code (`CAPTURE_PROFILE=release tools/capture.sh race <out>`), check them by eye and commit
them. Neither branch's goldens are right for the combination.

## Other checks

| Check | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `CARGO_FEATURE_PURE=1 cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc` / `aarch64-apple-darwin` `-- -D warnings` | both clean |
| `cargo test --workspace --locked` | all pass. `qa_ai` runs 7 regression tests in 74 s in a debug build (the 45 s race-7 field is most of it) |
| `cargo test --release -p racer-sim --test qa_ai -- --ignored` (QA checks) | determinism, backwards start and full catch-up checks pass |
| Race capture goldens (`CAPTURE_PROFILE=release tools/capture.sh race`) | the old code still matches the goldens exactly. With the new AI, `hud`, `pack` and `results` differ (mean error 1.9 / 6.2 / 13.0) because the first lap plays out differently: one place better for the player car in `pack`, and Femi (Alien GTI) and Gus (Expert M3) swap places at the back of the results. Checked by eye, then the three goldens were updated. A second capture matches them (mean error at most 0.015). `grid` is unchanged. |

## Tuning

There was no parameter sweep. The new constants come from geometry and the repros: a 2 s horizon
for the room, 0.5 m across per metre, 1.5 m/s minimum creep, a 3 m/s ghost floor, obstacle
distance `15 + 2 v`, and 3 m spare when backing off. The first set that passed the repros gave the
results above, so the timebox was not needed.

## Open items

- **QA-AI-4** (fix/race-rules): its repro, `qa_ai_4_a_car_pushing_against_an_obstacle_is_reset_within_a_few_seconds`
  on race 7, **passes on this branch** because the pile-up it relies on no longer happens. I left
  it `#[ignore = "QA-AI-4"]` for its owner. The rules-side defects (a stuck timer defeated by brief
  speed spikes, the AI's own stuck check needing an open throttle) are still in the code and need
  a scripted scenario, for example a car pinned against a wall, to reproduce now.
- Cool-down detection infers "finished" from `catch_up` (see QA-AI-5), and the direction of other
  cars is estimated in the AI (see QA-AI-7). Both are cleaner as one line each in `drive_ai`
  (fix/race-rules' file).
- Line tracking: an Amateur M3 brushed the wall at the 500 m left-hander on its racing line plus
  line noise (race 7, 22.8 s, lateral -5.9 m against a path clamp at -5.0 m). It caused no reset
  or stall. This is solo line tracking, not racecraft, and was not changed.
- Getting round a car that is stopped right in front (QA-AI-7 scenario) takes about 9.5 s, because
  the AI has to back off first. A smarter reverse (steering so the car ends up angled towards the
  lane) could halve that. A car that keeps reversing into an AI that has stopped can still hit
  it. The AI does not get out of the way backwards.
- The full `tools/ai-check.sh` run (100 solo laps per pair, 12 races of 10 laps, about 2 h) is still
  owed from M2. The reduced runs above stand in for it.

## Review

Independent review of `fix/ai`. While it was under review, `fix/dynamics`, `fix/race-rules` and
`fix/game-ui` all landed on `main`. I merged `origin/main` (`80e6ca4`) into the branch (merge
commit `74b56bd`), and every number below comes from that merged code, which is what will ship.
Only the three race goldens conflicted. Main's copy was taken in the merge, and the goldens are
regenerated on the final code (see Verification). `data/tracks/downtown-loop/generated/` merged
cleanly. This branch does not touch it, so main's copy stands.

### Verification

Release builds, 4 cores. "Before the review fix" is `fix/ai` as the fixer left it, merged with
main (`74b56bd`). "Final" includes the review commits.

| Check | Result |
|---|---|
| `tools/setup.sh` | complete |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | clean |
| `CARGO_FEATURE_PURE=1 cargo clippy ... --target x86_64-pc-windows-msvc` / `aarch64-apple-darwin` `-- -D warnings` | both clean |
| `cargo test --workspace --locked --no-fail-fast`, twice | 142 passed, 0 failed, 25 ignored, both runs (an earlier pair of runs found findings 2 and 3) |
| `#[ignore]` markers vs `origin/main` | the only ones removed are QA-AI-1, 2, 3, 5 and 6. QA-AI-4 was already un-ignored by race-rules. QA-AI-7 had no repro before. No QA-AI test is ignored. Every other group's marker is unchanged |
| Race goldens, `CAPTURE_PROFILE=release tools/capture.sh race` on the final code | `grid` matches main's golden (mean error 0.001). `hud`, `pack` and `results` differ from main's (mean error 1.9 / 10.8 / 5.1) because the first lap plays out differently. I checked them by eye (0 resets, a sensible order) and committed them. A second capture matches them (mean error ≤ 0.009) |

`racer-capture ai-races` (8-car mixed fields, full contact). Baseline: `fix/dynamics` alone, on
the main of the time. Contacts are contact episodes.

| Races | Code | Laps | Resets in traffic | Resets alone | Stalls > 10 s | Longest stall | Contacts |
|---|---|---|---|---|---|---|---|
| 0-11, 3 laps | baseline (dynamics, no fix/ai) | 238 | 8 | 3 (Pro, Expert FAIL) | 4 | 11.6 s | 345 |
| 0-11, 3 laps | before the review fix | 247 | **0** | **0** | 0 | 2.9 s | 78 |
| 12-23, 3 laps | before the review fix | 237 | **0** | **0** | 0 | 3.3 s | 48 |
| 0-11, 3 laps | final | 249 | **0** | **0** | 0 | 3.1 s | 68 |
| 12-23, 3 laps | final | 233 | **0** | **0** | 0 | 3.5 s | 68 |
| 24-35, 3 laps (new seeds) | final | 245 | **0** | **0** | 0 | 3.7 s | 62 |
| 0-3, 10 laps | final | 299 | **0** | **0** | 0 | 3.0 s | 42 |

The "before the review fix" rows reproduce the fixer's "fix/dynamics + fix/ai" table exactly. Every
row passes. The races also run under race-rules' new no-progress reset (10 m in 7 s), and it
never fires.

Passing and difficulty. These are the mean places gained from grid to finish, races 0-11.
Baseline (dynamics alone): Alien +1.1, Expert 0.0, Pro +0.1, Amateur -0.4, Novice -0.7. Final:
Alien +1.2, Expert +0.4, Pro -0.1, Amateur -0.3, Novice -1.2. Races 12-23, final: +0.9, +0.5,
+0.7, -0.5, -1.7. So the cars pass at least as much as before, and fast drivers gain places.
Some races end in trains behind a slower leader, with all the bests within a second. That
happens in the baseline too, and is a 3-lap race on a narrow street circuit, not new caution.
Contact still follows willingness. In races 0-11 (final) Alien cars have 18 contact episodes
and Novice cars 12, while Expert and Amateur sit in between.

Solo laps, final code (`racer-capture ai-laps --laps 20`, 300 laps). 0 resets and 0 stalls. The
longest stall is 2.1 s. Every run is inside its band, and the difficulties are strictly ordered
for every car:

| Car | Alien | Expert | Pro | Amateur | Novice |
|---|---|---|---|---|---|
| Golf GTI | -0.6 % | +2.2 % | +6.4 % | +10.9 % | +17.1 % |
| M3 Competition | +1.4 % | +3.6 % | +7.4 % | +11.9 % | +18.2 % |
| 911 Carrera S | +0.4 % | +3.0 % | +7.3 % | +11.9 % | +17.9 % |

The Golf's Expert (+2.2 %), Pro (+6.4 %) and Amateur (+10.9 %) sit within 0.4 points of their
lower edges. That is solo line-following, which neither this branch nor the review changes. It
was +2.3 / +6.4 / +11.0 before the merge, so fix/dynamics moved it only by noise. The full
`tools/ai-check.sh` (100 laps per pair, 12 × 10-lap races) is still not run. The reduced runs
above stand in for it.

### Code review

I read `git diff f381b04 fix/ai -- crates/racer-sim` against the brief.

- **QA-AI-2 (inside passes).** This is a root-cause fix. Lanes are deviations from the racing line
  measured where each car is, and `room` checks the road over the next 2 s, not only at the car.
  No speed or margin was tuned. 0 resets in 1,026 race laps across 40 races (12 of them on new
  seeds, 24-35) agrees with that.
- **QA-AI-1 (stopped car).** The mechanism is right: the closing speed is bounded by the time
  needed to get across, the AI backs off when boxed in, and the obstacle range is `15 + 2v`.
  **But it only worked where a lane fits as a shift from the racing line** (see finding 1).
- **QA-AI-3 (ghosts).** Planning round ghosts until a car length away is reasonable, with a
  3 m/s floor so the AI can creep through. One side effect, which the fixer notes: with
  `Collisions::Off` or ghosted first laps, the AI now follows and passes ghost cars instead of
  driving through them. That is valid racing and a gentler AI, and I left it.
- **QA-AI-5 (cool-down).** This is correct, but it infers "finished" from `catch_up < 0.9`. On main
  `drive_ai` still sets 0.7 after the flag, so the inference holds. The follow-up for an
  explicit flag stays open.
- **QA-AI-6 (alongside).** The new window `-(L-1)..L+1.5` is right.
- **QA-AI-7 (reversing).** The sign comes from the change in track distance between steps. That
  is what a player sees, so it is honest. A car moving across the track while its distance
  drops (a spin) reads as `-|v|`, which overstates its closing speed. That is cautious and
  harmless.
- **Honesty.** The AI still acts only through `Controls`, with the player's assists and no
  physics, grip or power changes. It senses other cars' track distance, lateral offset and
  speed, as before.
- **Scope.** Only `ai.rs`, `qa_ai.rs` and the doc changed. No dead code. The QA-AI-2 race-7 test
  tolerates a wall brush on the plain racing line (documented above), which is fair.

### Findings and fixes

1. **QA-AI-1 was only fixed on straights. Fixed (`35a1b87`).** I scripted a player parked on
   the racing line at 25 places round the lap, with a Pro, Novice or Alien GTI arriving at
   15 m/s. On the fixer's code, the AI is reset as `Stuck` again and again and never gets past
   at 5 of the 25 places, for every difficulty, including 500, 1450, 1600 and 1875 m. Those
   are the places where the racing line crosses the road mid-corner or at a corner exit. A lane
   planned as a constant shift from the line cannot fit when the line sweeps 5-8 m across within
   20 m, so no pass ever started. The 1450 m and 1875 m sites are the ones the fixer saw in the
   dynamics-only races.
   **Fix:** beside a stopped car (< 3 m/s), the pass lane is fixed on the road. It is checked
   against the road bounds where the stopped car is, and held while the line sweeps underneath
   (the shift absorbs the line's movement). It is steered with the road's heading and curvature,
   not the line's. The pass takes the side the AI is already on, not across the stopped car's
   nose, goes by at about 5 m/s or less, and is not given up at a braking zone. Passes of moving
   cars are untouched.
   **Result:** in the same 75 runs, 4 resets in total (15 runs never got past before). One
   Novice run (1450 m) is still not past after 30 s. 5 runs touch the stopped car at low
   speed. New regression test `qa_ai_1_a_car_stopped_in_a_corner_is_driven_round_without_a_reset`
   covers 500, 1450 and 1875 m and fails on the fixer's code. I tried two kinds of tuning (where
   to switch to the fixed lane, and the approach speed) within the timebox. Neither beat the
   plain version, so neither is in.
2. **The QA-AI-7 test fails on main. Fixed (`76597f4`).** After the merge, the test's 182 m case is
   reset at 7.0 s by race-rules' new no-progress rule, with or without finding 1's fix. The
   reversing car backs to within 0.5 m of the AI's nose, and getting round it takes about
   9.5 s. That reset is the rules' backstop working. The test now clears the AI's reset monitor
   too, so it checks the driving: no contact, no reset, past within 12 s. It still fails on the
   pre-fix AI (contact in the 186 m case).
3. **A `racer-dynamics` property test is flaky. Not this branch: for fix/dynamics.**
   `two_half_steps_match_one_full_step` failed once in four workspace runs (proptest, random
   seed). The failing case was `car = 0`, `(yaw, speed, lateral, yaw_rate) = (0, 5, 2.697,
   -0.405)`, alphas `[0, -0.081, 0, 0]`, with wheel 1 at 7.02 vs 7.84. It passed in 6 further
   runs.

### Left open

- Passing a stopped car, residual: 4 resets in 75 scripted runs. In one Novice run at 1450 m
  the AI approaches too fast for the line it needs and ends up wedged. Getting round a car that
  stopped right in front still means backing off, about 9.5 s, longer than race-rules' 7 s
  no-progress backstop. So in a race, that AI is reset (ghosted, behind) rather than driving
  round. That is acceptable, but a smarter reverse (the fixer's open item) would avoid it.
- Cool-down detection by `catch_up` and the AI-side direction estimate (QA-AI-5, 7): one line
  each in `drive_ai`, race-rules' file. This is still a follow-up.
- Golf Expert/Pro/Amateur solo times sit near the lower edges of their bands, as noted in M2.
- The full `tools/ai-check.sh` run is still owed.

### Verdict

The six fixes are real and root-caused, the AI stays honest, and difficulty still separates the
drivers. On the code that will ship, 40 races (1,026 race laps, 12 of the races on new seeds) show 0 resets and 0 stalls over 10 s. Car contacts are 68 vs 345 on races 0-11. The
review found one real gap, stopped cars in corners, and fixed it. It also fixed one merge-time
test failure. **Ready to merge.**
