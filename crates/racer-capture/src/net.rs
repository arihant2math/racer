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
    out_dir: &Path,
    track: Arc<Track>,
) -> Result<(ServerReport, Vec<BotResult>), String> {
    let addr = free_port()?;
    let report_path = out_dir.join("server.json");
    std::fs::remove_file(&report_path).ok();
    let c = &check.conditions;
    let log = std::fs::File::create(out_dir.join("server.log")).map_err(|e| e.to_string())?;
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
    let difficulties = [
        Difficulty::Alien,
        Difficulty::Expert,
        Difficulty::Pro,
        Difficulty::Amateur,
        Difficulty::Novice,
    ];
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
            config.autopilot = Some(difficulties[i % difficulties.len()]);
            std::thread::spawn(move || bot(config, track, deadline))
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
        .map_err(|e| format!("no server report ({e}); see {}", out_dir.join("server.log").display()))?;
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

/// Writes the analysis to `out`; returns whether every check passed.
pub fn analyse(
    check: &NetCheck,
    report: &ServerReport,
    bots: &[BotResult],
    out: &mut String,
) -> bool {
    let t = thresholds(&check.conditions);
    let c = &check.conditions;
    let mut ok = true;
    let mut fail = |out: &mut String, what: String| {
        ok = false;
        writeln!(out, "FAIL {what}").ok();
    };
    writeln!(
        out,
        "# Net check: {} bots, {} laps, {} ms RTT, {} ms jitter, {:.1} % loss\n",
        check.bots,
        check.laps,
        c.rtt_ms,
        c.jitter_ms,
        c.loss * 100.0
    )
    .ok();

    // Race completion and agreement on the results.
    let completed = bots.iter().all(|b| b.state == SessionState::Over);
    writeln!(
        out,
        "Race completed on the server ({} ticks) and every client: {}",
        report.ticks,
        if completed { "yes" } else { "NO" }
    )
    .ok();
    if !completed {
        let states: Vec<String> = bots.iter().map(|b| format!("{:?}", b.state)).collect();
        fail(out, format!("race not completed: {states:?}"));
    }
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
        "Finishing order, lap counts and times on every client equal to the server's: {}\n",
        if disagreements == 0 {
            "yes".to_string()
        } else {
            format!("NO ({disagreements} rows differ)")
        }
    )
    .ok();
    if disagreements > 0 {
        fail(out, format!("{disagreements} result rows differ"));
    }
    writeln!(out, "| Pos | Car | Laps | Time | Resets |\n|---|---|---|---|---|").ok();
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

    // Events that explain corrections: contacts and resets, by slot.
    let resets: Vec<(u8, u32)> = bots
        .iter()
        .flat_map(|b| &b.racers)
        .filter(|(s, _)| report.results.iter().any(|r| r.slot == *s))
        .flat_map(|(s, r)| {
            r.resets.iter().map(move |x| {
                let tick = report.start_tick as f32 + x.time * 240.0;
                (*s, tick.max(0.0) as u32)
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

    // Corrections.
    writeln!(
        out,
        "\n## Prediction corrections (local car, error at the corrected tick)\n\n\
         | Bot | Snapshots | Corrections | of which near contacts/resets | Position p50 / p95 / max (m) | \
         Rotation p95 / max (deg) | Displayed max (m) | Desyncs > {DESYNC} m | Unmatched |\n\
         |---|---|---|---|---|---|---|---|---|"
    )
    .ok();
    let minutes = report.ticks as f32 / 240.0 / 60.0;
    let (mut all_pos, mut all_rot) = (Vec::new(), Vec::new());
    let mut desyncs = 0;
    for (i, b) in bots.iter().enumerate() {
        let slot = b.slot.unwrap_or(u8::MAX);
        let (mut pos, mut rot) = (Vec::new(), Vec::new());
        let mut near = 0;
        let mut bot_desyncs = 0;
        for x in &b.stats.corrections {
            if near_event(slot, x.tick) {
                near += 1;
                continue;
            }
            pos.push(x.position);
            rot.push(x.rotation.to_degrees());
            if x.position > DESYNC {
                bot_desyncs += 1;
            }
        }
        desyncs += bot_desyncs;
        let displayed = b
            .stats
            .corrections
            .iter()
            .map(|x| x.displayed)
            .fold(0.0, f32::max);
        all_pos.extend(pos.iter().copied());
        all_rot.extend(rot.iter().copied());
        let max = pos.iter().copied().fold(0.0, f32::max);
        let rmax = rot.iter().copied().fold(0.0, f32::max);
        writeln!(
            out,
            "| {i} | {} | {} | {near} | {:.3} / {:.3} / {:.3} | {:.2} / {:.2} | {:.2} | {bot_desyncs} | {} |",
            b.stats.snapshots,
            b.stats.corrections.len(),
            percentile(&mut pos.clone(), 0.5),
            percentile(&mut pos.clone(), 0.95),
            max,
            percentile(&mut rot.clone(), 0.95),
            rmax,
            displayed,
            b.stats.unmatched,
        )
        .ok();
    }
    let p95 = percentile(&mut all_pos.clone(), 0.95);
    let max = all_pos.iter().copied().fold(0.0, f32::max);
    let r95 = percentile(&mut all_rot.clone(), 0.95);
    let rate = desyncs as f32 / bots.len().max(1) as f32 / minutes.max(1e-3);
    writeln!(
        out,
        "\nAll bots outside contacts and resets: position p95 {p95:.3} m (limit {}), max {max:.3} m \
         (limit {}), rotation p95 {r95:.2} deg (limit {}), desyncs {rate:.2} per car-minute (limit {})",
        t.position_p95, t.position_max, t.rotation_p95, t.desyncs_per_minute
    )
    .ok();
    let total: usize = bots.iter().map(|b| b.stats.corrections.len()).sum();
    let snaps: u32 = bots.iter().map(|b| b.stats.snapshots).sum();
    writeln!(
        out,
        "Snapshots needing a correction: {total} of {snaps} ({:.1} %)",
        100.0 * total as f32 / snaps.max(1) as f32
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

    // Contacts.
    let predicted = |slot: u8, other: u8, tick: u32, at: Vec3| {
        bots.iter()
            .find(|b| b.slot == Some(slot))
            .is_some_and(|b| {
                b.stats.contacts.iter().any(|&(t, s, p)| {
                    s == other && t.abs_diff(tick) <= CONTACT_TICKS && p.distance(at) < CONTACT_DISTANCE
                })
            })
    };
    let mut agreed = 0;
    let mut one_side = 0;
    for x in &report.contacts {
        let a = predicted(x.a, x.b, x.tick, x.position);
        let b = predicted(x.b, x.a, x.tick, x.position);
        if a && b {
            agreed += 1;
        } else if a || b {
            one_side += 1;
        }
    }
    let n = report.contacts.len();
    let share = if n == 0 { 1.0 } else { agreed as f32 / n as f32 };
    let false_contacts: usize = bots
        .iter()
        .map(|b| {
            let slot = b.slot.unwrap_or(u8::MAX);
            b.stats
                .contacts
                .iter()
                .filter(|&&(t, other, _)| {
                    !report.contacts.iter().any(|x| {
                        ((x.a == slot && x.b == other) || (x.b == slot && x.a == other))
                            && x.tick.abs_diff(t) <= CONTACT_TICKS
                    })
                })
                .count()
        })
        .sum();
    writeln!(
        out,
        "\n## Contacts\n\nServer car-car contacts: {n}; predicted by both cars' clients within \
         {} ms and {CONTACT_DISTANCE} m: {agreed} ({:.0} %, limit {:.0} %); by one of them: {one_side}; \
         predicted contacts the server did not have: {false_contacts}",
        CONTACT_TICKS * 1000 / 240,
        share * 100.0,
        t.contact_agreement * 100.0
    )
    .ok();
    if share < t.contact_agreement {
        fail(out, format!("contact agreement {:.0} %", share * 100.0));
    }

    // Network and cost.
    writeln!(out, "\n## Network and cost\n").ok();
    for (i, b) in bots.iter().enumerate() {
        let mut rtt = b.stats.rtt_ms.clone();
        let mut lead: Vec<f32> = b.stats.input_lead.iter().map(|&l| l as f32).collect();
        let per_tick = if b.stats.resim_ticks > 0 {
            b.stats.resim_time.as_secs_f64() * 1e6 / b.stats.resim_ticks as f64
        } else {
            0.0
        };
        writeln!(
            out,
            "- bot {i}: RTT median {:.0} ms, input lead median {:.0} ticks (min {:.0}), \
             {} ticks resimulated at {per_tick:.0} us each ({:.2} % of wall time), {:.1} fps",
            percentile(&mut rtt, 0.5),
            percentile(&mut lead, 0.5),
            lead.first().copied().unwrap_or(0.0),
            b.stats.resim_ticks,
            100.0 * b.stats.resim_time.as_secs_f64() / b.wall.as_secs_f64().max(1e-9),
            b.frames as f64 / b.wall.as_secs_f64().max(1e-9),
        )
        .ok();
    }
    for p in &report.players {
        writeln!(
            out,
            "- server, {}: {} of {} ticks driven with a repeated input (late or lost)",
            p.name, p.missing_inputs, p.ticks
        )
        .ok();
    }
    writeln!(
        out,
        "- server tick: {:.2} ms mean, {:.2} ms max",
        report.tick_ms_mean, report.tick_ms_max
    )
    .ok();
    writeln!(out, "\nResult: {}", if ok { "PASS" } else { "FAIL" }).ok();
    ok
}
