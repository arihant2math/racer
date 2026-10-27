//! QA scenarios for the AI drivers, see `docs/qa/ai-drivers.md`.
//!
//! Tests that reproduce an open bug are ignored with their QA-AI number so
//! the suite stays green; fixed ones run as regression tests (see
//! `docs/qa/fixes-ai.md`). Slower checks that pass are ignored as "QA
//! check". Run the ignored ones in release (a few seconds to a minute each):
//!
//! ```text
//! cargo test --release -p racer-sim --test qa_ai -- --ignored
//! ```
//!
//! Some assertions read private AI state through `AiDriver`'s `Debug`
//! output (see [`field`]), since the plan's state has no public getters.

use std::sync::Arc;

use avian3d::prelude::{AngularVelocity, CollidingEntities, LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use racer_dynamics::load_car;
use racer_sim::{
    ai::{AiDriver, Difficulty},
    race::{DriverKind, Race, RaceConfig, RacePlugin, RaceTrack, Racer, spawn_racer},
    rules::Status,
    track::{PieceKind, TrackPiece, city_track, spawn_track},
    vehicle::{CarBody, CarControls},
};
use racer_trackc::geometry::interpolate;

const GTI: &str = "vw-golf-gti-mk8";
const M3: &str = "bmw-m3-competition-g80";
const P911: &str = "porsche-911-carrera-s-992";

fn race_app(config: RaceConfig, entries: &[(&str, DriverKind)]) -> (App, Vec<Entity>) {
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
    let cars = entries
        .iter()
        .enumerate()
        .map(|(slot, (car, d))| {
            let spec = load_car(car).unwrap();
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

/// The 8-car field of `racer-capture ai-races` race `index` (same cars,
/// difficulties and grid order as `racer_capture::race::mixed`).
fn mixed_entries(index: usize) -> Vec<(&'static str, DriverKind)> {
    use racer_dynamics::spec::CARS;
    let mut entries: Vec<(&str, DriverKind)> = (0..8)
        .map(|i| {
            let d = Difficulty::ALL[(i + index) % Difficulty::ALL.len()];
            (CARS[(i + index / 2) % CARS.len()], DriverKind::Ai(d))
        })
        .collect();
    let mut seed = index as u64 * 0x9e37_79b9 + 1;
    for i in (1..entries.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        entries.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    entries
}

fn ai(d: Difficulty) -> DriverKind {
    DriverKind::Ai(d)
}

fn racer(app: &App, car: Entity) -> &Racer {
    app.world().get::<Racer>(car).unwrap()
}

fn speed(app: &App, car: Entity) -> f32 {
    app.world().get::<LinearVelocity>(car).unwrap().0.length()
}

fn time(app: &App) -> f32 {
    app.world().resource::<Race>().time
}

/// A private field of the car's `AiDriver`, as its `Debug` output prints it.
fn field(app: &App, car: Entity, name: &str) -> String {
    let s = format!("{:?}", app.world().get::<AiDriver>(car).unwrap());
    let i = s.find(&format!(" {name}: ")).unwrap() + name.len() + 3;
    let rest = &s[i..];
    let end = rest
        .find(", ")
        .unwrap_or(rest.len())
        .min(rest.find(" }").unwrap_or(rest.len()));
    rest[..end].to_string()
}

/// Puts `car` at centreline distance `d` (in the current lap), `lateral`
/// metres right of the centre, turned `yaw` radians to the right of the
/// track direction, moving at `speed` along its own nose. Gates up to `d`
/// are credited.
fn place(app: &mut App, car: Entity, d: f32, lateral: f32, yaw: f32, speed: f32) {
    let g = app.world().resource::<RaceTrack>().0.geometry.clone();
    let s = interpolate(&g.samples, g.length, d);
    let rotation = Quat::from_axis_angle(s.up, -yaw) * s.rotation();
    let position = s.at(lateral, 0.6);
    let world = app.world_mut();
    world.get_mut::<Position>(car).unwrap().0 = position;
    world.get_mut::<Rotation>(car).unwrap().0 = rotation;
    world.get_mut::<LinearVelocity>(car).unwrap().0 = rotation * Vec3::NEG_Z * speed;
    world.get_mut::<AngularVelocity>(car).unwrap().0 = Vec3::ZERO;
    *world.get_mut::<Transform>(car).unwrap() =
        Transform::from_translation(position).with_rotation(rotation);
    let mut r = world.get_mut::<Racer>(car).unwrap();
    r.progress.distance = d;
    r.progress.sample = g.samples.iter().position(|x| x.s >= d).unwrap_or(0);
    r.progress.next_gate = g.checkpoints.iter().filter(|&&c| c < d).count();
    r.monitor.last_valid = (d, lateral.clamp(-3.0, 3.0));
    if let Some(mut ai) = world.get_mut::<AiDriver>(car) {
        ai.reset();
    }
}

/// Keeps a human car parked: brakes on, never reset by the stuck monitor
/// (a player who stopped and waits).
fn park(app: &mut App, car: Entity) {
    let world = app.world_mut();
    world.get_mut::<CarControls>(car).unwrap().0.brake = 1.0;
    world.get_mut::<Racer>(car).unwrap().monitor.clear();
}

/// Keeps a human far ahead in the race (catch-up pushes every AI to its
/// +3 %), parked out of the way below the world.
fn far_ahead_human(app: &mut App, car: Entity) {
    let world = app.world_mut();
    world.get_mut::<Position>(car).unwrap().0 = Vec3::new(0.0, -200.0, 0.0);
    world.get_mut::<LinearVelocity>(car).unwrap().0 = Vec3::ZERO;
    let mut r = world.get_mut::<Racer>(car).unwrap();
    r.monitor.clear();
    r.progress.distance = 10_000.0;
}

fn run(app: &mut App, seconds: f32, mut each: impl FnMut(&mut App)) {
    for _ in 0..(seconds * 240.0) as usize {
        each(app);
        app.update();
    }
}

fn start(app: &mut App) {
    while time(app) < 0.0 {
        app.update();
    }
}

fn short() -> RaceConfig {
    RaceConfig {
        countdown: 0.2,
        ..default()
    }
}

/// The body collider of each car.
fn bodies(app: &App, cars: &[Entity]) -> Vec<Entity> {
    cars.iter()
        .map(|&c| {
            app.world()
                .get::<Children>(c)
                .unwrap()
                .iter()
                .find(|b| app.world().get::<CarBody>(*b).is_some())
                .unwrap()
        })
        .collect()
}

fn touches_wall(app: &App, body: Entity) -> bool {
    app.world().get::<CollidingEntities>(body).is_some_and(|c| {
        c.iter().any(|e| {
            app.world()
                .get::<TrackPiece>(*e)
                .is_some_and(|p| p.kind == PieceKind::Wall)
        })
    })
}

// ---------------------------------------------------------------- bugs --

/// QA-AI-1: a player stopped on the racing line of the 470 m main avenue
/// (13 m wide, nothing else around). The AI drives round it. (It used to
/// stop 6 m behind it, wait with the throttle closed and be reset as
/// `Stuck` 3 s later.)
#[test]
fn qa_ai_1_a_stopped_car_on_the_line_is_driven_round_without_a_reset() {
    let (mut app, cars) = race_app(
        short(),
        &[(GTI, DriverKind::Human), (GTI, ai(Difficulty::Pro))],
    );
    start(&mut app);
    place(&mut app, cars[0], 200.0, -5.24, 0.0, 0.0);
    run(&mut app, 25.0, |a| park(a, cars[0]));
    let r = racer(&app, cars[1]);
    assert!(
        r.resets.is_empty(),
        "AI reset behind a stopped car: {:?}",
        r.resets
    );
    assert!(r.progress.distance > 260.0, "{}", r.progress.distance);
}

/// QA-AI-2: first lap of `ai-races` race 0. The Alien M3 passes on the
/// inside of turn 1 under braking. Its path used to sweep to the inside
/// edge as the racing line crosses the road, and the car hit the inner wall
/// at 369.7 m at 16.5 m/s, was pinned there and reset as `Stuck` at 20.3 s.
/// No car may touch a wall in the first 17 s.
#[test]
fn qa_ai_2_an_inside_pass_at_turn_1_does_not_hit_the_wall() {
    no_wall_hits(0, 17.0, false);
}

/// QA-AI-2, second site: race 7, where the Alien GTI went into the inner
/// wall of the right-hander at ~946 m at 40.65 s, and an Alien hit the
/// turn-1 inner wall at 16.6 s, both while passing. Over the first 45 s no
/// car may touch a wall while passing or defending. (A brush on the plain
/// racing line, such as an Amateur's line noise at the 500 m left-hander,
/// is line tracking, not racecraft.)
#[test]
fn qa_ai_2_no_pass_or_defence_in_race_7_hits_the_wall() {
    no_wall_hits(7, 45.0, true);
}

fn no_wall_hits(race: usize, until: f32, racecraft_only: bool) {
    let entries = mixed_entries(race);
    let (mut app, cars) = race_app(RaceConfig::default(), &entries);
    let bodies = bodies(&app, &cars);
    while time(&app) < until {
        app.update();
        for (i, &body) in bodies.iter().enumerate() {
            let r = racer(&app, cars[i]);
            let passing = field(&app, cars[i], "overtaking") != "None"
                || field(&app, cars[i], "defending") != "0.0";
            assert!(
                !touches_wall(&app, body) || (racecraft_only && !passing),
                "car {i} ({:?} {}) hit the wall at {:.1} m, lateral {:.2}, {:.1} s, overtaking {}",
                entries[i].1,
                entries[i].0,
                r.progress.distance,
                r.progress.lateral,
                time(&app),
                field(&app, cars[i], "overtaking"),
            );
        }
    }
}

/// QA-AI-3: a car reset onto the racing line stands ghosted; an AI arrives
/// at 38 m/s. The ghost ends while the AI is ~30 m away (no car within
/// 5.5 m). The AI used to ignore ghosted cars, could not stop in time and
/// hit the standing car at ~33 m/s; now it plans round it.
#[test]
fn qa_ai_3_a_car_turning_solid_ahead_is_not_rammed() {
    let (mut app, cars) = race_app(
        short(),
        &[(GTI, DriverKind::Human), (M3, ai(Difficulty::Pro))],
    );
    start(&mut app);
    place(&mut app, cars[0], 250.0, -5.24, 0.0, 0.0);
    app.world_mut().get_mut::<Racer>(cars[0]).unwrap().ghost = 2.0;
    place(&mut app, cars[1], 150.0, -5.24, 0.0, 38.0);
    let mut hit = 0.0f32;
    run(&mut app, 5.0, |a| {
        park(a, cars[0]);
        hit = hit.max(speed(a, cars[0]));
    });
    assert!(hit < 3.0, "standing car knocked to {hit:.1} m/s");
}

/// QA-AI-4: race 7, first lap. The Alien GTI hits the inner wall at ~946 m
/// and the Expert GTI runs into it. Both are left pushing against the wall
/// and each other at 0-3 m/s; the speed keeps crossing 1 m/s, so neither
/// the rules' stuck timer (3 s below 1 m/s) nor the AI's own stuck check
/// (0.8 s below 0.8 m/s with the throttle open) runs out. The Alien makes
/// no 10 m of progress for 12.4 s and the Expert for ~11 s before the
/// AI's 12 s progress timeout or a `Stuck` reset ends it; two cars queued
/// behind are reset as well.
#[test]
#[ignore = "QA-AI-4"]
fn qa_ai_4_a_car_pushing_against_an_obstacle_is_reset_within_a_few_seconds() {
    let entries = mixed_entries(7);
    let (mut app, cars) = race_app(RaceConfig::default(), &entries);
    // Per car: distance and time of the last 10 m of progress, resets seen.
    let mut last: Vec<(f32, f32, usize)> = vec![(f32::MIN, 0.0, 0); cars.len()];
    let mut worst = (0.0f32, 0usize, 0.0f32);
    while time(&app) < 56.0 {
        app.update();
        let now = time(&app);
        if now < 0.0 {
            continue;
        }
        for (i, &c) in cars.iter().enumerate() {
            let r = racer(&app, c);
            if r.progress.distance > last[i].0 + 10.0 || r.resets.len() != last[i].2 {
                last[i] = (r.progress.distance, now, r.resets.len());
            }
            if now - last[i].1 > worst.0 {
                worst = (now - last[i].1, i, r.progress.distance);
            }
        }
    }
    assert!(
        worst.0 < 8.0,
        "car {} ({:?}) made no 10 m of progress for {:.1} s at {:.0} m without a reset",
        worst.1,
        entries[worst.1].1,
        worst.0,
        worst.2
    );
}

/// QA-AI-5: a finished car on its cool-down lap does not defend (it used to
/// move 3 m across to the inside with a racing Alien closing before turn 1).
#[test]
fn qa_ai_5_a_finished_car_does_not_defend_against_racing_cars() {
    let (mut app, cars) = race_app(
        short(),
        &[(GTI, ai(Difficulty::Expert)), (P911, ai(Difficulty::Alien))],
    );
    start(&mut app);
    place(&mut app, cars[0], 290.0, -5.2, 0.0, 25.0);
    place(&mut app, cars[1], 280.0, -5.2, 0.0, 33.0);
    app.world_mut().get_mut::<Racer>(cars[0]).unwrap().status = Status::Finished(1.0);
    let mut defended = false;
    run(&mut app, 2.0, |a| {
        defended |= field(a, cars[0], "defending") != "0.0";
    });
    assert!(!defended, "cool-down car defended its line");
}

/// QA-AI-6: a car 0.8 m behind the leader's bumper is not "alongside". It
/// used to be: with another car beside the leader, the leader was boxed in
/// by the car *behind* it ("squeezed") and dropped its target speed by 8 %
/// in front of the follower.
#[test]
fn qa_ai_6_a_car_close_behind_does_not_squeeze_the_leader() {
    let (mut app, cars) = race_app(
        short(),
        &[
            (GTI, ai(Difficulty::Pro)),
            (GTI, ai(Difficulty::Pro)),
            (GTI, ai(Difficulty::Pro)),
        ],
    );
    start(&mut app);
    run(&mut app, 6.0, |_| {});
    place(&mut app, cars[0], 150.0, -4.6, 0.0, 30.0);
    place(&mut app, cars[1], 144.4, -4.4, 0.0, 30.0);
    place(&mut app, cars[2], 150.0, -0.9, 0.0, 30.0);
    run(&mut app, 0.05, |_| {});
    assert_eq!(
        field(&app, cars[0], "squeezed"),
        "false",
        "leader squeezed by the car behind it: clear {}",
        field(&app, cars[0], "clear")
    );
}

/// QA-AI-7: a car reversing out of trouble on the racing line, towards an
/// AI following it at low speed. Read as "moving forward at 3 m/s" it was
/// planned as pulling away (and not as an obstacle), and the AI ran into
/// it. Read along the track it is closing: the AI stops short, backs off
/// and drives round it once it has stopped.
#[test]
fn qa_ai_7_a_car_reversing_towards_the_ai_is_avoided() {
    let (mut app, cars) = race_app(
        short(),
        &[(GTI, DriverKind::Human), (GTI, ai(Difficulty::Pro))],
    );
    start(&mut app);
    place(&mut app, cars[0], 200.0, -5.24, 0.0, 0.0);
    place(&mut app, cars[1], 182.0, -5.24, 0.0, 8.0);
    let bodies = bodies(&app, &cars);
    let t0 = time(&app);
    let mut touched = false;
    run(&mut app, 12.0, |a| {
        // Backs away at 3 m/s for 1.6 s (as the AI's own reverse manoeuvre
        // does), then stops.
        if time(a) < t0 + 1.6 {
            let world = a.world_mut();
            let rotation = world.get::<Rotation>(cars[0]).unwrap().0;
            world.get_mut::<LinearVelocity>(cars[0]).unwrap().0 = rotation * Vec3::Z * 3.0;
        } else {
            park(a, cars[0]);
        }
        a.world_mut()
            .get_mut::<Racer>(cars[0])
            .unwrap()
            .monitor
            .clear();
        touched |= a
            .world()
            .get::<CollidingEntities>(bodies[1])
            .is_some_and(|c| c.contains(&bodies[0]));
    });
    assert!(!touched, "AI ran into the reversing car");
    assert!(
        racer(&app, cars[1]).progress.distance > 205.0,
        "AI did not get past"
    );
}

// --------------------------------------------------- checked and fine --

/// Same field, same seed: bit-identical positions after 40 s.
#[test]
#[ignore = "QA check"]
fn check_races_are_deterministic() {
    let entries = mixed_entries(3);
    let positions = || {
        let (mut app, cars) = race_app(short(), &entries);
        run(&mut app, 40.0, |_| {});
        cars.iter()
            .map(|&c| app.world().get::<Position>(c).unwrap().0)
            .collect::<Vec<_>>()
    };
    assert_eq!(positions(), positions());
}

/// Standing still facing backwards on the avenue: turns round in a few
/// reverse-and-pull manoeuvres and drives on without a reset.
#[test]
#[ignore = "QA check"]
fn check_an_ai_facing_backwards_turns_round() {
    let (mut app, cars) = race_app(short(), &[(GTI, ai(Difficulty::Pro))]);
    start(&mut app);
    place(&mut app, cars[0], 150.0, 0.0, std::f32::consts::PI, 0.0);
    run(&mut app, 15.0, |_| {});
    let r = racer(&app, cars[0]);
    assert!(r.resets.is_empty(), "{:?}", r.resets);
    assert!(r.progress.distance > 200.0 && !r.progress.wrong_way);
}

/// Catch-up at its +3 % for an Alien (capped at the car's full-limit
/// profile): no resets, a little faster than without.
#[test]
#[ignore = "QA check"]
fn check_full_catch_up_keeps_an_alien_on_the_road() {
    let lap = |catch_up| {
        let config = RaceConfig {
            countdown: 0.2,
            catch_up,
            laps: 3,
            ..default()
        };
        let (mut app, cars) = race_app(
            config,
            &[(GTI, DriverKind::Human), (M3, ai(Difficulty::Alien))],
        );
        run(&mut app, 180.0, |a| far_ahead_human(a, cars[0]));
        let r = racer(&app, cars[1]);
        assert!(r.resets.is_empty(), "{:?}", r.resets);
        r.timing.laps[1]
    };
    let (off, on) = (lap(false), lap(true));
    assert!(on < off && on > off * 0.97, "{on} vs {off}");
}

/// The 8-car field of `ai-races` race `QA_RACE` (default 0) for `QA_SECS`
/// seconds (default 70): prints the last 10 s of every car near each reset
/// (distance, lateral, speed, target, pedals, AI mode, pass, squeeze, who
/// hit it). The tool behind the pile-up analysis in docs/qa/ai-drivers.md.
#[test]
#[ignore = "QA diagnostic"]
fn diag_first_lap_resets() {
    let var = |k: &str, d: f32| {
        std::env::var(k)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(d)
    };
    let entries = mixed_entries(var("QA_RACE", 0.0) as usize);
    let (mut app, cars) = race_app(RaceConfig::default(), &entries);
    let mut hist = vec![std::collections::VecDeque::new(); cars.len()];
    let mut seen = vec![0; cars.len()];
    let mut tick = 0u64;
    while time(&app) < var("QA_SECS", 70.0) {
        app.update();
        tick += 1;
        if tick.is_multiple_of(48) {
            for (i, &c) in cars.iter().enumerate() {
                let r = racer(&app, c);
                let ctl = app.world().get::<CarControls>(c).unwrap().0;
                hist[i].push_back(format!(
                    "t {:.1} s {:.1} lat {:.2} v {:.1} target {:.1} thr {:.2} brk {:.2} mode {} pass {} squeezed {} hit_by {}",
                    time(&app),
                    r.progress.distance,
                    r.progress.lateral,
                    speed(&app, c),
                    app.world().get::<AiDriver>(c).unwrap().debug.target_speed,
                    ctl.throttle,
                    ctl.brake,
                    field(&app, c, "mode"),
                    field(&app, c, "overtaking"),
                    field(&app, c, "squeezed"),
                    field(&app, c, "hit_by"),
                ));
                if hist[i].len() > 50 {
                    hist[i].pop_front();
                }
            }
        }
        for (i, &c) in cars.iter().enumerate() {
            let r = racer(&app, c);
            if r.resets.len() == seen[i] {
                continue;
            }
            seen[i] = r.resets.len();
            let here = app.world().get::<Position>(c).unwrap().0;
            println!(
                "\n=== car {i} {:?} {}: {:?}",
                entries[i].1,
                entries[i].0,
                r.resets.last()
            );
            for (j, &o) in cars.iter().enumerate() {
                if app.world().get::<Position>(o).unwrap().0.distance(here) < 25.0 {
                    println!("--- car {j} {:?} {}", entries[j].1, entries[j].0);
                    hist[j].iter().for_each(|l| println!("  {l}"));
                }
            }
        }
    }
}
