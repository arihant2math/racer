//! Scenes the harness can capture and their scripted camera poses.

use bevy::prelude::*;
use racer_citykit::{
    lighting::camera_bundle,
    materials::{TextureSource, TimeOfDay},
    showcase::{ShowcasePlugin, camera_poses_for},
};
use racer_game::{
    driving::{CameraMode, DrivingPlugin, Location, hood_camera},
    greybox::{GreyboxPlugin, SceneCamera, default_camera_transform},
};
use racer_sim::{
    track::{SKIDPAD_CENTER, test_track},
    vehicle::VehiclePlugin,
};

use crate::harness::Pose;

/// Names accepted by [`add_scene`].
pub const SCENES: &[&str] = &[
    "greybox",
    "driving",
    "downtown-kit",
    "downtown-kit-golden",
    "downtown-kit-night",
    "downtown-kit-wet",
    "downtown-kit-procedural",
];

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
        "downtown-kit"
        | "downtown-kit-golden"
        | "downtown-kit-night"
        | "downtown-kit-wet"
        | "downtown-kit-procedural" => {
            // Track-side asset kit showcase (racer-citykit), one scene per
            // lighting preset; the wet variant is at night. The procedural variant
            // uses the offline procedural textures instead of the scans.
            let (time, wet) = match name {
                "downtown-kit" | "downtown-kit-procedural" => (TimeOfDay::Day, false),
                "downtown-kit-golden" => (TimeOfDay::Golden, false),
                "downtown-kit-night" => (TimeOfDay::Night, false),
                _ => (TimeOfDay::Night, true),
            };
            // Scanned 2k PBR textures (tools/fetch_textures.sh) by default;
            // the procedural variant shows the offline fallback.
            let textures = if name == "downtown-kit-procedural" {
                TextureSource::Procedural
            } else {
                TextureSource::repo_scans()
            };
            app.add_plugins(ShowcasePlugin {
                time,
                wet,
                textures,
            })
            .add_systems(Startup, move |mut commands: Commands| {
                commands.spawn((SceneCamera, camera_bundle(time, wet), Transform::default()));
            });
            Some(
                camera_poses_for(time)
                    .into_iter()
                    // The dry night is too dark from above for the image check.
                    .filter(|(pose, _, _)| !(name == "downtown-kit-night" && *pose == "drone"))
                    .map(|(name, eye, at)| {
                        pose(
                            name,
                            Transform::from_translation(eye).looking_at(at, Vec3::Y),
                        )
                    })
                    .collect(),
            )
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
