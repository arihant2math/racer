//! `car-asset` scene: a car GLB from `assets/cars/<id>/car.glb` on a ground
//! plane, with the shared `car_paint` material swapped for
//! [`racer_render::car_paint`], to check the asset pipeline output in-engine.

use bevy::{gltf::GltfMaterialName, light::atmosphere::ScatteringMedium, prelude::*};
use racer_game::greybox::spawn_environment;

/// Car shown by the scene, relative to the workspace `assets/` folder.
pub const CAR_GLB: &str = "cars/porsche-911-carrera-s-992/car.glb";

/// Workspace `assets/` folder (racer-capture runs from any directory).
pub const ASSET_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// Paint colour used for the swapped-in car paint.
const PAINT: Color = Color::srgb(0.62, 0.04, 0.03);

pub struct CarAssetPlugin;

impl Plugin for CarAssetPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight::NONE)
            .add_systems(Startup, setup)
            .add_systems(Update, (swap_paint, hide_collision));
    }
}

#[derive(Resource)]
struct Paint(Handle<StandardMaterial>);

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
    assets: Res<AssetServer>,
) {
    spawn_environment(
        &mut commands,
        &mut media,
        Transform::from_xyz(4.0, 1.4, -5.0).looking_at(Vec3::new(0.0, 0.5, 0.0), Vec3::Y),
    );
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(200.0, 200.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.22, 0.22, 0.23),
            perceptual_roughness: 0.9,
            ..default()
        })),
    ));
    commands.insert_resource(Paint(materials.add(racer_render::car_paint(PAINT))));
    commands.spawn(SceneRoot(
        assets.load(GltfAssetLabel::Scene(0).from_asset(CAR_GLB)),
    ));
}

/// Replaces every primitive using the GLB's `car_paint` material.
fn swap_paint(
    mut commands: Commands,
    paint: Res<Paint>,
    added: Query<(Entity, &GltfMaterialName), Added<GltfMaterialName>>,
) {
    for (entity, name) in &added {
        if name.0 == "car_paint" {
            commands
                .entity(entity)
                .insert(MeshMaterial3d(paint.0.clone()));
        }
    }
}

/// The collision hull ships in the GLB for physics; it is not drawn.
fn hide_collision(mut commands: Commands, added: Query<(Entity, &Name), Added<Name>>) {
    for (entity, name) in &added {
        if name.as_str() == "collision" {
            commands.entity(entity).insert(Visibility::Hidden);
        }
    }
}

/// Camera poses around the car (car faces -Z, right side is +X).
pub fn poses() -> Vec<(&'static str, Transform)> {
    let at = Vec3::new(0.0, 0.55, 0.0);
    let look = |x, y, z| Transform::from_xyz(x, y, z).looking_at(at, Vec3::Y);
    vec![
        ("front-three-quarter", look(4.2, 1.3, -5.2)),
        ("rear-three-quarter", look(-4.0, 1.5, 5.4)),
        ("side", look(7.5, 0.9, 0.0)),
        ("front", look(0.0, 0.8, -7.0)),
        ("rear", look(0.0, 1.0, 7.0)),
    ]
}
