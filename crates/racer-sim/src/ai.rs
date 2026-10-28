//! AI drivers. They drive through the same [`Controls`] as a player, on the
//! same physics, with the same assists available: no grip or power help.
//!
//! - **Line and speed:** each driver follows the track's generated racing
//!   line and a speed profile computed for its car from the car model at
//!   its difficulty's share of the limits (lateral grip, braking, drive).
//! - **Steering:** Stanley-style tracking at the front axle (heading error
//!   plus lateral error) with curvature feed-forward and yaw-rate damping,
//!   previewed a little ahead to cover tire lag.
//! - **Throttle and brake:** speed-profile tracking with feed-forward from
//!   the profile's slope and feedback from the speed error; the target is
//!   trimmed when the front tires slide past their peak slip angle, and the
//!   throttle is cut while the rear slides.
//! - **Racecraft:** following with a speed cap behind the car ahead,
//!   overtaking on a line beside it, defending the inside, keeping clear of
//!   cars alongside, recovering from spins and walls (reversing out), and
//!   asking for a reset when recovery fails.
//! - **Difficulty** scales line precision, braking-point accuracy, corner
//!   speed, mistakes, overtaking aggression and willingness to make contact.

use std::sync::Arc;

use bevy::prelude::*;
use racer_dynamics::{Assists, CarSpec, Controls, Telemetry, Vehicle, drivetrain::Drivetrain};
use racer_trackc::{CarModel, Geometry, Limits, RacingLine, line::bounds, profile::speed_profile};

/// Car length and width used for spacing (m), a little over the largest car.
pub const CAR_LENGTH: f32 = 4.8;
pub const CAR_WIDTH: f32 = 1.95;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Difficulty {
    Novice,
    Amateur,
    Pro,
    Expert,
    Alien,
}

impl Difficulty {
    pub const ALL: [Difficulty; 5] = [
        Difficulty::Novice,
        Difficulty::Amateur,
        Difficulty::Pro,
        Difficulty::Expert,
        Difficulty::Alien,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Difficulty::Novice => "Novice",
            Difficulty::Amateur => "Amateur",
            Difficulty::Pro => "Pro",
            Difficulty::Expert => "Expert",
            Difficulty::Alien => "Alien",
        }
    }

    pub fn skill(self) -> Skill {
        // Columns: lateral, braking, drive, line noise, brake-point spread,
        // mistakes per corner, aggression, contact.
        let (lateral, braking, drive, line_noise, brake_spread, mistakes, aggression, contact) =
            match self {
                Difficulty::Novice => (0.70, 0.55, 0.80, 0.45, 6.0, 0.08, 0.1, 0.0),
                Difficulty::Amateur => (0.78, 0.65, 0.86, 0.30, 4.0, 0.05, 0.3, 0.2),
                Difficulty::Pro => (0.85, 0.76, 0.92, 0.18, 2.5, 0.025, 0.5, 0.4),
                Difficulty::Expert => (0.925, 0.88, 1.0, 0.08, 1.2, 0.01, 0.75, 0.7),
                Difficulty::Alien => (0.995, 0.985, 1.0, 0.02, 0.4, 0.002, 0.9, 1.0),
            };
        Skill {
            limits: Limits {
                lateral,
                braking,
                drive,
            },
            line_noise,
            brake_spread,
            mistakes,
            aggression,
            contact,
        }
    }
}

/// What a difficulty changes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skill {
    /// Share of the car model's limits the speed profile uses.
    pub limits: Limits,
    /// Standard deviation of the slowly wandering line error (m).
    pub line_noise: f32,
    /// Standard deviation of the braking point per corner (m, positive late).
    pub brake_spread: f32,
    /// Chance of a mistake at each corner.
    pub mistakes: f32,
    /// 0..=1: how readily it overtakes and how close it follows.
    pub aggression: f32,
    /// 0..=1: 0 avoids any contact, 1 leans on others and rams back when hit.
    pub contact: f32,
}

/// Another car as an AI sees it.
#[derive(Clone, Copy, Debug)]
pub struct Other {
    pub entity: Entity,
    /// Centreline distance (wrapped to the lap) and lateral offset.
    pub s: f32,
    pub lateral: f32,
    pub speed: f32,
    pub position: Vec3,
    /// False while ghosted: it cannot be hit.
    pub solid: bool,
}

/// The car's state as the AI reads it.
#[derive(Clone, Copy, Debug)]
pub struct CarView<'a> {
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
    pub telemetry: &'a Telemetry,
    /// Centreline distance (wrapped to the lap) and lateral offset.
    pub s: f32,
    pub lateral: f32,
}

/// The car's geometry and limits the AI plans with.
#[derive(Clone, Debug)]
pub struct CarParams {
    pub wheelbase: f32,
    /// Distance from the centre of mass forward to the front axle.
    pub front_axle: f32,
    pub max_steer: f32,
    /// Peak tire slip angle (rad).
    pub peak_slip: f32,
    /// Deceleration the brakes would give at full pedal without grip limit
    /// (m/s^2); the pedal is mapped through it.
    pub brake_gain: f32,
    pub model: CarModel,
    pub drivetrain: Drivetrain,
    /// Driven wheel radius (m).
    pub wheel_radius: f32,
}

impl CarParams {
    /// The forward gear with the most wheel torque at speed `v`, keeping the
    /// engine a little under the limiter.
    pub fn best_gear(&self, v: f32) -> i8 {
        let gb = &self.drivetrain.gearbox;
        let engine = &self.drivetrain.engine;
        let wheel_omega = f64::from(v.max(0.0) / self.wheel_radius);
        (1..=gb.forward_gears())
            .filter(|&g| {
                gb.ratio(g) * wheel_omega < 0.96 * engine.limiter || g == gb.forward_gears()
            })
            .max_by(|&a, &b| {
                let force = |g: i8| {
                    engine.full_torque((gb.ratio(g) * wheel_omega).max(engine.launch)) * gb.ratio(g)
                };
                force(a).total_cmp(&force(b))
            })
            .unwrap_or(1)
    }
}

impl CarParams {
    pub fn new(spec: &CarSpec, vehicle: &Vehicle, model: CarModel) -> Self {
        let b = &spec.estimated.brakes;
        let torque = 2.0
            * (b.front_torque_nm / vehicle.tire(0).radius
                + b.rear_torque_nm / vehicle.tire(2).radius);
        CarParams {
            wheelbase: vehicle.wheelbase as f32,
            front_axle: -vehicle.wheel_geometry(0).hardpoint.z as f32,
            max_steer: vehicle.max_steer as f32,
            peak_slip: vehicle.tire(0).peak_slip_tan.atan() as f32,
            brake_gain: (torque / vehicle.mass) as f32,
            model,
            drivetrain: vehicle.drivetrain.clone(),
            wheel_radius: vehicle
                .tire(if vehicle.drivetrain.weights[0] > 0.0 {
                    0
                } else {
                    2
                })
                .radius as f32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mode {
    Race,
    /// Sliding: catch it before driving on.
    Recover,
    /// Backing away from a wall or turning round, until the time runs out.
    Reverse {
        left: f32,
        steer: f32,
    },
    /// Backing off straight from a car in the way, to get the room to drive
    /// round it; then pulls away straight and races on.
    BackOff {
        left: f32,
    },
    /// Pulling away after reversing.
    Pull {
        left: f32,
        steer: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Mistake {
    /// Brakes this many metres late.
    LateBrake(f32),
    /// Over-slows the corner.
    Lift,
    /// Runs wide by this many metres (sign: right).
    Wide(f32),
}

/// How far `o` is off the racing line where it is (positive right).
fn off_line(line: &RacingLine, o: &Other) -> f32 {
    let n = line.points.len();
    let k = line.points.partition_point(|p| p.s < o.s) % n;
    o.lateral - line.points[k].offset
}

/// Below this speed (m/s) a car in the way is an obstacle to drive round.
const STOPPED: f32 = 3.0;

/// The usable offset range where `o` is.
fn bounds_at(track: &Geometry, o: &Other) -> (f32, f32) {
    let n = track.samples.len();
    let k = track.samples.partition_point(|p| p.s < o.s) % n;
    bounds(&track.samples[k], CAR_WIDTH / 2.0)
}

/// What the driver aimed for in the last step, for telemetry.
#[derive(Clone, Copy, Debug, Default)]
pub struct AiDebug {
    pub target_speed: f32,
    pub grip_trim: f32,
    pub lateral_error: f32,
    pub front_slip: f32,
    pub rear_slip: f32,
    pub throttle: f32,
    pub brake: f32,
}

/// One AI driver's plan and state.
#[derive(Component, Clone, Debug)]
pub struct AiDriver {
    pub difficulty: Difficulty,
    pub skill: Skill,
    pub params: Arc<CarParams>,
    /// Speed at each line point at this driver's limits, and at the car's
    /// full limits (the ceiling for catch-up).
    speeds: Vec<f32>,
    full_speeds: Vec<f32>,
    /// Speed-target multiplier from the race's catch-up option.
    pub catch_up: f32,
    hint: usize,
    shift: f32,
    shift_target: f32,
    noise: f32,
    grip_trim: f32,
    steer_cut: f32,
    since_shift: f32,
    /// Seconds spent stuck behind a slower car.
    held_up: f32,
    /// Race clock at the last step.
    clock: f32,
    /// Usable offset range at the car; the shifts from the racing line that
    /// stay inside it over the road just ahead (where a pass lane has to
    /// fit); and the direction of the next corner (1 right, -1 left, 0 none
    /// close).
    bounds: (f32, f32),
    room: (f32, f32),
    next_turn: f32,
    /// Racing-line offset at the car in the last step.
    last_line: f32,
    /// Passing a stopped car: the lane beside it, as an offset from the
    /// centre (it does not move, so the lane is fixed on the road).
    lane: Option<f32>,
    /// Offsets left free by cars alongside, and whether there is no room.
    clear: (f32, f32),
    squeezed: bool,
    speed_integral: f32,
    mode: Mode,
    mistake: Option<(Mistake, f32)>,
    brake_offset: f32,
    approaching: bool,
    /// Car being passed, side (1 right), seconds left and lateral gap.
    overtaking: Option<(Entity, f32, f32, f32)>,
    defending: f32,
    /// Too close to the car being passed to get beside it going forward:
    /// how many metres short (backs off once stopped).
    boxed_in: Option<f32>,
    boxed_time: f32,
    /// No car close behind (room to back off).
    room_behind: bool,
    stuck_time: f32,
    recoveries: u32,
    last_progress: (f32, f32),
    /// Set by contact from another car (rams back if willing).
    pub hit_by: Option<(Entity, f32)>,
    /// Each other car's track distance at the last step, and whether it was
    /// last seen moving backwards along the track.
    seen: Vec<(Entity, f32, bool)>,
    rng: u64,
    /// Set when the driver gives up on recovering and wants a reset.
    pub wants_reset: bool,
    pub debug: AiDebug,
}

impl AiDriver {
    pub fn new(difficulty: Difficulty, params: CarParams, line: &RacingLine, seed: u64) -> Self {
        let skill = difficulty.skill();
        AiDriver {
            difficulty,
            skill,
            speeds: speed_profile(line, &params.model, skill.limits),
            full_speeds: speed_profile(line, &params.model, Limits::FULL),
            params: Arc::new(params),
            catch_up: 1.0,
            hint: 0,
            shift: 0.0,
            shift_target: 0.0,
            noise: 0.0,
            grip_trim: 1.0,
            steer_cut: 0.0,
            since_shift: 0.0,
            held_up: 0.0,
            clock: 0.0,
            bounds: (0.0, 0.0),
            room: (0.0, 0.0),
            next_turn: 0.0,
            last_line: 0.0,
            lane: None,
            clear: (f32::MIN, f32::MAX),
            squeezed: false,
            speed_integral: 0.0,
            mode: Mode::Race,
            mistake: None,
            brake_offset: 0.0,
            approaching: false,
            overtaking: None,
            defending: 0.0,
            boxed_in: None,
            boxed_time: 0.0,
            room_behind: true,
            stuck_time: 0.0,
            recoveries: 0,
            last_progress: (0.0, 0.0),
            hit_by: None,
            seen: Vec::new(),
            rng: seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1,
            wants_reset: false,
            debug: AiDebug::default(),
        }
    }

    /// Lap time of this driver's own speed profile on `line`.
    pub fn planned_lap_time(&self, line: &RacingLine) -> f32 {
        racer_trackc::profile::lap_time(line, &self.speeds)
    }

    fn rand(&mut self) -> f32 {
        self.rng ^= self.rng << 13;
        self.rng ^= self.rng >> 7;
        self.rng ^= self.rng << 17;
        (self.rng >> 40) as f32 / (1u64 << 24) as f32
    }

    fn gauss(&mut self) -> f32 {
        let (u, v) = (self.rand().max(1e-6), self.rand());
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    }

    /// Forgets the plan's state after the car was moved (reset, respawn).
    pub fn reset(&mut self) {
        self.mode = Mode::Race;
        self.mistake = None;
        self.overtaking = None;
        self.defending = 0.0;
        self.shift = 0.0;
        self.shift_target = 0.0;
        self.speed_integral = 0.0;
        self.grip_trim = 1.0;
        self.stuck_time = 0.0;
        self.recoveries = 0;
        self.wants_reset = false;
        self.hint = usize::MAX;
    }

    /// Controls for this step. `others` are the other cars on the track,
    /// `time` the race clock (for progress checks).
    pub fn control(
        &mut self,
        car: &CarView,
        line: &RacingLine,
        track: &Geometry,
        others: &[Other],
        time: f32,
        dt: f32,
    ) -> Controls {
        let p = self.params.clone();
        self.clock = time;
        let others = &self.along_track(others, track.length);
        let n = line.points.len();
        let forward = car.rotation * Vec3::NEG_Z;
        let v = car.velocity.dot(forward);
        let speed = car.velocity.length();
        let front = car.position + forward * p.front_axle;
        let preview = front + forward * (v.max(0.0) * 0.08);
        self.hint = if self.hint >= n {
            line.nearest(preview, car.s as usize % n, n / 2)
        } else {
            line.nearest(preview, self.hint, 30)
        };
        let k = self.hint;
        let lp = &line.points[k];
        let sample = &track.samples[k];

        self.update_line_choice(car, line, track, others, dt);
        let path_offset = lp.offset + self.shift + self.noise + self.mistake_wide();
        let (lo, hi) = bounds(sample, CAR_WIDTH / 2.0);
        let path_offset = path_offset
            .clamp(lo - 0.3, hi + 0.3)
            .clamp(self.clear.0, self.clear.1);
        let path_point = sample.at(path_offset, 0.0);
        // The path's own direction (it crosses the road between corners;
        // a lane round a stopped car is fixed on the road).
        let (behind, ahead_k) = ((k + n - 2) % n, (k + 2) % n);
        let line_shift = if self.lane.is_some() { 0.0 } else { 1.0 };
        let shifted = |i: usize| {
            let off = line_shift * (line.points[i].offset - lp.offset);
            track.samples[i].at(off + path_offset, 0.0)
        };
        let tangent = (shifted(ahead_k) - shifted(behind)).normalize_or(sample.tangent);
        let path_right = tangent.cross(sample.up).normalize_or(sample.right);

        // Steering (positive right).
        let plan = |v: Vec3| Vec2::new(v.x, v.z).normalize_or_zero();
        let (f2, t2) = (plan(forward), plan(tangent));
        let heading_error = f2.perp_dot(t2).atan2(f2.dot(t2));
        let lateral_error = (preview - path_point).dot(path_right);
        let ahead = (k + (v.max(0.0) * 0.25) as usize) % n;
        let base = if self.lane.is_some() {
            track.samples[ahead].curvature
        } else {
            line.points[ahead].curvature
        };
        let curvature = base / (1.0 - path_offset.min(4.0) * base).max(0.5);
        let yaw_right = -car.angular_velocity.y;
        let feed_forward = (p.wheelbase * curvature).atan();
        let mut delta = heading_error
            + (-2.5 * lateral_error / (v.abs() + 3.0)).atan()
            + feed_forward
            + 0.06 * (v * curvature - yaw_right);
        // Never ask the front tires for far more angle than the corner needs,
        // and back off the lock while they slide past their peak slip angle.
        let slack = 1.6 * p.peak_slip + 2.0 / (v.abs() + 2.0);
        delta = delta.clamp(feed_forward - slack, feed_forward + slack);
        let w = &car.telemetry.wheels;
        let front_alpha = ((w[0].slip_angle + w[1].slip_angle) / 2.0) as f32;
        let excess = front_alpha.abs() - p.peak_slip;
        self.steer_cut = if excess > 0.0 && speed > 5.0 {
            (self.steer_cut + 20.0 * excess * dt).min(0.25)
        } else {
            (self.steer_cut - 1.5 * dt).max(0.0)
        };
        if -front_alpha * delta > 0.0 {
            delta = delta.signum() * (delta.abs() - self.steer_cut).max(0.0);
        }

        // Speed target.
        let look = |d: f32| (k + (d.max(0.0) / (line.length / n as f32)) as usize) % n;
        let anticipation = v.max(0.0) * 0.1 + 1.0 - self.brake_offset - self.mistake_late();
        let target_index = look(anticipation);
        let mut target = self.speeds[target_index] * self.grip_trim;
        if self.catch_up != 1.0 {
            target = (target * self.catch_up).min(self.full_speeds[target_index]);
        }
        if matches!(self.mistake, Some((Mistake::Lift, _))) {
            target *= 0.85;
        }
        // Off the line the corner is tighter (or wider): scale by the curvature.
        let offset_from_line = path_offset - lp.offset;
        if offset_from_line.abs() > 0.5 {
            let kc = line.points[target_index].curvature;
            let k_off = kc / (1.0 - offset_from_line * kc).max(0.3);
            if k_off.abs() > kc.abs() {
                target = target.min(p.model.corner_speed(k_off, self.skill.limits));
            }
        }
        target = target.min(self.traffic_cap(car, line, track, others, target, dt));
        if self.squeezed {
            // No room beside the car alongside: drop back (cautious drivers more).
            target *= 0.97 - 0.07 * (1.0 - self.skill.contact);
        }
        self.corner_entry(v, k, line);

        // Longitudinal command: profile slope feed-forward plus speed feedback.
        let next = look(anticipation + 5.0);
        let slope = (self.speeds[next].powi(2) - self.speeds[target_index].powi(2)) / 10.0;
        let error = target - v;
        self.speed_integral = (self.speed_integral + error * dt).clamp(-1.0, 1.0);
        let accel = slope.clamp(-p.model.braking, p.model.traction)
            + 2.5 * error
            + 0.8 * self.speed_integral;
        let grade = tangent.y * 9.81;
        let resist = p.model.drag * v * v + grade;
        let mut throttle =
            ((accel + resist) / p.model.drive_at(v.max(0.0)).max(0.5)).clamp(0.0, 1.0);
        let mut brake = (-(accel + resist) / p.brake_gain).clamp(0.0, 1.0);
        if brake > 0.02 {
            throttle = 0.0;
        }

        // Slip feedback from the tires.
        let front_slip = w[0].slip_angle.abs().max(w[1].slip_angle.abs()) as f32;
        let rear_slip = w[2].slip_angle.abs().max(w[3].slip_angle.abs()) as f32;
        if front_slip > 1.5 * p.peak_slip && speed > 8.0 {
            self.grip_trim = (self.grip_trim - 0.3 * dt).max(0.92);
        } else {
            self.grip_trim = (self.grip_trim + 0.3 * dt).min(1.0);
        }
        if front_slip > 1.2 * p.peak_slip {
            brake *= 0.8;
        }
        // Squeeze the throttle in as the rear tires near their peak slip angle.
        if speed > 5.0 {
            throttle *= ((1.3 * p.peak_slip - rear_slip) / (0.5 * p.peak_slip)).clamp(0.2, 1.0);
        }

        // Sideslip: catch slides by steering into them and lifting.
        let sideslip = if speed > 4.0 {
            let vel = plan(car.velocity);
            f2.perp_dot(vel).atan2(f2.dot(vel))
        } else {
            0.0
        };
        self.debug = AiDebug {
            target_speed: target,
            grip_trim: self.grip_trim,
            lateral_error,
            front_slip,
            rear_slip,
            throttle,
            brake,
        };
        // Manual gears: always the one with the most drive at this speed
        // (an automatic upshifts early while the throttle is still coming in).
        let gear = car.telemetry.gear;
        let best = p.best_gear(v);
        self.since_shift += dt;
        let can_shift =
            self.since_shift > 0.4 || (best < gear && brake > 0.0 && self.since_shift > 0.15);
        let mut controls = Controls {
            throttle: f64::from(throttle),
            brake: f64::from(brake),
            steer: f64::from((delta / p.max_steer).clamp(-1.0, 1.0)),
            automatic: false,
            shift_up: can_shift && gear >= 1 && best > gear,
            shift_down: can_shift && gear > 1 && best < gear,
            assists: Assists::default(),
            ..default()
        };
        self.recover(
            &mut controls,
            car,
            heading_error,
            sideslip,
            speed,
            v,
            time,
            dt,
        );
        if controls.shift_up || controls.shift_down {
            self.since_shift = 0.0;
        }
        controls
    }

    /// How fast the path moves sideways (m/s): faster for aggressive drivers.
    fn shift_rate(&self) -> f32 {
        1.2 + 1.5 * self.skill.aggression
    }

    /// `others` with their speed signed along the track: a car reversing
    /// towards us is closing, not pulling away. The direction comes from how
    /// each car's track distance changed since the last step (kept while it
    /// does not move measurably).
    fn along_track(&mut self, others: &[Other], length: f32) -> Vec<Other> {
        let seen = std::mem::take(&mut self.seen);
        self.seen = others
            .iter()
            .map(|o| {
                let backwards = seen.iter().find(|x| x.0 == o.entity).is_some_and(|x| {
                    let ds = (o.s - x.1 + length / 2.0).rem_euclid(length) - length / 2.0;
                    if ds.abs() > 1e-3 { ds < 0.0 } else { x.2 }
                });
                (o.entity, o.s, backwards)
            })
            .collect();
        others
            .iter()
            .zip(&self.seen)
            .map(|(o, x)| Other {
                speed: if x.2 { -o.speed.abs() } else { o.speed },
                ..*o
            })
            .collect()
    }

    /// After the flag: the race sets a slow cool-down pace (`catch_up` 0.7;
    /// the catch-up option only moves it within 0.97..=1.03 while racing).
    fn cooling_down(&self) -> bool {
        self.catch_up < 0.9
    }

    fn mistake_late(&self) -> f32 {
        match self.mistake {
            Some((Mistake::LateBrake(m), _)) => m,
            _ => 0.0,
        }
    }

    fn mistake_wide(&self) -> f32 {
        match self.mistake {
            Some((Mistake::Wide(m), _)) => m,
            _ => 0.0,
        }
    }

    /// At each corner approach: a new braking-point error, maybe a mistake.
    fn corner_entry(&mut self, v: f32, k: usize, line: &RacingLine) {
        let n = line.points.len();
        let ahead = self.speeds[(k + (2.0 * v.max(10.0)) as usize) % n];
        let approaching = ahead < v - 6.0;
        if approaching && !self.approaching {
            self.brake_offset = self.gauss() * self.skill.brake_spread;
            if self.mistake.is_none() && self.rand() < self.skill.mistakes {
                let side = if line.points[(k + 40) % n].curvature > 0.0 {
                    -1.0
                } else {
                    1.0
                };
                let mistake = match (self.rand() * 3.0) as u32 {
                    0 => Mistake::LateBrake(4.0 + 6.0 * self.rand()),
                    1 => Mistake::Lift,
                    _ => Mistake::Wide(side * (0.8 + self.rand())),
                };
                self.mistake = Some((mistake, 4.0));
            }
        }
        self.approaching = approaching;
    }

    /// Line noise, overtaking, defending and keeping clear alongside: sets
    /// the sideways shift from the racing line.
    fn update_line_choice(
        &mut self,
        car: &CarView,
        line: &RacingLine,
        track: &Geometry,
        others: &[Other],
        dt: f32,
    ) {
        let n = line.points.len();
        let k = self.hint;
        let tau = 3.0;
        let g = self.gauss();
        self.noise += -self.noise / tau * dt + self.skill.line_noise * (2.0 * dt / tau).sqrt() * g;
        if let Some((_, left)) = &mut self.mistake {
            *left -= dt;
            if *left <= 0.0 {
                self.mistake = None;
            }
        }
        if let Some((_, t)) = &mut self.hit_by {
            *t -= dt;
            if *t <= 0.0 {
                self.hit_by = None;
            }
        }
        let length = track.length;
        let rel = |o: &Other| {
            let d = (o.s - car.s).rem_euclid(length);
            if d > length / 2.0 { d - length } else { d }
        };
        let (lo, hi) = bounds(&track.samples[k], CAR_WIDTH / 2.0);
        self.bounds = (lo, hi);
        // Lanes beside the racing line are planned as shifts from it (the
        // line sweeps across the road between corners). A shift has to keep
        // the path on the road over the stretch it takes to unwind it, not
        // only here: at a corner ahead the line runs to the edge.
        let spacing = track.length / n as f32;
        let reach = (2.0 * (car.telemetry.speed as f32).max(12.0) / spacing) as usize;
        let (room_lo, room_hi) = (0..=reach)
            .map(|i| {
                let j = (k + i) % n;
                let (l, h) = bounds(&track.samples[j], CAR_WIDTH / 2.0);
                (l - line.points[j].offset, h - line.points[j].offset)
            })
            .fold((f32::MIN, f32::MAX), |(a, b), (l, h)| (a.max(l), b.min(h)));
        self.room = (room_lo.min(0.0), room_hi.max(0.0));
        let room = self.room;
        self.next_turn = (10..150)
            .map(|i| line.points[(k + i) % n].curvature)
            .find(|c| c.abs() > 1.0 / 80.0)
            .map_or(0.0, f32::signum);
        let my_line = line.points[k].offset;
        let mut target = 0.0;
        self.lane = None;

        // Overtaking: hold a line beside the car being passed until clear;
        // give up if still behind it when the braking for a corner starts.
        if let Some((entity, side, left, gap)) = self.overtaking {
            let other = others.iter().find(|o| o.entity == entity);
            let passed = other.is_none_or(|o| rel(o) < -(CAR_LENGTH + 2.0));
            // A stopped car is driven round slowly, braking zone or not.
            let behind = other.is_some_and(|o| rel(o) > CAR_LENGTH * 0.6 && o.speed >= STOPPED);
            if passed || left <= 0.0 || (self.approaching && behind) {
                self.overtaking = None;
            } else {
                self.overtaking = Some((entity, side, left - dt, gap));
                if let Some(o) = others.iter().find(|o| o.entity == entity)
                    && o.speed < STOPPED
                {
                    // A stopped car: a lane fixed on the road beside it.
                    // The pass is slow, so the path can follow it even
                    // where the racing line sweeps across (a car stopped
                    // in a corner); a shift from the line would not fit.
                    let want = o.lateral + side * (CAR_WIDTH + gap);
                    let (l, h) = bounds_at(track, o);
                    if (l - 0.2..=h + 0.2).contains(&want) {
                        let want = want.clamp(l, h);
                        self.lane = Some(want);
                        target = want - my_line;
                    } else {
                        self.overtaking = None;
                    }
                } else if let Some(o) = others.iter().find(|o| o.entity == entity) {
                    // Beside the other car's own line, measured from the
                    // racing line where it is (it may be well ahead).
                    let want = off_line(line, o) + side * (CAR_WIDTH + gap);
                    let fits = (room.0 - 0.2..=room.1 + 0.2).contains(&want);
                    // Once alongside, hold the lane at the edge of the room:
                    // the other car keeps clear of us.
                    let alongside = rel(o) < CAR_LENGTH * 0.3;
                    if fits || alongside {
                        target = want.clamp(room.0, room.1);
                    } else {
                        // The car being passed took the room away (it moved
                        // to its apex), or the road ahead is too narrow for
                        // two: back off and follow instead.
                        self.overtaking = None;
                    }
                }
            }
        }

        // Defending: move to the inside before a braking zone when a car closes from behind.
        self.defending = (self.defending - dt).max(0.0);
        // On the cool-down lap the race is over: no defending.
        if self.overtaking.is_none() && self.skill.aggression >= 0.5 && !self.cooling_down() {
            let threat = others.iter().any(|o| {
                o.solid
                    && (-14.0..-CAR_LENGTH).contains(&rel(o))
                    && o.speed > car.telemetry.speed as f32 + 1.0
            });
            let corner = line.points[(k + 80) % n].curvature;
            if threat && corner.abs() > 1.0 / 60.0 && self.defending == 0.0 {
                self.defending = 3.0;
            }
            if self.defending > 0.0 {
                target = if corner > 0.0 {
                    room.1.min(2.5)
                } else {
                    room.0.max(-2.5)
                };
            }
        }

        // Keep clear of cars alongside: they bound where the path may go
        // (absolute offsets, since the racing line itself sweeps across).
        let me = car.lateral;
        let sep = CAR_WIDTH + 0.6 - 0.3 * self.skill.contact;
        self.clear = (lo, hi);
        // Alongside: from just ahead of us to overlapping our rear by a
        // metre. A car nose-to-tail behind us is its own driver's problem.
        for o in others
            .iter()
            .filter(|o| o.solid && (-(CAR_LENGTH - 1.0)..CAR_LENGTH + 1.5).contains(&rel(o)))
        {
            let rammed = self.hit_by.is_some_and(|(e, _)| e == o.entity)
                && self.skill.contact > 0.8
                && !self.cooling_down();
            if rammed {
                continue;
            }
            if o.lateral > me {
                self.clear.1 = self.clear.1.min(o.lateral - sep);
            } else {
                self.clear.0 = self.clear.0.max(o.lateral + sep);
            }
        }
        self.squeezed = self.clear.0 > self.clear.1;
        if self.squeezed {
            self.clear = (me - 0.3, me + 0.3);
        }
        self.shift_target = target.clamp(lo - my_line, hi - my_line);
        if self.lane.is_some() {
            // Hold the path where it is on the road while the line sweeps.
            self.shift -= my_line - self.last_line;
        }
        self.last_line = my_line;
        let rate = self.shift_rate();
        self.shift += (self.shift_target - self.shift).clamp(-rate * dt, rate * dt);
    }

    /// Speed cap behind the nearest solid car in our path; starts an overtake
    /// when held up.
    fn traffic_cap(
        &mut self,
        car: &CarView,
        line: &RacingLine,
        track: &Geometry,
        others: &[Other],
        want: f32,
        dt: f32,
    ) -> f32 {
        let track_length = track.length;
        let mut cap = f32::MAX;
        let mut blocker = None;
        let mut boxed_in = None;
        // Following distance: a fixed margin plus a time headway, both
        // shorter for drivers who accept contact or race aggressively.
        let boldness = self.skill.contact.max(self.skill.aggression * 0.6);
        let headway = 0.5 - 0.35 * boldness;
        let v = car.velocity.length();
        for o in others {
            let mut d = (o.s - car.s).rem_euclid(track_length);
            if d > track_length / 2.0 {
                d -= track_length;
            }
            if !(0.0..80.0).contains(&d) {
                continue;
            }
            // A ghosted car ahead turns solid once its ghost time is up and
            // no car is close: plan round it like a solid one until we are
            // on it. Once we are that close it stays ghosted, so creeping
            // through it is safe.
            if !o.solid && d < CAR_LENGTH {
                continue;
            }
            let ghost_floor = if o.solid { 0.0 } else { 3.0 };
            // In our path: off the racing line by about our own shift (the
            // line sweeps across the road between here and there), or
            // beside us right now.
            if (off_line(line, o) - self.shift).abs() > CAR_WIDTH + 0.4
                && (o.lateral - car.lateral).abs() > CAR_WIDTH + 0.2
            {
                continue;
            }
            // Being passed on a lane that clears it: no need to follow it,
            // only to be beside it before reaching it. The time to get
            // across bounds the closing speed, so a car stopped in the way
            // is driven round instead of waited behind. A car only gets
            // about 0.5 m across per metre it drives: closer than that it
            // follows (stops short) and backs off (see `recover`).
            let clears = match self.lane {
                Some(lane) => (lane - o.lateral).abs() > CAR_WIDTH + 0.4,
                None => (off_line(line, o) - self.shift_target).abs() > CAR_WIDTH + 0.4,
            };
            if let Some((_, _, _, lane_gap)) = self.overtaking.filter(|(e, ..)| *e == o.entity)
                && clears
            {
                let need = CAR_WIDTH + lane_gap / 2.0 - (o.lateral - car.lateral).abs();
                if need <= 0.0 {
                    continue;
                }
                let reach = d - CAR_LENGTH - 0.5;
                if need < 0.5 * reach {
                    let mut safe = o.speed + (reach * self.shift_rate() / need).max(1.5);
                    if o.speed < STOPPED {
                        // Go by a stopped car slowly: at speed the car cuts
                        // inside its path in a corner.
                        let braking = 0.5 * self.params.model.braking;
                        safe = safe.min((25.0 + 2.0 * braking * reach).sqrt());
                    }
                    if safe < cap {
                        cap = safe;
                        blocker = Some((*o, d));
                    }
                    continue;
                }
                boxed_in = Some(need / 0.5 - reach);
            }
            let rammed = self.hit_by.is_some_and(|(e, _)| e == o.entity)
                && self.skill.contact > 0.8
                && !self.cooling_down();
            let margin = if rammed {
                0.0
            } else {
                4.0 - 3.0 * boldness + headway * v
            };
            let gap = d - CAR_LENGTH - margin;
            let braking = 0.5 * self.params.model.braking;
            // A car coming back towards us (reversing) closes the gap too.
            let safe = if gap > 0.0 {
                (o.speed.max(0.0).powi(2) + 2.0 * braking * gap).sqrt() + o.speed.min(0.0)
            } else {
                o.speed + gap
            }
            .max(ghost_floor);
            if safe < cap {
                cap = safe;
                blocker = Some((*o, d));
            }
        }
        // Room to back off: nobody close behind.
        let room_behind = others.iter().all(|x| {
            let d = (car.s - x.s).rem_euclid(track_length);
            d > CAR_LENGTH + 5.0 || (x.lateral - car.lateral).abs() > CAR_WIDTH + 0.5
        });
        self.room_behind = room_behind;
        self.boxed_in = boxed_in.filter(|_| room_behind);
        // Held up by a slower car: after a while (shorter for aggressive
        // drivers) go for the inside of the next corner, or the other side
        // if there is no room there.
        // The pack is still forming in the first seconds: no passing yet.
        let held = self.clock > 5.0
            && blocker.is_some_and(|(o, d)| cap < want - 0.5 && d < 35.0 && want > o.speed + 0.5);
        // A car stopped in the way is an obstacle: go round it at once,
        // early enough to get across before reaching it.
        let obstacle =
            blocker.is_some_and(|(o, d)| o.speed < STOPPED && d < 15.0 + 2.0 * v.max(0.0));
        self.held_up = if held {
            self.held_up + dt
        } else {
            (self.held_up - 2.0 * dt).max(0.0)
        };
        // On the cool-down lap only stopped cars are passed.
        let racing = !self.cooling_down();
        if let Some((o, _)) = blocker
            && self.overtaking.is_none()
            && ((racing && self.held_up > 2.5 - 2.0 * self.skill.aggression) || obstacle)
        {
            // A lane beside the blocker (a shift from the racing line) that
            // stays on the road over the stretch ahead and is clear of every
            // other car in the next 35 m. Beside a stopped car the lane is
            // fixed on the road instead (see `update_line_choice`).
            let fixed = o.speed < STOPPED;
            let (lo, hi) = if fixed {
                bounds_at(track, &o)
            } else {
                self.room
            };
            let gap = 0.9;
            let place = |x: &Other| if fixed { x.lateral } else { off_line(line, x) };
            let beside = place(&o);
            let lane = |side: f32| {
                let at = beside + side * (CAR_WIDTH + gap);
                let free = others.iter().filter(|x| x.entity != o.entity).all(|x| {
                    let d = (x.s - car.s).rem_euclid(track_length);
                    d > 35.0 || (place(x) - at).abs() > CAR_WIDTH + 0.3
                });
                (lo + 0.3..=hi - 0.3).contains(&at) && free
            };
            let roomier = if hi - beside > beside - lo { 1.0 } else { -1.0 };
            // Round a stopped car: on the side we are already on, if there
            // is room, rather than across its nose.
            let ours = car.lateral - o.lateral;
            let first = if obstacle && ours.abs() > 0.5 {
                ours.signum()
            } else if self.next_turn != 0.0 && !obstacle {
                self.next_turn
            } else {
                roomier
            };
            if let Some(side) = [first, -first].into_iter().find(|&s| lane(s)) {
                self.overtaking = Some((o.entity, side, 8.0, gap));
            }
            self.held_up = 0.0;
        }
        cap.max(0.0)
    }

    /// Spin catching, wall recovery and reset requests.
    #[allow(clippy::too_many_arguments)]
    fn recover(
        &mut self,
        controls: &mut Controls,
        car: &CarView,
        heading_error: f32,
        sideslip: f32,
        speed: f32,
        v: f32,
        time: f32,
        dt: f32,
    ) {
        // Held on the grid before the start: nothing to recover from.
        if time < 0.0 {
            self.last_progress = (car.s, 0.0);
            return;
        }
        // Progress check: stuck when the car has gone nowhere for a while.
        if (car.s - self.last_progress.0).abs() > 4.0 {
            self.last_progress = (car.s, time);
            self.recoveries = 0;
        }
        match self.mode {
            Mode::Race => {
                if sideslip.abs() > 0.5 && speed > 6.0 {
                    self.mode = Mode::Recover;
                } else if speed < 0.8 && controls.throttle > 0.3 {
                    self.stuck_time += dt;
                } else {
                    self.stuck_time = 0.0;
                }
                // Stopped too close behind a car to drive round it: back
                // off far enough to get round.
                match self.boxed_in {
                    Some(_) if speed < 0.5 => self.boxed_time += dt,
                    _ => self.boxed_time = 0.0,
                }
                if let Some(short) = self.boxed_in
                    && self.boxed_time > 0.5
                {
                    self.boxed_time = 0.0;
                    self.mode = Mode::BackOff {
                        // Gear change, then about 2.5 m/s; 3 m spare.
                        left: (1.0 + (short + 3.0) / 2.5).min(4.0),
                    };
                }
                let facing_back = heading_error.abs() > 1.9 && speed < 4.0;
                if self.stuck_time > 0.8 || facing_back {
                    self.stuck_time = 0.0;
                    self.recoveries += 1;
                    if self.recoveries > 3 {
                        self.wants_reset = true;
                    }
                    // Back away with the wheels turned so the nose swings towards the track direction.
                    let steer = if heading_error > 0.0 { -1.0 } else { 1.0 };
                    self.mode = Mode::Reverse { left: 1.6, steer };
                }
            }
            Mode::Recover => {
                // Steer into the slide, off the pedals, until it is caught.
                controls.steer =
                    (f64::from(sideslip) * 1.8 / f64::from(self.params.max_steer)).clamp(-1.0, 1.0);
                controls.throttle = 0.0;
                controls.brake = if speed > 25.0 { 0.2 } else { 0.0 };
                if sideslip.abs() < 0.2 || speed < 3.0 {
                    self.mode = Mode::Race;
                }
            }
            Mode::Reverse { left, .. } | Mode::BackOff { left } => {
                let steer = match self.mode {
                    Mode::Reverse { steer, .. } => steer,
                    _ => 0.0,
                };
                controls.throttle = if v > -3.0 { 0.5 } else { 0.0 };
                controls.brake = 0.0;
                controls.steer = f64::from(steer);
                controls.automatic = false;
                if car.telemetry.gear >= 0 {
                    controls.throttle = 0.0;
                    controls.brake = if v.abs() > 0.5 { 1.0 } else { 0.0 };
                    controls.shift_down = v.abs() < 1.0;
                }
                let left = left - dt;
                self.mode = match self.mode {
                    Mode::Reverse { .. } if left > 0.0 => Mode::Reverse { left, steer },
                    Mode::Reverse { .. } => Mode::Pull {
                        left: 1.5,
                        steer: -steer,
                    },
                    // A car came up behind: stop backing off.
                    _ if left > 0.0 && self.room_behind => Mode::BackOff { left },
                    _ => Mode::Pull {
                        left: 0.8,
                        steer: 0.0,
                    },
                };
            }
            Mode::Pull { left, steer } => {
                controls.steer = f64::from(steer);
                controls.throttle = 0.6;
                controls.brake = 0.0;
                if car.telemetry.gear < 1 {
                    controls.automatic = false;
                    controls.throttle = 0.0;
                    controls.brake = if v.abs() > 0.5 { 1.0 } else { 0.0 };
                    controls.shift_up = v.abs() < 1.0;
                }
                let left = left - dt;
                self.mode = if left <= 0.0 || (speed > 6.0 && heading_error.abs() < 0.6) {
                    Mode::Race
                } else {
                    Mode::Pull { left, steer }
                };
            }
        }
        if time - self.last_progress.1 > 12.0 {
            self.wants_reset = true;
            self.last_progress.1 = time;
        }
    }
}
