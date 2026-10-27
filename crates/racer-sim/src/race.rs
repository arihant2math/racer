//! The race loop on top of the vehicle plugin: grid and countdown, progress
//! and timing for every car, resets, ghosting and the collisions option,
//! AI drivers, standings, finish and DNF, and the results table.
//!
//! The rules themselves are in [`crate::rules`]; this module runs them for
//! every [`Racer`] each fixed step.

use std::sync::Arc;

use avian3d::prelude::*;
use bevy::prelude::*;
use racer_dynamics::{Assists, CarSpec};
use racer_trackc::Track;

use crate::{
    ai::{AiDriver, CarParams, CarView, Difficulty, Other},
    rules::{self, Goal, Progress, ProgressEvent, ResetMonitor, ResetReason, Status, Timing},
    track::Layer,
    vehicle::{
        Car, CarBody, CarControls, CarState, CarTelemetry, ControlSystems, VehicleSystems,
        spawn_car,
    },
};

/// Runs races. Needs [`crate::vehicle::VehiclePlugin`] and the
/// [`RaceTrack`] and [`RaceConfig`] resources before cars are spawned.
pub struct RacePlugin;

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.configure_sets(
            FixedUpdate,
            (
                RaceSystems::Rules,
                ControlSystems,
                RaceSystems::Hold,
                VehicleSystems,
            )
                .chain(),
        )
        .add_systems(
            FixedUpdate,
            (
                (
                    clock,
                    track_racers,
                    apply_resets,
                    update_collisions,
                    standings,
                    finish,
                )
                    .chain()
                    .in_set(RaceSystems::Rules),
                drive_ai.in_set(ControlSystems),
                hold.in_set(RaceSystems::Hold),
            )
                .run_if(
                    resource_exists::<Race>
                        .and_then(resource_exists::<RaceConfig>)
                        .and_then(resource_exists::<RaceTrack>),
                ),
        );
    }
}

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum RaceSystems {
    /// Clock, progress, resets, ghosting, standings and finish.
    Rules,
    /// Race-control overrides of the controls (held on the grid).
    Hold,
}

#[derive(Resource, Clone)]
pub struct RaceTrack(pub Arc<Track>);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RaceMode {
    #[default]
    Circuit,
    /// Point to point over the track opened: grid to the sprint finish.
    Sprint,
    /// Solo against the clock.
    TimeTrial,
}

impl RaceMode {
    pub const ALL: [RaceMode; 3] = [RaceMode::Circuit, RaceMode::Sprint, RaceMode::TimeTrial];

    pub fn name(self) -> &'static str {
        match self {
            RaceMode::Circuit => "Circuit",
            RaceMode::Sprint => "Sprint",
            RaceMode::TimeTrial => "Time Trial",
        }
    }
}

/// Car-to-car contact.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Collisions {
    #[default]
    Full,
    /// Cars pass through each other until they complete their first lap.
    GhostFirstLap,
    Off,
}

impl Collisions {
    pub const ALL: [Collisions; 3] = [Collisions::Full, Collisions::GhostFirstLap, Collisions::Off];

    pub fn name(self) -> &'static str {
        match self {
            Collisions::Full => "Full",
            Collisions::GhostFirstLap => "Ghosted first lap",
            Collisions::Off => "Off",
        }
    }
}

#[derive(Resource, Clone, Debug)]
pub struct RaceConfig {
    pub mode: RaceMode,
    /// Laps for circuits and time trials.
    pub laps: u32,
    pub collisions: Collisions,
    /// Mild catch-up: AI cars far ahead of the human ease off, far behind
    /// push to (never past) their car's limit.
    pub catch_up: bool,
    pub countdown: f32,
    /// Seconds unfinished cars get after the winner finishes.
    pub dnf_timeout: f32,
}

impl Default for RaceConfig {
    fn default() -> Self {
        RaceConfig {
            mode: RaceMode::Circuit,
            laps: 3,
            collisions: Collisions::Full,
            catch_up: false,
            countdown: 3.0,
            dnf_timeout: 30.0,
        }
    }
}

impl RaceConfig {
    pub fn goal(&self, track: &Track) -> Goal {
        match self.mode {
            RaceMode::Sprint => Goal::Distance(track.geometry.sprint_finish),
            _ => Goal::Laps(self.laps),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RacePhase {
    #[default]
    Countdown,
    Running,
    /// Every car finished or is out.
    Finished,
}

#[derive(Resource, Clone, Debug, Default)]
pub struct Race {
    pub phase: RacePhase,
    /// Race clock (s): negative during the countdown, 0 at the start.
    pub time: f32,
    /// Finish time of the winner.
    pub winner: Option<f32>,
}

impl Race {
    pub fn new(config: &RaceConfig) -> Self {
        Race {
            phase: RacePhase::Countdown,
            time: -config.countdown,
            winner: None,
        }
    }
}

/// Who drives a racer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DriverKind {
    Human,
    Ai(Difficulty),
}

/// A car in the race.
#[derive(Component, Clone, Debug)]
pub struct Racer {
    pub name: String,
    pub driver: DriverKind,
    pub slot: usize,
    pub progress: Progress,
    pub monitor: ResetMonitor,
    pub timing: Timing,
    pub status: Status,
    /// 1-based position in the standings.
    pub position: usize,
    /// Every reset: race time, reason and the lap distance it happened at.
    pub resets: Vec<Reset>,
    /// Seconds of ghosting left after a reset.
    pub ghost: f32,
    /// Whether the car currently passes through other cars.
    pub ghosted: bool,
    pub assists: Assists,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Reset {
    pub time: f32,
    pub reason: ResetReason,
    /// Centreline distance into the lap.
    pub at: f32,
}

/// Ask for a reset to the last valid track position (reset key, AI).
#[derive(Component)]
pub struct ResetRequest;

/// Spawns a racer in grid slot `slot` with `extra` components.
pub fn spawn_racer(
    commands: &mut Commands,
    track: &Track,
    spec: &CarSpec,
    slot: usize,
    name: String,
    driver: DriverKind,
    extra: impl Bundle,
) -> Result<Entity, String> {
    let grid = track
        .geometry
        .grid
        .get(slot)
        .ok_or_else(|| format!("no grid slot {slot}"))?;
    let at = Transform::from_translation(grid.position).with_rotation(grid.rotation);
    let progress = Progress::new(&track.geometry, grid.position);
    let entity = spawn_car(commands, spec, at, extra)?;
    let racer = Racer {
        name,
        driver,
        slot,
        monitor: ResetMonitor::new(&progress),
        progress,
        timing: Timing::start(0.0),
        status: Status::Racing,
        position: slot + 1,
        resets: Vec::new(),
        ghost: 0.0,
        ghosted: false,
        assists: Assists::default(),
    };
    commands.entity(entity).insert(racer);
    if let DriverKind::Ai(difficulty) = driver {
        let profile = track
            .car(&spec.id)
            .ok_or_else(|| format!("no generated profile for {}", spec.id))?;
        let vehicle = racer_dynamics::Vehicle::from_spec(spec)?;
        let params = CarParams::new(spec, &vehicle, profile.model.clone());
        let seed = (slot as u64 + 1) * 7919 + difficulty as u64;
        commands
            .entity(entity)
            .insert(AiDriver::new(difficulty, params, &track.line, seed));
    }
    Ok(entity)
}

fn clock(time: Res<Time>, mut race: ResMut<Race>, racers: Query<&Racer>) {
    let dt = time.delta_secs();
    if race.phase == RacePhase::Finished {
        return;
    }
    race.time += dt;
    if race.phase == RacePhase::Countdown && race.time >= 0.0 {
        race.phase = RacePhase::Running;
    }
    if race.phase == RacePhase::Running
        && !racers.is_empty()
        && racers.iter().all(|r| r.status != Status::Racing)
    {
        race.phase = RacePhase::Finished;
    }
}

type Body<'a> = (&'a Position, &'a Rotation, &'a LinearVelocity);

fn track_racers(
    time: Res<Time>,
    track: Res<RaceTrack>,
    config: Res<RaceConfig>,
    mut race: ResMut<Race>,
    mut commands: Commands,
    mut racers: Query<(Entity, &mut Racer, Body, Option<&ResetRequest>)>,
) {
    let dt = time.delta_secs();
    let g = &track.0.geometry;
    let goal = config.goal(&track.0);
    let now = race.time;
    let running = race.phase == RacePhase::Running;
    for (entity, mut racer, (pos, rot, vel), request) in &mut racers {
        let forward = rot.0 * Vec3::NEG_Z;
        let event = racer.progress.update(g, pos.0, vel.0, forward, dt);
        if let Some(event) = event
            && running
            && racer.status == Status::Racing
        {
            racer.timing.on_event(event, now, g);
            // Done at the goal, or at the line once the winner is home.
            let lapped_home = race.winner.is_some() && event == ProgressEvent::Lap;
            if matches!(event, ProgressEvent::Lap | ProgressEvent::Checkpoint(_))
                && (goal.reached(&racer.progress, g) || lapped_home)
            {
                racer.status = Status::Finished(now);
                race.winner.get_or_insert(now);
            }
        }
        let racing = running && racer.status == Status::Racing;
        let progress = racer.progress;
        let up = (rot.0 * Vec3::Y).y;
        let reason = racer
            .monitor
            .update(&progress, up, vel.0.length(), racing, dt);
        let reason = reason.or(request.map(|_| ResetReason::Requested));
        if request.is_some() {
            commands.entity(entity).remove::<ResetRequest>();
        }
        if let Some(reason) = reason
            && race.phase != RacePhase::Countdown
        {
            let at = progress.distance.rem_euclid(g.length);
            racer.resets.push(Reset {
                time: now,
                reason,
                at,
            });
            commands.entity(entity).insert(PendingReset);
        }
    }
}

/// Marks a car to be moved back onto the track this step.
#[derive(Component)]
struct PendingReset;

type ResetBody<'a> = (
    &'a mut Position,
    &'a mut Rotation,
    &'a mut LinearVelocity,
    &'a mut AngularVelocity,
    &'a mut Transform,
    &'a mut CarState,
    &'a Car,
);

fn apply_resets(
    mut commands: Commands,
    track: Res<RaceTrack>,
    race: Res<Race>,
    mut pending: Query<(Entity, &mut Racer, ResetBody, Option<&mut AiDriver>), With<PendingReset>>,
    others: Query<&Position, (With<Racer>, Without<PendingReset>)>,
) {
    let g = &track.0.geometry;
    let now = race.time;
    let positions: Vec<Vec3> = others.iter().map(|p| p.0).collect();
    for (
        entity,
        mut racer,
        (mut pos, mut rot, mut vel, mut ang, mut transform, mut state, car),
        ai,
    ) in &mut pending
    {
        let (pose, distance, lateral) = rules::reset_pose(g, racer.monitor.last_valid, &positions);
        let ride = car.vehicle.ride_height() as f32;
        let up = pose.rotation * Vec3::Y;
        pos.0 = pose.position + up * ride;
        rot.0 = pose.rotation;
        *transform = Transform::from_translation(pos.0).with_rotation(rot.0);
        vel.0 = Vec3::ZERO;
        ang.0 = Vec3::ZERO;
        state.0 = car.vehicle.initial_state();
        racer.ghost = rules::GHOST_AFTER_RESET;
        racer.monitor.clear();
        for event in racer.progress.move_back_to(g, distance, lateral) {
            racer.timing.on_event(event, now, g);
        }
        if let Some(mut ai) = ai {
            ai.reset();
        }
        commands.entity(entity).remove::<PendingReset>();
    }
}

/// Ghosting after resets and the collisions option. A car only becomes solid
/// again once no other car overlaps its space.
fn update_collisions(
    mut commands: Commands,
    time: Res<Time>,
    config: Res<RaceConfig>,
    mut racers: Query<(&mut Racer, &Position, &Children)>,
    bodies: Query<(), With<CarBody>>,
) {
    let dt = time.delta_secs();
    let positions: Vec<Vec3> = racers.iter().map(|(_, p, _)| p.0).collect();
    for (mut racer, pos, children) in &mut racers {
        racer.ghost = (racer.ghost - dt).max(0.0);
        let wants_ghost = racer.ghost > 0.0
            || match config.collisions {
                Collisions::Full => false,
                Collisions::GhostFirstLap => racer.progress.laps == 0,
                Collisions::Off => true,
            };
        let crowded = positions
            .iter()
            .any(|p| *p != pos.0 && p.distance(pos.0) < 5.5);
        let ghosted = wants_ghost || (racer.ghosted && crowded);
        if ghosted == racer.ghosted {
            continue;
        }
        racer.ghosted = ghosted;
        let filters: LayerMask = if ghosted {
            [Layer::World, Layer::Wall].into()
        } else {
            [Layer::World, Layer::Wall, Layer::Car].into()
        };
        for child in children.iter().filter(|c| bodies.contains(*c)) {
            commands
                .entity(child)
                .insert(CollisionLayers::new(Layer::Car, filters));
        }
    }
}

fn standings(mut racers: Query<&mut Racer>) {
    let mut list: Vec<_> = racers.iter_mut().collect();
    let keys: Vec<(Status, Progress)> = list.iter().map(|r| (r.status, r.progress)).collect();
    for (place, i) in rules::standings(&keys).into_iter().enumerate() {
        if list[i].position != place + 1 {
            list[i].position = place + 1;
        }
    }
}

fn finish(config: Res<RaceConfig>, race: Res<Race>, mut racers: Query<&mut Racer>) {
    if rules::dnf_due(race.winner, race.time, config.dnf_timeout) {
        for mut r in &mut racers {
            if r.status == Status::Racing {
                r.status = Status::Dnf;
            }
        }
    }
}

type AiBody<'a> = (
    &'a Position,
    &'a Rotation,
    &'a LinearVelocity,
    &'a AngularVelocity,
    &'a CarTelemetry,
    &'a Children,
);

#[allow(clippy::type_complexity)]
fn drive_ai(
    time: Res<Time>,
    track: Res<RaceTrack>,
    config: Res<RaceConfig>,
    race: Res<Race>,
    mut commands: Commands,
    racers: Query<(Entity, &Racer, &Position, &LinearVelocity)>,
    mut drivers: Query<(Entity, &mut AiDriver, &mut CarControls, AiBody)>,
    contacts: Query<&CollidingEntities, With<CarBody>>,
    parents: Query<&ChildOf, With<CarBody>>,
) {
    let dt = time.delta_secs();
    let t = &track.0;
    let length = t.geometry.length;
    let others: Vec<Other> = racers
        .iter()
        .map(|(entity, r, pos, vel)| Other {
            entity,
            s: r.progress.distance.rem_euclid(length),
            lateral: r.progress.lateral,
            speed: vel.0.length(),
            position: pos.0,
            solid: !r.ghosted,
        })
        .collect();
    let human = racers
        .iter()
        .find(|(_, r, _, _)| r.driver == DriverKind::Human)
        .map(|(_, r, _, _)| r.progress.distance);
    for (entity, mut ai, mut controls, (pos, rot, vel, ang, telemetry, children)) in &mut drivers {
        let Ok((_, racer, _, _)) = racers.get(entity) else {
            continue;
        };
        // Contact from a car behind: remember who, for drivers that ram back.
        for child in children.iter() {
            let Ok(touching) = contacts.get(child) else {
                continue;
            };
            for other_body in touching.iter() {
                let Ok(parent) = parents.get(*other_body) else {
                    continue;
                };
                if let Ok((other, r, _, _)) = racers.get(parent.parent())
                    && r.progress.distance < racer.progress.distance
                {
                    ai.hit_by = Some((other, 3.0));
                }
            }
        }
        ai.catch_up = match (racer.status, human) {
            (Status::Racing, Some(h)) if config.catch_up => {
                let gap = h - racer.progress.distance;
                1.0 + 0.03 * (gap / 150.0).clamp(-1.0, 1.0)
            }
            (Status::Racing, _) => 1.0,
            // Cool-down lap after the finish.
            _ => 0.7,
        };
        let view = CarView {
            position: pos.0,
            rotation: rot.0,
            velocity: vel.0,
            angular_velocity: ang.0,
            telemetry: &telemetry.telemetry,
            s: racer.progress.distance.rem_euclid(length),
            lateral: racer.progress.lateral,
        };
        let mine: Vec<Other> = others
            .iter()
            .filter(|o| o.entity != entity)
            .copied()
            .collect();
        let mut c = ai.control(&view, &t.line, &t.geometry, &mine, race.time, dt);
        c.assists = racer.assists;
        controls.0 = c;
        if ai.wants_reset {
            ai.wants_reset = false;
            if race.phase == RacePhase::Running {
                commands.entity(entity).insert(ResetRequest);
            }
        }
    }
}

/// Cars wait on the grid with the brakes on until the start; a held
/// throttle engages launch control.
fn hold(race: Res<Race>, mut cars: Query<&mut CarControls, With<Racer>>) {
    if race.phase != RacePhase::Countdown {
        return;
    }
    for mut c in &mut cars {
        c.0.brake = 1.0;
    }
}

/// One row of the results table.
#[derive(Clone, Debug)]
pub struct ResultRow {
    pub position: usize,
    pub name: String,
    pub car: String,
    pub driver: DriverKind,
    pub status: Status,
    pub best_lap: Option<f32>,
    /// Gap to the winner: seconds, or laps behind for lapped finishers.
    pub gap: Option<Gap>,
    pub laps: u32,
    pub resets: usize,
    pub assists: Assists,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gap {
    Seconds(f32),
    Laps(u32),
}

/// The standings as a results table.
pub fn results(racers: &[(&Racer, &Car)]) -> Vec<ResultRow> {
    let mut rows: Vec<ResultRow> = racers
        .iter()
        .map(|(r, car)| ResultRow {
            position: r.position,
            name: r.name.clone(),
            car: car.vehicle.name.clone(),
            driver: r.driver,
            status: r.status,
            best_lap: r.timing.best_lap,
            gap: None,
            laps: r.progress.laps,
            resets: r.resets.len(),
            assists: r.assists,
        })
        .collect();
    rows.sort_by_key(|r| r.position);
    let winner = rows.first().map(|r| (r.status, r.laps));
    for row in &mut rows {
        row.gap = match (winner, row.status) {
            (Some((Status::Finished(w), wl)), Status::Finished(t)) => Some(if row.laps < wl {
                Gap::Laps(wl - row.laps)
            } else {
                Gap::Seconds(t - w)
            }),
            _ => None,
        };
    }
    rows
}
