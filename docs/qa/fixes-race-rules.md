# Fixes: race rules (fix/race-rules)

Group from `docs/qa/TRIAGE.md`: QA-RULES-1 to 8, QA-EDGE-1, 9, 10 and QA-AI-4 (QA-EDGE-11 and
QA-EDGE-2 are duplicates of QA-RULES-1 and QA-RULES-3). Code: `crates/racer-sim/src/rules.rs`
and `race.rs`. Every repro test listed below had `#[ignore]`. The marker is removed and the test
now passes. Nothing outside the group's files was changed, except one line in the shared
`crates/racer-sim/tests/qa_ai.rs`: the `#[ignore = "QA-AI-4"]` marker is removed.

## The shared root cause: gate credit

Most of the progress bugs come from one thing. `Progress` kept its gate credit (`laps`,
`next_gate`) as state of its own, apart from the distance:

- `update` credited at most one gate per call.
- `move_back_to` could only take credit back.
- A jump the window could not produce was dropped, yet `sample` had already moved.

So the credit and the distance could drift apart, and once a gate was owed behind the car it
could never be crossed forwards again.

The fix makes credit a function of the distance. A car at unwrapped distance `d` has credit for
every gate (checkpoints, then the line, but not the line at the start) at or before `d`. That is
`Progress::credit_at`, the same definition the property tests' `expected_credit` uses. A private
`advance` steps `(laps, next_gate)` towards that value one gate at a time, in either direction,
and returns one event per gate. `update` and the reset's `move_to` (renamed from `move_back_to`,
since it now moves both ways) both use `advance`. So the invariant "credit = credit at the
distance" holds after every update and reset, and the properties now check it with no
carve-outs.

API change: `Progress::update` returns `Vec<ProgressEvent>` instead of `Option<ProgressEvent>`.
On almost every step the Vec is empty and allocates nothing. All the callers are in `racer-sim`.

## Per bug

### QA-RULES-1 / QA-EDGE-11 (critical): a reset after reversing over a gate strands the car

- **Root cause:** `reset_pose` places the car 8 m behind `last_valid`, and `last_valid` can
  still be ahead of a gate the car has just reversed back over. `move_back_to` then set the
  distance ahead of that gate without crediting it, so the gate stayed owed behind the car for
  good.
- **Fix:** `move_to` credits the gates the car is placed past. The car really did reach them,
  since `last_valid` only follows continuous on-track progress. `last_valid` and `reset_pose`
  are unchanged.
- **Tests:** `reset_after_reversing_over_a_gate_must_not_strand_the_car`,
  `qa_edge_11_reset_after_reversing_skips_an_uncredited_gate`. The excursions/resets property
  no longer skips resets after reversing.

### QA-EDGE-1: an update that jumps two gates loses the lap

- **Root cause:** `update` checked only the next gate.
- **Fix:** `advance` credits every gate passed, in order.
- **Tests:** `qa_edge_1_one_update_over_two_gates_loses_the_lap_for_good`. The
  excursions/resets property now also runs on the dense (20 m spacing) track.

### QA-RULES-3 / QA-EDGE-2: rejoining more than ~26 m further on freezes progress

- **Root cause:** a jump the window cannot produce was rejected, but only after `self.sample`
  had moved and `on_track` had been set to true. Every later step was then the same rejected
  jump, and with the car "on track" and moving, no reset ever came.
- **Fix:** an unreachable jump counts as off the track. `on_track` is false, and neither
  `sample` nor the distance moves. Progress waits where the car left, and the existing 3 s
  off-track reset brings it back. The jump is not accepted, because the window is what stops
  shortcuts (`shortcut_across_the_hairpin_is_rejected`). Coming back within the window still
  counts, as before, and now credits every gate passed.
- **Tests:** `rejoining_further_on_is_reset_or_counted_never_frozen`,
  `qa_edge_2_rejoining_far_downstream_freezes_progress` (the harness applies the reset, and
  progress then follows the car). Property excursions now go up to 60 m.

### QA-RULES-2: the wrong-way timer has no cap

- **Fix:** `wrong_way_time` is capped at `WRONG_WAY_AFTER + WRONG_WAY_CLEAR` (2 s). At its 2x
  decay, the warning clears within 0.5 s of turning round, and `last_valid` starts following the
  car again.
- **Test:** `wrong_way_warning_clears_soon_after_turning_round`.

### QA-RULES-4 / QA-EDGE-9: driving a full lap backwards corrupts lap and sector times

- **Root cause:** `Timing` kept one level of history (`previous_lap_start`, `last_sectors`,
  `last_checkpoint_times`).
- **Fix:** `Timing` keeps a `LapRecord` for every lap that stands: its start, sectors and
  checkpoint times. `Lap` pushes a record. `LapUndone` pops one and restores the current lap
  from it, without the lap's last sector. Any number of laps can be taken back.
- **Tests:** `timing_survives_a_full_lap_driven_backwards`,
  `qa_edge_9_second_lap_undone_restores_the_wrong_lap_start`.
- **Test bug:** the RULES-4 repro compared the re-credited lap with the time at 5 m past the
  line, with a 0.05 s tolerance. Those 5 m take 0.2 s in its drive, so it failed even with
  correct timing. It now compares with the crossing (`end - 0.2`). The bug itself showed up as
  86 s lost.

### QA-EDGE-10: a lap taken back keeps its best sectors and stays the delta reference

- **Fix:** the best lap, best sectors and delta reference (the best lap's checkpoint times) are
  all recomputed from the records that stand, after every `Lap` and `LapUndone`. After an undo,
  the delta is recomputed for the restored lap's last checkpoint.
- **Test:** `qa_edge_10_undone_lap_keeps_its_best_sectors_and_delta_reference`.
- **Not a bug:** the repro's second half asserted "no delta" on lap 2's first checkpoint, after
  the undone lap had been *redone*. The redone lap stands again, as the best lap, so a delta
  against it is correct (the old code gave the same value for the right reason). That half now
  checks the case that is a bug: a checkpoint retaken while no lap stands gets no delta, and the
  redone lap's sectors become the bests. The properties now compare best sectors with the
  standing laps after undos too.

### QA-AI-4: a car pushing against an obstacle isn't reset for 11-12 s

- **Root cause:** the rules' stuck timer (3 s below 1 m/s) restarts whenever the speed passes
  1 m/s, even for a single step. A car rocking at 0-3 m/s, or doing short reverse/pull
  manoeuvres, never ran it out, so only the AI's 12 s progress timeout ended it.
- **Fix:** `ResetMonitor` also counts a racing car as `Stuck` when its progress gains less than
  10 m (`STUCK_PROGRESS`) in 7 s (`NO_PROGRESS_AFTER`), however it moves. The anchor restarts
  when the car is not racing, and after a reset. I fixed it on the rules side because it covers
  human drivers and every AI behaviour alike. The AI's own checks in `ai.rs` belong to fix/ai,
  and I did not touch them. A human who deliberately drives the wrong way for more than 5 s is
  now reset after 7 s too. That seems right for a race.
- **Choosing the window:** I compared 12 `ai-races` (3 laps) for each value, all with the rest
  of this branch. Without the rule there were 15 resets, and the longest stall was 14.0 s. With
  a 5 s window there were 41 resets, many of them cars that would have recovered (3 to 5 of
  them counted as "alone"). A 6 s window gave 22 resets, and a 7 s window gave 20, with the
  longest stall at 12.9 s. The report counts a stall across a reset, so stalls over 10 s remain.
  I chose 7 s, so the rule is a backstop and not a hair trigger. The real cure for cars stuck in
  pile-ups is in the AI's recovery and racecraft (fix/ai).
- **Tests:** `qa_ai_4_a_car_pushing_against_an_obstacle_is_reset_within_a_few_seconds`. The
  worst stall without a reset is now 7.0 s, down from 12.4 s, under the test's 8 s limit. The
  unit test `monitor_resets_flipped_and_stuck_cars_after_three_seconds` gains a rocking case.
  Its "driving slowly but steadily" case now moves the progress, since it used to hold it still
  at 3 m/s.

### QA-RULES-5: same-step finishers are ordered by entity

- **Fix:** in `track_racers`, the finish time is interpolated inside the step from how far past
  the line (or the sprint finish) the car got. The winner is the earliest crossing of the step.
  As a last resort, `standings` breaks an exact tie of laps and time on distance; the pure
  repro tests this.
- **Tests:** `same_step_finish_is_ordered_by_who_crossed_first` (standings), plus a new
  headless test, `same_step_finishers_get_distinct_times_in_crossing_order`. It fails on the
  old code. A property checks the distance tie-break.
- **Lap times too:** a `Lap` event is timed at the same interpolated line crossing, so the
  finish time equals the sum of the lap times and the next lap starts at the crossing. This
  matters because the results screen shows both. Checkpoint and sector splits stay at step
  resolution (4.2 ms). The last sector is the lap time minus the others, so the sectors still
  sum to the lap time.

### QA-RULES-6: cars reset in the same step land on the same spot

- **Fix:** `apply_resets` adds each placed pose to the positions the next reset avoids.
  `update_collisions` tells cars apart by `Entity`, not by position (`*p != pos.0`).
- **Test:** `cars_reset_in_the_same_step_land_apart`.

### QA-RULES-7: rocking over the line after the flag finishes a car a lap early

- **Root cause:** after the flag, any `Lap` event finished a car, including the re-credit of a
  lap completed before the flag. The finish was also decided per car inside the progress loop,
  so a car earlier in the query than the winner didn't know about the flag yet.
- **Fix:** `Racer` has a private `flag_laps`: the laps the car had when the winner crossed.
  Once the winner is home, a car finishes as soon as `laps > flag_laps`. The finish is decided
  after every car's progress update in the step. A car that crossed in the winner's step counts
  as before or after the flag by its interpolated crossing time.
- **Tests:** `rocking_over_the_line_after_the_flag_does_not_finish_early`, and the new
  `lapped_car_in_the_winners_step_finishes_only_after_the_winner` (both orders). It fails on
  the old code.

### QA-RULES-8: `laps: 0` finishes everyone at the start

- **Fix:** `RaceConfig::goal` uses `Goal::Laps(laps.max(1))`, so a race is at least a lap.
  `Goal` itself keeps its plain meaning (`Goal::Laps(0)` is reached at once), since the unit
  tests use it that way.
- **Test:** `zero_laps_does_not_finish_everyone_at_the_start`.
- **Left open:** the HUD reads `RaceConfig::laps` directly (fix/game-ui's code). With 0 there,
  it would show "/0", but the menu and CLI clamp the value to 1-10.

## Checks

Run on the final head of `fix/race-rules`:

| Check | Result |
|---|---|
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --locked -- -D warnings`, and `-p racer-game --features wheel` | clean |
| Cross-target clippy (`CARGO_FEATURE_PURE=1`, `x86_64-pc-windows-msvc`, `aarch64-apple-darwin`) | clean |
| `cargo test --workspace --locked`, twice | 99 passed, 0 failed, both runs (the 55 ignored are other groups' QA repros and slow checks) |
| Every commit on the branch | clippy-clean, and the rules tests pass |
| `rules_properties` with `QA_EDGE_CASES=4000` | pass |
| Race capture vs goldens | grid, hud and pack identical; results mean error 0.149, differing pixels 0.16 % (finish and lap times change by milliseconds because they are interpolated), within tolerance |
| Menu capture vs goldens | identical |

`racer-capture ai-races --races 4 --laps 3` (the same seeds before and after). Every race
still runs to the end: the winner does 3 laps, and the rest finish within the DNF time, lapped
cars after 2.

| | Resets | Stalls > 10 s | Longest stall |
|---|---|---|---|
| Before (`main` at `f381b04`) | 4 | 2 | 13.2 s |
| After | 7 | 1 | 12.9 s |

The report's pass mark (no reset of a car alone, at most one reset in traffic per 10 laps per
difficulty) already failed on `main` (Alien), and fails for Expert and Alien after. A 4-race
sample is noisy, since any change reshuffles every race. Over 12 races with the rest of this
branch, the no-progress rule raises resets from 15 to 20 (see QA-AI-4). Every reset is a
`Stuck` reset in the same pile-ups at turn 1 (~381 m) and ~960 m, which fix/ai is working on
(QA-AI-1/2/3). The stall column counts across resets, so it moves less than the QA-AI-4 repro,
which counts per reset.
