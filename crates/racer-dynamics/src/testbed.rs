//! Headless test bench: a minimal rigid-body integrator over a height-field
//! ground, wheel contacts by an analytic sphere cast, and scripted drivers
//! for the reference maneuvers. The game uses Avian instead; this exists so
//! `cargo test` can drive every car without Bevy.

use glam::{DMat3, DQuat, DVec3};

use crate::{
    assists::Assists,
    chassis_dt,
    tire::Surface,
    vehicle::{ChassisState, Controls, GRAVITY, StepOutput, Vehicle, VehicleState, WheelContact},
};

/// Ground as a height field with +Y up.
pub trait Ground {
    fn height(&self, x: f64, z: f64) -> f64;

    fn normal(&self, x: f64, z: f64) -> DVec3 {
        let e = 0.02;
        let dx = (self.height(x + e, z) - self.height(x - e, z)) / (2.0 * e);
        let dz = (self.height(x, z + e) - self.height(x, z - e)) / (2.0 * e);
        DVec3::new(-dx, 1.0, -dz).normalize()
    }

    fn surface(&self, _x: f64, _z: f64) -> Surface {
        Surface::DryAsphalt
    }
}

/// Flat dry asphalt at y = 0.
pub struct Flat;

impl Ground for Flat {
    fn height(&self, _x: f64, _z: f64) -> f64 {
        0.0
    }
}

/// Half extents of the body box used for chassis-ground contact.
const BODY_HALF: DVec3 = DVec3::new(0.85, 0.35, 2.2);

/// A vehicle on a ground, integrated at the chassis rate.
pub struct Sim<'a> {
    pub vehicle: &'a Vehicle,
    pub state: VehicleState,
    pub body: ChassisState,
    pub ground: &'a dyn Ground,
    pub time: f64,
    /// Acceleration over the last step (world frame, without gravity).
    pub accel: DVec3,
    pub last: Option<StepOutput>,
}

impl<'a> Sim<'a> {
    /// Places the car at rest at static ride height at the origin, facing -Z.
    pub fn new(vehicle: &'a Vehicle, ground: &'a dyn Ground) -> Self {
        let mut sim = Sim {
            vehicle,
            state: vehicle.initial_state(),
            body: ChassisState {
                position: DVec3::new(0.0, ground.height(0.0, 0.0) + vehicle.ride_height(), 0.0),
                rotation: DQuat::IDENTITY,
                linear_velocity: DVec3::ZERO,
                angular_velocity: DVec3::ZERO,
            },
            ground,
            time: 0.0,
            accel: DVec3::ZERO,
            last: None,
        };
        sim.run(0.5, |_| Controls {
            brake: 1.0,
            ..Controls::default()
        });
        sim.time = 0.0;
        sim
    }

    /// Sets the car rolling straight ahead at `speed` in a gear that suits it.
    pub fn launch_at(&mut self, speed: f64) {
        let forward = self.body.rotation * DVec3::NEG_Z;
        self.body.linear_velocity = forward * speed;
        for (i, w) in self.state.wheels.iter_mut().enumerate() {
            w.omega = speed / self.vehicle.wheel_geometry(i).radius;
        }
        let drive = &self.vehicle.drivetrain;
        let gb = &drive.gearbox;
        let out = drive.output_speed(&self.state.wheels.map(|w| w.omega));
        let ds = &mut self.state.drive;
        ds.gear = (1..=gb.forward_gears())
            .find(|&g| gb.ratio(g) * out < 0.75 * gb.upshift[0])
            .unwrap_or(gb.forward_gears());
        ds.engine_omega = gb.ratio(ds.gear) * out;
    }

    pub fn forward(&self) -> DVec3 {
        self.body.rotation * DVec3::NEG_Z
    }

    pub fn speed(&self) -> f64 {
        self.body.linear_velocity.dot(self.forward())
    }

    pub fn contacts(&self) -> [Option<WheelContact>; 4] {
        std::array::from_fn(|i| {
            let g = self.vehicle.wheel_geometry(i);
            let up = self.body.rotation * DVec3::Y;
            let hp = self.body.position + self.body.rotation * g.hardpoint;
            // Sphere cast along -up: solve n . (hp - up d - q) = r on the
            // ground's tangent plane under the wheel, refined twice.
            let mut d = g.max_distance;
            let mut normal = DVec3::Y;
            for _ in 0..3 {
                let c = hp - up * d.clamp(0.0, g.max_distance);
                let q = DVec3::new(c.x, self.ground.height(c.x, c.z), c.z);
                normal = self.ground.normal(c.x, c.z);
                let along = normal.dot(up);
                if along < 0.3 {
                    return None;
                }
                d = (normal.dot(hp - q) - g.radius) / along;
            }
            let c = hp - up * d;
            (d <= g.max_distance).then(|| WheelContact {
                distance: d.max(0.0),
                normal,
                surface: self.ground.surface(c.x, c.z),
            })
        })
    }

    /// One chassis step with `controls`.
    pub fn step(&mut self, controls: &Controls) {
        let dt = chassis_dt();
        let contacts = self.contacts();
        let out = self
            .vehicle
            .step(&mut self.state, &self.body, controls, &contacts, dt);
        let (body_force, body_torque) = self.body_contact();
        let b = &mut self.body;
        let m = self.vehicle.mass;
        let force = out.force + body_force;
        let v0 = b.linear_velocity;
        b.linear_velocity += (force / m - DVec3::Y * GRAVITY) * dt;
        b.position += b.linear_velocity * dt;
        // Rigid-body rotation with the gyroscopic term, inertia in world frame.
        let r = DMat3::from_quat(b.rotation);
        let inertia = r * DMat3::from_diagonal(self.vehicle.inertia) * r.transpose();
        let w = b.angular_velocity;
        let torque = out.torque + body_torque;
        b.angular_velocity += inertia.inverse() * (torque - w.cross(inertia * w)) * dt;
        let w = b.angular_velocity;
        b.rotation = (b.rotation + DQuat::from_xyzw(w.x, w.y, w.z, 0.0) * b.rotation * (0.5 * dt))
            .normalize();
        self.accel = (b.linear_velocity - v0) / dt + DVec3::Y * GRAVITY;
        self.time += dt;
        self.last = Some(out);
    }

    /// Runs `driver` for `seconds`.
    pub fn run(&mut self, seconds: f64, mut driver: impl FnMut(&Sim) -> Controls) {
        let end = self.time + seconds;
        while self.time < end - 1e-9 {
            let c = driver(self);
            self.step(&c);
        }
    }

    /// Penalty contact of the body box corners with the ground (crashes,
    /// rollovers, bottoming out on landings).
    fn body_contact(&self) -> (DVec3, DVec3) {
        let b = &self.body;
        let (mut f, mut t) = (DVec3::ZERO, DVec3::ZERO);
        for i in 0..8 {
            let s = |bit: usize| if i & bit == 0 { -1.0 } else { 1.0 };
            let r = b.rotation * (BODY_HALF * DVec3::new(s(1), s(2), s(4)) + DVec3::Y * 0.15);
            let p = b.position + r;
            let depth = self.ground.height(p.x, p.z) - p.y;
            if depth <= 0.0 {
                continue;
            }
            let n = self.ground.normal(p.x, p.z);
            let v = b.linear_velocity + b.angular_velocity.cross(r);
            let normal = (2.0e5 * depth - 2.0e4 * v.dot(n)).max(0.0);
            let slide = v - n * v.dot(n);
            let friction = -slide.normalize_or_zero() * (0.5 * normal).min(slide.length() * 2.0e4);
            let force = n * normal + friction;
            f += force;
            t += r.cross(force);
        }
        (f, t)
    }
}

/// Throttle/brake and steering commands from simple feedback laws.
pub mod driver {
    use super::*;

    /// Steering input that holds the car on the line through `origin` along `dir` (XZ plane).
    pub fn hold_line(sim: &Sim, origin: DVec3, dir: DVec3) -> f64 {
        let offset = (sim.body.position - origin).dot(dir.cross(DVec3::Y).normalize());
        let heading = sim.forward().cross(dir).y;
        (0.3 * sim.body.angular_velocity.y - 2.0 * heading - 0.05 * offset).clamp(-1.0, 1.0)
    }

    /// Throttle and brake that track `target` forward speed.
    pub fn hold_speed(sim: &Sim, target: f64) -> (f64, f64) {
        let e = target - sim.speed();
        if e >= 0.0 {
            ((0.3 + 0.5 * e).min(1.0), 0.0)
        } else {
            (0.0, (-0.3 * e).min(1.0))
        }
    }

    /// Pure-pursuit steering towards `target` (world XZ).
    pub fn pursue(sim: &Sim, target: DVec3) -> f64 {
        let to = sim.body.rotation.inverse() * (target - sim.body.position);
        let lookahead = to.length().max(1.0);
        // Angle of the target left of straight ahead (body -Z).
        let alpha = (-to.x).atan2(-to.z);
        let delta = (2.0 * sim.vehicle.wheelbase * alpha.sin() / lookahead).atan();
        (-delta / sim.vehicle.max_steer).clamp(-1.0, 1.0)
    }
}

/// Automatic gearbox with all assists, the setup the reference tests use.
pub fn auto_controls() -> Controls {
    Controls {
        automatic: true,
        assists: Assists::default(),
        ..Controls::default()
    }
}

/// Standing start to 100 km/h with launch control; seconds from brake release.
pub fn zero_to_100(vehicle: &Vehicle) -> f64 {
    let mut sim = Sim::new(vehicle, &Flat);
    let base = auto_controls();
    let (origin, dir) = (sim.body.position, sim.forward());
    sim.run(1.5, |_| Controls {
        throttle: 1.0,
        brake: 1.0,
        ..base
    });
    sim.time = 0.0;
    while sim.speed() < 100.0 / 3.6 && sim.time < 30.0 {
        let steer = driver::hold_line(&sim, origin, dir);
        sim.step(&Controls {
            throttle: 1.0,
            steer,
            ..base
        });
    }
    sim.time
}

/// Full ABS stop from 100 km/h; metres.
pub fn braking_100_to_0(vehicle: &Vehicle) -> f64 {
    let mut sim = Sim::new(vehicle, &Flat);
    sim.launch_at(100.0 / 3.6 + 0.5);
    let (origin, dir) = (sim.body.position, sim.forward());
    let base = auto_controls();
    sim.run(1.0, |s| {
        let (throttle, brake) = driver::hold_speed(s, 100.0 / 3.6 + 0.5);
        Controls {
            throttle,
            brake,
            steer: driver::hold_line(s, origin, dir),
            ..base
        }
    });
    while sim.speed() > 100.0 / 3.6 {
        sim.step(&Controls {
            steer: driver::hold_line(&sim, origin, dir),
            ..base
        });
    }
    let start = sim.body.position;
    while sim.speed() > 0.05 && sim.time < 60.0 {
        let steer = driver::hold_line(&sim, origin, dir);
        sim.step(&Controls {
            brake: 1.0,
            steer,
            ..base
        });
    }
    (sim.body.position - start).length()
}

/// Steady-state skidpad on a circle of `radius` (left turn), speed raised
/// slowly until the car can no longer hold the line; peak sustained lateral
/// acceleration in g.
pub fn skidpad(vehicle: &Vehicle, radius: f64) -> f64 {
    let mut sim = Sim::new(vehicle, &Flat);
    // Centre of the circle to the car's left.
    let center = sim.body.position + DVec3::new(-radius, 0.0, 0.0);
    let base = Controls {
        assists: Assists::OFF,
        ..auto_controls()
    };
    let start_speed = (0.6 * GRAVITY * radius).sqrt();
    sim.launch_at(start_speed);
    let mut target = start_speed;
    let mut best: f64 = 0.0;
    let mut window = Vec::new();
    while sim.time < 120.0 {
        let rel = sim.body.position - center;
        let rel = DVec3::new(rel.x, 0.0, rel.z);
        let r = rel.length();
        let error = r - radius;
        if error.abs() > 2.0 {
            break;
        }
        // Aim at a point on the circle ahead of the car (counter-clockwise seen from above).
        let ahead = (0.6 * sim.speed()).max(6.0) / radius;
        let angle = rel.z.atan2(rel.x) - ahead;
        let aim = center + DVec3::new(angle.cos(), 0.0, angle.sin()) * radius;
        let (throttle, brake) = driver::hold_speed(&sim, target);
        let steer = driver::pursue(&sim, aim);
        sim.step(&Controls {
            throttle,
            brake,
            steer,
            ..base
        });
        target += 0.08 * chassis_dt();
        // Lateral acceleration = v^2 / r, averaged over 1 s while on the line.
        let v = DVec3::new(sim.body.linear_velocity.x, 0.0, sim.body.linear_velocity.z).length();
        window.push(if error.abs() < 0.75 { v * v / r } else { 0.0 });
        if window.len() > 240 {
            window.remove(0);
            best = best.max(window.iter().sum::<f64>() / window.len() as f64);
        }
    }
    best / GRAVITY
}

/// Full throttle until the car stops accelerating; km/h.
pub fn top_speed(vehicle: &Vehicle) -> f64 {
    let mut sim = Sim::new(vehicle, &Flat);
    sim.launch_at(30.0);
    let (origin, dir) = (sim.body.position, sim.forward());
    let base = auto_controls();
    let mut best: f64 = 0.0;
    let mut best_at = 0.0;
    while sim.time < 240.0 && sim.time - best_at < 10.0 {
        let steer = driver::hold_line(&sim, origin, dir);
        sim.step(&Controls {
            throttle: 1.0,
            steer,
            ..base
        });
        if sim.speed() > best + 0.01 {
            best = sim.speed();
            best_at = sim.time;
        }
    }
    best * 3.6
}

/// Yaw response to a steering step at speed.
#[derive(Clone, Copy, Debug)]
pub struct StepSteer {
    /// Yaw rate (rad/s) peak and mean over the last second.
    pub peak_yaw_rate: f64,
    pub steady_yaw_rate: f64,
    /// Steady lateral acceleration (g).
    pub lateral_g: f64,
    /// Largest body sideslip angle (rad).
    pub max_sideslip: f64,
    /// Largest yaw-rate change over the last second (rad/s), a jitter/divergence measure.
    pub final_variation: f64,
}

/// Holds `speed`, then steps the steering to `steer` and holds for 4 s.
pub fn step_steer(vehicle: &Vehicle, speed: f64, steer: f64, assists: Assists) -> StepSteer {
    let mut sim = Sim::new(vehicle, &Flat);
    sim.launch_at(speed);
    let base = Controls {
        assists,
        ..auto_controls()
    };
    let (origin, dir) = (sim.body.position, sim.forward());
    sim.run(1.0, |s| {
        let (throttle, brake) = driver::hold_speed(s, speed);
        Controls {
            throttle,
            brake,
            steer: driver::hold_line(s, origin, dir),
            ..base
        }
    });
    // Throttle held at the value that cruised at `speed`, as in an open-loop step-steer test.
    let throttle = driver::hold_speed(&sim, speed).0;
    let (mut peak, mut sideslip) = (0.0f64, 0.0f64);
    let mut last = Vec::new();
    let mut lat = Vec::new();
    while sim.time < 5.0 {
        sim.step(&Controls {
            throttle,
            steer,
            ..base
        });
        let yaw = sim.body.angular_velocity.y;
        peak = peak.max(yaw.abs());
        let v = sim.body.linear_velocity;
        if v.length() > 1.0 {
            sideslip = sideslip.max(sim.forward().cross(v.normalize()).y.abs().asin());
        }
        if sim.time > 4.0 {
            last.push(yaw.abs());
            lat.push(sim.accel.dot(sim.body.rotation * DVec3::X).abs());
        }
    }
    let mean = |x: &[f64]| x.iter().sum::<f64>() / x.len() as f64;
    let (lo, hi) = last
        .iter()
        .fold((f64::MAX, 0.0f64), |(lo, hi), &y| (lo.min(y), hi.max(y)));
    StepSteer {
        peak_yaw_rate: peak,
        steady_yaw_rate: mean(&last),
        lateral_g: mean(&lat) / GRAVITY,
        max_sideslip: sideslip,
        final_variation: hi - lo,
    }
}
