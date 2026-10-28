//! Game client library: scenes and plugins shared by the game binary and
//! the capture harness.

pub mod car_model;
pub mod driving;
pub mod flow;
pub mod greybox;
pub mod hud;
pub mod input;
pub mod race_scene;
#[cfg(feature = "wheel")]
mod sdl_wheel;
pub mod visuals;

use bevy::prelude::*;

/// The whole game: menus, race setup, races against the AI, results.
pub struct GamePlugin;

impl Plugin for GamePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            racer_sim::vehicle::VehiclePlugin,
            racer_sim::race::RacePlugin,
            input::InputPlugin,
            flow::FlowPlugin,
            race_scene::RaceScenePlugin,
        ));
    }
}
