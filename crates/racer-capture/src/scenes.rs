//! Scenes the harness can capture and their scripted camera poses.

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy};
use racer_game::{
    GamePlugin,
    driving::{CameraMode, DrivingPlugin, Location, hood_camera},
    flow::{AppState, OpponentCars, RaceSetup},
    greybox::{GreyboxPlugin, default_camera_transform},
};
use racer_sim::{
    race::{Race, RacePhase, RaceTrack, Racer},
    track::{SKIDPAD_CENTER, test_track},
    vehicle::VehiclePlugin,
};

use crate::harness::{Pose, PoseCamera};

/// Names accepted by [`add_scene`].
pub const SCENES: &[&str] = &["greybox", "driving", "race", "menu"];

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
        "race" => {
            // An 8-car, one-lap race with the player's car on autopilot.
            let setup = RaceSetup {
                opponents: 7,
                difficulty: None,
                opponent_cars: OpponentCars::Mixed,
                laps: 1,
                autopilot: true,
                racing_line: true,
                ..default()
            };
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                FAST_FORWARD,
            )))
            .add_plugins(GamePlugin)
            .insert_resource(setup)
            .insert_state(AppState::Loading)
            .add_systems(Startup, |mut t: ResMut<Time<Virtual>>| {
                t.set_max_delta(Duration::from_secs(2))
            });
            let grid = |world: &World| {
                let track = &world.resource::<RaceTrack>().0.geometry;
                let front = track.grid[0];
                let ahead = front.rotation * Vec3::NEG_Z;
                Transform::from_translation(front.position + ahead * 16.0 + Vec3::Y * 22.0)
                    .looking_at(front.position - ahead * 26.0, Vec3::Y)
            };
            Some(vec![
                Pose {
                    name: "grid".into(),
                    camera: PoseCamera::Dynamic(grid),
                    prepare: None,
                    ready: Some(|w| race_time(w) > -2.5),
                },
                Pose {
                    name: "pack".into(),
                    camera: PoseCamera::Dynamic(pack_camera),
                    prepare: None,
                    ready: Some(|w| race_time(w) > 5.0),
                },
                Pose {
                    name: "hud".into(),
                    camera: PoseCamera::Game,
                    prepare: None,
                    ready: Some(|w| race_time(w) > 32.0),
                },
                Pose {
                    name: "results".into(),
                    camera: PoseCamera::Game,
                    prepare: None,
                    ready: Some(|w| {
                        in_state(w, AppState::Results)
                            && w.get_resource::<Race>()
                                .is_some_and(|r| r.phase == RacePhase::Finished)
                    }),
                },
            ])
        }
        "menu" => {
            app.add_plugins(GamePlugin);
            Some(vec![
                Pose {
                    name: "main-menu".into(),
                    camera: PoseCamera::Game,
                    prepare: None,
                    ready: None,
                },
                Pose {
                    name: "setup".into(),
                    camera: PoseCamera::Game,
                    prepare: Some(|w| w.resource_mut::<NextState<AppState>>().set(AppState::Setup)),
                    ready: Some(|w| in_state(w, AppState::Setup)),
                },
            ])
        }
        _ => None,
    }
}

fn pose(name: &str, transform: Transform) -> Pose {
    Pose::fixed(name, transform)
}

/// Seconds of race time each rendered frame advances (the fixed 240 Hz
/// steps catch up inside the frame), so a race reaches its later poses in a
/// few hundred frames: on lavapipe a frame takes seconds.
const FAST_FORWARD: f32 = 1.0;

fn race_time(world: &World) -> f32 {
    world.get_resource::<Race>().map_or(f32::MIN, |r| r.time)
}

fn in_state(world: &World, state: AppState) -> bool {
    world
        .get_resource::<State<AppState>>()
        .is_some_and(|s| *s.get() == state)
}

/// A high camera behind the fourth car in the standings, looking down the
/// road at the pack ahead of it.
fn pack_camera(world: &World) -> Transform {
    let mut cars: Vec<(usize, Vec3, Vec3)> = Vec::new();
    for entity in world.iter_entities() {
        if let (Some(r), Some(t)) = (entity.get::<Racer>(), entity.get::<Transform>()) {
            cars.push((r.position, t.translation, *t.forward()));
        }
    }
    cars.sort_by_key(|c| c.0);
    let Some(&(_, at, forward)) = cars.get(3).or(cars.first()) else {
        return Transform::default();
    };
    let flat = forward.with_y(0.0).normalize_or(Vec3::NEG_Z);
    Transform::from_translation(at - flat * 22.0 + Vec3::Y * 14.0)
        .looking_at(at + flat * 25.0, Vec3::Y)
}
