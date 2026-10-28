//! The Options screen: from the main menu and from the pause menu, by
//! keyboard and gamepad, with the HUD following the units chosen. Runs the
//! whole game headless, as `qa_flow.rs` does.

use std::time::Duration;

use bevy::{
    input::{
        ButtonState,
        gamepad::{
            GamepadConnection, GamepadConnectionEvent, RawGamepadButtonChangedEvent,
            RawGamepadEvent,
        },
        keyboard::{Key, KeyboardInput, NativeKey},
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
    settings::{Settings, SettingsFile, SpeedUnit},
};
use racer_sim::{
    SimPlugin,
    race::{Race, RacePhase},
};

fn game_app(setup: Option<RaceSetup>, file: Option<SettingsFile>) -> App {
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
    ))
    .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
        1.0 / 60.0,
    )));
    if let Some(file) = file {
        app.insert_resource(file.load()).insert_resource(file);
    }
    app.add_plugins(GamePlugin);
    if let Some(setup) = setup {
        app.insert_resource(setup).insert_state(AppState::Loading);
    }
    app.finish();
    app.cleanup();
    app.update();
    app
}

fn tap(app: &mut App, code: KeyCode) {
    for state in [ButtonState::Pressed, ButtonState::Released] {
        app.world_mut().write_message(KeyboardInput {
            key_code: code,
            logical_key: Key::Unidentified(NativeKey::Unidentified),
            state,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }
}

fn pad_tap(app: &mut App, pad: Entity, button: GamepadButton) {
    for value in [1.0, 0.0] {
        app.world_mut()
            .write_message(RawGamepadEvent::Button(RawGamepadButtonChangedEvent::new(
                pad, button, value,
            )));
        app.update();
    }
}

fn state(app: &App) -> AppState {
    *app.world().resource::<State<AppState>>().get()
}

fn units(app: &App) -> SpeedUnit {
    app.world().resource::<Settings>().interface.speed_units
}

fn texts(app: &mut App) -> Vec<String> {
    let mut q = app.world_mut().query::<&Text>();
    q.iter(app.world()).map(|t| t.0.clone()).collect()
}

/// Main menu items: Race, Quick match, Create lobby, Join lobby, Direct
/// connect, Options, Quit.
const OPTIONS_ITEM: usize = 5;

#[test]
fn options_from_the_main_menu_by_keyboard() {
    let mut app = game_app(None, None);
    for _ in 0..OPTIONS_ITEM {
        tap(&mut app, KeyCode::ArrowDown);
    }
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(state(&app), AppState::Options);
    let all = texts(&mut app);
    assert!(all.iter().any(|t| t == "Interface"), "{all:?}");
    assert!(
        all.iter()
            .any(|t| t.contains("Speed units") && t.contains("km/h")),
        "{all:?}"
    );

    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(units(&app), SpeedUnit::Mph);
    app.update();
    assert!(
        texts(&mut app)
            .iter()
            .any(|t| t.contains("Speed units") && t.contains("mph"))
    );
    tap(&mut app, KeyCode::ArrowLeft);
    assert_eq!(units(&app), SpeedUnit::Kmh);
    tap(&mut app, KeyCode::Enter); // Enter changes the row too
    assert_eq!(units(&app), SpeedUnit::Mph);

    // Down to Back, then Enter: the main menu again.
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(state(&app), AppState::MainMenu);
    assert_eq!(units(&app), SpeedUnit::Mph);
    assert!(app.should_exit().is_none());
}

#[test]
fn options_by_gamepad_and_back_with_esc() {
    let mut app = game_app(None, None);
    let pad = app.world_mut().spawn_empty().id();
    app.world_mut().write_message(GamepadConnectionEvent::new(
        pad,
        GamepadConnection::Connected {
            name: "pad".into(),
            vendor_id: None,
            product_id: None,
        },
    ));
    app.update();
    for _ in 0..OPTIONS_ITEM {
        pad_tap(&mut app, pad, GamepadButton::DPadDown);
    }
    pad_tap(&mut app, pad, GamepadButton::South);
    app.update();
    assert_eq!(state(&app), AppState::Options);
    pad_tap(&mut app, pad, GamepadButton::DPadRight);
    assert_eq!(units(&app), SpeedUnit::Mph);
    pad_tap(&mut app, pad, GamepadButton::East);
    app.update();
    assert_eq!(state(&app), AppState::MainMenu);

    tap(&mut app, KeyCode::ArrowUp); // wraps to Quit; one more up is Options
    tap(&mut app, KeyCode::ArrowUp);
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert_eq!(state(&app), AppState::Options);
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(state(&app), AppState::MainMenu);
    assert!(app.should_exit().is_none());
}

#[test]
fn options_from_the_pause_menu_switch_the_hud_to_mph() {
    let mut app = game_app(
        Some(RaceSetup {
            opponents: 0,
            laps: 1,
            ..default()
        }),
        None,
    );
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
    app.update();
    assert!(texts(&mut app).iter().any(|t| t.ends_with(" km/h")));

    // Esc, down to Options (Resume, Restart, Options, Main menu), Enter.
    tap(&mut app, KeyCode::Escape);
    assert!(app.world().resource::<Paused>().0);
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::ArrowDown);
    tap(&mut app, KeyCode::Enter);
    app.update();
    let all = texts(&mut app);
    assert!(all.iter().any(|t| t == "Options"), "{all:?}");
    assert!(
        app.world().resource::<Paused>().0,
        "options unpaused the race"
    );
    tap(&mut app, KeyCode::ArrowRight);
    assert_eq!(units(&app), SpeedUnit::Mph);

    // Esc goes back to the pause menu, still paused, cursor on Options.
    tap(&mut app, KeyCode::Escape);
    app.update();
    assert_eq!(state(&app), AppState::Race);
    assert!(app.world().resource::<Paused>().0);
    let all = texts(&mut app);
    assert!(all.iter().any(|t| t == "Paused"), "{all:?}");
    assert!(!all.iter().any(|t| t.contains("Speed units")), "{all:?}");
    // Up twice from Options is Resume.
    tap(&mut app, KeyCode::ArrowUp);
    tap(&mut app, KeyCode::ArrowUp);
    tap(&mut app, KeyCode::Enter);
    app.update();
    assert!(!app.world().resource::<Paused>().0);
    let all = texts(&mut app);
    assert!(all.iter().any(|t| t.ends_with(" mph")), "{all:?}");
    assert!(!all.iter().any(|t| t.ends_with(" km/h")), "{all:?}");
}

#[test]
fn a_change_is_saved_and_loaded_on_the_next_start() {
    let dir = std::env::temp_dir().join(format!("racer-options-{}", std::process::id()));
    let path = dir.join("settings.toml");
    let mut app = game_app(None, Some(SettingsFile(path.clone())));
    app.update();
    assert!(!path.exists(), "saved without a change");
    for _ in 0..OPTIONS_ITEM {
        tap(&mut app, KeyCode::ArrowDown);
    }
    tap(&mut app, KeyCode::Enter);
    app.update();
    tap(&mut app, KeyCode::ArrowRight);
    let text = std::fs::read_to_string(&path).expect("saved on change");
    assert!(text.contains("speed_units = \"mph\""), "{text}");
    drop(app);

    let app = game_app(None, Some(SettingsFile(path.clone())));
    assert_eq!(units(&app), SpeedUnit::Mph);
    std::fs::remove_dir_all(&dir).unwrap();
}
