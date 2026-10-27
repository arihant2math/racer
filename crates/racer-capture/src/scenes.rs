//! Scenes the harness can capture and their scripted camera poses.

use bevy::prelude::*;
use racer_game::greybox::{GreyboxPlugin, default_camera_transform};

use crate::harness::Pose;

/// Names accepted by [`add_scene`].
pub const SCENES: &[&str] = &["greybox"];

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
        _ => None,
    }
}

fn pose(name: &str, transform: Transform) -> Pose {
    Pose {
        name: name.into(),
        transform,
    }
}
