//! The net check: one dedicated server process and headless bot clients
//! (the AI driving each client's car through its controls, as a player
//! would) over UDP on localhost with simulated latency, jitter and loss.
//! Compares what every client saw and predicted with the server's report.

use std::{
    fmt::Write as _,
    net::{SocketAddr, UdpSocket},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use bevy::prelude::*;
use racer_dynamics::{Assists, spec::CARS};
use racer_sim::{
    ai::Difficulty,
    net::{
        client::{NetCar, NetClientConfig, NetSession, NetStats, SessionState, headless_client},
        protocol::{Conditions, Join},
        report::ServerReport,
    },
    race::Racer,
    rules::Status,
};
use racer_trackc::Track;

/// What to run.
#[derive(Clone, Debug)]
pub struct NetCheck {
    pub server: PathBuf,
    pub bots: usize,
    pub laps: u32,
    pub conditions: Conditions,
    pub timeout: Duration,
}

/// What one bot client ended with.
#[derive(Debug)]
pub struct BotResult {
    pub slot: Option<u8>,
    pub state: SessionState,
    pub stats: NetStats,
    /// Every racer as this client last saw it, by slot.
    pub racers: Vec<(u8, Racer)>,
    pub frames: u32,
    pub wall: Duration,
}

fn free_port() -> Result<SocketAddr, String> {
    UdpSocket::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .map_err(|e| format!("no free port: {e}"))
}

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.kill().ok();
        self.0.wait().ok();
    }
}

/// Runs the race; returns the server report and every bot's result.
pub fn run(
    check: &NetCheck,
    race: usize,
    out_dir: &Path,
    track: Arc<Track>,
) -> Result<RaceRun, String> {
    let addr = free_port()?;
    let report_path = out_dir.join(format!("server-{}.json", race + 1));
    std::fs::remove_file(&report_path).ok();
    let c = &check.conditions;
    let log_path = out_dir.join(format!("server-{}.log", race + 1));
    let log = std::fs::File::create(&log_path).map_err(|e| e.to_string())?;
    let mut server = Server(
        Command::new(&check.server)
            .args([
                "--bind",
                &addr.to_string(),
                "--players",
                &check.bots.to_string(),
                "--grid",
                &check.bots.to_string(),
                "--no-ai-fill",
                "--laps",
                &check.laps.to_string(),
                "--lobby-timeout",
                "60",
                "--linger",
                "3",
                "--conditions",
                &format!("{},{},{}", c.rtt_ms, c.jitter_ms, c.loss * 100.0),
                "--report",
            ])
            .arg(&report_path)
            .stdout(Stdio::null())
            .stderr(log)
            .spawn()
            .map_err(|e| format!("{}: {e}", check.server.display()))?,
    );
    let deadline = Instant::now() + check.timeout;
    let bots: Vec<_> = (0..check.bots)
        .map(|i| {
            let track = track.clone();
            let join = Join {
                name: format!("bot{i}"),
                car: CARS[i % CARS.len()].to_string(),
                assists: Assists::default(),
            };
            let mut config = NetClientConfig::direct(addr, 1000 + i as u64, join);
            config.conditions = check.conditions;
            // Slowest first on the grid (bots join in order), so that the
            // faster ones have to pass: traffic and contacts.
            // Later races shift everyone up a difficulty.
            let d = i * Difficulty::ALL.len() / check.bots.max(1) + race;
            config.autopilot = Some(Difficulty::ALL[d % Difficulty::ALL.len()]);
            let start = Duration::from_millis(150 * i as u64);
            std::thread::spawn(move || {
                std::thread::sleep(start);
                bot(config, track, deadline)
            })
        })
        .collect();
    let results: Vec<BotResult> = bots
        .into_iter()
        .map(|b| b.join().map_err(|_| "a bot thread panicked".to_string()))
        .collect::<Result<_, _>>()?;
    // The server exits a few seconds after the results.
    while Instant::now() < deadline + Duration::from_secs(10) {
        if server.0.try_wait().map_err(|e| e.to_string())?.is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let json = std::fs::read_to_string(&report_path)
        .map_err(|e| format!("no server report ({e}); see {}", log_path.display()))?;
    let report: ServerReport = serde_json::from_str(&json).map_err(|e| e.to_string())?;
    Ok((report, results))
}

/// One bot client at 60 frames a second until the results are in.
fn bot(config: NetClientConfig, track: Arc<Track>, deadline: Instant) -> BotResult {
    let mut app = headless_client(config, track);
    let frame = Duration::from_secs_f64(1.0 / 60.0);
    let started = Instant::now();
    let mut next = started;
    let mut frames = 0;
    let mut over_at = None;
    while Instant::now() < deadline {
        app.update();
        frames += 1;
        if std::env::var_os("RACER_NET_DEBUG").is_some() && frames % 120 == 0 {
            let w = app.world();
            let p = w.get_resource::<racer_sim::net::client::Prediction>();
            let sync = w.get_resource::<lightyear::prelude::LocalTimelineSync>();
            let s = w.resource::<NetStats>();
            eprintln!(
                "{:?} tick {:?} lead {:?} synced {:?} speed {:?} rtt {:?} corrections {}",
                w.resource::<NetSession>().you,
                p.map(|p| p.tick),
                s.input_lead.last(),
                sync.map(|s| s.is_synced()),
                sync.map(|s| s.relative_speed()),
                w.resource::<NetSession>().rtt,
                s.corrections.len()
            );
        }
        let state = app.world().resource::<NetSession>().state.clone();
        match state {
            SessionState::Over => {
                // Let the last standings arrive.
                if over_at.get_or_insert_with(Instant::now).elapsed() > Duration::from_secs(1) {
                    break;
                }
            }
            SessionState::Failed(_) => break,
            _ => {}
        }
        next += frame;
        let now = Instant::now();
        if next > now {
            std::thread::sleep(next - now);
        } else {
            next = now;
        }
    }
    let world = app.world_mut();
    let racers = world
        .query::<(&NetCar, &Racer)>()
        .iter(world)
        .map(|(c, r)| (c.slot, r.clone()))
        .collect();
    let session = world.resource::<NetSession>();
    BotResult {
        slot: session.you,
        state: session.state.clone(),
        stats: world.resource::<NetStats>().clone(),
        racers,
        frames,
        wall: started.elapsed(),
    }
}

/// Pass/fail limits of the net check, by network condition (see
/// `docs/milestones/M3.md` for why these).
#[derive(Clone, Copy, Debug)]
pub struct Thresholds {
    /// 95th percentile and maximum of the local car's position error at a
    /// correction, outside contacts and resets (m).
    pub position_p95: f32,
    pub position_max: f32,
    /// Same for the rotation error (degrees).
    pub rotation_p95: f32,
    /// Desyncs (corrections over [`DESYNC`] m outside contacts and resets)
    /// per car and minute.
    pub desyncs_per_minute: f32,
    /// Share of server contacts both clients predicted.
    pub contact_agreement: f32,
}

/// A correction this large (m) is visible as a pop: a desync.
pub const DESYNC: f32 = 0.5;

/// Contacts match within this many ticks (0.5 s) and metres.
const CONTACT_TICKS: u32 = 120;
const CONTACT_DISTANCE: f32 = 5.0;

/// Corrections this close (ticks, 1 s) to a contact or reset are expected.
const EVENT_WINDOW: u32 = 240;

pub fn thresholds(c: &Conditions) -> Thresholds {
    if c.rtt_ms <= 150 {
        Thresholds {
            position_p95: 0.25,
            position_max: 2.0,
            rotation_p95: 2.0,
            desyncs_per_minute: 1.0,
            contact_agreement: 0.75,
        }
    } else {
        // Degraded but working: reported, gated loosely.
        Thresholds {
            position_p95: 1.0,
            position_max: 5.0,
            rotation_p95: 6.0,
            desyncs_per_minute: 4.0,
            contact_agreement: 0.5,
        }
    }
}

fn percentile(values: &mut [f32], p: f32) -> f32 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(f32::total_cmp);
    let i = ((values.len() - 1) as f32 * p).round() as usize;
    values[i]
}

/// One race of the check: the server's report and the bots' results.
pub type RaceRun = (ServerReport, Vec<BotResult>);

/// Writes the analysis of `races` to `out`; returns whether every check
/// passed.
pub fn analyse(check: &NetCheck, races: &[RaceRun], out: &mut String) -> bool {
    let t = thresholds(&check.conditions);
    let c = &check.conditions;
    let mut ok = true;
    let mut fail = |out: &mut String, what: String| {
        ok = false;
        writeln!(out, "FAIL {what}").ok();
    };
    writeln!(
        out,
        "# Net check: {} race(s), {} bots, {} laps, {} ms RTT, {} ms jitter, {:.1} % loss\n",
        races.len(),
        check.bots,
        check.laps,
        c.rtt_ms,
        c.jitter_ms,
        c.loss * 100.0
    )
    .ok();

    let (mut all_pos, mut all_rot) = (Vec::new(), Vec::new());
    let (mut near_pos, mut near_shown) = (Vec::new(), Vec::new());
    let (mut desyncs, mut minutes, mut corrections, mut snapshots) = (0, 0.0, 0, 0);
    let (mut contacts, mut agreed, mut one_side, mut false_contacts) = (0, 0, 0, 0);
    let (mut model, mut dead) = (Vec::new(), Vec::new());
    let mut cost = String::new();
    for (race, (report, bots)) in races.iter().enumerate() {
        // Race completion and agreement on the results.
        let completed = bots.iter().all(|b| b.state == SessionState::Over);
        let mut disagreements = 0;
        for b in bots {
            for r in &report.results {
                let Some((_, seen)) = b.racers.iter().find(|(s, _)| *s == r.slot) else {
                    disagreements += 1;
                    continue;
                };
                let finish = match seen.status {
                    Status::Finished(t) => Some(t),
                    _ => None,
                };
                if seen.position != r.position
                    || seen.timing.laps.len() as u32 != r.laps
                    || finish != r.finish
                {
                    disagreements += 1;
                }
            }
        }
        writeln!(
            out,
            "## Race {}\n\nCompleted on the server ({:.0} s) and every client: {}. Finishing order, \
             lap counts and times on every client equal to the server's: {}\n",
            race + 1,
            report.ticks as f32 / 240.0,
            if completed { "yes" } else { "NO" },
            if disagreements == 0 {
                "yes".to_string()
            } else {
                format!("NO ({disagreements} rows differ)")
            }
        )
        .ok();
        if !completed {
            let states: Vec<String> = bots.iter().map(|b| format!("{:?}", b.state)).collect();
            fail(out, format!("race {} not completed: {states:?}", race + 1));
        }
        if disagreements > 0 {
            fail(
                out,
                format!("race {}: {disagreements} result rows differ", race + 1),
            );
        }
        writeln!(
            out,
            "| Pos | Car | Laps | Time | Resets |\n|---|---|---|---|---|"
        )
        .ok();
        for r in &report.results {
            writeln!(
                out,
                "| {} | {} | {} | {} | {} |",
                r.position,
                r.name,
                r.laps,
                r.finish.map_or("DNF".into(), |t| format!("{t:.3}")),
                r.resets
            )
            .ok();
        }

        // Corrections close to contacts and resets are expected; the rest
        // measure the prediction.
        let resets: Vec<(u8, u32)> = report
            .results
            .iter()
            .flat_map(|r| {
                let seen = bots
                    .iter()
                    .find_map(|b| b.racers.iter().find(|(s, _)| *s == r.slot).map(|(_, x)| x));
                seen.into_iter().flat_map(move |x| {
                    x.resets.iter().map(move |reset| {
                        let tick = report.start_tick as f32 + reset.time * 240.0;
                        (r.slot, tick.max(0.0) as u32)
                    })
                })
            })
            .collect();
        let near_event = |slot: u8, tick: u32| {
            report
                .contacts
                .iter()
                .any(|c| (c.a == slot || c.b == slot) && c.tick.abs_diff(tick) < EVENT_WINDOW)
                || resets
                    .iter()
                    .any(|&(s, t)| s == slot && t.abs_diff(tick) < EVENT_WINDOW)
        };
        writeln!(
            out,
            "\n| Bot | Snapshots | Corrections | near contacts/resets | Position p50 / p95 / max (m) | \
             Rotation p95 / max (deg) | Displayed max (m) | Desyncs > {DESYNC} m |\n\
             |---|---|---|---|---|---|---|---|"
        )
        .ok();
        for (i, b) in bots.iter().enumerate() {
            let slot = b.slot.unwrap_or(u8::MAX);
            let (mut pos, mut rot) = (Vec::new(), Vec::new());
            let (mut near, mut bot_desyncs, mut displayed) = (0, 0, 0.0f32);
            for x in &b.stats.corrections {
                if near_event(slot, x.tick) {
                    near += 1;
                    near_pos.push(x.position);
                    near_shown.push(x.displayed);
                    continue;
                }
                pos.push(x.position);
                rot.push(x.rotation.to_degrees());
                displayed = displayed.max(x.displayed);
                if x.position > DESYNC {
                    bot_desyncs += 1;
                }
            }
            desyncs += bot_desyncs;
            corrections += b.stats.corrections.len();
            snapshots += b.stats.snapshots as usize;
            all_pos.extend(pos.iter().copied());
            all_rot.extend(rot.iter().copied());
            writeln!(
                out,
                "| {i} | {} | {} | {near} | {:.3} / {:.3} / {:.3} | {:.2} / {:.2} | {displayed:.2} | {bot_desyncs} |",
                b.stats.snapshots,
                b.stats.corrections.len(),
                percentile(&mut pos, 0.5),
                percentile(&mut pos, 0.95),
                pos.last().copied().unwrap_or(0.0),
                percentile(&mut rot, 0.95),
                rot.last().copied().unwrap_or(0.0),
            )
            .ok();
        }
        minutes += bots.len() as f32 * report.ticks as f32 / 240.0 / 60.0;

        // Contacts: each one the server had, predicted by both clients?
        let predicted = |slot: u8, other: u8, tick: u32, at: Vec3| {
            bots.iter().find(|b| b.slot == Some(slot)).is_some_and(|b| {
                b.stats.contacts.iter().any(|&(t, s, p)| {
                    s == other
                        && t.abs_diff(tick) <= CONTACT_TICKS
                        && p.distance(at) < CONTACT_DISTANCE
                })
            })
        };
        for x in &report.contacts {
            let a = predicted(x.a, x.b, x.tick, x.position);
            let b = predicted(x.b, x.a, x.tick, x.position);
            contacts += 1;
            if a && b {
                agreed += 1;
            } else if a || b {
                one_side += 1;
            }
        }
        for b in bots {
            let slot = b.slot.unwrap_or(u8::MAX);
            false_contacts += b
                .stats
                .contacts
                .iter()
                .filter(|&&(t, other, _)| {
                    !report.contacts.iter().any(|x| {
                        ((x.a == slot && x.b == other) || (x.b == slot && x.a == other))
                            && x.tick.abs_diff(t) <= CONTACT_TICKS
                    })
                })
                .count();
            model.extend(b.stats.remote_errors.iter().map(|e| e.0));
            dead.extend(b.stats.remote_errors.iter().map(|e| e.1));
        }

        // Network and cost.
        let mut rtt: Vec<f32> = bots
            .iter()
            .flat_map(|b| b.stats.rtt_ms.iter().copied())
            .collect();
        let mut lead: Vec<f32> = bots
            .iter()
            .flat_map(|b| b.stats.input_lead.iter().map(|&l| l as f32))
            .collect();
        let resim_ticks: u64 = bots.iter().map(|b| b.stats.resim_ticks).sum();
        let resim: f64 = bots.iter().map(|b| b.stats.resim_time.as_secs_f64()).sum();
        let wall: f64 = bots.iter().map(|b| b.wall.as_secs_f64()).sum();
        let fps: f64 = bots
            .iter()
            .map(|b| b.frames as f64 / b.wall.as_secs_f64().max(1e-9))
            .fold(f64::MAX, f64::min);
        let missing: u32 = report.players.iter().map(|p| p.missing_inputs).sum();
        let late: u32 = report.players.iter().map(|p| p.late_inputs).sum();
        let ticks: u32 = report.players.iter().map(|p| p.ticks).sum();
        writeln!(
            cost,
            "- race {}: RTT median {:.0} ms; input lead median {:.0} ticks; {} ticks resimulated \
             (all bots) at {:.0} us each, {:.2} % of the bots' wall time; slowest bot {fps:.1} fps; \
             {missing} of {ticks} player ticks driven with a repeated input ({late} of those inputs \
             came late); server tick {:.2} ms mean, {:.1} ms max",
            race + 1,
            percentile(&mut rtt, 0.5),
            percentile(&mut lead, 0.5),
            resim_ticks,
            resim * 1e6 / (resim_ticks.max(1) as f64),
            100.0 * resim / wall.max(1e-9),
            report.tick_ms_mean,
            report.tick_ms_max,
        )
        .ok();
        writeln!(out).ok();
    }

    writeln!(out, "## Summary\n").ok();
    let p95 = percentile(&mut all_pos, 0.95);
    let max = all_pos.last().copied().unwrap_or(0.0);
    let r95 = percentile(&mut all_rot, 0.95);
    let rate = desyncs as f32 / minutes.max(1e-3);
    writeln!(
        out,
        "Prediction corrections outside contacts and resets: position p95 {p95:.3} m (limit {}), \
         max {max:.3} m (limit {}); rotation p95 {r95:.2} deg (limit {}); desyncs (> {DESYNC} m) \
         {rate:.2} per car-minute (limit {}). Snapshots needing a correction: {corrections} of \
         {snapshots} ({:.2} %).\n",
        t.position_p95,
        t.position_max,
        t.rotation_p95,
        t.desyncs_per_minute,
        100.0 * corrections as f32 / snapshots.max(1) as f32
    )
    .ok();
    if p95 > t.position_p95 {
        fail(out, format!("position p95 {p95:.3} m"));
    }
    if max > t.position_max {
        fail(out, format!("position max {max:.3} m"));
    }
    if r95 > t.rotation_p95 {
        fail(out, format!("rotation p95 {r95:.2} deg"));
    }
    if rate > t.desyncs_per_minute {
        fail(out, format!("{rate:.2} desyncs per car-minute"));
    }
    writeln!(
        out,
        "Corrections within 1 s of a contact or reset (expected: the server resolves contacts, \
         resets teleport): {}, position p95 {:.2} m, max {:.2} m; displayed jump p95 {:.2} m, \
         max {:.2} m.\n",
        near_pos.len(),
        percentile(&mut near_pos, 0.95),
        near_pos.last().copied().unwrap_or(0.0),
        percentile(&mut near_shown, 0.95),
        near_shown.last().copied().unwrap_or(0.0),
    )
    .ok();
    let share = if contacts == 0 {
        1.0
    } else {
        agreed as f32 / contacts as f32
    };
    writeln!(
        out,
        "Contacts: the server had {contacts}; both cars' clients predicted {agreed} within {} ms \
         and {CONTACT_DISTANCE} m ({:.0} %, limit {:.0} %), one of them {one_side}; contacts \
         predicted that the server did not have: {false_contacts}.\n",
        CONTACT_TICKS * 1000 / 240,
        share * 100.0,
        t.contact_agreement * 100.0
    )
    .ok();
    if share < t.contact_agreement {
        fail(out, format!("contact agreement {:.0} %", share * 100.0));
    }
    writeln!(
        out,
        "Remote cars, shown position against the server's at the same tick before smoothing \
         ({} samples): extrapolated with their inputs p50 {:.2} m, p95 {:.2} m, max {:.2} m; \
         dead reckoning would give p50 {:.2} m, p95 {:.2} m, max {:.2} m.\n",
        model.len(),
        percentile(&mut model, 0.5),
        percentile(&mut model, 0.95),
        model.last().copied().unwrap_or(0.0),
        percentile(&mut dead, 0.5),
        percentile(&mut dead, 0.95),
        dead.last().copied().unwrap_or(0.0),
    )
    .ok();
    writeln!(out, "Network and cost:\n\n{cost}").ok();
    writeln!(out, "Result: {}", if ok { "PASS" } else { "FAIL" }).ok();
    ok
}

/// Wall time of one simulation tick (us) of an 8-car race world: with every
/// car simulated (the server, and what a rollback of the whole world would
/// replay), and with one car simulated and seven kinematic (what the client
/// replays). Single-threaded, like the server and the bots.
pub fn tick_cost(track: Arc<Track>) -> Result<(f64, f64), String> {
    use avian3d::prelude::{CollisionLayers, RigidBody};
    use racer_sim::{
        ai::AiDriver,
        race::{DriverKind, RaceConfig},
    };

    let entries: Vec<(&str, DriverKind)> = (0..8)
        .map(|i| (CARS[i % CARS.len()], DriverKind::Ai(Difficulty::ALL[i % 5])))
        .collect();
    let mut sim = crate::race::RaceSim::new(track, RaceConfig::default(), &entries)?;
    while sim.race().time < 15.0 {
        sim.step();
    }
    let time = |sim: &mut crate::race::RaceSim| {
        let ticks = 2400;
        let started = Instant::now();
        for _ in 0..ticks {
            sim.step();
        }
        started.elapsed().as_secs_f64() * 1e6 / f64::from(ticks)
    };
    let full = time(&mut sim);
    for &car in &sim.cars[1..] {
        sim.app
            .world_mut()
            .entity_mut(car)
            .insert(RigidBody::Kinematic)
            .remove::<AiDriver>();
    }
    // As on a client: the kinematic cars only collide with cars.
    for &body in &sim.bodies[1..] {
        sim.app
            .world_mut()
            .entity_mut(body)
            .insert(CollisionLayers::new(
                racer_sim::track::Layer::Car,
                racer_sim::track::Layer::Car,
            ));
    }
    let local = time(&mut sim);
    if std::env::var_os("RACER_COST_DEBUG").is_some() {
        for &car in &sim.cars[1..] {
            sim.app.world_mut().entity_mut(car).despawn();
        }
        eprintln!("one car alone: {:.0} us", time(&mut sim));
    }
    Ok((full, local))
}

/// The whole online path with the backend: a lobby created and joined by
/// two device keys, both ready, the host starts it, the backend's local
/// fleet starts a server, and two bots connect with the lobby's key and
/// race `laps` laps (AI fill on). Returns a short report.
pub fn lobby_check(backend: &Path, laps: u32, track: Arc<Track>) -> Result<String, String> {
    use serde_json::{Value, json};

    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .map_err(|e| e.to_string())?
        .port();
    let base = format!("http://127.0.0.1:{port}");
    let _backend = Server(
        Command::new(backend)
            .env("RACER_BACKEND_ADDR", format!("127.0.0.1:{port}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("{}: {e}", backend.display()))?,
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .into();
    let call = |key: &str, method: &str, path: &str, body: Value| -> Result<Value, String> {
        let url = format!("{base}{path}");
        let mut response = if method == "GET" {
            agent.get(&url).header("x-device-key", key).call()
        } else {
            agent.post(&url).header("x-device-key", key).send_json(body)
        }
        .map_err(|e| format!("{method} {path}: {e}"))?;
        response
            .body_mut()
            .read_json()
            .map_err(|e| format!("{method} {path}: {e}"))
    };
    let (host, guest) = ("a".repeat(64), "b".repeat(64));
    // The backend takes a moment to listen.
    let mut lobby = Value::Null;
    for _ in 0..50 {
        match call(
            &host,
            "POST",
            "/v1/lobbies",
            json!({ "laps": laps, "ai_fill": true }),
        ) {
            Ok(l) => {
                lobby = l;
                break;
            }
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
    let code = lobby["code"]
        .as_str()
        .ok_or("no lobby from the backend")?
        .to_string();
    let joined = call(
        &guest,
        "POST",
        &format!("/v1/lobbies/{code}/join"),
        json!({}),
    )?;
    call(
        &guest,
        "POST",
        &format!("/v1/lobbies/{code}/ready"),
        json!({ "ready": true }),
    )?;
    let started = call(
        &host,
        "POST",
        &format!("/v1/lobbies/{code}/start"),
        json!({}),
    )?;
    let server = &started["server"];
    let addr: SocketAddr = server["addr"]
        .as_str()
        .and_then(|a| a.parse().ok())
        .ok_or(format!("no server in {started}"))?;
    let hex = server["key"].as_str().ok_or("no key")?;
    let mut key = [0u8; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(2 * i..2 * i + 2).ok_or("short key")?, 16)
            .map_err(|e| e.to_string())?;
    }
    let ids = [started["you"].as_u64(), joined["you"].as_u64()];
    let deadline = Instant::now() + Duration::from_secs(120 + 150 * u64::from(laps));
    let bots: Vec<_> = ids
        .iter()
        .enumerate()
        .map(|(i, id)| {
            let track = track.clone();
            let join = Join {
                name: format!("lobby{i}"),
                car: CARS[i].into(),
                assists: Assists::default(),
            };
            let mut config = NetClientConfig::direct(addr, id.unwrap_or(i as u64), join);
            config.auth.key = key;
            config.autopilot = Some(Difficulty::Pro);
            std::thread::spawn(move || bot(config, track, deadline))
        })
        .collect();
    let results: Vec<BotResult> = bots
        .into_iter()
        .map(|b| b.join().map_err(|_| "a bot thread panicked".to_string()))
        .collect::<Result<_, _>>()?;
    let mut out = format!("lobby {code}: server {addr}\n");
    for (i, b) in results.iter().enumerate() {
        writeln!(
            out,
            "bot {i}: slot {:?}, {:?}, {} cars in the results",
            b.slot,
            b.state,
            b.racers.len()
        )
        .ok();
        if b.state != SessionState::Over {
            return Err(format!("{out}bot {i} did not finish the race"));
        }
    }
    Ok(out)
}
