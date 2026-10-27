//! QA repros for the race rules (`docs/qa/race-rules.md`). Every test here
//! fails on the current code and is ignored with its bug id so the suite
//! stays green; run them with `cargo test -p racer-sim --test qa_race_rules
//! -- --ignored`.

use std::sync::Arc;

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use racer_dynamics::load_car;
use racer_sim::{
    race::{
        Collisions, DriverKind, Race, RaceConfig, RacePhase, RacePlugin, RaceTrack, Racer,
        ResetRequest, spawn_racer,
    },
    rules::{
        Progress, ProgressEvent, ResetMonitor, ResetReason, Status, Timing, reset_pose, standings,
    },
    track::{city_track, spawn_track},
};
use racer_trackc::{Geometry, geometry::interpolate};

const DT: f32 = 1.0 / 240.0;

fn track() -> Geometry {
    racer_trackc::build(&racer_trackc::tracks_dir().join("downtown-loop")).unwrap()
}

/// Moves `p` (and `m`, if given) along the centreline from where it is to
/// `to` at `speed` m/s, facing `facing` (+1 along the track, -1 against),
/// collecting the events. Each step is one 240 Hz tick.
fn drive(
    t: &Geometry,
    p: &mut Progress,
    mut m: Option<&mut ResetMonitor>,
    to: f32,
    speed: f32,
    facing: f32,
) -> Vec<ProgressEvent> {
    let dir = (to - p.distance).signum();
    let step = speed * DT * dir;
    let mut d = p.distance;
    let mut events = Vec::new();
    while (to - d) * dir > 0.0 {
        d += step;
        let s = interpolate(&t.samples, t.length, d);
        let v = s.tangent * speed * dir;
        events.extend(p.update(t, s.at(0.0, 0.5), v, s.tangent * facing, DT));
        if let Some(m) = m.as_deref_mut() {
            m.update(p, 1.0, speed, true, DT);
        }
    }
    events
}

/// QA-RULES-1: reversing back over a gate and then being reset (key, AI
/// recovery, or stuck against the wall) puts the car ahead of the gate it
/// just lost, with that gate still owed: it can never be crossed forwards
/// again, so the car never completes another lap.
#[test]
fn reset_after_reversing_over_a_gate_must_not_strand_the_car() {
    let t = track();
    let mut p = Progress::new(&t, t.grid[0].position);
    let mut m = ResetMonitor::new(&p);
    let g = t.checkpoints[2];
    drive(&t, &mut p, Some(&mut m), g + 20.0, 20.0, 1.0);
    assert_eq!(p.next_gate, 3);
    // Spun round: reverse back over the gate at 3 m/s (the wrong-way flag
    // freezes `last_valid` after a second), then ask for a reset.
    let events = drive(&t, &mut p, Some(&mut m), g - 3.0, 3.0, 1.0);
    assert_eq!(events, [ProgressEvent::CheckpointUndone(2)]);
    let (_, d, x) = reset_pose(&t, m.last_valid, &[]);
    p.move_to(&t, d, x);
    assert!(
        p.distance > g,
        "reset at {d}, behind gate {g}: not this bug"
    );
    // Drive on: the next full lap must count.
    let events = drive(&t, &mut p, None, t.length + 10.0, 30.0, 1.0);
    assert_eq!(
        events.last(),
        Some(&ProgressEvent::Lap),
        "gate 2 at {g} is owed but behind the car (next_gate {}, distance {}): {events:?}",
        p.next_gate,
        p.distance
    );
}

/// QA-RULES-2: the wrong-way timer has no cap, so after driving the wrong
/// way for 20 s the warning stays on for ~9 s of correct driving (it decays
/// at 2x), and `last_valid` is frozen all that time.
#[test]
fn wrong_way_warning_clears_soon_after_turning_round() {
    let t = track();
    let mut p = Progress::new(&t, t.grid[0].position);
    let mut m = ResetMonitor::new(&p);
    drive(&t, &mut p, Some(&mut m), 900.0, 20.0, 1.0);
    // 20 s the wrong way, facing backwards.
    drive(&t, &mut p, Some(&mut m), 500.0, 20.0, -1.0);
    assert!(p.wrong_way);
    // Turned round: 3 s of driving the right way.
    drive(&t, &mut p, Some(&mut m), 560.0, 20.0, 1.0);
    assert!(!p.wrong_way, "still flagged wrong way after 3 s forwards");
    assert!(
        m.last_valid.0 > 540.0,
        "last valid point stuck at {}",
        m.last_valid.0
    );
}

/// QA-RULES-3: a car that leaves the corridor (over a 1.1 m wall, or
/// launched by contact) and comes back down on the same road more than ~26
/// m further on is back "on track", but every progress update is rejected
/// as a jump. Progress freezes for good, and since the car is on track and
/// moving, no reset ever comes.
#[test]
fn rejoining_further_on_is_reset_or_counted_never_frozen() {
    let t = track();
    let mut p = Progress::new(&t, t.grid[0].position);
    let mut m = ResetMonitor::new(&p);
    drive(&t, &mut p, Some(&mut m), 100.0, 25.0, 1.0);
    // 1.5 s outside the wall alongside the road, then back on at 140 m.
    for k in 0..360 {
        let s = interpolate(&t.samples, t.length, 100.0 + k as f32 / 9.0);
        let pos = s.at(s.half_width + 5.0, 0.5);
        p.update(&t, pos, s.tangent * 25.0, s.tangent, DT);
        assert!(m.update(&p, 1.0, 25.0, true, DT).is_none());
    }
    assert!(!p.on_track);
    let mut reset = None;
    let mut d = 140.0;
    // Ten seconds of driving on along the road.
    for _ in 0..2400 {
        d += 25.0 * DT;
        let s = interpolate(&t.samples, t.length, d);
        p.update(&t, s.at(0.0, 0.5), s.tangent * 25.0, s.tangent, DT);
        reset = reset.or(m.update(&p, 1.0, 25.0, true, DT));
    }
    assert!(
        reset.is_some() || p.distance > 300.0,
        "car at {d} m, progress frozen at {} (on_track {}), never reset",
        p.distance,
        p.on_track
    );
}

/// QA-RULES-4: `Timing` only remembers one previous lap start and one set of
/// split times. Driving a full lap backwards undoes two laps; the second
/// undo restores the wrong lap start and no sectors, so the re-driven lap is
/// timed from the end of lap 1 and has a single "sector".
#[test]
fn timing_survives_a_full_lap_driven_backwards() {
    let t = track();
    let mut p = Progress::new(&t, t.grid[0].position);
    let mut timing = Timing::start(0.0);
    let mut now = 0.0;
    let mut go = |p: &mut Progress, timing: &mut Timing, to: f32, facing: f32| {
        let dir = (to - p.distance).signum();
        let mut d = p.distance;
        while (to - d) * dir > 0.0 {
            d += 0.25 * dir;
            now += 0.01;
            let s = interpolate(&t.samples, t.length, d);
            let v = s.tangent * 25.0 * dir;
            for e in p.update(&t, s.at(0.0, 0.5), v, s.tangent * facing, 0.01) {
                timing.on_event(e, now, &t);
            }
        }
        now
    };
    go(&mut p, &mut timing, 2.0 * t.length + 5.0, 1.0);
    assert_eq!(timing.laps.len(), 2);
    // A full lap backwards: both laps are taken back.
    go(&mut p, &mut timing, t.length - 5.0, -1.0);
    assert_eq!((p.laps, timing.laps.len()), (0, 0));
    // Forwards over the line again: lap 1 is credited again. Its time must
    // be measured from the race start, not from the end of lap 1.
    let end = go(&mut p, &mut timing, t.length + 5.0, 1.0);
    assert_eq!(p.laps, 1);
    // The line is 5 m (20 steps of 0.01 s) before `end`.
    let lap = timing.laps[0];
    assert!(
        (lap - (end - 0.2)).abs() < 0.05,
        "lap 1 re-credited as {lap} s, race time {end} s"
    );
    assert_eq!(
        timing.last_sectors.len(),
        t.sector_ends.len() + 1,
        "{:?}",
        timing.last_sectors
    );
}

/// QA-RULES-5: two cars finishing in the same 240 Hz step get the same
/// finish time and are ordered by entity order, not by who is further
/// over the line.
#[test]
#[ignore = "QA-RULES-5"]
fn same_step_finish_is_ordered_by_who_crossed_first() {
    let t = track();
    let base = Progress::new(&t, t.grid[0].position);
    let at = |distance| {
        let mut p = base;
        (p.laps, p.next_gate, p.distance) = (3, 0, distance);
        p
    };
    // Car 1 is 0.2 m further over the line than car 0 in the same step.
    let cars = [
        (Status::Finished(200.0), at(3.0 * t.length + 0.05)),
        (Status::Finished(200.0), at(3.0 * t.length + 0.25)),
    ];
    assert_eq!(standings(&cars), [1, 0]);
}

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
    let spec = load_car("vw-golf-gti-mk8").unwrap();
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

fn geometry(app: &App) -> Geometry {
    app.world().resource::<RaceTrack>().0.geometry.clone()
}

/// Puts `car` on the centreline at `d` with `progress` state, moving at `v`
/// m/s along the track (negative: backwards).
fn place(app: &mut App, car: Entity, d: f32, laps: u32, next_gate: usize, v: f32) {
    let g = geometry(app);
    let s = interpolate(&g.samples, g.length, d);
    let world = app.world_mut();
    world.get_mut::<Position>(car).unwrap().0 = s.at(0.0, 0.6);
    world.get_mut::<Rotation>(car).unwrap().0 = s.rotation();
    world.get_mut::<LinearVelocity>(car).unwrap().0 = s.tangent * v;
    let mut r = world.get_mut::<Racer>(car).unwrap();
    r.progress.distance = d;
    r.progress.laps = laps;
    r.progress.next_gate = next_gate;
    r.progress.sample = g
        .samples
        .iter()
        .position(|x| x.s >= d.rem_euclid(g.length))
        .unwrap_or(0);
}

fn set_velocity(app: &mut App, car: Entity, v: f32) {
    let g = geometry(app);
    let d = racer(app, car).progress.distance;
    let s = interpolate(&g.samples, g.length, d);
    app.world_mut().get_mut::<LinearVelocity>(car).unwrap().0 = s.tangent * v;
}

/// QA-RULES-6: two cars reset in the same step are placed without seeing
/// each other (`apply_resets` only avoids cars *not* being reset), so a
/// pile-up's stuck cars land on the very same spot. The ghosting check then
/// skips the other car as "itself" (`*p != pos.0`), so both turn solid
/// inside each other after 3 s.
#[test]
#[ignore = "QA-RULES-6"]
fn cars_reset_in_the_same_step_land_apart() {
    let config = RaceConfig {
        countdown: 0.1,
        ..default()
    };
    let (mut app, cars) = race_app(config, &[DriverKind::Human, DriverKind::Human]);
    run_for(&mut app, 0.5);
    for &car in &cars {
        app.world_mut()
            .get_mut::<Racer>(car)
            .unwrap()
            .monitor
            .last_valid = (300.0, 0.0);
        app.world_mut().entity_mut(car).insert(ResetRequest);
    }
    run_for(&mut app, 2.0 / 240.0);
    let a = app.world().get::<Position>(cars[0]).unwrap().0;
    let b = app.world().get::<Position>(cars[1]).unwrap().0;
    assert!(
        a.distance(b) > 5.5,
        "both reset to {a} / {b} ({} m apart)",
        a.distance(b)
    );
}

/// QA-RULES-7: once the winner is home, any `Lap` event finishes a car,
/// including the re-credit of a lap it had already completed before the
/// flag. A car that crossed the line just before the winner finished can
/// back over it and drive over again to take the flag a lap early.
#[test]
#[ignore = "QA-RULES-7"]
fn rocking_over_the_line_after_the_flag_does_not_finish_early() {
    let config = RaceConfig {
        countdown: 0.1,
        laps: 3,
        collisions: Collisions::Off,
        ..default()
    };
    let (mut app, cars) = race_app(config, &[DriverKind::Human, DriverKind::Human]);
    run_for(&mut app, 0.5);
    let l = geometry(&app).length;
    // Car 1 finished; car 0 crossed the line for its lap 1 just before.
    {
        let world = app.world_mut();
        world.resource_mut::<Race>().winner = Some(0.4);
        let mut r = world.get_mut::<Racer>(cars[1]).unwrap();
        r.status = Status::Finished(0.4);
        let mut r = world.get_mut::<Racer>(cars[0]).unwrap();
        r.timing.laps.push(0.3);
    }
    // The car brakes itself hard with no input, so the pushes are short.
    place(&mut app, cars[0], l + 1.0, 1, 0, -6.0);
    run_for(&mut app, 0.6);
    assert_eq!(racer(&app, cars[0]).progress.laps, 0, "did not back over");
    set_velocity(&mut app, cars[0], 8.0);
    run_for(&mut app, 1.0);
    let r = racer(&app, cars[0]);
    assert_eq!(r.progress.laps, 1, "did not drive over again");
    assert_eq!(
        r.status,
        Status::Racing,
        "finished with {} laps after rocking over the line",
        r.timing.laps.len()
    );
}

/// QA-RULES-8: `RaceConfig { laps: 0 }` (reachable from the API and any
/// future lobby or server config; the menu and CLI clamp to 1..=10) ends the
/// race for everybody on the first running step.
#[test]
#[ignore = "QA-RULES-8"]
fn zero_laps_does_not_finish_everyone_at_the_start() {
    let config = RaceConfig {
        countdown: 0.1,
        laps: 0,
        ..default()
    };
    let (mut app, cars) = race_app(config, &[DriverKind::Human]);
    run_for(&mut app, 0.5);
    assert_eq!(
        racer(&app, cars[0]).status,
        Status::Racing,
        "phase {:?}",
        app.world().resource::<Race>().phase
    );
}

/// Sanity check for the harness: the reset path itself works.
#[test]
fn harness_reset_request_moves_the_car() {
    let config = RaceConfig {
        countdown: 0.1,
        ..default()
    };
    let (mut app, cars) = race_app(config, &[DriverKind::Human]);
    run_for(&mut app, 0.5);
    assert_eq!(app.world().resource::<Race>().phase, RacePhase::Running);
    app.world_mut()
        .get_mut::<Racer>(cars[0])
        .unwrap()
        .monitor
        .last_valid = (300.0, 0.0);
    app.world_mut().entity_mut(cars[0]).insert(ResetRequest);
    run_for(&mut app, 2.0 / 240.0);
    let r = racer(&app, cars[0]);
    assert_eq!(r.resets.len(), 1);
    assert_eq!(r.resets[0].reason, ResetReason::Requested);
    assert!((r.progress.distance - 292.0).abs() < 1.0);
}
