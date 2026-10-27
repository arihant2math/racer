//! The race scene: the city track, the grid of cars from the race setup,
//! the chase camera, the racing-line assist, reset key and the hand-over to
//! the results screen. A slow orbit of the track sits behind the menus.

use std::sync::Arc;

use bevy::{light::atmosphere::ScatteringMedium, prelude::*};
use racer_dynamics::{Assists, load_car, spec::CARS};
use racer_sim::{
    ai::{AiDriver, Difficulty},
    race::{
        DriverKind, Race, RaceConfig, RaceMode, RacePhase, RaceTrack, Racer, ResetRequest,
        spawn_racer,
    },
    rules::Status,
    track::{city_track, spawn_track},
};
use racer_trackc::Track;

use crate::{
    driving::{CameraPlugin, ChaseCamera},
    flow::{AppState, RaceSetup},
    greybox::{SceneCamera, spawn_environment},
    hud::HudPlugin,
    input::{DriverSettings, Player},
    visuals::{CarPaint, PAINT, VisualsPlugin, render_mesh},
};

pub struct RaceScenePlugin;

impl Plugin for RaceScenePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<VisualsPlugin>() {
            app.add_plugins(VisualsPlugin);
        }
        if !app.is_plugin_added::<CameraPlugin>() {
            app.add_plugins(CameraPlugin);
        }
        let track = racer_trackc::load("downtown-loop").expect("track data is checked by tests");
        app.add_plugins(HudPlugin)
            .insert_resource(RaceTrack(Arc::new(track)))
            .add_systems(Startup, setup_world)
            .add_systems(OnEnter(AppState::Race), start_race)
            .add_systems(
                Update,
                (
                    (reset_key, to_results).run_if(in_state(AppState::Race)),
                    orbit_camera
                        .run_if(in_state(AppState::MainMenu).or_else(in_state(AppState::Setup))),
                ),
            );
    }
}

/// Everything that belongs to one race and goes when it is cleared.
#[derive(Component)]
pub struct RaceEntity;

pub fn car_name(index: usize) -> String {
    load_car(CARS[index % CARS.len()]).map_or_else(|_| "?".into(), |s| s.name)
}

const NAMES: [&str; 8] = ["Ava", "Ben", "Chen", "Dana", "Eli", "Femi", "Gus", "Hana"];

fn setup_world(
    mut commands: Commands,
    mut media: ResMut<Assets<ScatteringMedium>>,
    track: Res<RaceTrack>,
) {
    let camera = spawn_environment(
        &mut commands,
        &mut media,
        Transform::from_xyz(0.0, 150.0, 300.0),
    );
    commands
        .entity(camera)
        .insert((ChaseCamera::default(), Projection::Perspective(default())));
    spawn_track(&mut commands, &city_track(&track.0.geometry));
}

type RaceOrRacer = Or<(With<RaceEntity>, With<Racer>)>;

/// Removes the last race's cars, HUD and resources.
pub fn clear_race(mut commands: Commands, entities: Query<Entity, RaceOrRacer>) {
    for e in &entities {
        commands.entity(e).despawn();
    }
    commands.remove_resource::<Race>();
    commands.remove_resource::<RaceConfig>();
}

/// The grid from the setup: AI cars ahead (fastest first), the player last.
pub fn grid(setup: &RaceSetup) -> Vec<(usize, DriverKind, String)> {
    let opponents = if setup.mode == RaceMode::TimeTrial {
        0
    } else {
        setup.opponents
    };
    let order = [
        Difficulty::Alien,
        Difficulty::Expert,
        Difficulty::Pro,
        Difficulty::Amateur,
        Difficulty::Novice,
    ];
    let mut grid: Vec<(usize, DriverKind, String)> = (0..opponents)
        .map(|i| {
            let d = setup.difficulty.unwrap_or(order[i % order.len()]);
            let car = match setup.opponent_cars {
                crate::flow::OpponentCars::Same => setup.car,
                crate::flow::OpponentCars::Mixed => (setup.car + i + 1) % CARS.len(),
            };
            (
                car,
                DriverKind::Ai(d),
                format!("{} ({})", NAMES[i], d.name()),
            )
        })
        .collect();
    let me = if setup.autopilot {
        DriverKind::Ai(Difficulty::Alien)
    } else {
        DriverKind::Human
    };
    grid.push((setup.car, me, "You".into()));
    grid
}

fn start_race(
    mut commands: Commands,
    setup: Res<RaceSetup>,
    track: Res<RaceTrack>,
    mut settings: ResMut<DriverSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let config = RaceConfig {
        mode: setup.mode,
        laps: if setup.mode == RaceMode::Sprint {
            1
        } else {
            setup.laps
        },
        collisions: setup.collisions,
        catch_up: setup.catch_up,
        ..default()
    };
    let assists = Assists {
        abs: setup.abs,
        tcs: setup.tcs,
        esc: setup.esc,
    };
    *settings = DriverSettings {
        assists,
        steering_assist: setup.steering_assist,
        automatic: setup.automatic,
    };
    commands.insert_resource(Race::new(&config));
    commands.insert_resource(config);
    let t = &track.0;
    for (slot, (car, driver, name)) in grid(&setup).into_iter().enumerate() {
        let Ok(spec) = load_car(CARS[car]) else {
            continue;
        };
        let paint = CarPaint(PAINT[slot % PAINT.len()]);
        let player = name == "You";
        let result = if player {
            spawn_racer(
                &mut commands,
                t,
                &spec,
                slot,
                name.clone(),
                driver,
                (paint, Player),
            )
        } else {
            spawn_racer(&mut commands, t, &spec, slot, name.clone(), driver, paint)
        };
        match result {
            Ok(e) if player => {
                // The human's assists show in the results; the AI's are the defaults.
                commands.queue(move |world: &mut World| {
                    if let Some(mut r) = world.get_mut::<Racer>(e) {
                        r.assists = assists;
                    }
                });
            }
            Ok(_) => {}
            Err(e) => error!("cannot spawn {name}: {e}"),
        }
    }
    if setup.racing_line {
        spawn_racing_line(
            &mut commands,
            t,
            CARS[setup.car],
            &mut meshes,
            &mut materials,
        );
    }
}

/// The racing-line assist: a strip on the road, green where the car
/// accelerates or holds speed, red where it should be braking.
fn spawn_racing_line(
    commands: &mut Commands,
    track: &Track,
    car: &str,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let Some(profile) = track.car(car) else {
        return;
    };
    let pts = &track.line.points;
    let n = pts.len();
    let mut strips = [(Vec::new(), Vec::new()), (Vec::new(), Vec::new())];
    for k in 0..n {
        let j = (k + 1) % n;
        let braking = profile.speeds[j] < profile.speeds[k] - 0.01;
        let (vertices, indices): &mut (Vec<Vec3>, Vec<u32>) = &mut strips[usize::from(braking)];
        let s = &track.geometry.samples;
        let edge = |i: usize, side: f32| pts[i].pos + s[i].right * side * 0.25 + s[i].up * 0.03;
        let base = vertices.len() as u32;
        vertices.extend([edge(k, -1.0), edge(k, 1.0), edge(j, -1.0), edge(j, 1.0)]);
        indices.extend([base, base + 1, base + 2, base + 1, base + 3, base + 2]);
    }
    for ((vertices, indices), color) in strips
        .into_iter()
        .zip([Color::srgb(0.1, 0.8, 0.2), Color::srgb(0.9, 0.1, 0.05)])
    {
        commands.spawn((
            RaceEntity,
            Mesh3d(meshes.add(render_mesh(vertices, indices))),
            MeshMaterial3d(materials.add(StandardMaterial {
                base_color: color,
                unlit: true,
                ..default()
            })),
        ));
    }
}

fn reset_key(
    mut commands: Commands,
    keys: Res<ButtonInput<KeyCode>>,
    players: Query<Entity, (With<Player>, Without<AiDriver>)>,
    race: Res<Race>,
) {
    if keys.just_pressed(KeyCode::KeyR) && race.phase == RacePhase::Running {
        for e in &players {
            commands.entity(e).insert(ResetRequest);
        }
    }
}

/// Results a few seconds after the player finishes (or when everyone has).
fn to_results(
    time: Res<Time>,
    race: Option<Res<Race>>,
    players: Query<&Racer, With<Player>>,
    mut next: ResMut<NextState<AppState>>,
    mut done_for: Local<f32>,
) {
    let player_done = players.iter().any(|r| r.status != Status::Racing);
    let all_done = race.is_some_and(|r| r.phase == RacePhase::Finished);
    *done_for = if player_done || all_done {
        *done_for + time.delta_secs()
    } else {
        0.0
    };
    if *done_for > 3.0 {
        *done_for = 0.0;
        next.set(AppState::Results);
    }
}

/// Slow orbit high over the track behind the menus.
fn orbit_camera(
    time: Res<Time>,
    track: Res<RaceTrack>,
    mut cameras: Query<&mut Transform, With<SceneCamera>>,
) {
    let (lo, hi) = track.0.geometry.bounds;
    let center = (lo + hi) / 2.0;
    let center = Vec3::new(center.x, 0.0, center.y);
    let a = time.elapsed_secs() * 0.04;
    for mut t in &mut cameras {
        let eye = center + Vec3::new(a.cos() * 420.0, 220.0, a.sin() * 420.0);
        *t = Transform::from_translation(eye).looking_at(center, Vec3::Y);
    }
}
