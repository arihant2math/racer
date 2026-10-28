use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    prelude::*,
};
use racer_game::{
    GamePlugin,
    driving::{DrivingPlugin, Location},
    flow::{AppState, RaceSetup},
    greybox::{GreyboxPlugin, SceneCamera},
    input::InputPlugin,
};
use racer_sim::{SimPlugin, vehicle::VehiclePlugin};

const USAGE: &str = "usage: racer-game [race [--car 1|2|3] [--ai N] [--difficulty NAME|mixed] [--laps N] [--autopilot] | drive [--car 1|2|3] [--skidpad] | greybox]";

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins
            .set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Racer".into(),
                    ..default()
                }),
                ..default()
            })
            .set(AssetPlugin {
                file_path: racer_game::car_model::assets_dir()
                    .to_string_lossy()
                    .into_owned(),
                ..default()
            }),
        SimPlugin,
    ));
    match args.first().map(String::as_str) {
        Some("greybox") => {
            app.add_plugins((FreeCameraPlugin, GreyboxPlugin))
                .add_systems(PostStartup, attach_free_camera);
        }
        None => {
            app.add_plugins(GamePlugin);
        }
        Some("race") => {
            // Straight into a race with the given setup.
            let value = |flag: &str| args.iter().skip_while(|a| *a != flag).nth(1).cloned();
            let mut setup = RaceSetup::default();
            if let Some(car) = value("--car").and_then(|c| c.parse::<usize>().ok()) {
                setup.car = car.saturating_sub(1) % racer_dynamics::spec::CARS.len();
            }
            if let Some(n) = value("--ai").and_then(|c| c.parse::<usize>().ok()) {
                setup.opponents = n.min(7);
            }
            if let Some(laps) = value("--laps").and_then(|c| c.parse::<u32>().ok()) {
                setup.laps = laps.clamp(1, 10);
            }
            if let Some(d) = value("--difficulty") {
                setup.difficulty = racer_sim::ai::Difficulty::ALL
                    .into_iter()
                    .find(|x| x.name().eq_ignore_ascii_case(&d));
            }
            setup.autopilot = args.iter().any(|a| a == "--autopilot");
            app.add_plugins(GamePlugin)
                .insert_resource(setup)
                .insert_state(AppState::Loading);
        }
        Some("drive") => {
            let car = args
                .iter()
                .skip_while(|a| *a != "--car")
                .nth(1)
                .and_then(|c| c.parse::<usize>().ok())
                .map_or(0, |c| c.saturating_sub(1));
            let location = if args.iter().any(|a| a == "--skidpad") {
                Location::Skidpad
            } else {
                Location::Track
            };
            app.add_plugins((VehiclePlugin, InputPlugin, DrivingPlugin { car, location }));
        }
        _ => {
            eprintln!("{USAGE}");
            return AppExit::from_code(2);
        }
    }
    app.run()
}

fn attach_free_camera(mut commands: Commands, cameras: Query<Entity, With<SceneCamera>>) {
    for camera in &cameras {
        commands.entity(camera).insert(FreeCamera::default());
    }
}
