//! The race loop end to end on the city track, headless: grid hold and
//! start, AI drivers lapping, resets with ghosting, finish order and DNF.

use std::sync::Arc;

use avian3d::prelude::{CollisionLayers, LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use racer_dynamics::load_car;
use racer_sim::{
    ai::Difficulty,
    race::{
        Collisions, DriverKind, Race, RaceConfig, RaceMode, RacePhase, RacePlugin, RaceTrack,
        Racer, spawn_racer,
    },
    rules::{ResetReason, Status},
    track::{Layer, city_track, spawn_track},
    vehicle::CarBody,
};

const GTI: &str = "vw-golf-gti-mk8";

fn race_app(config: RaceConfig, drivers: &[DriverKind]) -> (App, Vec<Entity>) {
    let track = Arc::new(racer_trackc::load("downtown-loop").unwrap());
    let mut app = racer_sim::headless_app();
    app.add_plugins(RacePlugin)
        .insert_resource(Race::new(&config))
        .insert_resource(config)
        .insert_resource(RaceTrack(track.clone()));
    racer_sim::finish_headless(&mut app);
    let world = app.world_mut();
    let mut commands = world.commands();
    spawn_track(&mut commands, &city_track(&track.geometry));
    let spec = load_car(GTI).unwrap();
    let cars = drivers
        .iter()
        .enumerate()
        .map(|(slot, d)| {
            spawn_racer(
                &mut commands,
                &track,
                &spec,
                slot,
                format!("car {slot}"),
                *d,
                (),
            )
            .unwrap()
        })
        .collect();
    world.flush();
    (app, cars)
}

fn run_for(app: &mut App, seconds: f32) {
    for _ in 0..(seconds * 240.0) as usize {
        app.update();
    }
}

fn racer(app: &App, car: Entity) -> &Racer {
    app.world().get::<Racer>(car).unwrap()
}

#[test]
fn cars_wait_for_the_start_then_race_and_finish_in_order() {
    let config = RaceConfig {
        laps: 1,
        collisions: Collisions::Off,
        dnf_timeout: 20.0,
        ..default()
    };
    let (mut app, cars) = race_app(
        config,
        &[
            DriverKind::Ai(Difficulty::Alien),
            DriverKind::Ai(Difficulty::Novice),
        ],
    );
    run_for(&mut app, 2.5);
    assert_eq!(app.world().resource::<Race>().phase, RacePhase::Countdown);
    for &car in &cars {
        let v = app.world().get::<LinearVelocity>(car).unwrap().0.length();
        assert!(v < 0.2, "moving on the grid: {v}");
    }
    run_for(&mut app, 1.0);
    assert_eq!(app.world().resource::<Race>().phase, RacePhase::Running);
    // One lap is about 90 s for an Alien GTI; the Novice is ~20 % slower.
    run_for(&mut app, 120.0);
    let (alien, novice) = (racer(&app, cars[0]), racer(&app, cars[1]));
    let Status::Finished(t) = alien.status else {
        panic!("Alien did not finish: {:?}", alien.status)
    };
    assert!((80.0..100.0).contains(&t), "Alien lap {t}");
    assert_eq!(alien.position, 1);
    assert!(alien.resets.is_empty() && novice.resets.is_empty());
    assert_eq!(alien.timing.laps.len(), 1);
    assert_eq!(alien.timing.last_sectors.len(), 3);
    // The Novice is either home or out after the timeout.
    run_for(&mut app, 10.0);
    let novice = racer(&app, cars[1]);
    assert!(
        matches!(novice.status, Status::Finished(n) if n > t) || novice.status == Status::Dnf,
        "{:?}",
        novice.status
    );
    assert_eq!(novice.position, 2);
}

#[test]
fn a_car_off_the_track_is_reset_ghosted_onto_the_road() {
    let config = RaceConfig {
        countdown: 0.5,
        ..default()
    };
    let (mut app, cars) = race_app(
        config,
        &[
            DriverKind::Ai(Difficulty::Pro),
            DriverKind::Ai(Difficulty::Pro),
        ],
    );
    run_for(&mut app, 8.0);
    let before = racer(&app, cars[0]).progress.distance;
    assert!(before > 50.0, "drove off: {before}");
    // Drop the car outside the walls.
    {
        let world = app.world_mut();
        let p = world.get::<Position>(cars[0]).unwrap().0;
        world.get_mut::<Position>(cars[0]).unwrap().0 = p + Vec3::new(40.0, 0.5, 0.0);
    }
    run_for(&mut app, 1.0);
    assert!(!racer(&app, cars[0]).progress.on_track);
    run_for(&mut app, 2.5);
    let r = racer(&app, cars[0]);
    assert_eq!(r.resets.len(), 1, "{:?}", r.resets);
    assert_eq!(r.resets[0].reason, ResetReason::OffTrack);
    assert!(r.progress.on_track && r.ghosted);
    assert!(
        (r.progress.distance - before).abs() < 40.0,
        "{} vs {before}",
        r.progress.distance
    );
    let body = app
        .world()
        .get::<Children>(cars[0])
        .unwrap()
        .iter()
        .find(|c| app.world().get::<CarBody>(*c).is_some())
        .unwrap();
    let layers = app.world().get::<CollisionLayers>(body).unwrap();
    assert!(
        !layers.filters.has_all(Layer::Car),
        "ghosted cars pass through others"
    );
    // Drives on and becomes solid again.
    run_for(&mut app, 6.0);
    let r = racer(&app, cars[0]);
    assert!(
        !r.ghosted && r.progress.distance > before,
        "{} {}",
        r.ghosted,
        r.progress.distance
    );
    assert_eq!(r.resets.len(), 1);
}

#[test]
fn a_sprint_finishes_at_the_finish_point_between_gates() {
    let config = RaceConfig {
        mode: RaceMode::Sprint,
        countdown: 0.1,
        ..default()
    };
    let (mut app, cars) = race_app(config, &[DriverKind::Human]);
    run_for(&mut app, 0.5);
    // Put the car 10 m before the finish with every gate up to there
    // credited, rolling towards it.
    let g = app.world().resource::<RaceTrack>().0.geometry.clone();
    let d = g.sprint_finish - 10.0;
    let s = racer_trackc::geometry::interpolate(&g.samples, g.length, d);
    {
        let world = app.world_mut();
        world.get_mut::<Position>(cars[0]).unwrap().0 = s.at(0.0, 0.6);
        world.get_mut::<Rotation>(cars[0]).unwrap().0 = s.rotation();
        world.get_mut::<LinearVelocity>(cars[0]).unwrap().0 = s.tangent * 20.0;
        let mut r = world.get_mut::<Racer>(cars[0]).unwrap();
        r.progress.distance = d;
        r.progress.sample = g.samples.iter().position(|x| x.s >= d).unwrap();
        r.progress.next_gate = g.checkpoints.iter().filter(|&&c| c < d).count();
    }
    run_for(&mut app, 0.1);
    assert_eq!(racer(&app, cars[0]).status, Status::Racing);
    run_for(&mut app, 1.5);
    let r = racer(&app, cars[0]);
    assert!(
        matches!(r.status, Status::Finished(_)),
        "{:?} at {}",
        r.status,
        r.progress.distance
    );
    assert_eq!(app.world().resource::<Race>().phase, RacePhase::Finished);
}
