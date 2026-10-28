//! An online race in one process: the server and two bot clients (each on
//! its own thread, like separate machines) over UDP on localhost with
//! simulated latency, jitter and loss.

use std::{
    net::{SocketAddr, UdpSocket},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use avian3d::prelude::Position;
use bevy::prelude::*;
use racer_dynamics::Assists;
use racer_server::{ServerConfig, build_app};
use racer_sim::{
    ai::Difficulty,
    net::{
        client::{NetClientConfig, NetSession, NetStats, SessionState, headless_client},
        protocol::{Conditions, Join},
    },
    race::{RaceConfig, Racer},
};

fn free_port() -> SocketAddr {
    UdpSocket::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
}

/// Updates `app` in real time on this thread until `stop` or `done`.
fn run_until(app: &mut App, stop: &AtomicBool, done: impl Fn(&App) -> bool) {
    while !stop.load(Ordering::Relaxed) && !done(app) {
        app.update();
        std::thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn two_bots_race_against_an_ai_with_latency_and_loss() {
    let addr = free_port();
    let conditions = Conditions {
        rtt_ms: 100,
        jitter_ms: 10,
        loss: 0.01,
    };
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut app = build_app(&ServerConfig {
                bind: addr,
                players: 2,
                lobby_timeout: 30.0,
                grid: 3,
                difficulty: Some(Difficulty::Pro),
                conditions,
                race: RaceConfig {
                    countdown: 2.0,
                    ..default()
                },
                ..default()
            });
            racer_sim::finish_headless(&mut app);
            // Ten seconds of racing after the countdown.
            let deadline = Instant::now() + Duration::from_secs(60);
            run_until(&mut app, &stop, |app| {
                Instant::now() > deadline
                    || app
                        .world()
                        .get_resource::<racer_sim::race::Race>()
                        .is_some_and(|r| r.time > 10.0)
            });
            stop.store(true, Ordering::Relaxed);
            let world = app.world_mut();
            world
                .query::<(&Racer, &Position)>()
                .iter(world)
                .map(|(r, p)| (r.slot, p.0))
                .collect::<Vec<_>>()
        })
    };
    let track = Arc::new(racer_trackc::load("downtown-loop").unwrap());
    let clients: Vec<_> = (0..2)
        .map(|i| {
            let (stop, track) = (stop.clone(), track.clone());
            std::thread::spawn(move || {
                let join = Join {
                    name: format!("bot{i}"),
                    car: racer_dynamics::spec::CARS[i].into(),
                    assists: Assists::default(),
                };
                let mut config = NetClientConfig::direct(addr, 100 + i as u64, join);
                config.conditions = conditions;
                config.autopilot = Some(Difficulty::Pro);
                let mut app = headless_client(config, track);
                run_until(&mut app, &stop, |app| {
                    matches!(
                        app.world().resource::<NetSession>().state,
                        SessionState::Failed(_)
                    )
                });
                let session = app.world().resource::<NetSession>();
                (
                    session.state.clone(),
                    session.rtt,
                    app.world().resource::<NetStats>().clone(),
                )
            })
        })
        .collect();
    let server_cars = server.join().unwrap();
    let clients: Vec<_> = clients.into_iter().map(|c| c.join().unwrap()).collect();

    assert_eq!(server_cars.len(), 3);
    for (slot, p) in &server_cars {
        let grid = track.geometry.grid[*slot].position;
        assert!(
            p.distance(grid) > 50.0,
            "car {slot} still near the grid at {p}"
        );
    }
    for (state, rtt, stats) in clients {
        assert_eq!(state, SessionState::Racing);
        assert!(rtt > Duration::from_millis(60), "{rtt:?}");
        assert!(stats.snapshots > 300, "{} snapshots", stats.snapshots);
        // Outside contacts the prediction matches the server almost always.
        let big = stats
            .corrections
            .iter()
            .filter(|c| c.position > 0.5)
            .count();
        assert!(big <= 3, "corrections over 0.5 m: {:?}", stats.corrections);
        let mut errors: Vec<f32> = stats.remote_errors.iter().map(|e| e.0).collect();
        errors.sort_by(f32::total_cmp);
        assert!(!errors.is_empty());
        let p95 = errors[errors.len() * 95 / 100];
        assert!(p95 < 2.0, "remote car p95 error {p95} m");
    }
}

/// A player whose client goes away mid-race: the AI drives the car on.
#[test]
fn a_player_who_leaves_is_replaced_by_the_ai() {
    let addr = free_port();
    let stop = Arc::new(AtomicBool::new(false));
    let server = {
        let stop = stop.clone();
        std::thread::spawn(move || {
            let mut app = build_app(&ServerConfig {
                bind: addr,
                players: 1,
                grid: 1,
                race: RaceConfig {
                    countdown: 1.0,
                    ..default()
                },
                ..default()
            });
            racer_sim::finish_headless(&mut app);
            let deadline = Instant::now() + Duration::from_secs(60);
            let taken_over = |app: &App| {
                let world = app.world();
                world
                    .iter_entities()
                    .any(|e| e.contains::<Racer>() && e.contains::<racer_sim::ai::AiDriver>())
            };
            run_until(&mut app, &stop, |app| {
                Instant::now() > deadline || taken_over(app)
            });
            stop.store(true, Ordering::Relaxed);
            taken_over(&app)
        })
    };
    let track = Arc::new(racer_trackc::load("downtown-loop").unwrap());
    let join = Join {
        name: "leaver".into(),
        car: racer_dynamics::spec::CARS[0].into(),
        assists: Assists::default(),
    };
    let mut app = headless_client(NetClientConfig::direct(addr, 7, join), track);
    // Race for two seconds, then vanish without a word.
    let started = Instant::now();
    run_until(&mut app, &stop, |app| {
        let racing = app.world().resource::<NetSession>().state == SessionState::Racing;
        racing && started.elapsed() > Duration::from_secs(4)
            || started.elapsed() > Duration::from_secs(40)
    });
    drop(app);
    assert!(server.join().unwrap(), "the AI did not take the car over");
}
