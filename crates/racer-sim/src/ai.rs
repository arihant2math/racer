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

// TEMP tuning hook
pub static TUNE: std::sync::LazyLock<Vec<f32>> = std::sync::LazyLock::new(|| {
    std::env::var("RACER_AI_TUNE")
        .unwrap_or_default()
        .split(',')
        .filter_map(|x| x.parse().ok())
        .collect()
});
fn tune(i: usize, d: f32) -> f32 {
    TUNE.get(i).copied().unwrap_or(d)
}

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
    /// Racing-line offset at the car.
    line_offset: f32,
    shift: f32,
    shift_target: f32,
    noise: f32,
    grip_trim: f32,
    steer_cut: f32,
    since_shift: f32,
    speed_integral: f32,
    mode: Mode,
    mistake: Option<(Mistake, f32)>,
    brake_offset: f32,
    approaching: bool,
    overtaking: Option<(Entity, f32, f32)>,
    defending: f32,
    stuck_time: f32,
    recoveries: u32,
    last_progress: (f32, f32),
    /// Set by contact from another car (rams back if willing).
    pub hit_by: Option<(Entity, f32)>,
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
            line_offset: 0.0,
            shift: 0.0,
            shift_target: 0.0,
            noise: 0.0,
            grip_trim: 1.0,
            steer_cut: 0.0,
            since_shift: 0.0,
            speed_integral: 0.0,
            mode: Mode::Race,
            mistake: None,
            brake_offset: 0.0,
            approaching: false,
            overtaking: None,
            defending: 0.0,
            stuck_time: 0.0,
            recoveries: 0,
            last_progress: (0.0, 0.0),
            hit_by: None,
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
        let n = line.points.len();
        let forward = car.rotation * Vec3::NEG_Z;
        let right_car = car.rotation * Vec3::X;
        let v = car.velocity.dot(forward);
        let speed = car.velocity.length();
        let front = car.position + forward * p.front_axle;
        let preview = front + forward * (v.max(0.0) * tune(3, 0.08));
        self.hint = if self.hint >= n {
            line.nearest(preview, car.s as usize % n, n / 2)
        } else {
            line.nearest(preview, self.hint, 30)
        };
        let k = self.hint;
        let lp = &line.points[k];
        let sample = &track.samples[k];
        self.line_offset = lp.offset;

        self.update_line_choice(car, line, track, others, dt);
        let path_offset = lp.offset + self.shift + self.noise + self.mistake_wide();
        let (lo, hi) = bounds(sample, CAR_WIDTH / 2.0);
        let path_offset = path_offset.clamp(lo - 0.3, hi + 0.3);
        let path_point = sample.at(path_offset, 0.0);
        // The path's own direction (it crosses the road between corners).
        let (behind, ahead_k) = ((k + n - 2) % n, (k + 2) % n);
        let shifted =
            |i: usize| track.samples[i].at(line.points[i].offset + path_offset - lp.offset, 0.0);
        let tangent = (shifted(ahead_k) - shifted(behind)).normalize_or(sample.tangent);
        let path_right = tangent.cross(sample.up).normalize_or(sample.right);

        // Steering (positive right).
        let plan = |v: Vec3| Vec2::new(v.x, v.z).normalize_or_zero();
        let (f2, t2) = (plan(forward), plan(tangent));
        let heading_error = f2.perp_dot(t2).atan2(f2.dot(t2));
        let lateral_error = (preview - path_point).dot(path_right);
        let ahead = (k + (v.max(0.0) * tune(0, 0.25)) as usize) % n;
        let curvature = line.points[ahead].curvature
            / (1.0 - path_offset.min(4.0) * line.points[ahead].curvature).max(0.5);
        let yaw_right = -car.angular_velocity.y;
        let feed_forward = (p.wheelbase * curvature).atan();
        let mut delta = heading_error
            + (-tune(1, 2.5) * lateral_error / (v.abs() + 3.0)).atan()
            + feed_forward
            + tune(2, 0.06) * (v * curvature - yaw_right);
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
        target = target.min(self.traffic_cap(car, track.length, others, v));
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
            throttle *=
                ((tune(4, 1.3) * p.peak_slip - rear_slip) / (0.5 * p.peak_slip)).clamp(0.2, 1.0);
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
            right_car,
            time,
            dt,
        );
        if controls.shift_up || controls.shift_down {
            self.since_shift = 0.0;
        }
        controls
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
        let my_line = line.points[k].offset;
        let mut target = 0.0;

        // Overtaking: hold a line beside the car being passed until clear.
        if let Some((entity, side, left)) = self.overtaking {
            let passed = others
                .iter()
                .find(|o| o.entity == entity)
                .is_none_or(|o| rel(o) < -(CAR_LENGTH + 2.0));
            if passed || left <= 0.0 {
                self.overtaking = None;
            } else {
                self.overtaking = Some((entity, side, left - dt));
                if let Some(o) = others.iter().find(|o| o.entity == entity) {
                    let want = o.lateral + side * (CAR_WIDTH + 0.9);
                    target = (want.clamp(lo, hi)) - my_line;
                }
            }
        }

        // Defending: move to the inside before a braking zone when a car closes from behind.
        self.defending = (self.defending - dt).max(0.0);
        if self.overtaking.is_none() && self.skill.aggression >= 0.5 {
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
                let inside = if corner > 0.0 { hi } else { lo };
                target = (inside - my_line).clamp(-2.5, 2.5);
            }
        }

        // Keep clear of a car alongside.
        let me = car.lateral;
        for o in others.iter().filter(|o| o.solid) {
            let d = rel(o);
            if d.abs() < CAR_LENGTH + 1.0 {
                let gap = (o.lateral - me).abs();
                let want = CAR_WIDTH + 0.6 - 0.5 * self.skill.contact;
                if gap < want {
                    let away = if o.lateral > me { -1.0 } else { 1.0 };
                    let rammed =
                        self.hit_by.is_some_and(|(e, _)| e == o.entity) && self.skill.contact > 0.8;
                    if !rammed {
                        target = target.min(hi - my_line).max(lo - my_line) + away * (want - gap);
                    }
                }
            }
        }
        self.shift_target = target.clamp(lo - my_line, hi - my_line);
        let rate = 1.2 + 1.5 * self.skill.aggression;
        self.shift += (self.shift_target - self.shift).clamp(-rate * dt, rate * dt);
    }

    /// Speed cap behind the nearest solid car in our path; starts an overtake
    /// when held up.
    fn traffic_cap(&mut self, car: &CarView, track_length: f32, others: &[Other], v: f32) -> f32 {
        let mut cap = f32::MAX;
        let my_path = self.line_offset + self.shift;
        let mut blocker = None;
        for o in others.iter().filter(|o| o.solid) {
            let mut d = (o.s - car.s).rem_euclid(track_length);
            if d > track_length / 2.0 {
                d -= track_length;
            }
            if !(0.0..80.0).contains(&d) {
                continue;
            }
            if (o.lateral - my_path).abs() > CAR_WIDTH + 0.4
                && (o.lateral - car.lateral).abs() > CAR_WIDTH + 0.2
            {
                continue;
            }
            let rammed =
                self.hit_by.is_some_and(|(e, _)| e == o.entity) && self.skill.contact > 0.8;
            let margin = if rammed {
                0.0
            } else {
                5.0 - 3.5 * self.skill.contact.max(self.skill.aggression * 0.6)
            };
            let gap = d - CAR_LENGTH - margin;
            let braking = 0.6 * self.params.model.braking;
            let safe = if gap > 0.0 {
                (o.speed.max(0.0).powi(2) + 2.0 * braking * gap).sqrt()
            } else {
                o.speed + gap
            };
            if safe < cap {
                cap = safe;
                blocker = Some((*o, d));
            }
        }
        // Held up and close: try a pass on the side with more room.
        if let Some((o, d)) = blocker
            && self.overtaking.is_none()
            && cap < v + 1.0
            && d < 30.0
            && self.speeds[self.hint] > o.speed + 1.0
            && self.rand() < self.skill.aggression * 0.05
        {
            let side = if o.lateral > 0.0 { -1.0 } else { 1.0 };
            self.overtaking = Some((o.entity, side, 8.0));
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
        _right: Vec3,
        time: f32,
        dt: f32,
    ) {
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
            Mode::Reverse { left, steer } => {
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
                self.mode = if left <= 0.0 {
                    Mode::Pull {
                        left: 1.5,
                        steer: -steer,
                    }
                } else {
                    Mode::Reverse { left, steer }
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
