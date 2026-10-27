# QA: game flow, UI and input

Area: `crates/racer-game` (menus, race setup, HUD, pause, results, restart/back flow,
cameras, keyboard/gamepad/wheel input, mock wheel and force feedback). Tested on
`qa/game-flow-ui-input` (based on `4253183`, end of the M2 review) in a GPU-less container on
2026-10-27.

**How it was tested.** `crates/racer-game/tests/qa_flow.rs` runs the whole game headless
(DefaultPlugins without winit and with no wgpu backend, `SimPlugin` + `GamePlugin`, 60 fps
manual clock) and drives it with synthetic `KeyboardInput`, `GamepadConnectionEvent` and
`RawGamepadEvent` messages, so every input goes through Bevy's real input systems. The SDL3
wheel backend is tested in `tests/qa_wheel.rs` (`--features wheel`) with an SDL virtual
joystick. Layout was checked with `tools/capture.sh menu|race <out> --size WxH` under Xvfb +
lavapipe.

```sh
cargo test -p racer-game --test qa_flow -- --include-ignored     # ~40 s, 12 repros fail
cargo test -p racer-game --features wheel --test qa_wheel -- --include-ignored
```

Every bug below has a failing test marked `#[ignore = "QA-UI-N"]` except QA-UI-13 (layout,
screenshots only). The suite stays green without `--include-ignored`.

## Summary

| # | Severity | Title |
|---|---|---|
| QA-UI-1 | minor | Esc / Backspace / pad East on the main menu quits at once |
| QA-UI-2 | minor | R, Q, E pressed in the pause menu act on the car after resuming |
| QA-UI-3 | major | Force feedback keeps the last race force while paused |
| QA-UI-4 | minor | Any gamepad trigger or stick input discards keyboard steering |
| QA-UI-5 | minor | Results hide steering assist, racing line and gearbox |
| QA-UI-6 | minor | `lap_time` shows `0:60.000` instead of `1:00.000` |
| QA-UI-7 | minor | Chase camera flies across the city after a restart or reset |
| QA-UI-8 | minor | No gamepad button for reset or camera |
| QA-UI-9 | minor | Unplugging the gamepad mid-race does not pause |
| QA-UI-10 | minor | Stick deadzone applied twice (about 12.6 % dead travel) |
| QA-UI-11 | major | `--features wheel`: any gamepad is opened as a wheel with 50 % throttle and brake |
| QA-UI-12 | minor | Pausing on the frame the results come up leaves the results paused |
| QA-UI-13 | major | Setup and results screens do not fit the window (clipped even at 1280x720) |

**Totals: 0 critical, 3 major, 10 minor.**

## Bugs

### QA-UI-1: Esc / Backspace / gamepad East on the main menu quits the game (minor)

- **Repro:** `back_on_the_main_menu_does_not_quit_the_game`. Main menu, then Enter for setup,
  Esc back to the main menu, then Esc again: `AppExit::Success`. The same happens after Esc
  on the results screen followed by one more Esc, and with the pad's East (B) button, the
  usual "back" button.
- **Expected:** "Back" on the root menu does nothing, or asks for confirmation. Only the
  Quit item exits.
- **Actual:** the game closes with no prompt. Double-tapping Esc on the setup screen closes
  the game.
- **Where:** `crates/racer-game/src/flow.rs:510-511` (`back` on `MainMenu` maps to
  `Action::Quit`).
- **Confidence:** high.

### QA-UI-2: Reset and shift keys pressed while paused act on resume (minor)

- **Repro:** `reset_and_shift_keys_are_ignored_while_paused`. In a running race, Esc, R, E,
  then Esc to resume: the car is reset (one `Requested` reset in `Racer::resets`), and
  `DriverInput::shift_up` is latched while paused, so the upshift happens on resume.
- **Expected:** driving input is ignored while the pause menu is open.
- **Actual:** `reset_key` runs in `AppState::Race` regardless of `Paused`, and inserts
  `ResetRequest`, which the fixed step applies when time resumes. `read_devices` keeps
  OR-ing `just_pressed(E/Q)` and pad bumpers into the latched shift flags, which only
  `apply_to_car` clears, and that system does not run while virtual time is paused.
  Pressing R to "get out" of a pause costs a reset (and a reset counted in the results).
- **Where:** `crates/racer-game/src/race_scene.rs:47,245-256`;
  `crates/racer-game/src/input.rs:125-126,147-148,193-194`.
- **Confidence:** high.

### QA-UI-3: Force feedback is not released while paused (major)

- **Repro:** `force_feedback_is_released_while_paused` (`MockWheel` steering 0.4 at 0.6
  throttle). The last force before Esc is about -0.86, and over one second paused, 0 new
  forces are sent.
- **Expected:** the rim goes slack (force 0) when the game pauses, as most racing games do.
- **Actual:** `force_feedback` runs in `FixedPostUpdate`, which does not run while
  `Time<Virtual>` is paused. The SDL backend keeps re-applying the stored level every 4 ms
  (`sdl_wheel.rs:192-197`), so a real wheel keeps pulling with up to full force for the
  whole pause, including while the player uses the pause menu. Any other stall of virtual
  time (for example a long frame clamped by `max_delta`) does the same.
- **Where:** `crates/racer-game/src/input.rs:21,207-212`.
- **Confidence:** high (mock wheel). Hardware is not available.

### QA-UI-4: Gamepad input throws away keyboard steering (minor)

- **Repro:** `keyboard_steering_survives_a_gamepad_trigger`. Hold D, with a pad's right
  trigger at 0.6: `DriverInput { device: Gamepad, steer: 0.0, throttle: 0.6 }`.
- **Expected:** combined inputs merge, as throttle and brake already do (`max`), or the
  stronger steering wins.
- **Actual:** once a pad has any stick or trigger input over its threshold, `next.steer` is
  overwritten with the pad stick (0 when centred). Keyboard steering with pad pedals
  (a common pairing) does not steer at all. With two active pads, the last one in query
  order wins.
- **Where:** `crates/racer-game/src/input.rs:134-141`.
- **Confidence:** high.

### QA-UI-5: Results hide steering assist, racing line and gearbox (minor)

- **Repro:** `results_show_steering_assist_and_racing_line`. With ABS, TCS and ESC off,
  and steering assist and racing line on, the assists cell reads `none`.
- **Expected:** PLAN.md section 3 lists assists as ABS, TCS, stability control, steering
  assist and racing line, "shown in the results screen so times stay comparable". The
  gearbox (automatic or manual) also matters for comparing times.
- **Actual:** only `racer_dynamics::Assists` (ABS/TCS/ESC) is stored on `Racer` and shown.
- **Where:** `crates/racer-game/src/flow.rs:693-704`;
  `crates/racer-game/src/race_scene.rs:148-157,181-187`.
- **Confidence:** high.

### QA-UI-6: `lap_time` rounds to `0:60.000` (minor)

- **Repro:** `lap_time_carries_rounding_into_minutes`: `lap_time(59.9996)` is `"0:60.000"`
  (and `119.9997` gives `"1:60.000"`).
- **Expected:** `1:00.000`.
- **Actual:** the minutes are truncated before the seconds are rounded by `{:06.3}`. The
  function is used by the HUD (lap, last, best) and the results (time, best lap). About 1
  in 2000 times hits it.
- **Where:** `crates/racer-game/src/flow.rs:663-665`.
- **Confidence:** high.

### QA-UI-7: Chase camera flies from the old spot after a restart or reset (minor)

- **Repro:** `chase_camera_starts_behind_the_car_after_a_restart`. The car is somewhere on
  the track, then pause and "Restart race". On the first frames of the new race, the
  camera is 319 m from the car on the grid. It then lerps across the city, through
  buildings, during the countdown. The same happens on every reset (R or the automatic
  3 s reset), because the car teleports up to a few hundred metres back and the chase
  camera smooths towards it.
- **Expected:** the camera snaps behind the car when it is spawned or teleported.
- **Actual:** `ChaseCamera::position` survives across races, because the camera entity is
  kept, and is never cleared on a reset.
- **Where:** `crates/racer-game/src/driving.rs:579-590` (no snap), and the camera spawned
  once in `race_scene.rs:65-78`.
- **Confidence:** high for the restart case. For resets, confidence is from reading the
  code: same smoothing path.

### QA-UI-8: No gamepad button for reset or camera (minor)

- **Repro:** `some_gamepad_button_resets_the_car`. North, West, Select, both thumbs, the
  d-pad and Mode do nothing in a race.
- **Expected:** a pad-only player can reset (README: "R reset") and switch camera ("V
  chase/hood camera"), as they can pause with Start.
- **Actual:** reset and camera are keyboard only (`KeyCode::KeyR`, `KeyCode::KeyV`). A pad
  player stuck against a wall must wait for the automatic reset, and cannot leave the
  chase camera.
- **Where:** `crates/racer-game/src/race_scene.rs:245-256`;
  `crates/racer-game/src/driving.rs:551-559`.
- **Confidence:** high. This is a gap rather than a regression.

### QA-UI-9: Unplugging the gamepad mid-race does not pause (minor)

- **Repro:** `gamepad_disconnect_pauses_the_race`. At full trigger, disconnect the pad: the
  race keeps running and the car coasts with no driver.
- **Expected:** auto-pause on controller disconnect (a platform requirement on consoles,
  and standard on PC).
- **Actual:** nothing listens for `GamepadConnectionEvent::Disconnected`.
- **Where:** `crates/racer-game/src/flow.rs:406-436` (pause only on Esc or Start).
- **Confidence:** high.

### QA-UI-10: Stick deadzone applied twice (minor)

- **Repro:** `small_stick_deflection_steers`. The raw stick at 0.12 gives `steer 0`, and the
  device stays `Keyboard`.
- **Expected:** one deadzone. Bevy's default `AxisSettings` already maps ±0.05 to 0 and
  rescales the rest to 0..1.
- **Actual:** `read_devices` cuts another 0.08 of the rescaled value, so the effective
  deadzone is 0.05 + 0.08 x 0.95 ≈ 12.6 % of travel, which is large for fine steering at
  speed with the assist's reduced lock. The triggers similarly lose about 10 %
  (Bevy 0.05 plus the game's 0.05 gate).
- **Where:** `crates/racer-game/src/input.rs:134-138`.
- **Confidence:** high.

### QA-UI-11: `--features wheel` opens any gamepad as a wheel (major)

- **Repro:** `tests/qa_wheel.rs` `a_gamepad_is_not_taken_for_a_wheel`
  (`cargo test -p racer-game --features wheel --test qa_wheel -- --include-ignored`). An SDL
  virtual *gamepad* at rest is opened by `default_wheel()`, which reads
  `WheelReading { steer: 0.0, throttle: 0.5, brake: 0.5, clutch: 0.5 }`.
- **Expected:** only steering wheels (`SDL_JOYSTICK_TYPE_WHEEL`, or a known wheel) are
  taken. Gamepads are left to Bevy/gilrs.
- **Actual:** `open()` takes the first joystick with two or more axes. Pedals are mapped
  as `(1 - axis) / 2`, so centred sticks read as half throttle, half brake and half clutch.
  Because the wheel reading replaces steer, throttle, brake and clutch and sets
  `device = Wheel` (no steering filter), a wheel-feature build with a gamepad plugged in
  cannot be driven with the gamepad or the keyboard.
  Also unverified on hardware: the fixed `(1 - axis) / 2` pedal mapping assumes inverted
  pedal axes. A wheel whose pedals rest at -1 would read full throttle and full brake at
  rest. Pedal axes and ranges should be per device or calibrated.
- **Where:** `crates/racer-game/src/sdl_wheel.rs:102-107,170-189`;
  `crates/racer-game/src/input.rs:150-158`.
- **Confidence:** high for the gamepad case (virtual device). Medium for the pedal-range
  remark.

### QA-UI-12: Pause on the frame the results come up leaves the results paused (minor)

- **Repro:** `pause_on_the_frame_the_results_come_up_does_not_stick`. The player finishes;
  pressing Esc on frame 180 after it (the frame `to_results` fires) leaves the game in
  `Results` with `Paused(true)` and virtual time paused.
- **Expected:** no pause outside a race, or the results unpause.
- **Actual:** `toggle_pause` and `to_results` are unordered in `Update`. Both run, and the
  pause menu is despawned on exit, but the time pause stays. The results screen then
  shows a frozen race: the others stay "running", the DNF timer never runs and the table
  never completes. Choosing either menu item recovers.
- **Where:** `crates/racer-game/src/flow.rs:50`; `crates/racer-game/src/race_scene.rs:47,259-277`.
- **Confidence:** high (reproduced; frame-exact timing).

### QA-UI-13: Setup and results screens do not fit the window (major)

- **Repro:** `tools/capture.sh menu <out> --size 800x600` and
  `CAPTURE_PROFILE=release tools/capture.sh race <out> --size 800x600`.
  - `game-flow-ui-input/setup-1280x720.png` (the committed golden, at the default window
    size): the panel is taller than the window. The title touches the top edge and the
    panel's bottom padding is cut off.
  - `game-flow-ui-input/setup-800x600.png`: the title is gone, "Start race" is cut in half
    and "Back" is off-screen.
  - `game-flow-ui-input/results-800x600.png`: the results table (about 1044 px of fixed
    column widths) is cut on both sides. Positions, driver names and the assists column
    are not visible.
- **Expected:** menus fit or scroll at common small sizes (1280x720, 1366x768, laptop
  windows) and resize with the window.
- **Actual:** fixed pixel sizes are centred with no overflow handling (17 rows of 20 px
  text plus a 40 px title on the setup screen, and fixed column widths on the results).
  Keyboard and pad navigation still work, but the cursor can sit on rows that are
  off-screen.
- **Where:** `crates/racer-game/src/flow.rs:297-363` (`screen`), `568-577,579-660`
  (results columns).
- **Confidence:** high. The HUD itself is fine at 800x600 (`hud-800x600.png`).

## Checked and fine

- Full flow by keyboard and by pad: main menu → setup → race → pause → resume / restart /
  menu → results → restart / menu. Gamepad Start opens and closes the pause menu (M2 fix
  holds). The results ignore keys and pad buttons for the first second (M2 fix holds).
- Restart loops: 6 restarts from the pause menu plus one results → menu → new race, with
  2 AI. The entity count is identical every time (`restart_loops_do_not_leak_entities`).
  No HUD, minimap-dot, pause-menu or car leak.
- Key and button mashing: 1500 random Enter/Esc/Space/arrows/Backspace/R/V and pad
  South/East/Start/d-pad presses across every state. No panic, and time is never paused
  outside a race (except QA-UI-12's single frame).
- Setup selectors wrap correctly (mode, car 0..2, AI 0..7, difficulty incl. Mixed, laps 1..10,
  collisions, toggles). Time Trial forces 0 AI and Sprint forces one run. The CLI clamps
  `--ai` to 7 and `--laps` to 1..10, and `--car` wraps.
- HUD against the sim at the start (`P1 / 1`, `Lap 1 / 1`). Gear shows R/N for reverse
  and neutral, speed uses `abs` (km/h), the countdown shows 3-2-1 then GO!, and the
  off-track countdown is fixed (M2). HUD and minimap layout at 800x600 and 1280x720.
- Steering filter: clamps out-of-range targets, the assist limit falls with |speed| (and
  reverse), the wheel bypasses it, and there are no NaNs for finite input.
- FFB sign and scale with `MockWheel`: steering right at speed pulls the rim left, the force
  is symmetric for left and right, and it is 0 at standstill. On the test track (GTI,
  steer 0.02-0.6, up to 33 m/s) it peaks around 0.64 with 0-2 % of samples saturated, and
  there are no NaNs. Out-of-range wheel steering is unclamped in `Controls` but clamped in
  `racer-dynamics` (`vehicle.rs:299`).
- Camera: V toggles chase/hood mid-race, hood snaps, and switching back to chase snaps
  (position cleared). Menus orbit the track.

## Cross-area notes (not filed here)

- An unknown `--difficulty` name silently gives a mixed field (already in the M2 review).
- In the results screen, the player's car still takes driving input: the arrow keys
  that move the menu cursor also drive it. This is harmless, since the race is over for the
  player.
