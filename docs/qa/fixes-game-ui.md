# Fixes: game UI, flow and input (fix/game-ui)

Group `fix/game-ui` from [TRIAGE.md](TRIAGE.md): QA-UI-1 to QA-UI-13 (QA-EDGE-15 is QA-UI-6).
All changes are in `crates/racer-game`, plus the README controls paragraph (QA-UI-8). Every
repro test in `crates/racer-game/tests/qa_flow.rs`, `qa_wheel.rs` and the QA-EDGE-15 test in
`format_properties.rs` is un-ignored and passes; all failed on the old source (checked by
running the new tests against the stashed `src/`), except QA-UI-10, see below.

## Majors

### QA-UI-3: force feedback held while paused

- **Cause:** `force_feedback` runs in `FixedPostUpdate`, which does not run while
  `Time<Virtual>` is paused, so the backend kept the last race force (the SDL thread
  re-applies it every 4 ms).
- **Fix:** `release_force_while_paused` (`input.rs`, `PostUpdate`) sends 0 every frame while
  virtual time is paused. Fixed steps send the race force again on resume.
- **Test:** `force_feedback_is_released_while_paused` (MockWheel).

### QA-UI-11: `--features wheel` takes any gamepad for a wheel

- **Cause:** `sdl_wheel::open` opened the first joystick with two or more axes; pedals
  mapped as `(1 - axis) / 2`, so a resting gamepad read half throttle and half brake and
  overrode keyboard and Bevy gamepad input.
- **Fix:** only joysticks SDL types as `SDL_JOYSTICK_TYPE_WHEEL` are opened (SDL's own
  classification, from its list of known wheels), and the first such one is
  taken, not just the first joystick. Pedal direction: a pedal whose SDL initial axis state
  is negative is read as resting at -1 (`pedal()`), otherwise at +1 as before.
- **Test:** `qa_wheel.rs` `a_gamepad_is_not_taken_for_a_wheel` attaches a virtual gamepad
  (must read `None`), then a virtual wheel (must be opened, steering 0.25, released pedals
  0). One test function on purpose: the `sdl3` crate allows SDL to be initialised from one
  thread per process, so a second backend in the same test binary would not start. Unit
  test `pedals_rest_at_either_end`.
- **Open:** still unverified on hardware; per-device pedal calibration (ranges, combined
  pedal axes) is not done. A wheel SDL does not classify as a wheel is no longer picked up.

### QA-UI-13: setup and results screens clipped

- **Cause:** fixed pixel sizes centred with no overflow handling.
- **Fix:** menu panels (main menu, setup, pause, results) carry `FitToWindow`: each frame
  `fit_to_window` compares the panel's layout size with its parent's content box (window
  less a 16 px margin) and sets a uniform `UiTransform` scale `min(1, room / size)`. Layout
  sizes ignore the transform, so there is no feedback, and picking honours `UiTransform`,
  so mouse clicks still hit the rows. Panels never scale up. The setup rows were also made
  more compact (row padding 4 → 2 px, gap 4 → 2 px, title 40 → 36 px) so the setup screen
  fits 1280×720 unscaled. The results panel is now centred instead of top-aligned.
- **Verified:** `tools/capture.sh menu|race` at 1280×720, 1024×600 and 2560×1080; every
  screenshot looked at. Setup: unscaled at 1280×720 and 2560×1080, scaled to ~0.93 at
  1024×600, every row visible. Results: whole table visible at every size.
- **Open:** at small sizes the scaled text is slightly soft (glyphs are rasterised at the
  unscaled size). Scrolling was not needed: the screens have a bounded number of rows.

## Minors

- **QA-UI-1 (Esc/Backspace/pad East quits from the main menu):** "back" maps to no action on
  the main menu; only the Quit item quits (`menu_input`). Tests:
  `back_on_the_main_menu_does_not_quit_the_game`, `pad_back_on_the_main_menu_does_not_quit_the_game`.
- **QA-UI-2 (R/Q/E while paused act on resume):** `reset_key` ignores presses while virtual
  time is paused; `read_devices` drops shift presses (keyboard, pad, wheel) while paused
  instead of latching them. Test: `reset_and_shift_keys_are_ignored_while_paused`.
- **QA-UI-4 (pad input discards keyboard steering):** the stronger of keyboard and pad
  steering wins (several pads likewise); the device is `Gamepad` only when the pad's
  steering is used or the keyboard is not steering. Test:
  `keyboard_steering_survives_a_gamepad_trigger`.
- **QA-UI-5 (results hide steering assist, racing line, gearbox):** the player's car gets a
  `DriverAids` component (steering assist, racing line, automatic gearbox) from the setup;
  the assists column reads e.g. `ABS TCS ESC Steer Line Auto` (`assists_label`), wider
  column (250 px). AI rows show the car assists only. Result rows are matched to racers by
  name (unique on the grid) because `racer_sim::race::ResultRow` carries no entity. Test:
  `results_show_steering_assist_and_racing_line`.
- **QA-UI-6 / QA-EDGE-15 (`0:60.000`):** `lap_time` rounds once to whole milliseconds and
  splits that. Tests: `lap_time_carries_rounding_into_minutes`,
  `qa_edge_15_time_just_under_a_minute_shows_60_seconds`; the proptest no longer skips times
  just under a minute. Negative inputs now clamp to `0:00.000`.
- **QA-UI-7 (chase camera flies after restart/reset):** `ChaseCamera` remembers the
  followed car and its reset count; when either changes (new car on restart or respawn,
  or a reset) the camera snaps behind the car. Tests:
  `chase_camera_starts_behind_the_car_after_a_restart`, `chase_camera_snaps_behind_the_car_after_a_reset`.
- **QA-UI-8 (no pad button for reset/camera):** North (Y/Triangle) resets (`RESET_BUTTON`,
  in races and the drive scene), Select/Back/View switches chase/hood (`CAMERA_BUTTON`).
  Neither was bound in races (South handbrake, bumpers shift, Start pause, d-pad and East
  only act in menus). README controls updated. Tests: `some_gamepad_button_resets_the_car`,
  `gamepad_reset_and_camera_buttons` (also checks no reset while paused).
- **QA-UI-9 (unplugged pad doesn't pause):** `toggle_pause` also opens the pause menu on a
  `GamepadConnectionEvent` disconnect during a race. Test: `gamepad_disconnect_pauses_the_race`.
- **QA-UI-10 (stick deadzone applied twice): not a bug in Bevy 0.19.1.** `Gamepad` stores
  the raw axis value (`gamepad_event_processing_system` sets `analog` to `filtered_value.raw`;
  `AxisSettings` only gate and scale the change events), so the game's 8 % stick deadzone
  and 5 % trigger gate are the only ones. The repro passes on the old code. No code change;
  the test is kept as a regression test and now also checks that 6 % stick noise does not steer.
- **QA-UI-12 (pause on the results frame sticks):** `resume_time` on `OnExit(AppState::Race)`
  unpauses however the race is left. Test: `pause_on_the_frame_the_results_come_up_does_not_stick`.

## Goldens

Updated deliberately, nothing else: `menu/main-menu.png` and `menu/setup.png` (compact rows,
title 36 px, panel padding 20 px), `race/results.png` (centred panel; the player's row reads
`ABS TCS ESC Steer Line Auto`). `racer-capture diff` against the old goldens: those three
fail, `race/grid`, `race/hud` and `race/pack` are pixel-identical. Note that the capture race
puts the player's car on autopilot, yet its row lists the setup's aids; that is cosmetic
(autopilot is for demos and captures only).

## Verification

- `cargo fmt --all --check`: clean.
- `cargo clippy --workspace --all-targets --locked -- -D warnings`: clean; also
  `-p racer-game --features wheel`.
- `CARGO_FEATURE_PURE=1 cargo clippy --workspace --all-targets --locked --target
  x86_64-pc-windows-msvc|aarch64-apple-darwin -- -D warnings`: clean.
- `cargo test --workspace --locked`: green. Remaining ignored tests belong to other groups.
  `cargo test -p racer-game --features wheel` (lib + `qa_wheel`): green.
- Captures: `tools/capture.sh menu` and `CAPTURE_PROFILE=release tools/capture.sh race` at
  1280×720, 1024×600 and 2560×1080. Every setup and results capture was looked at, plus
  the main menu and the HUD at 1024×600. All rows and columns are visible at every size.

## Review

Independent review of `fix/game-ui` at `85e35a8` against `origin/main`. The fixer's notes were
not taken on trust: every claim below was re-checked.

### Checks run

| Command | Result |
| --- | --- |
| `tools/setup.sh` | exit 0, nothing missing. |
| `cargo fmt --all --check` | Clean. |
| `cargo clippy --workspace --all-targets --locked -- -D warnings` | Clean. |
| `cargo clippy -p racer-game --all-targets --features wheel --locked -- -D warnings` | Clean. |
| `CARGO_FEATURE_PURE=1 cargo clippy --workspace --all-targets --locked --target x86_64-pc-windows-msvc -- -D warnings` | Clean. |
| same, `--target aarch64-apple-darwin` | Clean. |
| `cargo test --workspace --locked`, twice | Both green: 98 passed, 0 failed, 57 ignored. |
| `cargo test -p racer-game --features wheel --locked` | Green; `qa_wheel` (virtual gamepad ignored, virtual wheel opened) and `pedals_rest_at_either_end` pass. |
| `cargo test --workspace --locked --no-fail-fast -- --ignored` | 57 ignored tests run: 52 QA repros of the other groups fail as on `main` (PHYS, RULES, AI, EDGE-1..14, TOOL), 5 `QA check`/`diagnostic`/long-run tests pass. No QA-UI or QA-EDGE-15 test is ignored any more. |
| `#[ignore]` inventory, `origin/main` vs branch | Only the 13 QA-UI ignores and QA-EDGE-15 were removed; every other group's ignore is unchanged. |
| Branch's `qa_flow.rs`/`format_properties.rs` against `main`'s `src/` (separate worktree) | 13 QA-UI tests and `qa_edge_15_...` fail on the old source, as claimed; `small_stick_deflection_steers` passes there (QA-UI-10, below). |

### QA-UI-10 ("not a bug")

Confirmed against `bevy_input-0.19.1/src/gamepad.rs`: `gamepad_event_processing_system` runs
`AxisSettings::filter`, which only decides whether the change passes the threshold and computes
the *scaled* value for `GamepadAxisChangedEvent`; the `Gamepad` component stores
`filtered_value.raw` (`gamepad_axis.analog.set(axis, filtered_value.raw)`), and buttons/triggers
likewise. `bevy_gilrs-0.19.1` builds gilrs with `with_default_filters(false)`, so gilrs adds no
deadzone either. The repro test feeds `RawGamepadEvent`s through that path, and 0.12 already
steered on the old code. The game's 8 % stick / 5 % trigger gates are the only ones. Verdict
stands: not a bug, the test is a fair regression test.

### Captures

`tools/capture.sh menu` and `CAPTURE_PROFILE=release tools/capture.sh race` at 1280×720
(default), 1024×600, 800×600 (the original QA-UI-13 repro size) and 2560×1080; every setup,
main-menu and results image looked at, plus HUD and grid at the extremes.

- 1280×720: all six captures are pixel-identical to the goldens (`racer-capture diff`: mean
  error 0, 0 % differing pixels). `race/grid`, `hud`, `pack` goldens are byte-identical to
  `main`; only `menu/main-menu`, `menu/setup`, `race/results` changed.
- The three updated goldens are correct: setup shows the title, all 15 option rows, Start race
  and Back with margin; the main menu is the same layout with the smaller title; results show
  all 8 rows, every column, and the player's row `ABS TCS ESC Steer Line Auto`.
- 1024×600 and 800×600: setup and results scaled down as a whole, every row and column on
  screen; results table no longer cut on either side.
- 2560×1080: panels centred at natural size (not scaled up), nothing stretched.

### Code review

Root causes, not symptoms: pause was not reaching FFB because fixed steps stop (fixed by a
`PostUpdate` release while virtual time is paused); results stuck paused because nothing unpaused
on leaving the race (`OnExit(Race)`); `0:60.000` from rounding two fields separately (rounded once);
the wheel backend's "first joystick" choice (now SDL's own wheel type, which SDL checks against
its known-wheel list before any gamepad heuristics, `SDL_joystick.c` in sdl3-src 3.4.18). Mouse
picking honours the fit-to-window scale (`bevy_ui` picking inverts `UiGlobalTransform`, which
includes `UiTransform`).

Input interplay: keyboard + pad combine (stronger steering wins, pedals max); the wheel still
overrides when present, as before, but a gamepad can no longer be taken for one. North (reset)
and Select (camera) were unbound in races and menus, so no conflict with South/East/d-pad/Start/
bumpers. Reset and shifts are dropped while paused; the camera toggle is allowed while paused
(as V already was). Force feedback: released every frame while paused, 0 whenever no player car
exists (menus, setup, after a race is cleared), and the SDL haptic is closed on wheel unplug and
app exit (unchanged). State across restarts: `Paused`/virtual time reset on every exit from the
race; the camera snaps on a new car entity or a new reset. Scope: only `crates/racer-game`, its
goldens, the README controls paragraph and this note. No dead code. README controls match the
bindings.

### Issues found

None that needed a code change. Left open, all minor:

- `toggle_pause` reads `GamepadConnectionEvent` only while in `AppState::Race`, so a pad unplugged
  in the one or two frames before a race starts can still be read on its first frame and pause it.
  Harmless (the player resumes) and hard to hit.
- `fit_to_window` runs in `Update` on the previous frame's layout, so a newly spawned panel that
  is too large shows unscaled for one frame before it is fitted.
- `rests_low` relies on SDL knowing an axis's initial value at open; backends that only learn it
  on the first poll fall back to the old "rests at +1" mapping. Wheels SDL does not classify as
  wheels are no longer picked up (documented above). Still unverified on hardware.
- Unplugging the SDL *wheel* mid-race does not pause (only gamepads do); not part of QA-UI-9.
- The 250 px Assists column just fits the longest label (`ABS TCS ESC Steer Line Auto`).

### Verdict

Approve. Every QA-UI bug and QA-EDGE-15 is fixed at its cause with a regression test that fails
on the old source, QA-UI-10 is correctly closed as not a bug, all checks are green, and the
three golden updates are deliberate and correct.

### Merge of `main` at `c4110b8` (Porsche 911 car model)

`origin/main` (owner's commit `c4110b8`, car model + preloading) merged into `fix/game-ui` as a
merge commit; it applied with no conflicts. Overlap is limited to `flow.rs`: the Loading state now
spawns a loading screen through `screen()` (so it gets `FitToWindow` too) and waits on
`car_model::models_ready` before `go_to_race`; the UI fixes are untouched and `OnExit(Race)`
still unpauses. Re-run on the merged tree: setup, fmt, both clippy runs, both cross-target clippy
runs clean; `cargo test --workspace --locked` three times green (98 passed, 57 ignored);
`--features wheel` green; `--ignored` unchanged (52 other-group repros fail, 5 checks pass). (One
wheel-feature run first failed on a stale `racer-dynamics` artifact pointing at the temporary
worktree used above for the old-source check; `cargo clean -p` of the workspace crates fixed it,
not a code issue.) Captures at 1280×720, 1024×600, 800×600 and 2560×1080 were redone and looked at:
1280×720 is pixel-identical to every golden, the rest match the earlier review. The built 911 GLB
lives in the gitignored `assets/` (absent here and in CI), so cars still render as greybox and
**no golden changed**; none was regenerated. Verdict unchanged: approve.
