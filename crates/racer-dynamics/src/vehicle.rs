//! The vehicle step: chassis state + controls + wheel contacts + dt ->
//! forces and torques on the chassis + new internal state.
//!
//! Body frame (same as Bevy): +X right, +Y up, -Z forward, origin at the
//! centre of mass. World quantities use the caller's frame with +Y up.

use glam::{DQuat, DVec3};

use crate::{
    DRIVETRAIN_SUBSTEPS,
    assists::{self, Assists},
    drivetrain::{DriveState, Drivetrain},
    spec::{AxleFit, CarSpec, tire_radius},
    tire::{Surface, Tire},
};

pub const GRAVITY: f64 = 9.81;
const AIR_DENSITY: f64 = 1.225;
/// Below this forward speed (m/s) tire slip is damped directly to stop the
/// relaxation springs oscillating at standstill.
const LOW_SPEED: f64 = 3.0;

/// Wheel order used everywhere.
pub const FL: usize = 0;
pub const FR: usize = 1;
pub const RL: usize = 2;
pub const RR: usize = 3;

/// Rigid-body state of the chassis, at the centre of mass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChassisState {
    pub position: DVec3,
    pub rotation: DQuat,
    pub linear_velocity: DVec3,
    pub angular_velocity: DVec3,
}

/// Driver inputs for one step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Controls {
    /// 0..=1
    pub throttle: f64,
    /// 0..=1
    pub brake: f64,
    /// -1 (full left) ..= 1 (full right)
    pub steer: f64,
    /// 0..=1
    pub handbrake: f64,
    /// Clutch pedal, 0 (released) ..= 1 (pressed).
    pub clutch: f64,
    /// Shift requests (edge-triggered: true for the one step the button went down).
    pub shift_up: bool,
    pub shift_down: bool,
    /// Automatic gearbox mode (manual shifting otherwise).
    pub automatic: bool,
    pub assists: Assists,
}

/// Result of the wheel's shape cast, done by the caller along the body's
/// down axis from the wheel's hardpoint.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WheelContact {
    /// Distance travelled by the wheel centre from the hardpoint, 0..=`max_distance`.
    pub distance: f64,
    /// Ground normal at the contact, world frame.
    pub normal: DVec3,
    pub surface: Surface,
}

/// Where the caller casts each wheel from.
#[derive(Clone, Copy, Debug)]
pub struct WheelGeometry {
    /// Top of the suspension travel, body frame.
    pub hardpoint: DVec3,
    pub radius: f64,
    pub width: f64,
    /// Cast length: wheel centre travel from the hardpoint to full droop.
    pub max_distance: f64,
}

#[derive(Clone, Debug)]
struct Corner {
    geometry: WheelGeometry,
    tire: Tire,
    /// Cast distance at static ride height, spring free length as a cast
    /// distance, and spring rate.
    static_distance: f64,
    free_length: f64,
    spring_rate: f64,
    damping_bump: f64,
    damping_rebound: f64,
    anti_roll: f64,
    roll_center_height: f64,
    max_brake: f64,
    max_handbrake: f64,
}

/// Immutable vehicle parameters derived from a [`CarSpec`].
#[derive(Clone, Debug)]
pub struct Vehicle {
    pub name: String,
    pub mass: f64,
    /// Principal moments of inertia about body X (pitch), Y (yaw), Z (roll).
    pub inertia: DVec3,
    pub wheelbase: f64,
    pub cg_height: f64,
    /// Max road-wheel angle (rad) at full steering lock.
    pub max_steer: f64,
    ackermann: f64,
    corners: [Corner; 4],
    pub drivetrain: Drivetrain,
    drag_area: f64,
    lift_area: [f64; 2],
    axle_z: [f64; 2],
    governor: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelState {
    /// Spin (rad/s), positive rolling forward.
    pub omega: f64,
    /// Relaxed slip ratio and lateral slip (tan of the slip angle).
    pub slip_ratio: f64,
    pub slip_lat: f64,
    /// ABS brake-pressure factor.
    pub abs: f64,
    /// Lateral tire force along the body's +X from the last step (N), for
    /// the suspension's geometric load transfer.
    pub lateral_force: f64,
}

/// Everything the vehicle carries between steps. Plain `Copy` data, so
/// rollback is a copy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VehicleState {
    pub wheels: [WheelState; 4],
    pub drive: DriveState,
    /// TCS engine-torque factor.
    pub tcs: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct WheelTelemetry {
    pub contact: bool,
    pub load: f64,
    pub slip_ratio: f64,
    /// Slip angle (rad).
    pub slip_angle: f64,
    pub fx: f64,
    pub fy: f64,
    /// Self-aligning torque (Nm, positive turns the wheel left).
    pub mz: f64,
    pub omega: f64,
    /// Suspension compression from static ride height (m).
    pub compression: f64,
    /// Wheel-centre distance below the hardpoint (full droop when airborne).
    pub travel: f64,
    pub steer: f64,
    pub brake_torque: f64,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Telemetry {
    pub wheels: [WheelTelemetry; 4],
    pub rpm: f64,
    pub gear: i8,
    pub engine_torque: f64,
    pub throttle: f64,
    /// Forward speed (m/s).
    pub speed: f64,
    /// Aligning torque about the steering axis summed over the front wheels.
    pub steering_torque: f64,
    pub abs_active: bool,
    pub tcs_active: bool,
    pub esc_active: bool,
}

/// Force and torque on the chassis about its centre of mass (world frame,
/// gravity not included) plus telemetry.
#[derive(Clone, Copy, Debug)]
pub struct StepOutput {
    pub force: DVec3,
    pub torque: DVec3,
    pub telemetry: Telemetry,
}

impl Vehicle {
    pub fn from_spec(spec: &CarSpec) -> Result<Self, String> {
        let c = &spec.chassis;
        let e = &spec.estimated;
        let mass = c.curb_mass_kg + e.chassis.payload_kg;
        let f = c.front_weight_fraction;
        // Front axle ahead of the CoM (negative z), rear behind.
        let axle_z = [-c.wheelbase_m * (1.0 - f), c.wheelbase_m * f];
        let radius = [
            tire_radius(&spec.tires.front)?,
            tire_radius(&spec.tires.rear)?,
        ];
        let width = |size: &str| {
            size.split('/')
                .next()
                .and_then(|w| w.parse::<f64>().ok())
                .unwrap_or(225.0)
                / 1000.0
        };
        let widths = [width(&spec.tires.front), width(&spec.tires.rear)];
        let tracks = [c.track_front_m, c.track_rear_m];
        let corner_mass = [mass * f / 2.0, mass * (1.0 - f) / 2.0];
        let axles = [&e.suspension.front, &e.suspension.rear];
        let tires = [&e.tires.front, &e.tires.rear];
        let brakes = [e.brakes.front_torque_nm, e.brakes.rear_torque_nm];
        let corner = |i: usize| {
            let axle = i / 2;
            let fit: &AxleFit = axles[axle];
            let m = corner_mass[axle];
            let k = m * (2.0 * std::f64::consts::PI * fit.ride_frequency_hz).powi(2);
            let critical = 2.0 * (k * m).sqrt();
            let static_distance = fit.bump_travel_m;
            let side = if i.is_multiple_of(2) { -0.5 } else { 0.5 };
            Corner {
                geometry: WheelGeometry {
                    hardpoint: DVec3::new(
                        side * tracks[axle],
                        radius[axle] - e.chassis.cg_height_m + static_distance,
                        axle_z[axle],
                    ),
                    radius: radius[axle],
                    width: widths[axle],
                    max_distance: static_distance + fit.droop_travel_m,
                },
                tire: Tire::new(tires[axle], radius[axle]),
                static_distance,
                free_length: static_distance + m * GRAVITY / k,
                spring_rate: k,
                damping_bump: critical * fit.damping_ratio_bump,
                damping_rebound: critical * fit.damping_ratio_rebound,
                anti_roll: fit.anti_roll_n_per_m,
                roll_center_height: fit.roll_center_height_m,
                max_brake: brakes[axle],
                max_handbrake: if axle == 1 {
                    e.brakes.handbrake_torque_nm
                } else {
                    0.0
                },
            }
        };
        let g = &e.chassis;
        Ok(Vehicle {
            name: spec.name.clone(),
            mass,
            inertia: DVec3::new(
                g.gyration_pitch_m.powi(2),
                g.gyration_yaw_m.powi(2),
                g.gyration_roll_m.powi(2),
            ) * mass,
            wheelbase: c.wheelbase_m,
            cg_height: g.cg_height_m,
            max_steer: (spec.steering.lock_to_lock_turns * 180.0 / spec.steering.ratio)
                .to_radians(),
            ackermann: e.steering.ackermann,
            corners: [corner(FL), corner(FR), corner(RL), corner(RR)],
            drivetrain: Drivetrain::from_spec(spec),
            drag_area: spec.aero.drag_coefficient * e.aero.frontal_area_m2,
            lift_area: [e.aero.lift_area_front_m2, e.aero.lift_area_rear_m2],
            axle_z,
            governor: spec
                .engine
                .top_speed_governed
                .then_some(spec.engine.top_speed_kmh / 3.6),
        })
    }

    pub fn wheel_geometry(&self, wheel: usize) -> WheelGeometry {
        self.corners[wheel].geometry
    }

    pub fn tire(&self, wheel: usize) -> &Tire {
        &self.corners[wheel].tire
    }

    /// Height of the centre of mass above flat ground at static ride height.
    pub fn ride_height(&self) -> f64 {
        self.cg_height
    }

    pub fn initial_state(&self) -> VehicleState {
        VehicleState {
            wheels: [WheelState {
                abs: 1.0,
                ..Default::default()
            }; 4],
            drive: self.drivetrain.initial_state(),
            tcs: 1.0,
        }
    }

    /// Road-wheel angles (rad, positive = left) for a steering input, with Ackermann.
    fn steer_angles(&self, steer: f64) -> [f64; 2] {
        let delta = -steer.clamp(-1.0, 1.0) * self.max_steer;
        if delta.abs() < 1e-6 {
            return [0.0; 2];
        }
        let track = self.corners[FR].geometry.hardpoint.x - self.corners[FL].geometry.hardpoint.x;
        let radius = self.wheelbase / delta.tan();
        let ack = |r: f64| (self.wheelbase / r).atan();
        // Left wheel is on the inside of a left turn (positive radius).
        let (left, right) = (ack(radius - track / 2.0), ack(radius + track / 2.0));
        [
            delta + self.ackermann * (left - delta),
            delta + self.ackermann * (right - delta),
        ]
    }

    /// Advances the vehicle by one chassis step `dt` (wheels and drivetrain
    /// substepped internally) and returns the chassis force and torque.
    pub fn step(
        &self,
        state: &mut VehicleState,
        chassis: &ChassisState,
        controls: &Controls,
        contacts: &[Option<WheelContact>; 4],
        dt: f64,
    ) -> StepOutput {
        let rot = chassis.rotation;
        let up = rot * DVec3::Y;
        let forward = rot * DVec3::NEG_Z;
        let v = chassis.linear_velocity;
        let w = chassis.angular_velocity;
        let forward_speed = v.dot(forward);
        let drive = &self.drivetrain;
        let mut tel = Telemetry::default();

        // Gear changes happen once per chassis step.
        let ds = &mut state.drive;
        if controls.shift_up {
            drive.shift(ds, 1, forward_speed);
        }
        if controls.shift_down {
            drive.shift(ds, -1, forward_speed);
        }
        let omegas = state.wheels.map(|w| w.omega);
        if controls.automatic {
            drive.auto_shift(ds, &omegas, controls.throttle, forward_speed);
        }

        // Suspension: travel and load per wheel.
        let travel: [Option<f64>; 4] = std::array::from_fn(|i| {
            let c = contacts[i]?;
            (c.normal.dot(up) > 0.3)
                .then(|| c.distance.clamp(0.0, self.corners[i].geometry.max_distance))
        });
        let mut load = [0.0; 4];
        for i in 0..4 {
            let (Some(d), Some(contact)) = (travel[i], contacts[i]) else {
                continue;
            };
            let c = &self.corners[i];
            let r = rot * c.geometry.hardpoint;
            // Rate of travel change from the hardpoint's velocity along the normal.
            let v_hp = v + w.cross(r);
            let compress_speed =
                (-v_hp.dot(contact.normal) / contact.normal.dot(up)).clamp(-5.0, 5.0);
            let damper = compress_speed
                * if compress_speed > 0.0 {
                    c.damping_bump
                } else {
                    c.damping_rebound
                };
            let spring = c.spring_rate * (c.free_length - d);
            // Progressive bump stop over the last 3 cm of travel.
            let bump = 20.0 * c.spring_rate * (0.03 - d).max(0.0);
            let other = travel[i ^ 1].unwrap_or(c.geometry.max_distance);
            let arb = c.anti_roll * (other - d);
            // Geometric load transfer: the linkage carries the lateral force's
            // moment about the roll centre without compressing the spring (an
            // inward force pushes the wheel down, an outward one lifts it).
            let jacking =
                -state.wheels[i].lateral_force * c.roll_center_height / c.geometry.hardpoint.x;
            load[i] = (spring + damper + bump + arb + jacking)
                .clamp(0.0, 20.0 * self.mass * GRAVITY / 4.0);
        }

        // Contact frames and the ground velocity of each contact patch.
        let steer = self.steer_angles(controls.steer);
        struct Patch {
            point: DVec3,
            fwd: DVec3,
            right: DVec3,
            normal: DVec3,
            vx: f64,
            vy: f64,
            grip: f64,
            drag: f64,
        }
        let patches: [Option<Patch>; 4] = std::array::from_fn(|i| {
            let (d, contact) = (travel[i]?, contacts[i]?);
            let c = &self.corners[i];
            let n = contact.normal;
            let angle = if i < 2 { steer[i] } else { 0.0 };
            let heading = DQuat::from_axis_angle(up, angle) * forward;
            let fwd = (heading - n * heading.dot(n)).try_normalize()?;
            let right = fwd.cross(n);
            let point = rot * c.geometry.hardpoint - up * d - n * c.geometry.radius;
            let vp = v + w.cross(point);
            Some(Patch {
                point,
                fwd,
                right,
                normal: n,
                vx: vp.dot(fwd),
                vy: vp.dot(right),
                grip: contact.surface.grip(),
                drag: contact.surface.drag(),
            })
        });

        // Brakes, handbrake and assists (ESC once per step).
        let mut brake = [0.0; 4];
        for (i, b) in brake.iter_mut().enumerate() {
            let c = &self.corners[i];
            *b = controls.brake.clamp(0.0, 1.0) * c.max_brake
                + controls.handbrake.clamp(0.0, 1.0) * c.max_handbrake;
        }
        if controls.assists.esc && forward_speed > 5.0 {
            let delta = (steer[0] + steer[1]) / 2.0;
            let mu = self.corners[FL].tire.mu(self.mass * GRAVITY / 4.0).1;
            let limit = 0.85 * mu * GRAVITY / forward_speed;
            let reference = (forward_speed * delta.tan() / self.wheelbase).clamp(-limit, limit);
            let esc = assists::esc_torques(w.dot(up), reference, self.corners[FL].max_brake * 0.6);
            for i in 0..4 {
                brake[i] += esc[i];
                tel.esc_active |= esc[i] > 0.0;
            }
        }
        let governed = self.governor.is_some_and(|vmax| forward_speed > vmax);
        let base_throttle = if governed {
            0.0
        } else {
            controls.throttle.clamp(0.0, 1.0)
        };
        // Launch control: brake and full throttle at a standstill in gear.
        let launch = controls.brake > 0.5
            && controls.throttle > 0.9
            && forward_speed.abs() < 0.5
            && state.drive.gear > 0;

        // Wheel and drivetrain substeps.
        let n = DRIVETRAIN_SUBSTEPS;
        let h = dt / f64::from(n);
        let inertia: [f64; 4] = std::array::from_fn(|i| self.corners[i].tire.inertia);
        let mut omega = state.wheels.map(|w| w.omega);
        let mut fx_sum = [0.0; 4];
        let mut fy_sum = [0.0; 4];
        let mut mz_sum = [0.0; 4];
        let mut drive_out = Default::default();
        for _ in 0..n {
            let mut wheel_torque = [0.0; 4];
            let mut friction = [0.0; 4];
            let mut max_driven_slip: f64 = 0.0;
            for i in 0..4 {
                let c = &self.corners[i];
                let ws = &mut state.wheels[i];
                let tire = &c.tire;
                let Some(p) = &patches[i] else {
                    ws.slip_ratio = 0.0;
                    ws.slip_lat = 0.0;
                    friction[i] = brake[i];
                    continue;
                };
                let r = tire.radius;
                // First-order relaxation, integrated implicitly so it stays
                // stable at any speed; at standstill it becomes a spring.
                let slip_speed = omega[i] * r - p.vx;
                let decay = |sigma: f64| 1.0 + h * p.vx.abs() / sigma;
                ws.slip_ratio = (ws.slip_ratio + h * slip_speed / tire.relaxation_long)
                    / decay(tire.relaxation_long);
                ws.slip_lat =
                    (ws.slip_lat + h * p.vy / tire.relaxation_lat) / decay(tire.relaxation_lat);
                let mut f = tire.force(ws.slip_ratio, ws.slip_lat, load[i], p.grip);
                // Low-speed damping of the relaxation springs, bounded by grip.
                let fade = (1.0 - p.vx.abs() / LOW_SPEED).max(0.0);
                if fade > 0.0 && load[i] > 0.0 {
                    let m = load[i] / GRAVITY;
                    let ky = tire.cornering_stiffness(load[i]) / tire.relaxation_lat;
                    let kx = tire.long_stiffness(load[i]) / tire.relaxation_long;
                    let cx = (2.0 * (kx * m).sqrt()).min(0.5 * inertia[i] / (r * r * h));
                    f.fx += fade * cx * slip_speed;
                    f.fy -= fade * 2.0 * (ky * m).sqrt() * p.vy;
                    let (mu_x, mu_y) = tire.mu(load[i]);
                    let limit = load[i] * p.grip;
                    let e =
                        ((f.fx / (mu_x * limit)).powi(2) + (f.fy / (mu_y * limit)).powi(2)).sqrt();
                    if e > 1.0 {
                        f.fx /= e;
                        f.fy /= e;
                    }
                }
                fx_sum[i] += f.fx;
                fy_sum[i] += f.fy;
                mz_sum[i] += f.mz;
                wheel_torque[i] = -f.fx * r;
                // ABS and TCS see wheel-speed-sensor slip (kinematic, not the
                // tire's relaxed slip, which lags more and more at low speed).
                let sensor_slip = slip_speed * p.vx.signum() / p.vx.abs().max(2.0);
                if controls.assists.abs && controls.brake > 0.0 && p.vx.abs() > 1.0 {
                    ws.abs = assists::abs_update(ws.abs, sensor_slip, tire.peak_slip_ratio, h);
                } else {
                    ws.abs = 1.0;
                }
                let rolling = tire.rolling_resistance * p.drag * load[i] * r;
                friction[i] = brake[i] * ws.abs + rolling;
                if drive.weights[i] > 0.0 {
                    max_driven_slip = max_driven_slip.max(sensor_slip);
                }
            }
            if controls.assists.tcs {
                let peak = self.corners[if drive.weights[FL] > 0.0 { FL } else { RL }]
                    .tire
                    .peak_slip_ratio;
                state.tcs = assists::tcs_update(state.tcs, max_driven_slip, peak, h);
            } else {
                state.tcs = 1.0;
            }
            drive_out = drive.substep(
                &mut state.drive,
                &mut omega,
                &inertia,
                wheel_torque,
                &friction,
                base_throttle,
                state.tcs,
                controls.clutch,
                launch,
                h,
            );
        }
        for (w, o) in state.wheels.iter_mut().zip(omega) {
            w.omega = o;
        }

        // Assemble chassis force and torque.
        let mut force = DVec3::ZERO;
        let mut torque = DVec3::ZERO;
        let mut apply = |f: DVec3, at: DVec3| {
            force += f;
            torque += at.cross(f);
        };
        let inv = 1.0 / f64::from(n);
        let mut aligning = DVec3::ZERO;
        for i in 0..4 {
            let c = &self.corners[i];
            let ws = &mut state.wheels[i];
            let t = &mut tel.wheels[i];
            t.omega = ws.omega;
            t.steer = if i < 2 { steer[i] } else { 0.0 };
            t.brake_torque = brake[i] * ws.abs;
            t.travel = travel[i].unwrap_or(c.geometry.max_distance);
            let Some(p) = &patches[i] else {
                ws.lateral_force = 0.0;
                continue;
            };
            let (fx, fy, mz) = (fx_sum[i] * inv, fy_sum[i] * inv, mz_sum[i] * inv);
            // The tire force and the linkage's jacking force (part of `load`)
            // act along the line from the contact patch through the roll
            // centre, so they enter the body at the patch: the total load
            // transfer is m a h / track, and only the share above the roll
            // centre goes through the springs.
            apply(p.normal * load[i] + p.fwd * fx + p.right * fy, p.point);
            aligning += p.normal * mz;
            ws.lateral_force = fy * p.right.dot(rot * DVec3::X);
            *t = WheelTelemetry {
                contact: true,
                load: load[i],
                slip_ratio: ws.slip_ratio,
                slip_angle: ws.slip_lat.atan(),
                fx,
                fy,
                mz,
                compression: c.static_distance - travel[i].unwrap_or(0.0),
                ..*t
            };
        }
        // Aero: drag through the centre of mass, lift/downforce at the axles.
        let q = 0.5 * AIR_DENSITY;
        apply(-q * self.drag_area * v.length() * v, DVec3::ZERO);
        for (axle, &z) in self.axle_z.iter().enumerate() {
            let lift = q * self.lift_area[axle] * forward_speed * forward_speed;
            apply(up * lift, rot * DVec3::new(0.0, 0.0, z));
        }
        torque += aligning;

        let ds = &state.drive;
        tel.rpm = ds.engine_omega / crate::drivetrain::RPM_TO_RAD_S;
        tel.gear = ds.gear;
        tel.engine_torque = drive_out.engine_torque;
        tel.throttle = base_throttle * state.tcs;
        tel.speed = forward_speed;
        tel.steering_torque = tel.wheels[FL].mz + tel.wheels[FR].mz;
        tel.abs_active = state.wheels.iter().any(|w| w.abs < 1.0);
        tel.tcs_active = state.tcs < 1.0;
        StepOutput {
            force,
            torque,
            telemetry: tel,
        }
    }
}
