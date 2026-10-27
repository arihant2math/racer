//! Speed profiles: the fastest speed a car can carry at every point of a
//! line, from a quasi-steady point-mass model with a friction ellipse.
//!
//! The model's numbers come from the car's `racer-dynamics` parameters: the
//! sustained lateral grip and the braking deceleration are measured by
//! driving the vehicle model on the headless testbed (skidpad and 100-0
//! stop), the drive acceleration at each speed comes from a full-throttle
//! run on the testbed (shift gaps, traction control and rotating inertia
//! included), and drag and lift come from the aero fit.

use racer_dynamics::{CarSpec, Controls, DVec3, Vehicle, spec::Layout, testbed, vehicle::GRAVITY};

use crate::line::RacingLine;

const AIR_DENSITY: f32 = 1.225;

/// Point-mass performance model of one car.
#[derive(Clone, Debug, PartialEq)]
pub struct CarModel {
    pub id: String,
    pub mass: f32,
    /// Sustained lateral acceleration (m/s^2) at low speed.
    pub lateral: f32,
    /// Braking deceleration (m/s^2) with ABS.
    pub braking: f32,
    /// Traction limit of the driven wheels (m/s^2).
    pub traction: f32,
    /// Drag and lift as acceleration per (m/s)^2: 0.5 rho CdA / m, 0.5 rho ClA / m.
    pub drag: f32,
    pub lift: f32,
    /// Top speed (governed or where drive force meets drag), m/s.
    pub top_speed: f32,
    /// Best-gear wheel force over mass (m/s^2) at 0, 1, 2, ... m/s.
    pub drive: Vec<f32>,
}

/// Drive acceleration (net of drag, which the model subtracts itself) per
/// 1 m/s speed band, from a launch-control start at full throttle on the
/// testbed: shift gaps, traction control, wheel and engine inertia and
/// rolling resistance are all in it.
fn full_throttle_run(spec: &CarSpec, vehicle: &Vehicle) -> Vec<f32> {
    let mut sim = testbed::Sim::new(vehicle, &testbed::Flat);
    let base = testbed::auto_controls();
    sim.run(1.0, |_| Controls {
        throttle: 1.0,
        brake: 1.0,
        ..base
    });
    let drag = 0.5
        * f64::from(AIR_DENSITY)
        * spec.aero.drag_coefficient
        * spec.estimated.aero.frontal_area_m2;
    let mut sum = vec![0.0f64; 120];
    let mut count = vec![0u32; 120];
    let mut last = sim.speed();
    let dt = racer_dynamics::chassis_dt();
    let mut since_gain = 0.0;
    while sim.time < 120.0 && since_gain < 3.0 {
        let steer = testbed::driver::hold_line(&sim, DVec3::ZERO, DVec3::NEG_Z);
        sim.step(&Controls {
            throttle: 1.0,
            steer,
            ..base
        });
        let v = sim.speed();
        let a = (v - last) / dt + drag * v * v / vehicle.mass;
        since_gain = if v > last + 1e-4 {
            0.0
        } else {
            since_gain + dt
        };
        last = v;
        let band = v.max(0.0) as usize;
        if band < sum.len() {
            sum[band] += a;
            count[band] += 1;
        }
    }
    let mut out: Vec<f32> = sum
        .iter()
        .zip(&count)
        .map(|(s, &c)| {
            if c > 0 {
                (s / f64::from(c)) as f32
            } else {
                0.0
            }
        })
        .collect();
    // Bands the launch passed in under a tick take the next band's value.
    for i in (0..out.len() - 1).rev() {
        if count[i] == 0 && out[i + 1] > 0.0 {
            out[i] = out[i + 1];
        }
    }
    out
}

/// Fractions of the model a driver uses (1.0 = the limit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Limits {
    pub lateral: f32,
    pub braking: f32,
    pub drive: f32,
}

impl Limits {
    pub const FULL: Limits = Limits {
        lateral: 1.0,
        braking: 1.0,
        drive: 1.0,
    };
}

impl CarModel {
    /// Measures the model for a car (about a second of testbed driving).
    pub fn from_spec(spec: &CarSpec) -> Result<Self, String> {
        let vehicle = Vehicle::from_spec(spec)?;
        let mass = vehicle.mass as f32;
        let lateral = (testbed::skidpad(&vehicle, 45.72) * GRAVITY) as f32;
        let stop = testbed::braking_100_to_0(&vehicle) as f32;
        let braking = (100.0f32 / 3.6).powi(2) / (2.0 * stop);
        let f = spec.chassis.front_weight_fraction as f32;
        let driven = match spec.driveline.layout {
            Layout::Fwd => f,
            Layout::Rwd => 1.0 - f,
            Layout::Awd => 1.0,
        };
        let wheel = if spec.driveline.layout == Layout::Fwd {
            0
        } else {
            2
        };
        let static_load = vehicle.mass * GRAVITY / 4.0;
        let mu_long = vehicle.tire(wheel).mu(static_load).0 as f32;
        let traction = mu_long * GRAVITY as f32 * driven;
        let aero = &spec.estimated.aero;
        let drag =
            0.5 * AIR_DENSITY * (spec.aero.drag_coefficient * aero.frontal_area_m2) as f32 / mass;
        let lift =
            0.5 * AIR_DENSITY * (aero.lift_area_front_m2 + aero.lift_area_rear_m2) as f32 / mass;
        let governed = spec
            .engine
            .top_speed_governed
            .then_some(spec.engine.top_speed_kmh as f32 / 3.6);
        let measured = full_throttle_run(spec, &vehicle);
        let mut drive = Vec::new();
        let mut top_speed = 0.0;
        for v in 0..120 {
            let v = v as f32;
            let a = measured.get(v as usize).copied().unwrap_or(0.0);
            drive.push(a);
            if a - drag * v * v > 0.05 && governed.is_none_or(|g| v <= g) {
                top_speed = v;
            }
        }
        Ok(CarModel {
            id: spec.id.clone(),
            mass,
            lateral,
            braking,
            traction,
            drag,
            lift,
            top_speed,
            drive,
        })
    }

    /// Drive acceleration available at `v` (before drag).
    pub fn drive_at(&self, v: f32) -> f32 {
        let x = v.clamp(0.0, (self.drive.len() - 1) as f32);
        let k = (x as usize).min(self.drive.len() - 2);
        let t = x - k as f32;
        self.drive[k] * (1.0 - t) + self.drive[k + 1] * t
    }

    /// Lateral acceleration available at `v` (lift reduces the load).
    pub fn lateral_at(&self, v: f32, limits: Limits) -> f32 {
        limits.lateral * self.lateral * (1.0 - self.lift * v * v / GRAVITY as f32).max(0.5)
    }

    /// Corner speed for a plan curvature.
    pub fn corner_speed(&self, curvature: f32, limits: Limits) -> f32 {
        // v^2 k = mu (g - lift v^2) per unit mass, with mu = lateral / g.
        let mu_g = limits.lateral * self.lateral;
        let c = mu_g * self.lift / GRAVITY as f32;
        (mu_g / (curvature.abs() + c).max(1e-6))
            .sqrt()
            .min(self.top_speed)
    }
}

/// Speed (m/s) at every line point, and the line's lap time at those speeds.
pub fn speed_profile(line: &RacingLine, model: &CarModel, limits: Limits) -> Vec<f32> {
    let pts = &line.points;
    let n = pts.len();
    let mut v: Vec<f32> = pts
        .iter()
        .map(|p| model.corner_speed(p.curvature, limits))
        .collect();
    let step = |k: usize| {
        let j = (k + 1) % n;
        (pts[j].pos - pts[k].pos).length().max(1e-3)
    };
    let grade = |k: usize| {
        let j = (k + 1) % n;
        (pts[j].pos.y - pts[k].pos.y) / step(k)
    };
    // Share of the friction ellipse left for longitudinal acceleration.
    let ellipse = |k: usize, v: f32| {
        let lat = v * v * pts[k].curvature.abs() / model.lateral_at(v, limits);
        (1.0 - lat * lat).max(0.0).sqrt()
    };
    // Accelerating out of a corner the tire model's combined slip leaves less
    // than the ellipse: the driven axle carries the drive and its share of
    // the cornering force at once.
    let traction = |k: usize, v: f32| {
        let lat = v * v * pts[k].curvature.abs() / model.lateral_at(v, limits);
        (1.0 - lat * lat).max(0.0)
    };
    // Twice round so the passes wrap.
    for k in 0..2 * n {
        let (i, j) = (k % n, (k + 1) % n);
        let a = (limits.drive * model.drive_at(v[i]) * traction(i, v[i])
            - model.drag * v[i] * v[i])
            - GRAVITY as f32 * grade(i);
        v[j] = v[j].min((v[i] * v[i] + 2.0 * a.max(0.0) * step(i)).sqrt());
    }
    for k in (0..2 * n).rev() {
        let (i, j) = (k % n, (k + 1) % n);
        let a = limits.braking * model.braking * ellipse(j, v[j])
            + model.drag * v[j] * v[j]
            + GRAVITY as f32 * grade(i);
        v[i] = v[i].min((v[j] * v[j] + 2.0 * a.max(0.0) * step(i)).sqrt());
    }
    v
}

/// Time to drive the closed line at `speeds`.
pub fn lap_time(line: &RacingLine, speeds: &[f32]) -> f32 {
    let n = line.points.len();
    (0..n)
        .map(|k| {
            let j = (k + 1) % n;
            let d = line.points[k].pos.distance(line.points[j].pos);
            2.0 * d / (speeds[k] + speeds[j]).max(0.1)
        })
        .sum()
}
