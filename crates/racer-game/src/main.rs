use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    prelude::*,
};
use racer_game::{
    driving::{DrivingPlugin, Location},
    greybox::{GreyboxPlugin, SceneCamera},
    input::InputPlugin,
};
use racer_sim::{SimPlugin, vehicle::VehiclePlugin};

const USAGE: &str = "usage: racer-game [drive [--car 1|2|3] [--skidpad] | greybox]";

fn main() -> AppExit {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut app = App::new();
    app.add_plugins((
        DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: "Racer".into(),
                ..default()
            }),
            ..default()
        }),
        SimPlugin,
    ));
    match args.first().map(String::as_str) {
        Some("greybox") => {
            app.add_plugins((FreeCameraPlugin, GreyboxPlugin))
                .add_systems(PostStartup, attach_free_camera);
        }
        None | Some("drive") => {
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
