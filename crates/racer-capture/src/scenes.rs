//! Scenes the harness can capture and their scripted camera poses.

use bevy::prelude::*;
use racer_game::{
    driving::{CameraMode, DrivingPlugin, Location, hood_camera},
    greybox::{GreyboxPlugin, default_camera_transform},
};
use racer_sim::{
    track::{SKIDPAD_CENTER, test_track},
    vehicle::VehiclePlugin,
};

use crate::harness::Pose;

/// Names accepted by [`add_scene`].
pub const SCENES: &[&str] = &["greybox", "driving"];

/// Adds the named scene to `app` and returns its camera poses.
pub fn add_scene(app: &mut App, name: &str) -> Option<Vec<Pose>> {
    match name {
        "greybox" => {
            app.add_plugins(GreyboxPlugin);
            let target = Vec3::new(0.0, 0.6, 0.0);
            let look = |x, y, z| Transform::from_xyz(x, y, z).looking_at(target, Vec3::Y);
            Some(vec![
                pose("front-three-quarter", default_camera_transform()),
                pose("side", look(0.0, 1.2, 9.0)),
                pose("rear-high", look(-9.0, 5.0, -6.0)),
            ])
        }
        "driving" => {
            // The car (GTI) rests at the start line; the game cameras are
            // replaced by fixed poses so every capture is reproducible.
            app.insert_resource(CameraMode::Fixed).add_plugins((
                VehiclePlugin,
                DrivingPlugin {
                    car: 0,
                    location: Location::Track,
                },
            ));
            let start = test_track().start.translation;
            let car = start + Vec3::Y * 0.54;
            let look =
                |from: Vec3, at: Vec3| Transform::from_translation(from).looking_at(at, Vec3::Y);
            Some(vec![
                pose(
                    "chase",
                    look(
                        car + Vec3::new(0.0, 1.7, 6.5),
                        car + Vec3::new(0.0, 0.4, -2.0),
                    ),
                ),
                pose("hood", Transform::from_translation(car) * hood_camera()),
                pose(
                    "front-three-quarter",
                    look(car + Vec3::new(4.5, 1.2, -5.5), car),
                ),
                pose(
                    "hairpin",
                    look(Vec3::new(-25.0, 30.0, -215.0), Vec3::new(15.0, 0.0, -265.0)),
                ),
                pose(
                    "chicane",
                    look(
                        Vec3::new(110.0, 25.0, -150.0),
                        Vec3::new(130.0, 0.0, -195.0),
                    ),
                ),
                pose(
                    "skidpad",
                    look(SKIDPAD_CENTER + Vec3::new(0.0, 60.0, 90.0), SKIDPAD_CENTER),
                ),
            ])
        }
        _ => None,
    }
}

fn pose(name: &str, transform: Transform) -> Pose {
    Pose {
        name: name.into(),
        transform,
    }
}
