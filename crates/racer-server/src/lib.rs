//! Headless dedicated server: one race, authoritative, without rendering,
//! windowing or audio.
//!
//! Lobby (players connect and join) → countdown → race → results →
//! shutdown. The server runs the race rules, Avian and `racer-dynamics` for
//! every car at 240 Hz, drives empty grid slots (and cars whose player left)
//! with the AI, applies each player's inputs at the tick they were made for,
//! and sends a snapshot of every car at 60 Hz.

use std::{
    collections::{BTreeMap, BTreeSet},
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::Instant,
};

use avian3d::prelude::*;
use bevy::{
    app::{ScheduleRunnerPlugin, TaskPoolOptions},
    log::LogPlugin,
    prelude::*,
};
use lightyear::prelude::{server::*, *};
use racer_dynamics::{load_car, spec::CARS};
use racer_sim::{
    SimPlugin,
    ai::{AiDriver, CarParams, Difficulty},
    net::{
        protocol::*,
        report::{Contact, PlayerRecord, ResultRecord, ServerReport},
    },
    race::{
        DriverKind, Race, RaceConfig, RacePhase, RacePlugin, RaceTrack, Racer, ResetRequest,
        spawn_racer,
    },
    rules::Status,
    track::{city_track, spawn_track},
    vehicle::{CarBody, CarControls, CarState, ControlSystems, VehiclePlugin},
};

/// Server configuration.
#[derive(Clone, Debug)]
pub struct ServerConfig {
    /// Exit after this many simulation steps. Runs until the race is over
    /// when `None`.
    pub max_ticks: Option<u64>,
    /// UDP address to listen on (port 0 picks a free port; the server logs
    /// and prints the address it got).
    pub bind: SocketAddr,
    /// Players to wait for before the countdown. With none, the race starts
    /// at once with AI cars.
    pub players: usize,
    /// Seconds to wait for missing players before starting without them.
    pub lobby_timeout: f32,
    pub race: RaceConfig,
    /// Cars on the grid; slots no player takes get AI drivers.
    pub grid: usize,
    pub ai_fill: bool,
    /// AI difficulty, or a mix of all five.
    pub difficulty: Option<Difficulty>,
    /// netcode.io private key that connect tokens are signed with.
    pub key: [u8; 32],
    /// Simulated network conditions on the client-to-server paths (tests).
    pub conditions: Conditions,
    /// Where to write the [`ServerReport`] after the race.
    pub report: Option<PathBuf>,
    /// Seconds the results stay up (and the server running) after the race.
    pub linger: f32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            max_ticks: None,
            bind: "127.0.0.1:0".parse().expect("a socket address"),
            players: 0,
            lobby_timeout: 60.0,
            race: RaceConfig::default(),
            grid: 8,
            ai_fill: true,
            difficulty: None,
            key: DEV_KEY,
            conditions: Conditions::default(),
            report: None,
            linger: 5.0,
        }
    }
}

#[derive(Resource, Clone)]
struct Config(ServerConfig);

#[derive(Resource)]
struct MaxTicks(u64);

/// Fixed steps completed since startup.
#[derive(Resource, Default, Debug)]
pub struct TickCount(pub u64);

/// Where the server is in its one race.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub enum Phase {
    Lobby,
    /// The grid is spawned at the next fixed step.
    Starting,
    Racing,
    /// Results sent at this real time; exits after the linger.
    Over(f32),
}

/// A connected player.
#[derive(Clone, Debug)]
struct Player {
    client: Entity,
    join: Join,
    car: Option<Entity>,
    slot: Option<u8>,
    disconnected: bool,
}

#[derive(Resource, Default)]
struct Players(Vec<Player>);

/// A car driven by a player's inputs.
#[derive(Component)]
struct NetDriver {
    client: Entity,
    inputs: BTreeMap<u32, NetControls>,
    last: NetControls,
    newest: u32,
    missing: u32,
    ticks: u32,
    /// Recent ticks driven without the player's input, and how late (ticks)
    /// the inputs for them came after all.
    missed: BTreeSet<u32>,
    late: Vec<u32>,
    applied: u32,
}

/// What every car was driven with this tick (sent with the snapshots).
#[derive(Component, Default)]
struct Applied(NetControls);

#[derive(Resource, Default)]
struct RaceLog {
    created: u32,
    start: u32,
    contacts: Vec<Contact>,
    touching: Vec<(u8, u8)>,
    tick_time: Vec<f32>,
    tick_started: Option<Instant>,
}

/// Builds the headless server app.
pub fn build_app(config: &ServerConfig) -> App {
    let mut app = App::new();
    // Single-threaded (see racer_sim::headless_app): one race's systems are
    // too small to gain from threads. Finish the app with
    // racer_sim::finish_headless.
    app.add_plugins((
        MinimalPlugins
            .set(ScheduleRunnerPlugin::run_loop(racer_sim::tick_duration()))
            .set(TaskPoolPlugin {
                task_pool_options: TaskPoolOptions::with_num_threads(1),
            }),
        LogPlugin::default(),
        SimPlugin,
        ServerPlugins {
            tick_duration: racer_sim::tick_duration(),
        },
    ))
    .add_plugins((ProtocolPlugin, VehiclePlugin, RacePlugin))
    .insert_resource(Config(config.clone()))
    .insert_resource(Phase::Lobby)
    .init_resource::<Players>()
    .init_resource::<RaceLog>()
    .init_resource::<TickCount>()
    .add_observer(on_connect)
    .add_systems(Startup, (load_track, listen))
    .add_systems(
        PreUpdate,
        (
            receive_joins,
            receive_inputs,
            receive_resets,
            track_disconnects,
        )
            .after(MessageSystems::ReceiveMessages),
    )
    .add_systems(Update, (announce_address, lobby, finish))
    .add_systems(
        FixedPreUpdate,
        start_race.run_if(resource_equals(Phase::Starting)),
    )
    .add_systems(FixedUpdate, count_ticks)
    .add_systems(FixedFirst, tick_started)
    .add_systems(FixedLast, tick_ended)
    .add_systems(FixedUpdate, apply_inputs.in_set(ControlSystems))
    .add_systems(
        FixedUpdate,
        check_start
            .after(racer_sim::race::RaceSystems::Rules)
            .run_if(resource_exists::<Race>),
    )
    .add_systems(
        FixedPostUpdate,
        (record_applied, log_contacts, send_snapshots, send_standings)
            .chain()
            .after(PhysicsSystems::Last)
            .run_if(resource_equals(Phase::Racing)),
    );
    if let Some(max) = config.max_ticks {
        // Also checked at startup, before the first fixed step, so that
        // `--max-ticks 0` runs none.
        app.insert_resource(MaxTicks(max))
            .add_systems(Startup, exit_after_max_ticks)
            .add_systems(FixedUpdate, exit_after_max_ticks.after(count_ticks));
    }
    app
}

fn count_ticks(mut ticks: ResMut<TickCount>) {
    ticks.0 += 1;
}

fn tick_started(mut log: ResMut<RaceLog>) {
    log.tick_started = Some(Instant::now());
}

/// Wall time of each fixed step while racing (simulation cost).
fn tick_ended(phase: Res<Phase>, mut log: ResMut<RaceLog>) {
    if let (Phase::Racing, Some(t)) = (*phase, log.tick_started.take()) {
        log.tick_time.push(t.elapsed().as_secs_f32() * 1000.0);
    }
}

fn exit_after_max_ticks(
    ticks: Res<TickCount>,
    max: Res<MaxTicks>,
    mut exit: MessageWriter<AppExit>,
    mut done: Local<bool>,
) {
    // Checked every fixed step (one frame can run several) so the server
    // stops exactly at the limit; later steps in the same frame are ignored.
    if !*done && ticks.0 >= max.0 {
        *done = true;
        info!("reached {} ticks, shutting down", ticks.0);
        exit.write(AppExit::Success);
    }
}

fn load_track(mut commands: Commands, mut exit: MessageWriter<AppExit>) {
    match racer_trackc::load("downtown-loop") {
        Ok(track) => {
            spawn_track(&mut commands, &city_track(&track.geometry));
            commands.insert_resource(RaceTrack(Arc::new(track)));
        }
        Err(e) => {
            error!("cannot load the track: {e}");
            exit.write(AppExit::error());
        }
    }
}

fn listen(mut commands: Commands, config: Res<Config>) {
    let c = &config.0;
    let netcode = NetcodeConfig::default()
        .with_protocol_id(PROTOCOL_ID)
        .with_key(c.key)
        .with_max_clients(c.grid.max(1));
    let netcode = NetcodeConfig {
        // Tokens name the address clients dial, which behind NAT or with a
        // wildcard bind is not the local one.
        server_addr_check: false,
        ..netcode
    };
    let server = commands
        .spawn((
            Name::new("server"),
            NetcodeServer::new(netcode),
            LocalAddr(c.bind),
            ServerUdpIo::default(),
        ))
        .id();
    commands.trigger(Start { entity: server });
}

/// Prints the bound address once (the backend's fleet reads it).
fn announce_address(
    servers: Query<&LocalAddr, (With<NetcodeServer>, With<Started>)>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    if let Some(addr) = servers.iter().next() {
        *done = true;
        info!("listening on {}", addr.0);
        println!("racer-server listening on {}", addr.0);
    }
}

/// New connections get simulated network conditions (tests).
fn on_connect(trigger: On<Add, LinkOf>, config: Res<Config>, mut links: Query<&mut Link>) {
    if let (Some(c), Ok(mut link)) = (
        config.0.conditions.conditioner(),
        links.get_mut(trigger.entity),
    ) {
        link.recv.conditioner = Some(c);
    }
}

fn receive_joins(
    phase: Res<Phase>,
    config: Res<Config>,
    mut players: ResMut<Players>,
    mut joins: Query<(Entity, &mut MessageReceiver<Join>)>,
) {
    for (client, mut receiver) in &mut joins {
        for join in receiver.receive() {
            let known = players.0.iter().any(|p| p.client == client);
            if known || *phase != Phase::Lobby || players.0.len() >= config.0.grid {
                warn!("join from {client} ignored");
                continue;
            }
            info!("{} joined", join.name);
            players.0.push(Player {
                client,
                join,
                car: None,
                slot: None,
                disconnected: false,
            });
        }
    }
}

fn receive_inputs(
    players: Res<Players>,
    mut receivers: Query<(Entity, &mut MessageReceiver<Inputs>)>,
    mut drivers: Query<&mut NetDriver>,
) {
    for (client, mut receiver) in &mut receivers {
        let car = players
            .0
            .iter()
            .find(|p| p.client == client)
            .and_then(|p| p.car);
        let Some(mut driver) = car.and_then(|c| drivers.get_mut(c).ok()) else {
            receiver.receive().for_each(drop);
            continue;
        };
        for inputs in receiver.receive() {
            let n = inputs.controls.len() as u32;
            for (i, c) in inputs.controls.into_iter().enumerate() {
                let tick = (inputs.last + 1 + i as u32).saturating_sub(n);
                if driver.missed.remove(&tick) {
                    let late = driver.applied - tick;
                    driver.late.push(late);
                } else if tick > driver.applied {
                    driver.inputs.entry(tick).or_insert(c);
                }
            }
            driver.newest = driver.newest.max(inputs.last);
        }
    }
}

fn receive_resets(
    mut commands: Commands,
    players: Res<Players>,
    race: Option<Res<Race>>,
    mut receivers: Query<(Entity, &mut MessageReceiver<RequestReset>)>,
) {
    let running = race.is_some_and(|r| r.phase == RacePhase::Running);
    for (client, mut receiver) in &mut receivers {
        let asked = receiver.receive().count() > 0;
        let car = players
            .0
            .iter()
            .find(|p| p.client == client)
            .and_then(|p| p.car);
        if let (true, true, Some(car)) = (asked, running, car) {
            commands.entity(car).insert(ResetRequest);
        }
    }
}

/// A player who leaves: the AI takes the car over.
fn track_disconnects(
    mut commands: Commands,
    mut players: ResMut<Players>,
    track: Option<Res<RaceTrack>>,
    clients: Query<(), (With<Connected>, Without<Disconnected>)>,
    cars: Query<(&Racer, &racer_sim::vehicle::Car)>,
) {
    for p in players.0.iter_mut().filter(|p| !p.disconnected) {
        if clients.contains(p.client) {
            continue;
        }
        p.disconnected = true;
        info!("{} left", p.join.name);
        let (Some(car), Some(track)) = (p.car, track.as_ref()) else {
            continue;
        };
        let Ok((racer, c)) = cars.get(car) else {
            continue;
        };
        let Some(profile) = track.0.car(&c.id) else {
            continue;
        };
        let Ok(spec) = load_car(&c.id) else { continue };
        let params = CarParams::new(&spec, &c.vehicle, profile.model.clone());
        let seed = racer.slot as u64 + 1;
        commands
            .entity(car)
            .remove::<NetDriver>()
            .insert(AiDriver::new(Difficulty::Pro, params, &track.0.line, seed));
    }
}

/// Waits for the players, then starts.
fn lobby(
    time: Res<Time<Real>>,
    config: Res<Config>,
    mut phase: ResMut<Phase>,
    players: Res<Players>,
    mut senders: Query<&mut MessageSender<LobbyStatus>>,
    mut last: Local<Option<LobbyStatus>>,
    mut exit: MessageWriter<AppExit>,
) {
    if *phase != Phase::Lobby {
        return;
    }
    let c = &config.0;
    let waited = time.elapsed_secs();
    let here: Vec<&Player> = players.0.iter().filter(|p| !p.disconnected).collect();
    let status = LobbyStatus {
        players: here.iter().map(|p| p.join.name.clone()).collect(),
        expected: c.players as u32,
        timeout: Some((c.lobby_timeout - waited).max(0.0).ceil()),
    };
    if last.as_ref() != Some(&status) {
        for p in &here {
            if let Ok(mut s) = senders.get_mut(p.client) {
                s.send::<ControlChannel>(status.clone());
            }
        }
        *last = Some(status);
    }
    let all_here = here.len() >= c.players;
    let timed_out = waited >= c.lobby_timeout;
    if all_here || timed_out {
        if here.is_empty() && !(c.ai_fill && c.race.mode != racer_sim::race::RaceMode::TimeTrial) {
            info!("nobody joined, shutting down");
            exit.write(AppExit::Success);
            return;
        }
        info!("starting with {} player(s)", here.len());
        *phase = Phase::Starting;
    }
}

const NAMES: [&str; 8] = ["Ava", "Ben", "Chen", "Dana", "Eli", "Femi", "Gus", "Hana"];

/// Spawns the grid: players in the order they joined, then AI cars.
#[allow(clippy::too_many_arguments)]
fn start_race(
    mut commands: Commands,
    config: Res<Config>,
    track: Res<RaceTrack>,
    timeline: Res<LocalTimeline>,
    mut phase: ResMut<Phase>,
    mut players: ResMut<Players>,
    mut log: ResMut<RaceLog>,
    mut senders: Query<&mut MessageSender<RaceStart>>,
) {
    let c = &config.0;
    let t = &track.0;
    let slots = c.grid.min(t.geometry.grid.len());
    let mut grid = Vec::new();
    let humans: Vec<usize> = (0..players.0.len())
        .filter(|&i| !players.0[i].disconnected)
        .take(slots)
        .collect();
    for (slot, &i) in humans.iter().enumerate() {
        let p = &mut players.0[i];
        let car = if CARS.contains(&p.join.car.as_str()) {
            p.join.car.clone()
        } else {
            CARS[0].to_string()
        };
        let Ok(spec) = load_car(&car) else { continue };
        let driver = NetDriver {
            client: p.client,
            inputs: BTreeMap::new(),
            last: NetControls::default(),
            newest: 0,
            missing: 0,
            ticks: 0,
            missed: BTreeSet::new(),
            late: Vec::new(),
            applied: 0,
        };
        let name = p.join.name.clone();
        match spawn_racer(
            &mut commands,
            t,
            &spec,
            slot,
            name.clone(),
            DriverKind::Human,
            (driver, Applied::default()),
        ) {
            Ok(e) => {
                p.car = Some(e);
                p.slot = Some(slot as u8);
                let assists = p.join.assists;
                commands.queue(move |world: &mut World| {
                    if let Some(mut r) = world.get_mut::<Racer>(e) {
                        r.assists = assists;
                    }
                });
                grid.push(GridCar {
                    slot: slot as u8,
                    car,
                    name,
                    driver: DriverKind::Human,
                });
            }
            Err(e) => error!("cannot spawn {name}: {e}"),
        }
    }
    let order = [
        Difficulty::Alien,
        Difficulty::Expert,
        Difficulty::Pro,
        Difficulty::Amateur,
        Difficulty::Novice,
    ];
    // A time trial is solo: no AI.
    let ai = c.ai_fill && c.race.mode != racer_sim::race::RaceMode::TimeTrial;
    let ai_slots = if ai { humans.len()..slots } else { 0..0 };
    for (k, slot) in ai_slots.enumerate() {
        let d = c.difficulty.unwrap_or(order[k % order.len()]);
        let car = CARS[slot % CARS.len()];
        let Ok(spec) = load_car(car) else { continue };
        let name = format!("{} ({})", NAMES[slot % NAMES.len()], d.name());
        match spawn_racer(
            &mut commands,
            t,
            &spec,
            slot,
            name.clone(),
            DriverKind::Ai(d),
            Applied::default(),
        ) {
            Ok(_) => grid.push(GridCar {
                slot: slot as u8,
                car: car.to_string(),
                name,
                driver: DriverKind::Ai(d),
            }),
            Err(e) => error!("cannot spawn {name}: {e}"),
        }
    }
    let race = Race::new(&c.race);
    // This step is the race clock's first.
    let created = timeline.tick().0;
    let start = created + race.steps_to_start(racer_sim::tick_duration().as_secs_f64()) - 1;
    commands.insert_resource(race);
    commands.insert_resource(c.race.clone());
    log.created = created;
    log.start = start;
    for p in &players.0 {
        let Some(you) = p.slot else { continue };
        if let Ok(mut s) = senders.get_mut(p.client) {
            s.send::<ControlChannel>(RaceStart {
                config: c.race.clone(),
                grid: grid.clone(),
                you,
                created,
                start,
            });
        }
    }
    info!("race created at tick {created}, starts at tick {start}");
    *phase = Phase::Racing;
}

/// The clients hold their cars on the grid until the start tick they were
/// told; the race clock must agree.
fn check_start(
    timeline: Res<LocalTimeline>,
    race: Res<Race>,
    log: Res<RaceLog>,
    mut done: Local<bool>,
) {
    if !*done && race.phase != RacePhase::Countdown {
        *done = true;
        let tick = timeline.tick().0;
        if tick != log.start {
            error!(
                "race started at tick {tick}, clients were told {}",
                log.start
            );
        }
    }
}

/// Each player's input for this tick; a late or lost one repeats the last.
fn apply_inputs(timeline: Res<LocalTimeline>, mut cars: Query<(&mut NetDriver, &mut CarControls)>) {
    let tick = timeline.tick().0;
    for (mut driver, mut controls) in &mut cars {
        let c = match driver.inputs.remove(&tick) {
            Some(c) => c,
            None => {
                driver.missing += 1;
                driver.missed.insert(tick);
                driver.last.held()
            }
        };
        driver.applied = tick;
        // Inputs older than this arrive too late to tell how late.
        while driver.missed.len() > 480 {
            driver.missed.pop_first();
        }
        // Inputs for ticks already simulated can never be used.
        driver.inputs = driver.inputs.split_off(&tick);
        driver.ticks += 1;
        driver.last = c;
        controls.0 = c.decode();
    }
}

fn record_applied(mut cars: Query<(&CarControls, &mut Applied)>) {
    for (c, mut applied) in &mut cars {
        applied.0 = NetControls::encode(&c.0);
    }
}

/// Car-to-car contacts as they start.
#[allow(clippy::type_complexity)]
fn log_contacts(
    timeline: Res<LocalTimeline>,
    mut log: ResMut<RaceLog>,
    cars: Query<(&Racer, &Position, &LinearVelocity, &Children)>,
    touching: Query<&CollidingEntities, With<CarBody>>,
    parents: Query<&ChildOf, With<CarBody>>,
) {
    let tick = timeline.tick().0;
    let mut now = Vec::new();
    for (racer, pos, vel, children) in &cars {
        for other in children
            .iter()
            .filter_map(|c| touching.get(c).ok())
            .flat_map(|t| t.iter().copied().collect::<Vec<_>>())
            .filter_map(|o| parents.get(o).ok())
        {
            let Ok((r, p, v, _)) = cars.get(other.parent()) else {
                continue;
            };
            let (a, b) = (racer.slot as u8, r.slot as u8);
            if a >= b {
                continue;
            }
            now.push((a, b));
            if !log.touching.contains(&(a, b)) {
                log.contacts.push(Contact {
                    tick,
                    a,
                    b,
                    position: (pos.0 + p.0) / 2.0,
                    speed: (vel.0 - v.0).length(),
                });
            }
        }
    }
    log.touching = now;
}

#[allow(clippy::type_complexity)]
fn send_snapshots(
    timeline: Res<LocalTimeline>,
    cars: Query<(
        Entity,
        &Racer,
        &Position,
        &Rotation,
        &LinearVelocity,
        &AngularVelocity,
        &CarState,
        &Applied,
    )>,
    drivers: Query<&NetDriver>,
    players: Res<Players>,
    mut senders: Query<&mut MessageSender<Snapshot>>,
) {
    let tick = timeline.tick().0;
    if !tick.is_multiple_of(SNAPSHOT_INTERVAL) {
        return;
    }
    let all: Vec<CarSnapshot> = cars
        .iter()
        .map(|(_, r, p, q, v, w, _, applied)| CarSnapshot {
            slot: r.slot as u8,
            body: Body::read(p, q, v, w),
            controls: applied.0,
            ghosted: r.ghosted,
        })
        .collect();
    for p in players.0.iter().filter(|p| !p.disconnected) {
        let Some(car) = p.car else { continue };
        let Ok(mut sender) = senders.get_mut(p.client) else {
            continue;
        };
        let own = cars.get(car).ok().map(|c| c.6.0);
        let input_ack = drivers.get(car).map_or(0, |d| d.newest);
        sender.send::<StateChannel>(Snapshot {
            tick,
            cars: all.clone(),
            own,
            input_ack,
        });
    }
}

/// One racer's race state every few ticks, round robin.
fn send_standings(
    timeline: Res<LocalTimeline>,
    race: Res<Race>,
    cars: Query<&Racer>,
    players: Res<Players>,
    mut senders: Query<&mut MessageSender<Standings>>,
) {
    let tick = timeline.tick().0;
    let n = cars.iter().count() as u32;
    let every = (STANDINGS_INTERVAL / n.max(1)).max(1);
    if n == 0 || !tick.is_multiple_of(every) {
        return;
    }
    let slot = (tick / every) % n;
    let Some(racer) = cars.iter().find(|r| r.slot as u32 == slot) else {
        return;
    };
    let message = Standings {
        tick,
        slot: slot as u8,
        racer: racer.clone(),
        phase: race.phase,
        winner: race.winner,
    };
    for p in players.0.iter().filter(|p| !p.disconnected) {
        if let Ok(mut s) = senders.get_mut(p.client) {
            s.send::<StateChannel>(message.clone());
        }
    }
}

/// After the race: results to everyone, the report, then exit.
#[allow(clippy::too_many_arguments)]
fn finish(
    time: Res<Time<Real>>,
    config: Res<Config>,
    race: Option<Res<Race>>,
    timeline: Res<LocalTimeline>,
    mut phase: ResMut<Phase>,
    players: Res<Players>,
    log: Res<RaceLog>,
    cars: Query<(&Racer, Option<&NetDriver>)>,
    mut senders: Query<&mut MessageSender<RaceOver>>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = time.elapsed_secs();
    match *phase {
        Phase::Racing if race.is_some_and(|r| r.phase == RacePhase::Finished) => {
            let racers: Vec<(u8, Racer)> = cars
                .iter()
                .map(|(r, _)| (r.slot as u8, r.clone()))
                .collect();
            for p in players.0.iter().filter(|p| !p.disconnected) {
                if let Ok(mut s) = senders.get_mut(p.client) {
                    s.send::<ControlChannel>(RaceOver {
                        racers: racers.clone(),
                    });
                }
            }
            info!("race over");
            if let Some(path) = &config.0.report {
                let report = report(&log, timeline.tick().0, &cars, &players);
                match serde_json::to_string_pretty(&report) {
                    Ok(json) => {
                        if let Err(e) = std::fs::write(path, json) {
                            error!("cannot write {}: {e}", path.display());
                        }
                    }
                    Err(e) => error!("report: {e}"),
                }
            }
            *phase = Phase::Over(now);
        }
        Phase::Over(at) if now - at >= config.0.linger => {
            info!("shutting down");
            exit.write(AppExit::Success);
        }
        _ => {}
    }
}

fn report(
    log: &RaceLog,
    tick: u32,
    cars: &Query<(&Racer, Option<&NetDriver>)>,
    players: &Players,
) -> ServerReport {
    let mut results: Vec<ResultRecord> = cars
        .iter()
        .map(|(r, _)| ResultRecord {
            slot: r.slot as u8,
            name: r.name.clone(),
            position: r.position,
            laps: r.timing.laps.len() as u32,
            finish: match r.status {
                Status::Finished(t) => Some(t),
                _ => None,
            },
            resets: r.resets.len(),
        })
        .collect();
    results.sort_by_key(|r| r.position);
    let player_records = cars
        .iter()
        .filter_map(|(r, d)| {
            let d = d?;
            let p = players.0.iter().find(|p| p.client == d.client)?;
            Some(PlayerRecord {
                slot: r.slot as u8,
                name: r.name.clone(),
                missing_inputs: d.missing,
                late_inputs: d.late.len() as u32,
                mean_lateness: d.late.iter().sum::<u32>() as f32 / d.late.len().max(1) as f32,
                ticks: d.ticks,
                disconnected: p.disconnected,
            })
        })
        .collect();
    let n = log.tick_time.len().max(1) as f32;
    ServerReport {
        ticks: tick.saturating_sub(log.start),
        start_tick: log.start,
        contacts: log.contacts.clone(),
        results,
        players: player_records,
        tick_ms_mean: log.tick_time.iter().sum::<f32>() / n,
        tick_ms_max: log.tick_time.iter().copied().fold(0.0, f32::max),
    }
}
