//! Greybox driving scene: the test track and skidpad, a drivable car with
//! car selection, chase and hood cameras and a debug HUD.

use bevy::{
    asset::RenderAssetUsages,
    light::atmosphere::ScatteringMedium,
    mesh::{Indices, PrimitiveTopology},
    prelude::*,
};
use racer_dynamics::{CarSpec, load_car, spec::CARS};
use racer_sim::{
    track::{PieceKind, Shape, Track, TrackPiece, skidpad_start, spawn_track, test_track},
    vehicle::{Car, CarTelemetry, spawn_car, wheel_center},
};

use crate::{
    greybox::{SceneCamera, spawn_environment},
    input::Player,
};

/// Spawns the driving scene. `car` indexes [`CARS`].
pub struct DrivingPlugin {
    pub car: usize,
    pub location: Location,
}

impl Plugin for DrivingPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Selection {
            car: self.car,
            location: self.location,
        })
        .insert_resource(TrackLayout(test_track()))
        .init_resource::<CameraMode>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                select,
                respawn_car,
                toggle_camera,
                update_hud,
                mesh_track_pieces,
            ),
        )
        .add_systems(
            PostUpdate,
            (animate_wheels, follow_camera).before(TransformSystems::Propagate),
        );
    }
}

/// Where the car is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Location {
    Track,
    Skidpad,
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct Selection {
    pub car: usize,
    pub location: Location,
}

#[derive(Resource)]
pub struct TrackLayout(pub Track);

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CameraMode {
    #[default]
    Chase,
    Hood,
}

/// Smoothed chase-camera state.
#[derive(Component, Default)]
pub struct ChaseCamera {
    position: Option<Vec3>,
    heading: Vec3,
}

/// Visual wheel `index` of its parent car, with its accumulated spin angle.
#[derive(Component)]
struct WheelVisual {
    index: usize,
    spin: f32,
}

#[derive(Component)]
struct Hud;

/// Body colour per car, in [`CARS`] order.
const PAINT: [Color; 3] = [
    Color::srgb(0.75, 0.05, 0.04),
    Color::srgb(0.05, 0.35, 0.18),
    Color::srgb(0.95, 0.75, 0.05),
];

fn setup(
    mut commands: Commands,
    mut media: ResMut<Assets<ScatteringMedium>>,
    layout: Res<TrackLayout>,
) {
    let camera = spawn_environment(
        &mut commands,
        &mut media,
        Transform::from_xyz(0.0, 3.0, 8.0),
    );
    commands
        .entity(camera)
        .insert((ChaseCamera::default(), Projection::Perspective(default())));
    spawn_track(&mut commands, &layout.0);
    commands.spawn((
        Hud,
        Text::new(""),
        TextFont::from_font_size(16.0),
        TextColor(Color::WHITE),
        Node {
            position_type: PositionType::Absolute,
            top: px(10),
            left: px(10),
            ..default()
        },
        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.45)),
    ));
}

/// Adds greybox meshes to new track pieces.
fn mesh_track_pieces(
    mut commands: Commands,
    pieces: Query<(Entity, &TrackPiece), Added<TrackPiece>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: Local<Vec<(PieceKind, Handle<StandardMaterial>)>>,
) {
    for (entity, piece) in &pieces {
        let material = match cache.iter().find(|(k, _)| *k == piece.kind) {
            Some((_, m)) => m.clone(),
            None => {
                let (color, roughness) = match piece.kind {
                    PieceKind::Grass => (Color::srgb(0.16, 0.24, 0.12), 0.95),
                    PieceKind::Road => (Color::srgb(0.22, 0.22, 0.23), 0.9),
                    PieceKind::Curb(true) => (Color::srgb(0.7, 0.08, 0.06), 0.7),
                    PieceKind::Curb(false) | PieceKind::Marking => {
                        (Color::srgb(0.85, 0.85, 0.8), 0.7)
                    }
                };
                let m = materials.add(StandardMaterial {
                    base_color: color,
                    perceptual_roughness: roughness,
                    ..default()
                });
                cache.push((piece.kind, m.clone()));
                m
            }
        };
        let mesh = match &piece.shape {
            Shape::Box(size) => Cuboid::from_size(*size).into(),
            Shape::Mesh(m) => Mesh::new(
                PrimitiveTopology::TriangleList,
                RenderAssetUsages::RENDER_WORLD,
            )
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, m.vertices.clone())
            .with_inserted_indices(Indices::U32(m.indices.iter().flatten().copied().collect()))
            .with_computed_smooth_normals(),
        };
        commands
            .entity(entity)
            .insert((Mesh3d(meshes.add(mesh)), MeshMaterial3d(material)));
    }
}

fn start_pose(location: Location, layout: &Track) -> Transform {
    match location {
        Location::Track => layout.start,
        Location::Skidpad => skidpad_start(),
    }
}

/// Spawns the player's car with a greybox body and four wheels.
pub fn spawn_player_car(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    spec: &CarSpec,
    paint: Color,
    at: Transform,
) -> Entity {
    let car = spawn_car(commands, spec, at).expect("car.toml is validated by tests");
    let c = &spec.chassis;
    let (len, wid, hgt) = (c.length_m as f32, c.width_m as f32, c.height_m as f32);
    let ride = spec.estimated.chassis.cg_height_m as f32;
    let paint = materials.add(racer_render::car_paint(paint));
    let glass = materials.add(StandardMaterial {
        base_color: Color::srgb(0.05, 0.06, 0.08),
        perceptual_roughness: 0.1,
        ..default()
    });
    let tire = materials.add(StandardMaterial {
        base_color: Color::srgb(0.03, 0.03, 0.03),
        perceptual_roughness: 0.8,
        ..default()
    });
    let lower = 0.55 * hgt - 0.15;
    commands.entity(car).insert(Player).with_children(|p| {
        p.spawn((
            Mesh3d(meshes.add(Cuboid::new(wid, lower, len))),
            MeshMaterial3d(paint.clone()),
            Transform::from_xyz(0.0, 0.15 + lower / 2.0 - ride, 0.0),
        ));
        p.spawn((
            Mesh3d(meshes.add(Cuboid::new(0.85 * wid, hgt - 0.15 - lower, 0.5 * len))),
            MeshMaterial3d(glass),
            Transform::from_xyz(
                0.0,
                0.15 + lower + (hgt - 0.15 - lower) / 2.0 - ride,
                0.08 * len,
            ),
        ));
        for index in 0..4 {
            let r = racer_dynamics::spec::tire_radius(if index < 2 {
                &spec.tires.front
            } else {
                &spec.tires.rear
            })
            .unwrap_or(0.33) as f32;
            p.spawn((
                WheelVisual { index, spin: 0.0 },
                Mesh3d(meshes.add(Cylinder::new(r, 0.24))),
                MeshMaterial3d(tire.clone()),
                Transform::default(),
            ));
        }
    });
    car
}

fn select(keys: Res<ButtonInput<KeyCode>>, mut selection: ResMut<Selection>) {
    for (i, key) in [KeyCode::Digit1, KeyCode::Digit2, KeyCode::Digit3]
        .into_iter()
        .enumerate()
    {
        if keys.just_pressed(key) {
            selection.car = i;
        }
    }
    if keys.just_pressed(KeyCode::KeyT) {
        selection.location = Location::Track;
    }
    if keys.just_pressed(KeyCode::KeyK) {
        selection.location = Location::Skidpad;
    }
    // Reset: re-trigger change detection to respawn at the start.
    if keys.just_pressed(KeyCode::KeyR) {
        selection.set_changed();
    }
}

fn respawn_car(
    mut commands: Commands,
    selection: Res<Selection>,
    layout: Res<TrackLayout>,
    players: Query<Entity, With<Player>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !selection.is_changed() {
        return;
    }
    for e in &players {
        commands.entity(e).despawn();
    }
    let id = CARS[selection.car % CARS.len()];
    match load_car(id) {
        Ok(spec) => {
            let at = start_pose(selection.location, &layout.0);
            spawn_player_car(
                &mut commands,
                &mut meshes,
                &mut materials,
                &spec,
                PAINT[selection.car % 3],
                at,
            );
        }
        Err(e) => error!("cannot load {id}: {e}"),
    }
}

fn toggle_camera(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<CameraMode>) {
    if keys.just_pressed(KeyCode::KeyV) {
        *mode = match *mode {
            CameraMode::Chase => CameraMode::Hood,
            CameraMode::Hood => CameraMode::Chase,
        };
    }
}

fn animate_wheels(
    time: Res<Time>,
    cars: Query<(&Car, &CarTelemetry, &Children)>,
    mut wheels: Query<(&mut WheelVisual, &mut Transform)>,
) {
    for (car, telemetry, children) in &cars {
        for child in children.iter() {
            let Ok((mut wheel, mut transform)) = wheels.get_mut(child) else {
                continue;
            };
            let w = &telemetry.telemetry.wheels[wheel.index];
            wheel.spin = (wheel.spin - w.omega as f32 * time.delta_secs()) % std::f32::consts::TAU;
            let g = car.vehicle.wheel_geometry(wheel.index);
            *transform = Transform::from_translation(wheel_center(&g, w.travel)).with_rotation(
                Quat::from_rotation_y(w.steer as f32)
                    * Quat::from_rotation_x(wheel.spin)
                    * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
            );
        }
    }
}

/// Chase camera: trails the car with smoothing and widens the field of view
/// with speed. Hood camera: fixed to the car.
fn follow_camera(
    time: Res<Time>,
    mode: Res<CameraMode>,
    cars: Query<(&Transform, &CarTelemetry), (With<Player>, Without<SceneCamera>)>,
    mut cameras: Query<(&mut Transform, &mut Projection, &mut ChaseCamera), With<SceneCamera>>,
) {
    let Some((car, telemetry)) = cars.iter().next() else {
        return;
    };
    let speed = telemetry.telemetry.speed as f32;
    let dt = time.delta_secs();
    for (mut transform, mut projection, mut chase) in &mut cameras {
        let fov = match *mode {
            CameraMode::Chase => {
                let forward = car.forward().as_vec3();
                let heading = Vec3::new(forward.x, 0.0, forward.z).normalize_or(Vec3::NEG_Z);
                chase.heading = chase
                    .heading
                    .lerp(heading, 1.0 - (-4.0 * dt).exp())
                    .normalize_or(heading);
                let target = car.translation - chase.heading * 6.5 + Vec3::Y * 2.2;
                let position = chase
                    .position
                    .map_or(target, |p| p.lerp(target, 1.0 - (-10.0 * dt).exp()));
                chase.position = Some(position);
                *transform = Transform::from_translation(position).looking_at(
                    car.translation + chase.heading * 2.0 + Vec3::Y * 0.6,
                    Vec3::Y,
                );
                60.0 + 25.0 * (speed.abs() / 80.0).min(1.0)
            }
            CameraMode::Hood => {
                chase.position = None;
                *transform = *car * Transform::from_xyz(0.0, 0.62, -0.4);
                75.0
            }
        };
        if let Projection::Perspective(p) = projection.as_mut() {
            p.fov = fov.to_radians();
        }
    }
}

fn update_hud(
    selection: Res<Selection>,
    cars: Query<(&Car, &CarTelemetry, &Transform), With<Player>>,
    mut hud: Query<&mut Text, With<Hud>>,
) {
    let (Some((car, t, transform)), Ok(mut text)) = (cars.iter().next(), hud.single_mut()) else {
        return;
    };
    let tel = &t.telemetry;
    let gear = match tel.gear {
        g if g < 0 => "R".to_string(),
        0 => "N".to_string(),
        g => g.to_string(),
    };
    let w = &tel.wheels;
    let deg = |a: f64| a.to_degrees();
    let flag = |on: bool, name: &str| {
        if on {
            name.to_string()
        } else {
            "-".repeat(name.len())
        }
    };
    text.0 = format!(
        "{}  [{:?}]\n{:5.0} km/h  gear {}  {:5.0} rpm\n\
         slip angle  F {:+5.1}/{:+5.1}  R {:+5.1}/{:+5.1} deg\n\
         slip ratio  F {:+.2}/{:+.2}  R {:+.2}/{:+.2}\n\
         lat {:+.2} g  long {:+.2} g   {} {} {}\n\
         WASD/arrows drive, Space handbrake, Q/E shift, V camera\n1/2/3 car, T track, K skidpad, R reset",
        car.vehicle.name,
        selection.location,
        tel.speed * 3.6,
        gear,
        tel.rpm,
        deg(w[0].slip_angle),
        deg(w[1].slip_angle),
        deg(w[2].slip_angle),
        deg(w[3].slip_angle),
        w[0].slip_ratio,
        w[1].slip_ratio,
        w[2].slip_ratio,
        w[3].slip_ratio,
        t.acceleration.dot(*transform.right()) / 9.81,
        t.acceleration.dot(*transform.forward()) / 9.81,
        flag(tel.abs_active, "ABS"),
        flag(tel.tcs_active, "TCS"),
        flag(tel.esc_active, "ESC"),
    );
}
