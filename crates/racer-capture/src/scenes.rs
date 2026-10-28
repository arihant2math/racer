//! Scenes the harness can capture and their scripted camera poses.

use std::time::Duration;

use bevy::{prelude::*, time::TimeUpdateStrategy};
use racer_game::{
    GamePlugin,
    car_model::ModelWheel,
    driving::{CameraMode, DrivingPlugin, Location, hood_camera},
    flow::{AppState, OpponentCars, RaceSetup},
    greybox::{GreyboxPlugin, default_camera_transform},
    online::{EntryPurpose, LobbyView, MemberView, Online, SettingsView, TextEntry},
    settings::{Settings, SpeedUnit},
};
use racer_sim::{
    race::{Race, RacePhase, RaceTrack, Racer},
    track::{SKIDPAD_CENTER, test_track},
    vehicle::VehiclePlugin,
};

use crate::harness::{Pose, PoseCamera};

/// Names accepted by [`add_scene`].
pub const SCENES: &[&str] = &[
    "greybox", "driving", "race", "menu", "showroom", "lobby", "online", "units",
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
                    ready: Some(|w| race_time(w) > 14.0),
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
        "showroom" => {
            // A pipeline car model at rest on the start line, close up.
            // RACER_SHOWROOM_CAR picks the car index (default 2, the 911).
            let car_index = std::env::var("RACER_SHOWROOM_CAR")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(2);
            app.insert_resource(CameraMode::Fixed).add_plugins((
                VehiclePlugin,
                DrivingPlugin {
                    car: car_index,
                    location: Location::Track,
                },
            ));
            let start = test_track().start.translation;
            let car = start + Vec3::Y * 0.45;
            let look =
                |from: Vec3, at: Vec3| Transform::from_translation(from).looking_at(at, Vec3::Y);
            let loaded = |w: &World| w.iter_entities().any(|e| e.contains::<ModelWheel>());
            let shot = |name: &str, t: Transform| Pose {
                name: name.into(),
                camera: PoseCamera::Fixed(t),
                prepare: None,
                ready: Some(loaded),
            };
            Some(vec![
                shot(
                    "front-three-quarter",
                    look(car + Vec3::new(-4.0, 0.6, -4.6), car),
                ),
                shot(
                    "rear-three-quarter",
                    look(car + Vec3::new(4.2, 1.0, 4.6), car),
                ),
                shot("side", look(car + Vec3::new(-7.5, 0.4, 0.0), car)),
                shot(
                    "chase",
                    look(
                        car + Vec3::new(0.0, 1.7, 6.5),
                        car + Vec3::new(0.0, 0.4, -2.0),
                    ),
                ),
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
                Pose {
                    name: "options".into(),
                    camera: PoseCamera::Game,
                    prepare: Some(|w| {
                        w.resource_mut::<NextState<AppState>>()
                            .set(AppState::Options)
                    }),
                    ready: Some(|w| in_state(w, AppState::Options)),
                },
            ])
        }
        "units" => {
            // The HUD with speeds in mph: a solo race, the player's car on
            // autopilot (manual check, no goldens).
            let setup = RaceSetup {
                opponents: 0,
                laps: 1,
                autopilot: true,
                ..default()
            };
            let mut settings = Settings::default();
            settings.interface.speed_units = SpeedUnit::Mph;
            app.insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f32(
                FAST_FORWARD,
            )))
            .insert_resource(settings)
            .add_plugins(GamePlugin)
            .insert_resource(setup)
            .insert_state(AppState::Loading)
            .add_systems(Startup, |mut t: ResMut<Time<Virtual>>| {
                t.set_max_delta(Duration::from_secs(2))
            });
            Some(vec![Pose {
                name: "hud-mph".into(),
                camera: PoseCamera::Game,
                prepare: None,
                ready: Some(|w| race_time(w) > 12.0),
            }])
        }
        "lobby" => {
            app.add_plugins(GamePlugin);
            Some(vec![
                Pose {
                    name: "join".into(),
                    camera: PoseCamera::Game,
                    prepare: Some(|w| {
                        *w.resource_mut::<TextEntry>() = TextEntry {
                            purpose: EntryPurpose::JoinCode,
                            text: "K7QX2M".into(),
                        };
                        w.resource_mut::<NextState<AppState>>().set(AppState::Entry);
                    }),
                    ready: Some(|w| in_state(w, AppState::Entry)),
                },
                Pose {
                    name: "lobby-host".into(),
                    camera: PoseCamera::Game,
                    prepare: Some(|w| {
                        w.resource_mut::<Online>().show(sample_lobby());
                        w.resource_mut::<NextState<AppState>>().set(AppState::Lobby);
                    }),
                    ready: Some(|w| in_state(w, AppState::Lobby)),
                },
            ])
        }
        "online" => {
            // An online race against a running server (manual check, no
            // goldens): `racer-server --bind 127.0.0.1:7777`, or
            // RACER_CAPTURE_SERVER=host:port.
            let server =
                std::env::var("RACER_CAPTURE_SERVER").unwrap_or_else(|_| "127.0.0.1:7777".into());
            let mut online = Online::default();
            if let Err(e) = online.connect_direct(&server) {
                error!("{e}");
                return None;
            }
            app.add_plugins(GamePlugin)
                .insert_resource(online)
                .insert_resource(RaceSetup {
                    autopilot: true,
                    ..default()
                })
                .insert_state(AppState::Lobby);
            Some(vec![
                Pose {
                    name: "online-lobby".into(),
                    camera: PoseCamera::Game,
                    prepare: None,
                    ready: Some(|w| in_state(w, AppState::Lobby)),
                },
                Pose {
                    name: "online-race".into(),
                    camera: PoseCamera::Game,
                    prepare: None,
                    ready: Some(|w| in_state(w, AppState::Race) && race_time(w) > 8.0),
                },
            ])
        }
        _ => None,
    }
}

/// A lobby with three players, seen by its host.
fn sample_lobby() -> LobbyView {
    let member = |id, name: &str, ready, car: &str| MemberView {
        id,
        name: name.into(),
        ready,
        car: car.into(),
    };
    LobbyView {
        code: "K7QX2M".into(),
        host: 1,
        you: 1,
        members: vec![
            member(1, "Ana", false, "vw-golf-gti-mk8"),
            member(2, "Bo", true, "bmw-m3-competition-g80"),
            member(3, "Cy", false, "porsche-911-carrera-s-992"),
        ],
        settings: SettingsView {
            track: "downtown-loop".into(),
            mode: "circuit".into(),
            laps: 3,
            collisions: "full".into(),
            ai_fill: true,
        },
        state: "open".into(),
        quick: None,
        server: None,
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
