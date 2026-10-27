# QA: race rules

Area: `crates/racer-sim` race loop and rules (`race.rs`, `rules.rs`), timing, standings, resets,
collisions option, Sprint/Circuit/Time Trial, DNF, wrong way, and `data/tracks/downtown-loop`.
Tested on `4253183` (end of the M2 review) on 2026-10-27.

Every bug has a headless repro in `crates/racer-sim/tests/qa_race_rules.rs`, ignored with its
id. They all fail on the current code (about 0.3 s in total):

```
cargo test -p racer-sim --test qa_race_rules -- --ignored
```

Track facts used below: length 2142.5 m, 2143 samples about 1 m apart, 24 checkpoints (90 m
apart, plus 689 and 1373.5), sectors end at checkpoints 7 and 15, sprint finish 2070.5 m (0.5 m
past checkpoint 23), grid slots at -4 m to -53 m.

## Bugs

### QA-RULES-1: a reset after reversing over a gate strands the car; it never laps again

**Critical.** Confidence: high.

Repro: `reset_after_reversing_over_a_gate_must_not_strand_the_car`. The car drives to checkpoint
2 + 20 m, reverses at 3 m/s to 3 m behind the gate (so `CheckpointUndone(2)`), then gets a reset.
After that it drives a full lap forwards.

Expected: the lap counts. Either the reset lands behind the owed gate, or the gate is credited.
Actual: the car lands at gate + 9 m with `next_gate` still pointing at the gate behind it.
`update` only credits a gate when `old < gate <= new`, so the gate can never be crossed forwards
again. No checkpoint, lap or finish ever counts for this car. It drives on until it is DNF.

Why: `ResetMonitor` keeps updating `last_valid` during the first second of reversing, until the
wrong-way flag sets. Reversing past the gate then takes the gate's credit back, and `reset_pose`
puts the car 8 m behind `last_valid`, which can still be ahead of that gate.
`Progress::move_back_to` (`rules.rs:172-197`) only handles moving backwards. When the target is
ahead of the car it just sets `distance` (`rules.rs:190`) and skips the gates in between
without crediting them. The same thing happens if the car flips and slides back over a gate
(`up_y < 0.8` freezes `last_valid`), or is stuck against the wall behind the gate (a `Stuck`
reset).

How it happens in play: a spin, reversing out, then the reset key, or an AI recovery that ends
with `wants_reset`. It is easy to hit next to any of the 24 gates.

### QA-RULES-2: the wrong-way timer has no cap, so the warning outlives the wrong-way driving

**Major.** Confidence: high.

Repro: `wrong_way_warning_clears_soon_after_turning_round`. The car drives 20 s against the
track, then 3 s the right way.

Expected: the warning clears within about a second of turning round. Actual: it is still on
after 3 s. `wrong_way_time` grows by `dt` without limit (`rules.rs:127-131`) and only decays at
2x, so 20 s of wrong-way driving leaves the warning on for about 9.5 s. Over that time
`ResetMonitor` refuses to update `last_valid` (`rules.rs:265`, `!progress.wrong_way`). A reset in
that window (a crash right after turning round) sends the car back to wherever it was before it
turned round, which can be hundreds of metres behind.

Suggested fix: clamp `wrong_way_time` to a little over `WRONG_WAY_AFTER`.

### QA-RULES-3: rejoining the road more than ~26 m further on freezes progress for good, with no reset

**Major.** Confidence: high for the logic, medium for how often it happens (the car has to leave
the corridor over the 1.1 m wall or be launched by contact).

Repro: `rejoining_further_on_is_reset_or_counted_never_frozen`. The car leaves the corridor at
100 m, runs 1.5 s alongside it outside the wall (off track, under the 3 s reset), comes back on
the road at 140 m and drives on for 10 s.

Expected: progress resumes, or the car is reset. Actual: progress stays at 100.06 m while the car
is at 390 m. `on_track` is true, so the off-track timer clears. The car is moving, so it is not
stuck, and no reset ever comes. Standings, checkpoints and laps are frozen until the car comes
round to 100 m again, a whole lap later.

Why: `nearest` searches ±24 samples around the old sample, and on a straight its projection
`a.s + along` finds the true station anyway, so the car reads as on track. The distance jump is
more than `(WINDOW + 2)` samples, so it is rejected (`rules.rs:140-143`), but `self.sample` has
already moved (`rules.rs:136`) and the rejection repeats every step. Treating a rejected jump as
off track (so the off-track reset runs) would close it.

### QA-RULES-4: driving a full lap backwards corrupts lap and sector times

**Minor.** Confidence: high.

Repro: `timing_survives_a_full_lap_driven_backwards`. The car does two laps, reverses one full
lap (which undoes laps 2 and 1), then crosses the line forwards again.

Expected: the re-credited lap 1 is timed from the race start, with 3 sectors. Actual: it is timed
from the end of the old lap 1: 172.2 s at race time 258.2 s, so 86 s are lost. `last_sectors`
holds a single entry. `Timing` remembers only one `previous_lap_start`, `last_sectors` and
`last_checkpoint_times` (`rules.rs:396-407`), so the second `LapUndone` restores the wrong
start and empty splits. The same applies to `best_sectors` and `best_checkpoint_times`: they are
never rolled back when the lap that set them is undone, so the delta can be measured against a
lap that no longer exists.

### QA-RULES-5: cars finishing in the same step are ordered by entity order, not by who crossed first

**Minor.** Confidence: high.

Repro: `same_step_finish_is_ordered_by_who_crossed_first`. Two finishers have the same
`Finished(200.0)`, and car 1 is 0.2 m further over the line.

Expected: `[1, 0]`. Actual: `[0, 1]`. Finish times are whole 240 Hz steps (`race.rs:321`), and
`standings` breaks ties only by laps and time (`rules.rs:462`). After that the stable sort falls
back to query order. Side-by-side photo finishes, and dead heats when 8 cars finish in one step,
are decided by spawn order, and the results show a 0.000 gap. Interpolating the crossing time
inside the step, or breaking ties on distance past the line, would fix it.

### QA-RULES-6: cars reset in the same step land on the same spot, then turn solid inside each other

**Major.** Confidence: high for the placement. The solid-overlap follow-on is from reading the
code (medium).

Repro: `cars_reset_in_the_same_step_land_apart`. Two cars get a reset in the same step with
nearby `last_valid` points (the typical first-lap pile-up, where both `Stuck` timers start at
the same impact).

Expected: they land at least 7 m apart. Actual: both land at exactly the same position.
`apply_resets` collects the positions to avoid from cars `Without<PendingReset>`
(`race.rs:368,372`), so cars being reset never see each other. Then `update_collisions` treats
another car at exactly the same position as itself (`*p != pos.0`, `race.rs:426`), so neither
counts as crowded. Both become solid after the 3 s ghost while still overlapping. The physics
then pushes them apart violently, or they stick. The M2 review counted 15 resets in traffic in
12 races, all in pile-ups, so same-step resets are likely there.

### QA-RULES-7: after the winner is home, rocking over the line finishes a car a lap early

**Minor.** Confidence: high.

Repro: `rocking_over_the_line_after_the_flag_does_not_finish_early`. Car 0 has crossed the line
for lap 1 just before the winner finishes. It backs over the line (`LapUndone`) and drives over
again.

Expected: it is still racing. It took no new lap after the flag, so it must come round again.
Actual: `Finished` with 1 lap. `lapped_home` (`race.rs:319`) accepts any `Lap` event once
`race.winner` is set, including the re-credit of a lap completed before the flag. This saves a
lap and avoids the DNF timeout.

The same line also depends on order. If a lapped car crosses the line in the same step as the
winner's finish, but comes earlier in the query, `race.winner` is still `None` for it. It then
has to do another whole lap.

### QA-RULES-8: `laps: 0` finishes everyone on the first running step

**Minor.** Confidence: high. The menu and CLI clamp laps to 1-10, so this is reachable only from
the API, tests, or a future lobby or server config.

Repro: `zero_laps_does_not_finish_everyone_at_the_start`. Expected: the config is rejected or
treated as 1 lap. Actual: the car is `Finished(0.0)` and the race goes to `Finished` at once.
`Goal::Laps(0)` is reached by `laps >= 0` (`rules.rs:434`). Nothing validates
`RaceConfig::laps`.

## Checked and fine

These were checked by reading the code against the existing unit and headless tests. None of them has a test of its own:

- Lap counting forwards over the line, at the exact boundary (`>=` / `<` are symmetric), and
  under repeated jitter. Each crossing gives exactly one `Lap` / `LapUndone`.
- Crossing the line from the grid gives no lap. Driving backwards from the grid earns nothing.
  A full lap driven backwards takes every credit back in order. The hairpin shortcut is rejected.
- No two gates (or a gate and the sprint finish) are close enough to fall in one step. The
  closest pair is checkpoint 23 and the sprint finish, 0.5 m apart. The goal is checked after
  the event in the same step, so a car crossing both in one step still finishes.
- The Sprint geometry: the finish (2070.5 m) is 19 m behind the last grid slot. A sprint car
  never gets a `Lap`, and the results and standings handle 0 laps.
- Time Trial is forced to 0 AI, the menu caps AI at 7, and the 8 grid slots match. Laps are
  clamped to 1-10 in the menu and CLI.
- DNF exactly at the boundary (`now > winner + timeout`, strict). A car crossing the line on
  the timeout step finishes, because `track_racers` runs before `finish`.
- A finished car reversing over the line, or reset behind it: its times and its standings laps
  stand (they come from `timing.laps`).
- Pause stops `Time<Virtual>`, so the fixed step, race clock, countdown, ghost and reset timers
  all stop together. Restart despawns the racers and recreates `Race` and `RaceConfig`, and no
  rules state lives outside them.
- A reset during the countdown is dropped. A car that never moves is reset every 3 s to the
  same spot, 8 m behind its grid slot, without drifting. A reset in the first second avoids the
  other grid slots.
- Ghosting with the collisions option set to full, ghosted first lap, or off, including a ghost
  ending next to a car (as opposed to exactly on one, see QA-RULES-6). Layers are checked both
  ways.
- Race clock precision: f64 accumulation and f32 `race.time` keep lap subtraction at about
  0.5 ms after 2 h. Unwrapped `distance` in f32 is fine for the 10-lap cap. It would start to
  stall around 1000 laps (0.25 m ULP), which no mode reaches.

## Cross-area notes (not filed here)

- HUD (`hud.rs`): the Sprint percentage is `distance / sprint_finish`, so it ignores the grid
  offset. The lap counter uses `progress.laps`, so it ticks down when a car reverses over the
  line. Both are cosmetic.
- Headless harness: a human `Racer` keeps `brake = 1.0` from `hold` after the countdown, because
  nothing clears it without the input plugin. This matters only for tests that push a human car
  around.
