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

Updated deliberately: `menu/main-menu.png`, `menu/setup.png` (compact rows, smaller title),
`race/results.png` (centred panel, assists column with the player's aids). See the commit
for any others.
