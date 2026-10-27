use bevy::{
    camera_controller::free_camera::{FreeCamera, FreeCameraPlugin},
    prelude::*,
};
use racer_game::greybox::{GreyboxPlugin, SceneCamera};
use racer_sim::SimPlugin;

fn main() -> AppExit {
    App::new()
        .add_plugins((
            DefaultPlugins.set(WindowPlugin {
                primary_window: Some(Window {
                    title: "Racer".into(),
                    ..default()
                }),
                ..default()
            }),
            FreeCameraPlugin,
            SimPlugin,
            GreyboxPlugin,
        ))
        .add_systems(PostStartup, attach_free_camera)
        .run()
}

fn attach_free_camera(mut commands: Commands, cameras: Query<Entity, With<SceneCamera>>) {
    for camera in &cameras {
        commands.entity(camera).insert(FreeCamera::default());
    }
}
