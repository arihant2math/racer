//! The client side of an online race, headless (the game adds visuals and
//! the HUD; the test bots run it as is).
//!
//! - **Local car: predicted.** Every fixed tick the car's controls are
//!   quantized, kept by tick and sent; the car is simulated with them through
//!   Avian and `racer-dynamics` like offline. When a snapshot shows the
//!   server's state of the car at tick `T` differing from the prediction for
//!   `T`, the car is put back to the server's state (rigid body and vehicle
//!   model) and the ticks since are simulated again with the kept inputs.
//!   Only the local car is resimulated: the other cars are kinematic bodies.
//! - **Remote cars: extrapolated.** Kinematic bodies placed every tick on a
//!   path extrapolated from their last snapshot with the inputs the server
//!   last applied to them ([`super::extrapolate`]); the path is re-run from
//!   every new snapshot. The local car predicts its contacts against them.
//! - **Corrections are blended**: the displayed car (its `Transform`, after
//!   physics interpolation) carries the jump a correction made and lets it
//!   decay, over a few frames for the local car and 100-150 ms for remote
//!   cars. Teleports (resets) snap.
//! - **Race state is the server's**: [`Racer`] components, the race phase and
//!   the results are replaced by what the server sends; the race rules do not
//!   run here (see [`RemoteRace`]).

use std::{
    collections::{BTreeMap, VecDeque},
    net::SocketAddr,
    time::{Duration, Instant},
};

use avian3d::prelude::*;
use bevy::{app::FixedMain, prelude::*, transform::TransformSystems};
use lightyear::prelude::{client::*, *};
use racer_dynamics::{Assists, load_car};

use super::{
    extrapolate::{self, Model},
    protocol::*,
};
use crate::{
    ai::{AiDriver, CarParams, CarView, Difficulty, Other},
    race::{DriverKind, Race, RacePhase, RaceTrack, Racer, spawn_racer},
    rules::Progress,
    track::Layer,
    vehicle::{CarBody, CarControls, CarState, CarTelemetry, ControlSystems, VehicleSystems},
};

/// Ticks of inputs and predicted states kept (1 s): the deepest rollback.
const HISTORY: u32 = 240;

/// A prediction this far off the server state is corrected (m, rad, m/s):
/// well below what shows, so a difference is put right long before it can
/// be seen, but not every float-rounding difference costs a replay.
const POSITION_TOLERANCE: f32 = 0.01;
const ROTATION_TOLERANCE: f32 = 0.005;
const VELOCITY_TOLERANCE: f32 = 0.05;

/// Extra lead of the client's clock over the server's (ticks, 25 ms).
const INPUT_MARGIN_TICKS: f32 = 6.0;

/// Displayed jumps larger than this (m) snap: a reset, not a correction.
const SNAP_DISTANCE: f32 = 5.0;

/// Decay time constants (s) of the displayed correction: the local car
/// blends over a few frames, remote cars over 100-150 ms.
const LOCAL_BLEND: f32 = 0.025;
const REMOTE_BLEND: f32 = 0.04;

/// The online race client. Insert a [`NetClientConfig`] to connect.
pub struct NetClientPlugin;

impl Plugin for NetClientPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(ClientPlugins {
            tick_duration: crate::tick_duration(),
        })
        .add_plugins(ProtocolPlugin)
        // Inputs must reach the server before it simulates their tick, also
        // when either side's frame comes late: the clock runs ahead of the
        // server by half the round trip, four times the jitter and this many
        // ticks (lightyear's default is one).
        .insert_resource(InputTimelineConfig::default().with_sync_config(SyncConfig {
            jitter_margin: INPUT_MARGIN_TICKS,
            ..default()
        }))
        .init_resource::<NetSession>()
        .init_resource::<NetStats>()
        .configure_sets(
            FixedUpdate,
            (
                ControlSystems,
                NetSystems::Inputs,
                NetSystems::Remote,
                VehicleSystems,
            )
                .chain(),
        )
        .add_systems(PreUpdate, connect.run_if(resource_added::<NetClientConfig>))
        .add_systems(
            PreUpdate,
            (
                track_connection,
                receive_control,
                receive_standings,
                reconcile,
                update_race,
            )
                .chain()
                .after(MessageSystems::ReceiveMessages),
        )
        .add_systems(
            FixedFirst,
            advance_tick.run_if(resource_exists::<Prediction>.and_then(not(resimulating))),
        )
        .add_systems(
            FixedUpdate,
            (
                drive_bot
                    .in_set(ControlSystems)
                    .run_if(not(resimulating)),
                inputs.in_set(NetSystems::Inputs),
                place_remote_cars.in_set(NetSystems::Remote),
            )
                .run_if(resource_exists::<Prediction>),
        )
        .add_systems(
            FixedPostUpdate,
            record
                .after(PhysicsSystems::Last)
                .run_if(resource_exists::<Prediction>),
        )
        .add_systems(First, remove_visual_offset)
        .add_systems(Update, remote_layers)
        .add_systems(
            PostUpdate,
            (
                send_inputs.before(MessageSystems::Send),
                apply_visual_offset.before(TransformSystems::Propagate),
            ),
        );
    }
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum NetSystems {
    /// The local car's controls: kept and sent, or replayed in a rollback.
    Inputs,
    /// Remote cars placed on their extrapolated paths.
    Remote,
}

/// Where and how to connect.
#[derive(Resource, Clone, Debug)]
pub struct NetClientConfig {
    pub server: SocketAddr,
    pub auth: ClientAuth,
    pub join: Join,
    /// Simulated network conditions on the server-to-client path.
    pub conditions: Conditions,
    /// Let the AI drive the local car (test bots, demos).
    pub autopilot: Option<Difficulty>,
}

/// The netcode.io key of the server (the backend hands out each race's key
/// to its lobby members; direct connections use [`DEV_KEY`]) and a client
/// id unique in the race.
#[derive(Clone, Debug)]
pub struct ClientAuth {
    pub key: [u8; 32],
    pub client_id: u64,
}

impl NetClientConfig {
    pub fn direct(server: SocketAddr, client_id: u64, join: Join) -> Self {
        NetClientConfig {
            server,
            auth: ClientAuth {
                key: DEV_KEY,
                client_id,
            },
            join,
            conditions: Conditions::default(),
            autopilot: None,
        }
    }
}

/// Present while an online race is shown: the race rules are the server's,
/// so [`crate::race::RacePlugin`] does not run.
#[derive(Resource)]
pub struct RemoteRace;

#[derive(Clone, Debug, Default, PartialEq)]
pub enum SessionState {
    #[default]
    Idle,
    Connecting,
    Lobby(LobbyStatus),
    Racing,
    /// The race is over and the results are in.
    Over,
    Failed(String),
}

#[derive(Resource, Default, Debug)]
pub struct NetSession {
    pub state: SessionState,
    pub rtt: Duration,
    pub jitter: Duration,
    /// The local car's grid slot.
    pub you: Option<u8>,
    client: Option<Entity>,
    connected_at: Option<Instant>,
}

/// A car in an online race.
#[derive(Component, Clone, Copy, Debug)]
pub struct NetCar {
    pub slot: u8,
    pub local: bool,
}

/// A car simulated by someone else: its path from the last snapshot.
#[derive(Component, Debug)]
pub struct RemoteCar {
    model: Model,
    tick: u32,
    controls: NetControls,
    /// Extrapolated state after each tick from `tick` on.
    path: VecDeque<Body>,
}

impl RemoteCar {
    fn at(&mut self, tick: u32) -> Body {
        let i = tick.saturating_sub(self.tick) as usize;
        let controls = self.controls.held().decode();
        let dt = crate::tick_duration().as_secs_f32();
        while self.path.len() <= i {
            let mut next = *self.path.back().expect("the snapshot is in the path");
            extrapolate::step(&mut next, &controls, &self.model, dt);
            self.path.push_back(next);
        }
        self.path[i]
    }
}

/// The displayed offset from the simulated pose, decaying to zero.
#[derive(Component, Debug)]
pub struct VisualOffset {
    pub translation: Vec3,
    pub rotation: Quat,
    blend: f32,
    applied: Option<(Vec3, Quat)>,
}

impl VisualOffset {
    fn new(blend: f32) -> Self {
        VisualOffset {
            translation: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            blend,
            applied: None,
        }
    }

    /// Keeps the displayed pose where it was across a jump of the simulated
    /// one from `from` to `to`.
    fn absorb(&mut self, from: &Body, to: &Body) {
        if from.position.distance(to.position) > SNAP_DISTANCE {
            self.translation = Vec3::ZERO;
            self.rotation = Quat::IDENTITY;
        } else {
            self.translation += from.position - to.position;
            self.rotation = (self.rotation * from.rotation * to.rotation.inverse()).normalize();
        }
    }
}

/// Track position of every car as this client sees it (for the bot AI).
#[derive(Component, Clone, Copy, Debug)]
pub struct LocalProgress(pub Progress);

/// Prediction state of the running race.
#[derive(Resource, Debug)]
pub struct Prediction {
    /// Current (or, in a rollback, replayed) tick.
    pub tick: u32,
    pub start: u32,
    inputs: BTreeMap<u32, NetControls>,
    states: BTreeMap<u32, (Body, racer_dynamics::VehicleState)>,
    sent: u32,
    last_snapshot: u32,
    ghosted: Vec<bool>,
    /// Remote cars this car touches, as of the last tick.
    touching: Vec<u8>,
}

/// Present while ticks are replayed in a rollback.
#[derive(Resource)]
pub struct Resimulating;

fn resimulating(r: Option<Res<Resimulating>>) -> bool {
    r.is_some()
}

/// What the prediction did, for the net check.
#[derive(Resource, Default, Debug, Clone)]
pub struct NetStats {
    /// Snapshots compared with the prediction.
    pub snapshots: u32,
    /// Corrections: position and rotation error (m, rad) of the prediction
    /// at the snapshot tick, and the displayed jump they caused (m).
    pub corrections: Vec<Correction>,
    /// Snapshots that arrived too late to compare (outside the history) or
    /// ahead of the local tick.
    pub unmatched: u32,
    pub resim_ticks: u64,
    pub resim_time: Duration,
    /// Contacts the local car predicted with other cars: (tick, their slot).
    pub contacts: Vec<(u32, u8, Vec3)>,
    /// Lead of the inputs over the server tick (ticks), per snapshot.
    pub input_lead: Vec<i32>,
    pub rtt_ms: Vec<f32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Correction {
    pub tick: u32,
    pub position: f32,
    pub rotation: f32,
    pub displayed: f32,
    /// Rollback depth (ticks replayed).
    pub depth: u32,
}

fn connect(
    mut commands: Commands,
    config: Res<NetClientConfig>,
    mut session: ResMut<NetSession>,
    mut stats: ResMut<NetStats>,
) {
    if let Some(old) = session.client.take() {
        commands.entity(old).despawn();
    }
    *stats = NetStats::default();
    let auth = Authentication::Manual {
        server_addr: config.server,
        client_id: config.auth.client_id,
        private_key: config.auth.key,
        protocol_id: PROTOCOL_ID,
    };
    let netcode = match NetcodeClient::new(auth, NetcodeConfig::default()) {
        Ok(n) => n,
        Err(e) => {
            session.state = SessionState::Failed(format!("{e}"));
            return;
        }
    };
    let local = if config.server.is_ipv4() {
        "0.0.0.0:0"
    } else {
        "[::]:0"
    };
    let mut link = Link::default();
    if let Some(c) = config.conditions.conditioner() {
        link = link.with_conditioner(c);
    }
    let client = commands
        .spawn((
            Client,
            Name::new("net client"),
            LocalAddr(local.parse().expect("a socket address")),
            PeerAddr(config.server),
            link,
            netcode,
            UdpIo::default(),
        ))
        .id();
    commands.trigger(Connect { entity: client });
    session.client = Some(client);
    session.state = SessionState::Connecting;
    session.connected_at = None;
    session.you = None;
}

/// Sends the join message once connected; notices disconnections; reads
/// the round-trip time.
fn track_connection(
    config: Option<Res<NetClientConfig>>,
    mut session: ResMut<NetSession>,
    mut stats: ResMut<NetStats>,
    mut clients: Query<(
        Has<Connected>,
        Has<Disconnected>,
        &PingManager,
        &mut MessageSender<Join>,
    )>,
) {
    let Some(client) = session.client else {
        return;
    };
    let Ok((connected, disconnected, ping, mut join)) = clients.get_mut(client) else {
        return;
    };
    session.rtt = ping.rtt();
    session.jitter = ping.jitter();
    if connected && session.connected_at.is_none() {
        session.connected_at = Some(Instant::now());
        if let Some(config) = config {
            join.send::<ControlChannel>(config.join.clone());
        }
        if session.state == SessionState::Connecting {
            session.state = SessionState::Lobby(LobbyStatus::default());
        }
    }
    if session.state == SessionState::Racing && ping.rtt() > Duration::ZERO {
        stats.rtt_ms.push(ping.rtt().as_secs_f32() * 1000.0);
    }
    if disconnected
        && session.connected_at.is_some()
        && !matches!(session.state, SessionState::Over | SessionState::Failed(_))
    {
        session.state = SessionState::Failed("disconnected from the server".into());
    }
}

#[allow(clippy::too_many_arguments)]
fn receive_control(
    mut commands: Commands,
    config: Option<Res<NetClientConfig>>,
    track: Option<Res<RaceTrack>>,
    mut session: ResMut<NetSession>,
    mut lobby: Query<&mut MessageReceiver<LobbyStatus>>,
    mut starts: Query<&mut MessageReceiver<RaceStart>>,
    mut overs: Query<&mut MessageReceiver<RaceOver>>,
    mut racers: Query<(&NetCar, &mut Racer)>,
    mut race: Option<ResMut<Race>>,
) {
    for status in lobby.iter_mut().flat_map(|mut r| r.receive().collect::<Vec<_>>()) {
        if matches!(session.state, SessionState::Lobby(_)) {
            session.state = SessionState::Lobby(status);
        }
    }
    let start = starts
        .iter_mut()
        .flat_map(|mut r| r.receive().collect::<Vec<_>>())
        .last();
    if let (Some(start), Some(track)) = (start, track) {
        let autopilot = config.and_then(|c| c.autopilot);
        start_race(&mut commands, &track, &start, autopilot);
        session.you = Some(start.you);
        session.state = SessionState::Racing;
    }
    for over in overs.iter_mut().flat_map(|mut r| r.receive().collect::<Vec<_>>()) {
        for (car, mut racer) in &mut racers {
            if let Some((_, r)) = over.racers.iter().find(|(s, _)| *s == car.slot) {
                *racer = r.clone();
            }
        }
        if let Some(race) = race.as_mut() {
            race.phase = RacePhase::Finished;
        }
        session.state = SessionState::Over;
    }
}

/// Spawns the grid as the server set it and starts predicting.
fn start_race(
    commands: &mut Commands,
    track: &RaceTrack,
    start: &RaceStart,
    autopilot: Option<Difficulty>,
) {
    let t = &track.0;
    for car in &start.grid {
        let spec = match load_car(&car.car) {
            Ok(spec) => spec,
            Err(e) => {
                error!("cannot load {}: {e}", car.car);
                continue;
            }
        };
        let local = car.slot == start.you;
        let slot = usize::from(car.slot);
        let marker = NetCar {
            slot: car.slot,
            local,
        };
        let offset = VisualOffset::new(if local { LOCAL_BLEND } else { REMOTE_BLEND });
        let entity = match spawn_racer(
            commands,
            t,
            &spec,
            slot,
            car.name.clone(),
            DriverKind::Human,
            (marker, offset),
        ) {
            Ok(e) => e,
            Err(e) => {
                error!("cannot spawn {}: {e}", car.name);
                continue;
            }
        };
        let driver = car.driver;
        let grid = &t.geometry.grid[slot];
        let progress = Progress::new(&t.geometry, grid.position);
        commands.entity(entity).insert(LocalProgress(progress));
        commands.queue(move |world: &mut World| {
            if let Some(mut r) = world.get_mut::<Racer>(entity) {
                r.driver = driver;
            }
        });
        let params = || {
            let vehicle = racer_dynamics::Vehicle::from_spec(&spec).ok()?;
            let profile = t.car(&spec.id)?;
            Some(CarParams::new(&spec, &vehicle, profile.model.clone()))
        };
        if local {
            if let Some(difficulty) = autopilot {
                match params() {
                    Some(p) => {
                        let seed = (slot as u64 + 1) * 7919 + difficulty as u64;
                        commands
                            .entity(entity)
                            .insert(AiDriver::new(difficulty, p, &t.line, seed));
                    }
                    None => error!("no AI profile for {}", spec.id),
                }
            }
        } else {
            let Some(p) = params() else {
                error!("no car model for {}", spec.id);
                continue;
            };
            let at = Transform::from_translation(grid.position).with_rotation(grid.rotation);
            let ride = racer_dynamics::Vehicle::from_spec(&spec)
                .map(|v| v.ride_height() as f32)
                .unwrap_or(0.5);
            let body = Body {
                position: at.translation + at.up() * ride,
                rotation: at.rotation,
                ..default()
            };
            commands.entity(entity).insert((
                RigidBody::Kinematic,
                RemoteCar {
                    model: Model::from(&p),
                    tick: start.created,
                    controls: NetControls::default(),
                    path: VecDeque::from([body]),
                },
            ));
        }
    }
    commands.insert_resource(start.config.clone());
    commands.insert_resource(Race::new(&start.config));
    commands.insert_resource(RemoteRace);
    commands.insert_resource(Prediction {
        tick: start.created,
        start: start.start,
        inputs: BTreeMap::new(),
        states: BTreeMap::new(),
        sent: start.created,
        last_snapshot: start.created,
        ghosted: vec![false; start.grid.len()],
        touching: Vec::new(),
    });
}

/// Ends the online race on this client (leaving it, or after the results).
pub fn leave(mut commands: Commands, mut session: ResMut<NetSession>) {
    if let Some(client) = session.client.take() {
        commands.trigger(Disconnect { entity: client });
        commands.entity(client).despawn();
    }
    commands.remove_resource::<NetClientConfig>();
    commands.remove_resource::<Prediction>();
    commands.remove_resource::<RemoteRace>();
    *session = NetSession::default();
}

fn receive_standings(
    mut receivers: Query<&mut MessageReceiver<Standings>>,
    mut racers: Query<(&NetCar, &mut Racer)>,
    mut race: Option<ResMut<Race>>,
    mut newest: Local<Vec<u32>>,
) {
    for s in receivers
        .iter_mut()
        .flat_map(|mut r| r.receive().collect::<Vec<_>>())
    {
        let slot = usize::from(s.slot);
        if newest.len() <= slot {
            newest.resize(slot + 1, 0);
        }
        if s.tick < newest[slot] {
            continue;
        }
        newest[slot] = s.tick;
        if let Some((_, mut racer)) = racers.iter_mut().find(|(c, _)| c.slot == s.slot) {
            *racer = s.racer;
        }
        if let Some(race) = race.as_mut() {
            race.winner = s.winner;
            if s.phase == RacePhase::Finished {
                race.phase = RacePhase::Finished;
            }
        }
    }
}

/// The race clock from the predicted tick.
fn update_race(prediction: Option<Res<Prediction>>, mut race: Option<ResMut<Race>>) {
    let (Some(p), Some(race)) = (prediction, race.as_mut()) else {
        return;
    };
    let dt = crate::tick_duration().as_secs_f64();
    race.time = ((f64::from(p.tick) - f64::from(p.start)) * dt) as f32;
    if race.phase != RacePhase::Finished {
        race.phase = if p.tick < p.start {
            RacePhase::Countdown
        } else {
            RacePhase::Running
        };
    }
}

fn advance_tick(timeline: Res<LocalTimeline>, mut p: ResMut<Prediction>) {
    p.tick = timeline.tick().0;
}

type LocalBody<'a> = (
    &'a mut Position,
    &'a mut Rotation,
    &'a mut LinearVelocity,
    &'a mut AngularVelocity,
    &'a mut Transform,
    &'a mut CarState,
    &'a mut VisualOffset,
);

/// Applies new snapshots: re-runs the remote cars' paths and checks the
/// local car's prediction, rolling back and replaying if it was off.
fn reconcile(world: &mut World) {
    let Some(prediction) = world.get_resource::<Prediction>() else {
        // Drop snapshots of a race we are not showing.
        let mut q = world.query::<&mut MessageReceiver<Snapshot>>();
        for mut r in q.iter_mut(world) {
            r.receive().for_each(drop);
        }
        return;
    };
    let (last, now) = (prediction.last_snapshot, prediction.tick);
    let mut q = world.query::<&mut MessageReceiver<Snapshot>>();
    let Some(snapshot) = q
        .iter_mut(world)
        .flat_map(|mut r| r.receive().collect::<Vec<_>>())
        .filter(|s| s.tick > last)
        .max_by_key(|s| s.tick)
    else {
        return;
    };
    world.resource_mut::<Prediction>().last_snapshot = snapshot.tick;
    world
        .resource_mut::<NetStats>()
        .input_lead
        .push(snapshot.input_ack as i32 - snapshot.tick as i32);

    // Remote cars: new path from the snapshot, displayed pose kept.
    let mut remotes = world.query::<(&NetCar, &mut RemoteCar, &mut VisualOffset)>();
    for (car, mut remote, mut offset) in remotes.iter_mut(world) {
        let Some(s) = snapshot.cars.iter().find(|s| s.slot == car.slot) else {
            continue;
        };
        let before = remote.at(now);
        remote.tick = snapshot.tick;
        remote.controls = s.controls;
        remote.path = VecDeque::from([s.body]);
        let after = remote.at(now);
        offset.absorb(&before, &after);
    }
    let ghosted: Vec<(u8, bool)> = snapshot.cars.iter().map(|c| (c.slot, c.ghosted)).collect();
    set_ghosting(world, &ghosted);

    // Local car.
    let Some(own) = snapshot.own else { return };
    let mut locals = world.query::<(Entity, &NetCar)>();
    let Some((entity, slot)) = locals
        .iter(world)
        .find(|(_, c)| c.local)
        .map(|(e, c)| (e, c.slot))
    else {
        return;
    };
    let Some(server) = snapshot.cars.iter().find(|c| c.slot == slot).map(|c| c.body) else {
        return;
    };
    world.resource_mut::<NetStats>().snapshots += 1;
    let predicted = world
        .resource::<Prediction>()
        .states
        .get(&snapshot.tick)
        .copied();
    let depth = now.saturating_sub(snapshot.tick);
    let Some((predicted, predicted_state)) = predicted.filter(|_| depth < HISTORY) else {
        // Nothing to compare with: the race just started, or the snapshot is
        // older than the history. Take the server state as is.
        world.resource_mut::<NetStats>().unmatched += 1;
        if snapshot.tick >= now {
            set_local(world, entity, &server, own);
        }
        return;
    };
    let position = predicted.position.distance(server.position);
    let rotation = predicted.rotation.angle_between(server.rotation);
    let velocity = predicted
        .velocity
        .distance(server.velocity)
        .max(predicted.angular_velocity.distance(server.angular_velocity));
    let state_off = predicted_state.drive.gear != own.drive.gear
        || predicted_state
            .wheels
            .iter()
            .zip(own.wheels.iter())
            .any(|(a, b)| (a.omega - b.omega).abs() > 0.5);
    if position < POSITION_TOLERANCE
        && rotation < ROTATION_TOLERANCE
        && velocity < VELOCITY_TOLERANCE
        && !state_off
    {
        return;
    }

    // Rollback: the server state at the snapshot tick, then replay.
    let before = body_of(world, entity);
    set_local(world, entity, &server, own);
    let started = Instant::now();
    resimulate(world, snapshot.tick + 1, now);
    let after = body_of(world, entity);
    if let Some(mut offset) = world.get_mut::<VisualOffset>(entity) {
        offset.absorb(&before, &after);
    }
    let mut stats = world.resource_mut::<NetStats>();
    stats.resim_ticks += u64::from(depth);
    stats.resim_time += started.elapsed();
    stats.corrections.push(Correction {
        tick: snapshot.tick,
        position,
        rotation,
        displayed: before.position.distance(after.position),
        depth,
    });
}

fn body_of(world: &World, entity: Entity) -> Body {
    let e = world.entity(entity);
    Body {
        position: e.get::<Position>().map_or(Vec3::ZERO, |p| p.0),
        rotation: e.get::<Rotation>().map_or(Quat::IDENTITY, |r| r.0),
        velocity: e.get::<LinearVelocity>().map_or(Vec3::ZERO, |v| v.0),
        angular_velocity: e.get::<AngularVelocity>().map_or(Vec3::ZERO, |w| w.0),
    }
}

fn set_local(world: &mut World, entity: Entity, body: &Body, state: racer_dynamics::VehicleState) {
    let mut q = world.query::<LocalBody>();
    let Ok((mut p, mut r, mut v, mut w, mut t, mut s, _)) = q.get_mut(world, entity) else {
        return;
    };
    p.0 = body.position;
    r.0 = body.rotation;
    v.0 = body.velocity;
    w.0 = body.angular_velocity;
    *t = Transform::from_translation(body.position).with_rotation(body.rotation);
    s.0 = state;
}

/// Replays ticks `from..=to` of the fixed schedule.
fn resimulate(world: &mut World, from: u32, to: u32) {
    if from > to {
        return;
    }
    let timeline = world.resource::<LocalTimeline>().clone();
    let time = world.resource::<Time>().as_generic();
    world.insert_resource(Resimulating);
    for tick in from..=to {
        world.resource_mut::<Prediction>().tick = tick;
        *world.resource_mut::<Time>() = world.resource::<Time<Fixed>>().as_generic();
        world.run_schedule(FixedMain);
    }
    world.remove_resource::<Resimulating>();
    *world.resource_mut::<Time>() = time;
    *world.resource_mut::<LocalTimeline>() = timeline;
}

/// Ghosted cars (after a reset, or by the collisions option) don't collide
/// with other cars.
fn set_ghosting(world: &mut World, ghosted: &[(u8, bool)]) {
    let changed: Vec<(u8, bool)> = {
        let p = world.resource::<Prediction>();
        ghosted
            .iter()
            .copied()
            .filter(|&(slot, g)| p.ghosted.get(usize::from(slot)) != Some(&g))
            .collect()
    };
    if changed.is_empty() {
        return;
    }
    let mut cars = world.query::<(&NetCar, &Children)>();
    let bodies: Vec<(Entity, bool, bool)> = cars
        .iter(world)
        .filter_map(|(c, children)| {
            let (_, g) = changed.iter().find(|(s, _)| *s == c.slot)?;
            Some(children.iter().map(move |child| (child, !c.local, *g)))
        })
        .flatten()
        .collect();
    for (child, remote, ghost) in bodies {
        if world.get::<CarBody>(child).is_some() {
            world
                .entity_mut(child)
                .insert(car_layers(remote, ghost));
        }
    }
    let mut p = world.resource_mut::<Prediction>();
    for (slot, g) in changed {
        let i = usize::from(slot);
        if p.ghosted.len() <= i {
            p.ghosted.resize(i + 1, false);
        }
        p.ghosted[i] = g;
    }
}

/// Collision layers of a car body: the local car hits the world and (unless
/// ghosted) the other cars; remote cars, moved along their paths, only
/// need to be hit by it.
fn car_layers(remote: bool, ghost: bool) -> CollisionLayers {
    let mut filters = if remote {
        LayerMask::NONE
    } else {
        LayerMask::from([Layer::World, Layer::Wall])
    };
    if !ghost {
        filters |= Layer::Car;
    }
    CollisionLayers::new(Layer::Car, filters)
}

/// Remote cars' bodies start with their layers (spawned this frame).
fn remote_layers(
    mut commands: Commands,
    bodies: Query<(Entity, &ChildOf), Added<CarBody>>,
    cars: Query<&NetCar>,
) {
    for (body, parent) in &bodies {
        if let Ok(car) = cars.get(parent.parent()) {
            commands.entity(body).insert(car_layers(!car.local, false));
        }
    }
}

/// The local car's controls for this tick: what the driver (player or bot)
/// set, quantized and kept; in a rollback, the kept ones. Held on the brakes
/// before the start, as the server does.
fn inputs(
    resim: Option<Res<Resimulating>>,
    mut p: ResMut<Prediction>,
    mut cars: Query<(&NetCar, &mut CarControls)>,
) {
    let tick = p.tick;
    for (car, mut controls) in &mut cars {
        if !car.local {
            continue;
        }
        let net = if resim.is_some() {
            p.inputs.get(&tick).copied().unwrap_or_default()
        } else {
            let net = NetControls::encode(&controls.0);
            p.inputs.insert(tick, net);
            while p.inputs.len() > HISTORY as usize {
                p.inputs.pop_first();
            }
            net
        };
        controls.0 = net.decode();
        if tick < p.start {
            controls.0.brake = 1.0;
        }
    }
}

/// Kinematic remote cars move from their pose after the previous tick to
/// the one after this tick.
fn place_remote_cars(
    p: Res<Prediction>,
    mut cars: Query<(
        &mut RemoteCar,
        &mut Position,
        &mut Rotation,
        &mut LinearVelocity,
        &mut AngularVelocity,
    )>,
) {
    let dt = crate::tick_duration().as_secs_f32();
    for (mut remote, mut pos, mut rot, mut vel, mut ang) in &mut cars {
        let previous = p.tick.saturating_sub(1).max(remote.tick);
        let from = remote.at(previous);
        let to = remote.at(p.tick);
        pos.0 = from.position;
        rot.0 = from.rotation;
        vel.0 = (to.position - from.position) / dt;
        let turn = to.rotation * from.rotation.inverse();
        let (axis, angle) = turn.to_axis_angle();
        ang.0 = if angle.is_finite() {
            axis * angle / dt
        } else {
            Vec3::ZERO
        };
        if to.position == from.position {
            vel.0 = to.velocity;
        }
    }
}

/// Keeps the predicted state of the local car after each tick, the track
/// position of every car, and the contacts the local car predicts.
#[allow(clippy::type_complexity)]
fn record(
    resim: Option<Res<Resimulating>>,
    track: Res<RaceTrack>,
    mut p: ResMut<Prediction>,
    mut stats: ResMut<NetStats>,
    mut cars: Query<(
        &NetCar,
        &Position,
        &Rotation,
        &LinearVelocity,
        &AngularVelocity,
        &CarState,
        &mut LocalProgress,
        &Children,
    )>,
    touching: Query<&CollidingEntities, With<CarBody>>,
    parents: Query<&ChildOf, With<CarBody>>,
    slots: Query<&NetCar>,
) {
    let tick = p.tick;
    let dt = crate::tick_duration().as_secs_f32();
    for (car, pos, rot, vel, ang, state, mut progress, children) in &mut cars {
        if resim.is_none() {
            progress
                .0
                .update(&track.0.geometry, pos.0, vel.0, rot.0 * Vec3::NEG_Z, dt);
        }
        if !car.local {
            continue;
        }
        p.states.insert(tick, (Body::read(pos, rot, vel, ang), state.0));
        while p.states.len() > HISTORY as usize {
            p.states.pop_first();
        }
        let now: Vec<u8> = children
            .iter()
            .filter_map(|c| touching.get(c).ok())
            .flat_map(|t| t.iter().copied().collect::<Vec<_>>())
            .filter_map(|other| parents.get(other).ok())
            .filter_map(|parent| slots.get(parent.parent()).ok())
            .map(|c| c.slot)
            .collect();
        for &slot in &now {
            let new = !p.touching.contains(&slot);
            let seen = stats
                .contacts
                .iter()
                .rev()
                .take(16)
                .any(|&(t, s, _)| s == slot && t.abs_diff(tick) < 24);
            if new && !seen {
                stats.contacts.push((tick, slot, pos.0));
            }
        }
        p.touching = now;
    }
}

/// Sends the inputs not yet sent, with the ones before them again.
fn send_inputs(mut p: Option<ResMut<Prediction>>, mut senders: Query<&mut MessageSender<Inputs>>) {
    let Some(p) = p.as_mut() else { return };
    let Some((&last, _)) = p.inputs.last_key_value() else {
        return;
    };
    if last <= p.sent {
        return;
    }
    let first = last.saturating_sub(INPUT_REDUNDANCY as u32 - 1);
    // Contiguous ticks only: a gap (a jump of the clock) restarts the run.
    let mut controls = Vec::new();
    for tick in (first..=last).rev() {
        match p.inputs.get(&tick) {
            Some(c) => controls.push(*c),
            None => break,
        }
    }
    controls.reverse();
    for mut sender in &mut senders {
        sender.send::<StateChannel>(Inputs {
            last,
            controls: controls.clone(),
        });
    }
    p.sent = last;
}

/// The AI driving the local car through its controls, as a player would.
#[allow(clippy::type_complexity)]
fn drive_bot(
    track: Res<RaceTrack>,
    race: Option<Res<Race>>,
    p: Res<Prediction>,
    mut cars: Query<(
        Entity,
        &NetCar,
        &Racer,
        &LocalProgress,
        &Position,
        &Rotation,
        &LinearVelocity,
        &AngularVelocity,
        &CarTelemetry,
        Option<&mut AiDriver>,
        &mut CarControls,
    )>,
    mut resets: Query<&mut MessageSender<RequestReset>>,
) {
    let t = &track.0;
    let length = t.geometry.length;
    let others: Vec<Other> = cars
        .iter()
        .map(|(e, _, r, lp, pos, _, vel, ..)| Other {
            entity: e,
            s: lp.0.distance.rem_euclid(length),
            lateral: lp.0.lateral,
            speed: vel.0.length(),
            position: pos.0,
            solid: !r.ghosted,
        })
        .collect();
    let time = race.map_or(0.0, |r| r.time);
    let dt = crate::tick_duration().as_secs_f32();
    for (entity, car, racer, lp, pos, rot, vel, ang, telemetry, ai, mut controls) in &mut cars {
        let Some(mut ai) = ai.filter(|_| car.local) else {
            continue;
        };
        ai.catch_up = if racer.status == crate::rules::Status::Racing {
            1.0
        } else {
            0.7
        };
        let view = CarView {
            position: pos.0,
            rotation: rot.0,
            velocity: vel.0,
            angular_velocity: ang.0,
            telemetry: &telemetry.telemetry,
            s: lp.0.distance.rem_euclid(length),
            lateral: lp.0.lateral,
        };
        let mine: Vec<Other> = others.iter().filter(|o| o.entity != entity).copied().collect();
        let mut c = ai.control(&view, &t.line, &t.geometry, &mine, time, dt);
        c.assists = Assists::default();
        controls.0 = c;
        if ai.wants_reset {
            ai.wants_reset = false;
            if p.tick >= p.start {
                for mut s in &mut resets {
                    s.send::<ControlChannel>(RequestReset);
                }
            }
        }
    }
}

/// Ask the server to put the local car back on the track.
pub fn request_reset(mut resets: Query<&mut MessageSender<RequestReset>>) {
    for mut s in &mut resets {
        s.send::<ControlChannel>(RequestReset);
    }
}

fn apply_visual_offset(time: Res<Time>, mut cars: Query<(&mut Transform, &mut VisualOffset)>) {
    let dt = time.delta_secs();
    for (mut transform, mut offset) in &mut cars {
        let keep = (-dt / offset.blend).exp();
        offset.translation *= keep;
        offset.rotation = Quat::IDENTITY.slerp(offset.rotation, keep);
        if offset.translation.length_squared() < 1e-8
            && offset.rotation.angle_between(Quat::IDENTITY) < 1e-5
        {
            offset.translation = Vec3::ZERO;
            offset.rotation = Quat::IDENTITY;
            continue;
        }
        transform.translation += offset.translation;
        transform.rotation = offset.rotation * transform.rotation;
        offset.applied = Some((offset.translation, offset.rotation));
    }
}

fn remove_visual_offset(mut cars: Query<(&mut Transform, &mut VisualOffset)>) {
    for (mut transform, mut offset) in &mut cars {
        if let Some((t, r)) = offset.applied.take() {
            transform.translation -= t;
            transform.rotation = r.inverse() * transform.rotation;
        }
    }
}

/// Whether the client's simulation tick has synced with the server.
pub fn synced(sync: Option<Res<LocalTimelineSync>>) -> bool {
    sync.is_some_and(|s| s.is_synced())
}

/// Default resources of a headless client: the track's colliders and the
/// race track resource (the game sets up its own).
pub fn headless_world(app: &mut App, track: std::sync::Arc<racer_trackc::Track>) {
    app.insert_resource(RaceTrack(track.clone()));
    let world = app.world_mut();
    let mut commands = world.commands();
    crate::track::spawn_track(&mut commands, &crate::track::city_track(&track.geometry));
    world.flush();
}


/// A headless client app (test bots): the simulation, the track's colliders
/// and the net client, single-threaded so that many run side by side. Drive
/// it with `update()` in real time.
pub fn headless_client(config: NetClientConfig, track: std::sync::Arc<racer_trackc::Track>) -> App {
    use bevy::app::TaskPoolOptions;
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(TaskPoolPlugin {
            task_pool_options: TaskPoolOptions::with_num_threads(1),
        }),
        TransformPlugin,
        crate::SimPlugin,
        crate::vehicle::VehiclePlugin,
        NetClientPlugin,
    ))
    .insert_resource(config);
    headless_world(&mut app, track);
    crate::finish_headless(&mut app);
    app
}
