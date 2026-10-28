//! Reversing under the race rules, with the car driven by its pedals in the
//! automatic gearbox (the brake held at a standstill reverses): a short
//! reverse raises no warning and no reset, and reversing over the line and
//! back takes the lap back and credits it once again, with its time intact.

use std::sync::Arc;

use avian3d::prelude::{LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use racer_dynamics::{Controls, load_car};
use racer_sim::{
    race::{Collisions, DriverKind, Race, RaceConfig, RacePlugin, RaceTrack, Racer, spawn_racer},
    rules::{Progress, ProgressEvent, Status},
    track::{city_track, spawn_track},
    vehicle::{CarControls, CarTelemetry},
};
use racer_trackc::{Geometry, geometry::interpolate};

fn race_app() -> (App, Entity) {
    let config = RaceConfig {
        countdown: 0.1,
        laps: 3,
        collisions: Collisions::Off,
        ..default()
    };
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
    let car = spawn_racer(
        &mut commands,
        &track,
        &spec,
        0,
        "player".into(),
        DriverKind::Human,
        (),
    )
    .unwrap();
    world.flush();
    // Past the countdown.
    for _ in 0..120 {
        app.update();
    }
    (app, car)
}

fn geometry(app: &App) -> Geometry {
    app.world().resource::<RaceTrack>().0.geometry.clone()
}

fn racer(app: &App, car: Entity) -> &Racer {
    app.world().get::<Racer>(car).unwrap()
}

fn gear(app: &App, car: Entity) -> i8 {
    app.world().get::<CarTelemetry>(car).unwrap().telemetry.gear
}

/// Puts `car` on the centreline at `d`, facing along the track and moving at
/// `v`, with every gate before `d` credited on lap `laps`.
fn place(app: &mut App, car: Entity, d: f32, laps: u32, v: f32) {
    let g = geometry(app);
    let s = interpolate(&g.samples, g.length, d);
    let world = app.world_mut();
    world.get_mut::<Position>(car).unwrap().0 = s.at(0.0, 0.6);
    world.get_mut::<Rotation>(car).unwrap().0 = s.rotation();
    world.get_mut::<LinearVelocity>(car).unwrap().0 = s.tangent * v;
    let mut r = world.get_mut::<Racer>(car).unwrap();
    r.progress.distance = d;
    r.progress.laps = laps;
    let along = d - laps as f32 * g.length;
    r.progress.next_gate = g.checkpoints.iter().filter(|&&c| c <= along).count();
    r.progress.sample = g
        .samples
        .iter()
        .position(|x| x.s >= d.rem_euclid(g.length))
        .unwrap_or(0);
    r.monitor.clear();
}

/// What the driving saw: the lowest gear, whether the wrong-way warning
/// came on, and the most laps counted.
#[derive(Default)]
struct Seen {
    lowest_gear: i8,
    wrong_way: bool,
    most_laps: u32,
}

/// Drives `car` with `controls` (automatic gearbox) until `done` or `limit` seconds.
fn drive(
    app: &mut App,
    car: Entity,
    controls: Controls,
    limit: f32,
    seen: &mut Seen,
    done: impl Fn(&App) -> bool,
) -> f32 {
    let mut t = 0.0;
    while t < limit && !done(app) {
        app.world_mut().get_mut::<CarControls>(car).unwrap().0 = Controls {
            automatic: true,
            ..controls
        };
        app.update();
        t += 1.0 / 240.0;
        let r = racer(app, car);
        seen.lowest_gear = seen.lowest_gear.min(gear(app, car));
        seen.wrong_way |= r.progress.wrong_way;
        seen.most_laps = seen.most_laps.max(r.progress.laps);
    }
    t
}

const BRAKE: Controls = Controls {
    throttle: 0.0,
    brake: 1.0,
    steer: 0.0,
    handbrake: 0.0,
    clutch: 0.0,
    shift_up: false,
    shift_down: false,
    automatic: true,
    assists: racer_dynamics::Assists {
        abs: true,
        tcs: true,
        esc: true,
    },
    hold: false,
};
const THROTTLE: Controls = Controls {
    throttle: 1.0,
    brake: 0.0,
    ..BRAKE
};

/// A driver stopped mid-lap (nose in a wall, say) holds the brake to back
/// out 1.5 m, then drives on: no wrong-way warning, no reset, no gate lost.
/// (The warning needs 1 s backwards above 1 m/s; at full pedal that is
/// about 2 m of reversing, see `docs/features/reverse.md`.)
#[test]
fn a_short_reverse_raises_no_warning_and_no_reset() {
    let (mut app, car) = race_app();
    let start = 300.0;
    place(&mut app, car, start, 0, 0.0);
    let gates = racer(&app, car).progress.next_gate;
    let mut seen = Seen::default();
    drive(&mut app, car, BRAKE, 5.0, &mut seen, |a| {
        racer(a, car).progress.distance < start - 1.5
    });
    assert_eq!(seen.lowest_gear, -1, "never engaged reverse");
    // Throttle: brakes the reversing car, back to 1st and on.
    drive(&mut app, car, THROTTLE, 6.0, &mut seen, |a| {
        racer(a, car).progress.distance > start + 15.0
    });
    let r = racer(&app, car);
    assert!(
        r.progress.distance > start + 15.0,
        "{}",
        r.progress.distance
    );
    assert!(gear(&app, car) >= 1);
    assert!(!seen.wrong_way, "wrong-way warning on a 1.5 m reverse");
    assert!(r.resets.is_empty(), "{:?}", r.resets);
    assert_eq!((r.progress.laps, r.progress.next_gate), (0, gates));
    assert_eq!(r.status, Status::Racing);
}

/// Over the line, stopped, reversed back over it, and over it again: the
/// lap is taken back and then counted once, timed from the race start.
#[test]
fn reversing_over_the_line_and_back_counts_the_lap_once() {
    let (mut app, car) = race_app();
    let l = geometry(&app).length;
    // The end of lap 1, 3 m before the line at 6 m/s.
    place(&mut app, car, l - 3.0, 0, 6.0);
    let mut seen = Seen::default();
    drive(&mut app, car, THROTTLE, 3.0, &mut seen, |a| {
        racer(a, car).progress.distance > l + 1.0
    });
    let first = {
        let r = racer(&app, car);
        assert_eq!(r.progress.laps, 1);
        assert_eq!(r.timing.laps.len(), 1);
        r.timing.laps[0]
    };
    // Stop and reverse back over the line.
    drive(&mut app, car, BRAKE, 6.0, &mut seen, |a| {
        racer(a, car).progress.distance < l - 3.0
    });
    assert_eq!(seen.lowest_gear, -1, "never engaged reverse");
    let r = racer(&app, car);
    assert_eq!(r.progress.laps, 0, "lap not taken back");
    assert!(r.timing.laps.is_empty(), "{:?}", r.timing.laps);
    // Over the line again: the lap counts once more, timed from the start.
    let time_before = app.world().resource::<Race>().time;
    drive(&mut app, car, THROTTLE, 6.0, &mut seen, |a| {
        racer(a, car).progress.distance > l + 5.0
    });
    let r = racer(&app, car);
    assert_eq!(gear(&app, car), 1);
    assert_eq!(r.progress.laps, 1);
    assert_eq!(seen.most_laps, 1, "a lap credited twice");
    assert_eq!(r.timing.laps.len(), 1);
    let again = r.timing.laps[0];
    assert!(
        again > first && again > time_before,
        "lap re-timed as {again} s (first {first} s, race time {time_before} s)"
    );
    assert!(r.resets.is_empty(), "{:?}", r.resets);
    assert_eq!(r.status, Status::Racing);
}

/// The same at the rules level: reversing over the line facing forwards
/// undoes the lap and driving over again redoes it, never two laps. A short
/// reverse stays under the wrong-way rule (1 s backwards above 1 m/s); a
/// longer one sets it off, as the rules intend.
#[test]
fn progress_reversing_over_the_line_facing_forwards() {
    let t = racer_trackc::build(&racer_trackc::tracks_dir().join("downtown-loop")).unwrap();
    let mut p = Progress::new(&t, t.grid[0].position);
    // Moves to `to` at `speed`, facing along the track: the events, and
    // whether the wrong-way warning came on.
    let go = |p: &mut Progress, to: f32, speed: f32| {
        let dir = (to - p.distance).signum();
        let dt = 1.0 / 240.0;
        let mut d = p.distance;
        let (mut events, mut wrong_way) = (Vec::new(), false);
        while (to - d) * dir > 0.0 {
            d += speed * dt * dir;
            let s = interpolate(&t.samples, t.length, d);
            events.extend(p.update(&t, s.at(0.0, 0.5), s.tangent * speed * dir, s.tangent, dt));
            wrong_way |= p.wrong_way;
        }
        (events, wrong_way)
    };
    go(&mut p, t.length + 1.0, 20.0);
    assert_eq!(p.laps, 1);
    // 2 m back at 2.5 m/s (0.8 s): over the line, no warning.
    let (events, wrong_way) = go(&mut p, t.length - 1.0, 2.5);
    assert_eq!(events, [ProgressEvent::LapUndone]);
    assert!(!wrong_way);
    assert_eq!(p.laps, 0);
    let (events, _) = go(&mut p, t.length + 10.0, 5.0);
    assert_eq!(events, [ProgressEvent::Lap]);
    assert_eq!(p.laps, 1);
    // 10 m back (4 s): the warning comes on, and the lap still comes back once.
    let (events, wrong_way) = go(&mut p, t.length + 0.5, 2.5);
    assert!(wrong_way);
    assert!(events.is_empty(), "{events:?}");
    let (events, _) = go(&mut p, t.length + 10.0, 5.0);
    assert!(events.is_empty(), "{events:?}");
    assert_eq!(p.laps, 1);
}

/// A longer reverse with real physics: 8 m back at full pedal (out of a
/// run-off, say), then on. The wrong-way warning comes on while reversing
/// and clears once the car drives on; no reset, no gate lost.
#[test]
fn a_longer_reverse_warns_then_clears_without_a_reset() {
    let (mut app, car) = race_app();
    let start = 300.0;
    place(&mut app, car, start, 0, 0.0);
    let gates = racer(&app, car).progress.next_gate;
    let mut seen = Seen::default();
    drive(&mut app, car, BRAKE, 8.0, &mut seen, |a| {
        racer(a, car).progress.distance < start - 8.0
    });
    assert!(racer(&app, car).progress.distance < start - 8.0);
    assert!(seen.wrong_way, "8 m of reversing should set off the warning");
    drive(&mut app, car, THROTTLE, 8.0, &mut seen, |a| {
        racer(a, car).progress.distance > start + 15.0
    });
    let mut after = Seen::default();
    drive(&mut app, car, THROTTLE, 1.5, &mut after, |_| false);
    let r = racer(&app, car);
    assert!(!r.progress.wrong_way, "warning still on after driving on");
    assert!(r.resets.is_empty(), "{:?}", r.resets);
    assert_eq!((r.progress.laps, r.progress.next_gate), (0, gates));
    assert_eq!(r.status, Status::Racing);
}

/// Reversing 12 m back over the line with real physics, then over it again:
/// the lap is taken back and counted once.
#[test]
fn a_longer_reverse_over_the_line_counts_the_lap_once() {
    let (mut app, car) = race_app();
    let l = geometry(&app).length;
    place(&mut app, car, l - 3.0, 0, 6.0);
    let mut seen = Seen::default();
    drive(&mut app, car, THROTTLE, 3.0, &mut seen, |a| {
        racer(a, car).progress.distance > l + 1.0
    });
    assert_eq!(racer(&app, car).progress.laps, 1);
    drive(&mut app, car, BRAKE, 10.0, &mut seen, |a| {
        racer(a, car).progress.distance < l - 11.0
    });
    let r = racer(&app, car);
    assert_eq!(r.progress.laps, 0, "lap not taken back");
    drive(&mut app, car, THROTTLE, 10.0, &mut seen, |a| {
        racer(a, car).progress.distance > l + 5.0
    });
    let r = racer(&app, car);
    assert_eq!(r.progress.laps, 1);
    assert_eq!(seen.most_laps, 1, "a lap credited twice");
    assert_eq!(r.timing.laps.len(), 1);
    assert!(r.resets.is_empty(), "{:?}", r.resets);
}
