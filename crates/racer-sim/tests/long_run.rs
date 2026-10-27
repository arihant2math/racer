//! QA (adversarial edge cases): a full 8-car AI race left running for a long
//! simulated time. Entity counts must stay constant, every car's state
//! finite, the race clock exact against the tick count, and lap bookkeeping
//! consistent. See `docs/qa/adversarial-edge-cases.md`.
//!
//! Ignored by default (minutes of wall time); run it in release:
//! `QA_LONG_RUN_MINUTES=20 cargo test --release -p racer-sim --test long_run -- --ignored`
//! (default 10 simulated minutes).

use std::sync::Arc;

use avian3d::prelude::Position;
use bevy::prelude::*;
use racer_dynamics::load_car;
use racer_sim::{
    ai::Difficulty,
    race::{Collisions, DriverKind, Race, RaceConfig, RacePlugin, RaceTrack, Racer, spawn_racer},
    rules::Status,
    track::{city_track, spawn_track},
    vehicle::CarState,
};

#[test]
#[ignore = "long run: minutes of wall time"]
fn eight_car_race_runs_for_a_long_time_without_drift_or_leaks() {
    let minutes: u64 = std::env::var("QA_LONG_RUN_MINUTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(10);
    let track = Arc::new(racer_trackc::load("downtown-loop").unwrap());
    let config = RaceConfig {
        laps: 10_000,
        collisions: Collisions::Full,
        ..default()
    };
    let mut app = racer_sim::headless_app();
    app.add_plugins(RacePlugin)
        .insert_resource(Race::new(&config))
        .insert_resource(config)
        .insert_resource(RaceTrack(track.clone()));
    racer_sim::finish_headless(&mut app);
    let world = app.world_mut();
    let mut commands = world.commands();
    spawn_track(&mut commands, &city_track(&track.geometry));
    let cars: Vec<Entity> = (0..8)
        .map(|slot| {
            let id = racer_dynamics::spec::CARS[slot % 3];
            let d = Difficulty::ALL[slot % Difficulty::ALL.len()];
            spawn_racer(
                &mut commands,
                &track,
                &load_car(id).unwrap(),
                slot,
                format!("car {slot}"),
                DriverKind::Ai(d),
                (),
            )
            .unwrap()
        })
        .collect();
    world.flush();
    // Settle a second, then take the baseline entity count.
    let mut ticks = 0u64;
    for _ in 0..240 {
        app.update();
        ticks += 1;
    }
    let entities = app.world().entities().len();
    // The clock from here on must advance exactly one tick per update (the
    // very first update has no time delta, so compare against this baseline).
    let base = (f64::from(app.world().resource::<Race>().time), ticks);
    let started = std::time::Instant::now();
    for minute in 1..=minutes {
        for _ in 0..240 * 60 {
            app.update();
            ticks += 1;
        }
        let w = app.world();
        let race = w.resource::<Race>();
        let expected = base.0 + (ticks - base.1) as f64 * racer_sim::tick_duration().as_secs_f64();
        assert!(
            (f64::from(race.time) - expected).abs() < 2e-3,
            "minute {minute}: race clock {} vs {expected} from {ticks} ticks",
            race.time
        );
        assert_eq!(
            w.entities().len(),
            entities,
            "minute {minute}: entity count changed"
        );
        let mut laps = Vec::new();
        for &car in &cars {
            let r = w.get::<Racer>(car).unwrap();
            let p = w.get::<Position>(car).unwrap().0;
            let s = &w.get::<CarState>(car).unwrap().0;
            assert!(p.is_finite(), "minute {minute}: {} at {p}", r.name);
            assert!(
                s.wheels.iter().all(|w| w.omega.is_finite()) && s.drive.engine_omega.is_finite(),
                "minute {minute}: {} state {s:?}",
                r.name
            );
            assert!(r.progress.distance.is_finite());
            if r.status == Status::Racing {
                assert_eq!(
                    r.timing.laps.len() as u32,
                    r.progress.laps,
                    "minute {minute}: {} timing vs progress",
                    r.name
                );
            }
            assert!(
                r.timing.laps.iter().all(|&l| l > 20.0),
                "{:?}",
                r.timing.laps
            );
            let total: f32 = r.timing.laps.iter().sum();
            assert!(
                total <= race.time + 1e-2,
                "minute {minute}: {} laps sum to {total} > race time {}",
                r.name,
                race.time
            );
            laps.push((r.progress.laps, r.resets.len()));
        }
        println!(
            "minute {minute}: laps/resets {laps:?}, {:.0} s wall",
            started.elapsed().as_secs_f32()
        );
    }
}
