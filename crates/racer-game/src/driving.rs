//! Greybox driving scene: the test track and skidpad, a drivable car with
//! car selection, chase and hood cameras and a debug HUD.

use bevy::{light::atmosphere::ScatteringMedium, prelude::*};
use racer_dynamics::{CarSpec, load_car, spec::CARS};
use racer_sim::{
    track::{Track, skidpad_start, spawn_track, test_track},
    vehicle::{Car, CarTelemetry, spawn_car},
};

use crate::{
    greybox::{SceneCamera, spawn_environment},
    input::Player,
    visuals::{CarPaint, PAINT, VisualsPlugin},
};

/// Spawns the driving scene. `car` indexes [`CARS`].
pub struct DrivingPlugin {
    pub car: usize,
    pub location: Location,
}

impl Plugin for DrivingPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<VisualsPlugin>() {
            app.add_plugins(VisualsPlugin);
        }
        if !app.is_plugin_added::<CameraPlugin>() {
            app.add_plugins(CameraPlugin);
        }
        app.insert_resource(Selection {
            car: self.car,
            location: self.location,
        })
        .insert_resource(TrackLayout(test_track()))
        .add_systems(Startup, setup)
        .add_systems(Update, (select, respawn_car, update_hud));
    }
}

/// Chase and hood cameras following the [`Player`] car (V toggles).
pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CameraMode>()
            .add_systems(Update, toggle_camera)
            .add_systems(
                PostUpdate,
                follow_camera.before(TransformSystems::Propagate),
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
    /// Camera left alone (scripted captures).
    Fixed,
}

/// Smoothed chase-camera state.
#[derive(Component, Default)]
pub struct ChaseCamera {
    position: Option<Vec3>,
    heading: Vec3,
}

#[derive(Component)]
struct Hud;

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

fn start_pose(location: Location, layout: &Track) -> Transform {
    match location {
        Location::Track => layout.start,
        Location::Skidpad => skidpad_start(),
    }
}

/// Spawns the player's car with a greybox body and four wheels.
pub fn spawn_player_car(
    commands: &mut Commands,
    spec: &CarSpec,
    paint: Color,
    at: Transform,
) -> Entity {
    spawn_car(commands, spec, at, (Player, CarPaint(paint)))
        .expect("car.toml is validated by tests")
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
            spawn_player_car(&mut commands, &spec, PAINT[selection.car % 3], at);
        }
        Err(e) => error!("cannot load {id}: {e}"),
    }
}

fn toggle_camera(keys: Res<ButtonInput<KeyCode>>, mut mode: ResMut<CameraMode>) {
    if keys.just_pressed(KeyCode::KeyV) {
        *mode = match *mode {
            CameraMode::Chase => CameraMode::Hood,
            CameraMode::Hood => CameraMode::Chase,
            CameraMode::Fixed => CameraMode::Fixed,
        };
    }
}

/// The player's car, disjoint from the camera whose transform is written.
type PlayerNotCamera = (With<Player>, Without<SceneCamera>);

/// Chase camera: trails the car with smoothing and widens the field of view
/// with speed. Hood camera: fixed to the car.
fn follow_camera(
    time: Res<Time>,
    mode: Res<CameraMode>,
    cars: Query<(&Transform, &CarTelemetry), PlayerNotCamera>,
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
                *transform = *car * hood_camera();
                75.0
            }
            CameraMode::Fixed => return,
        };
        if let Projection::Perspective(p) = projection.as_mut() {
            p.fov = fov.to_radians();
        }
    }
}

/// Hood camera in the car's frame.
pub fn hood_camera() -> Transform {
    Transform::from_xyz(0.0, 0.62, -0.4)
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
