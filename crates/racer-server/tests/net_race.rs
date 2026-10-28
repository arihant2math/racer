//! An online race in one process: the server and two bot clients over UDP
//! on localhost with simulated latency, jitter and loss.

use std::{
    net::{SocketAddr, UdpSocket},
    sync::Arc,
    time::{Duration, Instant},
};

use avian3d::prelude::Position;
use bevy::prelude::*;
use racer_dynamics::Assists;
use racer_server::{ServerConfig, build_app};
use racer_sim::{
    ai::Difficulty,
    net::{
        client::{NetCar, NetClientConfig, NetSession, NetStats, SessionState, headless_client},
        protocol::{Conditions, Join},
    },
    race::{RaceConfig, Racer},
};

fn free_port() -> SocketAddr {
    UdpSocket::bind("127.0.0.1:0").unwrap().local_addr().unwrap()
}

#[test]
fn two_bots_race_against_an_ai_with_latency_and_loss() {
    let addr = free_port();
    let conditions = Conditions {
        rtt_ms: 100,
        jitter_ms: 10,
        loss: 0.01,
    };
    let mut server = build_app(&ServerConfig {
        bind: addr,
        players: 2,
        lobby_timeout: 20.0,
        grid: 3,
        difficulty: Some(Difficulty::Pro),
        conditions,
        race: RaceConfig {
            countdown: 2.0,
            ..default()
        },
        ..default()
    });
    racer_sim::finish_headless(&mut server);
    let track = Arc::new(racer_trackc::load("downtown-loop").unwrap());
    let mut clients: Vec<App> = (0..2)
        .map(|i| {
            let join = Join {
                name: format!("bot{i}"),
                car: racer_dynamics::spec::CARS[i].into(),
                assists: Assists::default(),
            };
            let mut config = NetClientConfig::direct(addr, 100 + i as u64, join);
            config.conditions = conditions;
            config.autopilot = Some(Difficulty::Pro);
            headless_client(config, track.clone())
        })
        .collect();

    let started = Instant::now();
    let mut racing_since = None;
    while started.elapsed() < Duration::from_secs(40) {
        server.update();
        for c in &mut clients {
            c.update();
        }
        let racing = clients
            .iter()
            .all(|c| c.world().resource::<NetSession>().state == SessionState::Racing);
        if racing && racing_since.is_none() {
            racing_since = Some(Instant::now());
        }
        if racing_since.is_some_and(|t| t.elapsed() > Duration::from_secs(10)) {
            break;
        }
        for c in &clients {
            if let SessionState::Failed(e) = &c.world().resource::<NetSession>().state {
                panic!("client failed: {e}");
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(racing_since.is_some(), "the race never started on both clients");

    // Every car is where the server has it (within what the extrapolation
    // and blending of 100 ms of latency allow).
    let server_cars: Vec<(usize, Vec3)> = server
        .world_mut()
        .query::<(&Racer, &Position)>()
        .iter(server.world())
        .map(|(r, p)| (r.slot, p.0))
        .collect();
    assert_eq!(server_cars.len(), 3);
    eprintln!(
        "server ticks {} race time {:?}",
        server.world().resource::<racer_server::TickCount>().0,
        server.world().get_resource::<racer_sim::race::Race>().map(|r| r.time)
    );
    for c in &clients {
        let s = c.world().resource::<NetStats>();
        eprintln!("client: snapshots {} corrections {} unmatched {} lead {:?}", s.snapshots, s.corrections.len(), s.unmatched, &s.input_lead[s.input_lead.len().saturating_sub(5)..]);
    }
    let moved = server_cars
        .iter()
        .filter(|(slot, p)| {
            let grid = track.geometry.grid[*slot].position;
            p.distance(grid) > 50.0
        })
        .count();
    eprintln!("grid {:?}", track.geometry.grid.iter().map(|g| g.position).collect::<Vec<_>>());
    assert_eq!(moved, 3, "every car drove off the grid: {server_cars:?}");
    for c in &mut clients {
        let seen: Vec<(u8, Vec3)> = c
            .world_mut()
            .query::<(&NetCar, &Position)>()
            .iter(c.world())
            .map(|(n, p)| (n.slot, p.0))
            .collect();
        assert_eq!(seen.len(), 3);
        for (slot, p) in seen {
            let s = server_cars.iter().find(|(s, _)| *s == slot as usize).unwrap().1;
            // The client runs ~RTT ahead of the server, so its cars are
            // ahead by up to RTT x speed.
            assert!(p.distance(s) < 12.0, "slot {slot}: client {p} server {s}");
        }
        let stats = c.world().resource::<NetStats>().clone();
        assert!(stats.snapshots > 300, "{stats:?}");
        let big = stats.corrections.iter().filter(|c| c.position > 0.5).count();
        assert!(big <= 3, "corrections over 0.5 m: {:?}", stats.corrections);
        let session = c.world().resource::<NetSession>();
        assert!(session.rtt > Duration::from_millis(60), "{:?}", session.rtt);
    }
}
