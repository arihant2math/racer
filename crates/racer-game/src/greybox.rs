//! Greybox test scene: ground plane, a car-sized box with clearcoat paint, a
//! shadow-casting sun and a physical sky.

use bevy::{
    camera::Exposure,
    core_pipeline::tonemapping::Tonemapping,
    light::{
        Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfigBuilder,
        atmosphere::ScatteringMedium, light_consts::lux,
    },
    pbr::AtmosphereSettings,
    prelude::*,
};

/// Car body dimensions in metres (length, height, width), roughly a hot hatch.
pub const CAR_SIZE: Vec3 = Vec3::new(4.3, 1.45, 1.8);

/// Spawns the greybox scene and its camera.
pub struct GreyboxPlugin;

impl Plugin for GreyboxPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(GlobalAmbientLight::NONE)
            .add_systems(Startup, setup);
    }
}

/// Marks the camera that views the scene.
#[derive(Component)]
pub struct SceneCamera;

/// Default viewpoint: front three-quarter view of the car.
pub fn default_camera_transform() -> Transform {
    Transform::from_xyz(6.0, 2.2, 7.5).looking_at(Vec3::new(0.0, 0.6, 0.0), Vec3::Y)
}

fn setup(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut media: ResMut<Assets<ScatteringMedium>>,
) {
    commands.spawn(Atmosphere::earth(
        media.add(ScatteringMedium::earth(256, 256)),
    ));

    commands.spawn((
        SceneCamera,
        Camera3d::default(),
        default_camera_transform(),
        AtmosphereSettings::default(),
        AtmosphereEnvironmentMapLight::default(),
        Exposure::SUNLIGHT,
        Tonemapping::AcesFitted,
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: lux::RAW_SUNLIGHT,
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(-4.0, 6.0, 3.0).looking_at(Vec3::ZERO, Vec3::Y),
        CascadeShadowConfigBuilder {
            maximum_distance: 80.0,
            first_cascade_far_bound: 12.0,
            ..default()
        }
        .build(),
    ));

    // Ground: matte asphalt-grey plane with lighter lane markings.
    commands.spawn((
        Mesh3d(meshes.add(Plane3d::default().mesh().size(400.0, 400.0))),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: Color::srgb(0.18, 0.18, 0.19),
            perceptual_roughness: 0.9,
            ..default()
        })),
    ));
    let marking = materials.add(StandardMaterial {
        base_color: Color::srgb(0.85, 0.85, 0.8),
        perceptual_roughness: 0.7,
        ..default()
    });
    let stripe = meshes.add(Plane3d::default().mesh().size(3.0, 0.15));
    for i in -10..=10 {
        commands.spawn((
            Mesh3d(stripe.clone()),
            MeshMaterial3d(marking.clone()),
            Transform::from_xyz(i as f32 * 6.0, 0.005, -2.0),
        ));
    }

    // Car: clearcoat body on four wheels.
    let body_y = 0.35 + CAR_SIZE.y / 2.0;
    commands.spawn((
        Mesh3d(meshes.add(Cuboid::from_size(CAR_SIZE))),
        MeshMaterial3d(materials.add(racer_render::car_paint(Color::srgb(0.75, 0.05, 0.04)))),
        Transform::from_xyz(0.0, body_y, 0.0),
    ));
    let wheel = meshes.add(Cylinder::new(0.33, 0.24));
    let tire = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.03, 0.03),
        perceptual_roughness: 0.8,
        ..default()
    });
    for (x, z) in [(1.35, 0.85), (1.35, -0.85), (-1.35, 0.85), (-1.35, -0.85)] {
        commands.spawn((
            Mesh3d(wheel.clone()),
            MeshMaterial3d(tire.clone()),
            Transform::from_xyz(x, 0.33, z)
                .with_rotation(Quat::from_rotation_x(std::f32::consts::FRAC_PI_2)),
        ));
    }
}
