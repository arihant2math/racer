//! QA repro tests for game flow, UI and input (`docs/qa/game-flow-ui-input.md`).
//!
//! The whole game runs headless (no window, no GPU): synthetic keyboard and
//! gamepad messages go through Bevy's input systems and the app is stepped
//! frame by frame. Tests for open bugs are `#[ignore = "QA-UI-N"]`.

use std::time::Duration;

use bevy::{
    input::{
        ButtonState,
        gamepad::{
            GamepadConnection, GamepadConnectionEvent, RawGamepadAxisChangedEvent,
            RawGamepadButtonChangedEvent, RawGamepadEvent,
        },
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    render::{
        RenderPlugin,
        settings::{RenderCreation, WgpuSettings},
    },
    time::TimeUpdateStrategy,
    winit::WinitPlugin,
};
use racer_game::{
    GamePlugin,
    flow::{AppState, Paused, RaceSetup},
    input::{DriverInput, MockWheel, Player, Wheel, WheelReading},
};
use racer_sim::{
    SimPlugin,
    race::{Race, RacePhase, Racer},
    rules::Status,
};

const FRAME: f32 = 1.0 / 60.0;

fn game_app(setup: Option<RaceSetup>) -> App {
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .build()
            .disable::<WinitPlugin>()
            .set(RenderPlugin {
                render_creation: RenderCreation::Automatic(Box::new(WgpuSettings {
                    backends: None,
                    ..default()
                })),
                ..default()
            }),
        SimPlugin,
        GamePlugin,
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
        FRAME,
    )));
    if let Some(setup) = setup {
        app.insert_resource(setup).insert_state(AppState::Loading);
    }
    app.finish();
    app.cleanup();
    app.update();
    app
}

/// A small, quick race: no AI unless asked.
fn quick_setup() -> RaceSetup {
    RaceSetup {
        opponents: 0,
        laps: 1,
        ..default()
    }
}

fn frames(app: &mut App, n: usize) {
    for _ in 0..n {
        app.update();
    }
}

fn key(app: &mut App, code: KeyCode, state: ButtonState) {
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: Key::Unidentified(bevy::input::keyboard::NativeKey::Unidentified),
        state,
        text: None,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

/// Press and release a key over two frames.
fn tap(app: &mut App, code: KeyCode) {
    key(app, code, ButtonState::Pressed);
    app.update();
    key(app, code, ButtonState::Released);
    app.update();
}

fn connect_pad(app: &mut App) -> Entity {
    let pad = app.world_mut().spawn_empty().id();
    app.world_mut().write_message(GamepadConnectionEvent::new(
        pad,
        GamepadConnection::Connected {
            name: "QA pad".into(),
            vendor_id: None,
            product_id: None,
        },
    ));
    app.update();
    pad
}

fn pad_button(app: &mut App, pad: Entity, button: GamepadButton, value: f32) {
    app.world_mut()
        .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
            pad, button, value,
        )));
}

fn pad_axis(app: &mut App, pad: Entity, axis: GamepadAxis, value: f32) {
    app.world_mut()
        .write_message(RawGamepadEvent::Axis(RawGamepadAxisChangedEvent::new(
            pad, axis, value,
        )));
}

fn pad_tap(app: &mut App, pad: Entity, button: GamepadButton) {
    pad_button(app, pad, button, 1.0);
    app.update();
    pad_button(app, pad, button, 0.0);
    app.update();
}

fn state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

fn paused(app: &App) -> bool {
    app.world().resource::<Paused>().0
}

fn player(app: &mut App) -> Entity {
    let mut q = app
        .world_mut()
        .query_filtered::<Entity, (With<Player>, With<Racer>)>();
    q.single(app.world()).expect("one player car")
}

fn race(app: &App) -> &Race {
    app.world().resource::<Race>()
}

/// Run until the countdown is over.
fn to_green(app: &mut App) {
    let mut guard = 0;
    while app
        .world()
        .get_resource::<Race>()
        .is_none_or(|r| r.phase == RacePhase::Countdown)
    {
        app.update();
        guard += 1;
        assert!(guard < 600, "countdown never ended");
    }
}

/// Mark the player finished and wait for the results screen.
fn finish_player(app: &mut App) {
    let me = player(app);
    let now = race(app).time;
    app.world_mut().get_mut::<Racer>(me).unwrap().status = Status::Finished(now.max(1.0));
    let mut guard = 0;
    while state(app) != AppState::Results {
        app.update();
        guard += 1;
        assert!(guard < 600, "results never came up");
    }
}

fn entity_count(app: &mut App) -> usize {
    app.world_mut().query::<Entity>().iter(app.world()).count()
}

fn texts(app: &mut App) -> Vec<String> {
    let mut q = app.world_mut().query::<&Text>();
    q.iter(app.world()).map(|t| t.0.clone()).collect()
}

// ---------------------------------------------------------------------------
// Smoke: the flow works headless at all (these pass).

#[test]
fn menu_to_race_and_back_by_keyboard() {
    let mut app = game_app(None);
    assert_eq!(state(&app), AppState::MainMenu);
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(state(&app), AppState::Setup);
    tap(&mut app, KeyCode::Enter); // cursor starts on "Start race"
    frames(&mut app, 3);
    assert_eq!(state(&app), AppState::Race);
    tap(&mut app, KeyCode::Escape);
    assert!(paused(&app));
    tap(&mut app, KeyCode::Escape);
    assert!(!paused(&app));
}

#[test]
fn hud_matches_the_sim_at_the_start() {
    let mut app = game_app(Some(quick_setup()));
    to_green(&mut app);
    frames(&mut app, 30);
    let all = texts(&mut app);
    assert!(all.iter().any(|t| t == "P1 / 1"), "{all:?}");
    assert!(all.iter().any(|t| t == "Lap 1 / 1"), "{all:?}");
}

// ---------------------------------------------------------------------------
// Open bugs.

/// QA-UI-1: Esc (or gamepad East / Backspace) on the main menu quits the
/// game at once, with no confirmation. Esc twice from the setup screen, or
/// once more after leaving the results with Esc, closes the game.
#[test]
#[ignore = "QA-UI-1"]
fn back_on_the_main_menu_does_not_quit_the_game() {
    let mut app = game_app(None);
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(state(&app), AppState::Setup);
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(state(&app), AppState::MainMenu);
    key(&mut app, KeyCode::Escape, ButtonState::Pressed);
    app.update();
    assert!(
        app.should_exit().is_none(),
        "a second Esc (setup -> main menu -> Esc) quit the game"
    );
}

/// QA-UI-2: keys pressed while the pause menu is open act on the car when
/// the race resumes: R resets the car, Q/E shifts are latched.
#[test]
#[ignore = "QA-UI-2"]
fn reset_and_shift_keys_are_ignored_while_paused() {
    let mut app = game_app(Some(quick_setup()));
    to_green(&mut app);
    frames(&mut app, 60);
    tap(&mut app, KeyCode::Escape);
    assert!(paused(&app));
    tap(&mut app, KeyCode::KeyR);
    tap(&mut app, KeyCode::KeyE);
    let latched = app.world().resource::<DriverInput>().shift_up;
    tap(&mut app, KeyCode::Escape);
    assert!(!paused(&app));
    frames(&mut app, 10);
    let me = player(&mut app);
    let resets = app.world().get::<Racer>(me).unwrap().resets.len();
    assert_eq!(
        resets, 0,
        "R pressed in the pause menu reset the car on resume"
    );
    assert!(
        !latched,
        "E pressed in the pause menu is queued as an upshift"
    );
}

/// QA-UI-3: force feedback is not released while the game is paused (or
/// while time stands still): the last race force keeps pulling the rim.
#[test]
#[ignore = "QA-UI-3"]
fn force_feedback_is_released_while_paused() {
    let forces = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut app = game_app(None);
    app.insert_resource(Wheel(Box::new(MockWheel {
        reading: Some(WheelReading {
            steer: 0.4,
            throttle: 0.6,
            ..default()
        }),
        forces: forces.clone(),
    })))
    .insert_resource(quick_setup());
    app.world_mut()
        .resource_mut::<NextState<AppState>>()
        .set(AppState::Loading);
    frames(&mut app, 3);
    to_green(&mut app);
    frames(&mut app, 120);
    let before = *forces.lock().unwrap().last().unwrap();
    assert!(before.abs() > 0.05, "no force while cornering: {before}");
    tap(&mut app, KeyCode::Escape);
    assert!(paused(&app));
    let sent = forces.lock().unwrap().len();
    frames(&mut app, 60);
    let after = forces.lock().unwrap();
    assert_eq!(
        after.last().copied().unwrap(),
        0.0,
        "paused for 1 s: the wheel still holds the last race force ({} forces sent since the pause)",
        after.len() - sent
    );
}

/// QA-UI-4: holding a gamepad trigger (or nudging the stick) throws away
/// the keyboard's steering: keyboard + pad cannot be combined.
#[test]
#[ignore = "QA-UI-4"]
fn keyboard_steering_survives_a_gamepad_trigger() {
    let mut app = game_app(None);
    let pad = connect_pad(&mut app);
    pad_button(&mut app, pad, GamepadButton::RightTrigger2, 0.6);
    key(&mut app, KeyCode::KeyD, ButtonState::Pressed);
    frames(&mut app, 2);
    let input = *app.world().resource::<DriverInput>();
    assert!((input.throttle - 0.6).abs() < 0.01, "{input:?}");
    assert_eq!(input.steer, 1.0, "D held, pad stick centred: {input:?}");
}

/// QA-UI-5: the results screen shows only ABS/TCS/ESC; steering assist,
/// racing line and gearbox (PLAN.md section 3: assists are shown in the
/// results so times stay comparable) are missing.
#[test]
#[ignore = "QA-UI-5"]
fn results_show_steering_assist_and_racing_line() {
    let mut app = game_app(Some(RaceSetup {
        abs: false,
        tcs: false,
        esc: false,
        steering_assist: true,
        racing_line: true,
        ..quick_setup()
    }));
    to_green(&mut app);
    finish_player(&mut app);
    frames(&mut app, 2);
    let all = texts(&mut app);
    assert!(
        !all.iter().any(|t| t == "none"),
        "assists column says \"none\" with steering assist and racing line on: {all:?}"
    );
}

/// QA-UI-6: `lap_time` rounds the seconds field to 60.000 instead of
/// carrying into the minutes (59.9996 s shows as 0:60.000).
#[test]
#[ignore = "QA-UI-6"]
fn lap_time_carries_rounding_into_minutes() {
    use racer_game::flow::lap_time;
    assert_eq!(lap_time(59.9996), "1:00.000");
    assert_eq!(lap_time(119.9997), "2:00.000");
}

/// QA-UI-7: the chase camera is not snapped when the car is teleported: on
/// a restart (and on a reset) it flies from the old spot across the city.
#[test]
#[ignore = "QA-UI-7"]
fn chase_camera_starts_behind_the_car_after_a_restart() {
    use racer_game::greybox::SceneCamera;
    let mut app = game_app(Some(quick_setup()));
    to_green(&mut app);
    // Somewhere else on the track, as after a lap.
    let me = player(&mut app);
    let far = Vec3::new(300.0, 0.0, 300.0);
    for _ in 0..30 {
        app.world_mut()
            .get_mut::<Transform>(me)
            .unwrap()
            .translation = far;
        app.update();
    }
    tap(&mut app, KeyCode::Escape);
    tap(&mut app, KeyCode::ArrowDown); // Restart race
    tap(&mut app, KeyCode::Enter);
    frames(&mut app, 2);
    assert_eq!(state(&app), AppState::Race);
    let me = player(&mut app);
    let car = app.world().get::<Transform>(me).unwrap().translation;
    let mut cams = app
        .world_mut()
        .query_filtered::<&Transform, With<SceneCamera>>();
    let cam = cams.single(app.world()).unwrap().translation;
    let d = cam.distance(car);
    assert!(d < 15.0, "camera {d:.0} m from the car at the restart");
}

/// QA-UI-8: a gamepad-only player cannot reset the car or switch camera
/// (R and V are keyboard only; no pad button maps to them).
#[test]
#[ignore = "QA-UI-8"]
fn some_gamepad_button_resets_the_car() {
    let mut app = game_app(Some(quick_setup()));
    let pad = connect_pad(&mut app);
    to_green(&mut app);
    frames(&mut app, 30);
    for b in [
        GamepadButton::North,
        GamepadButton::West,
        GamepadButton::Select,
        GamepadButton::LeftThumb,
        GamepadButton::RightThumb,
        GamepadButton::DPadUp,
        GamepadButton::DPadDown,
        GamepadButton::DPadLeft,
        GamepadButton::DPadRight,
        GamepadButton::Mode,
    ] {
        pad_tap(&mut app, pad, b);
    }
    frames(&mut app, 5);
    let me = player(&mut app);
    assert!(
        !app.world().get::<Racer>(me).unwrap().resets.is_empty(),
        "no gamepad button requests a reset"
    );
}

/// QA-UI-9: a gamepad that disconnects mid-race does not pause the game;
/// the car coasts on with nobody driving it.
#[test]
#[ignore = "QA-UI-9"]
fn gamepad_disconnect_pauses_the_race() {
    let mut app = game_app(Some(quick_setup()));
    let pad = connect_pad(&mut app);
    to_green(&mut app);
    pad_button(&mut app, pad, GamepadButton::RightTrigger2, 1.0);
    frames(&mut app, 60);
    app.world_mut().write_message(GamepadConnectionEvent::new(
        pad,
        GamepadConnection::Disconnected,
    ));
    frames(&mut app, 5);
    assert!(paused(&app), "pad unplugged at speed: race keeps running");
}

/// QA-UI-10: the stick deadzone is applied twice (Bevy's default 0.05 axis
/// deadzone rescales the value, then `read_devices` cuts another 0.08):
/// the first ~12.6 % of stick travel does nothing.
#[test]
#[ignore = "QA-UI-10"]
fn small_stick_deflection_steers() {
    let mut app = game_app(None);
    let pad = connect_pad(&mut app);
    pad_axis(&mut app, pad, GamepadAxis::LeftStickX, 0.12);
    frames(&mut app, 2);
    let input = *app.world().resource::<DriverInput>();
    assert!(input.steer > 0.0, "stick at 12 %: {input:?}");
}

/// QA-UI-12: Esc (or gamepad Start) on the frame the results come up opens
/// the pause menu and pauses time; the results screen then shows a frozen
/// race (the others never finish, no DNF) with the game still paused.
#[test]
#[ignore = "QA-UI-12"]
fn pause_on_the_frame_the_results_come_up_does_not_stick() {
    let mut stuck = None;
    for wait in 176..186 {
        let mut app = game_app(Some(quick_setup()));
        to_green(&mut app);
        let me = player(&mut app);
        app.world_mut().get_mut::<Racer>(me).unwrap().status = Status::Finished(1.0);
        frames(&mut app, wait);
        if state(&app) != AppState::Race {
            break;
        }
        key(&mut app, KeyCode::Escape, ButtonState::Pressed);
        app.update();
        app.update();
        if state(&app) == AppState::Results && paused(&app) {
            stuck = Some(wait);
            break;
        }
    }
    assert_eq!(stuck, None, "results screen entered with time paused");
}

// ---------------------------------------------------------------------------
// Leak and stress checks.

#[test]
fn restart_loops_do_not_leak_entities() {
    let mut app = game_app(Some(RaceSetup {
        opponents: 2,
        ..quick_setup()
    }));
    frames(&mut app, 10);
    let first = entity_count(&mut app);
    let mut counts = Vec::new();
    for _ in 0..6 {
        tap(&mut app, KeyCode::Escape);
        tap(&mut app, KeyCode::ArrowDown);
        tap(&mut app, KeyCode::Enter);
        frames(&mut app, 8);
        assert_eq!(state(&app), AppState::Race);
        counts.push(entity_count(&mut app));
    }
    // And through results and the menu.
    to_green(&mut app);
    finish_player(&mut app);
    frames(&mut app, 70);
    tap(&mut app, KeyCode::Escape);
    frames(&mut app, 2);
    assert_eq!(state(&app), AppState::MainMenu);
    tap(&mut app, KeyCode::Enter);
    app.update();
    tap(&mut app, KeyCode::Enter);
    frames(&mut app, 8);
    counts.push(entity_count(&mut app));
    assert!(
        counts.iter().all(|c| *c == first),
        "first race {first}, after restarts {counts:?}"
    );
}

#[test]
fn key_mashing_in_every_state_does_not_panic_or_stick() {
    let mut app = game_app(None);
    let pad = connect_pad(&mut app);
    let keys = [
        KeyCode::Enter,
        KeyCode::Escape,
        KeyCode::Space,
        KeyCode::ArrowUp,
        KeyCode::ArrowDown,
        KeyCode::ArrowLeft,
        KeyCode::ArrowRight,
        KeyCode::Backspace,
        KeyCode::KeyR,
        KeyCode::KeyV,
    ];
    let buttons = [
        GamepadButton::South,
        GamepadButton::East,
        GamepadButton::Start,
        GamepadButton::DPadDown,
        GamepadButton::DPadRight,
    ];
    // A cheap deterministic sequence; never Esc on the main menu (quits).
    let mut seed = 12345u32;
    let mut seen = std::collections::HashSet::new();
    for _ in 0..1500 {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
        let r = (seed >> 16) as usize;
        if state(&app) == AppState::MainMenu {
            tap(&mut app, KeyCode::Enter);
            continue;
        }
        if r.is_multiple_of(3) {
            pad_tap(&mut app, pad, buttons[r % buttons.len()]);
        } else {
            tap(&mut app, keys[r % keys.len()]);
        }
        seen.insert(format!("{:?}", state(&app)));
        // Time never stays paused outside a race.
        if state(&app) != AppState::Race {
            assert!(!paused(&app), "paused in {:?}", state(&app));
            assert!(!app.world().resource::<Time<Virtual>>().is_paused());
        }
    }
    assert!(app.should_exit().is_none());
    assert!(seen.len() >= 3, "states visited: {seen:?}");
}
