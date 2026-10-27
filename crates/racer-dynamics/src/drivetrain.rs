//! Engine, clutch, gearbox and differentials.
//!
//! Every coupling (clutch, limited-slip differentials) is solved implicitly:
//! each substep computes the torque that would bring both sides to the same
//! speed at the end of the substep and clamps it to the coupling's capacity.
//! A locked coupling is then exactly rigid and a slipping one transmits its
//! capacity, with no stiff spring to destabilise the 960 Hz substep.

use crate::spec::{CarSpec, DiffKind, DiffSpec, Layout};

pub const RPM_TO_RAD_S: f64 = std::f64::consts::PI / 30.0;

#[derive(Clone, Debug)]
pub struct Engine {
    /// Full-throttle torque curve, `(rad/s, Nm)`.
    curve: Vec<(f64, f64)>,
    pub idle: f64,
    pub limiter: f64,
    pub launch: f64,
    pub inertia: f64,
    drag: [f64; 2],
}

impl Engine {
    /// Full-throttle torque at engine speed `omega` (rad/s).
    pub fn full_torque(&self, omega: f64) -> f64 {
        let c = &self.curve;
        if omega <= c[0].0 {
            return c[0].1;
        }
        for w in c.windows(2) {
            let ((w0, t0), (w1, t1)) = (w[0], w[1]);
            if omega <= w1 {
                return t0 + (t1 - t0) * (omega - w0) / (w1 - w0);
            }
        }
        // Past the last point torque falls away quickly.
        let (wl, tl) = c[c.len() - 1];
        (tl * (1.0 - (omega - wl) / (0.15 * wl))).max(0.0)
    }

    /// Net crank torque at `throttle` in 0..=1: combustion torque minus pumping and friction drag.
    pub fn torque(&self, omega: f64, throttle: f64) -> f64 {
        let t = ((omega - self.idle) / (self.limiter - self.idle)).clamp(0.0, 1.0);
        let drag = self.drag[0] + (self.drag[1] - self.drag[0]) * t;
        throttle * self.full_torque(omega) - (1.0 - throttle) * drag
    }
}

#[derive(Clone, Debug)]
pub struct Gearbox {
    ratios: Vec<f64>,
    finals: Vec<f64>,
    reverse: f64,
    pub shift_time: f64,
    pub upshift: [f64; 2],
    pub downshift: f64,
    pub clutch_max: f64,
    pub efficiency: f64,
}

impl Gearbox {
    pub fn forward_gears(&self) -> i8 {
        self.ratios.len() as i8
    }

    /// Overall ratio, engine to wheel, for `gear` (-1 reverse, 0 neutral, 1.. forward).
    pub fn ratio(&self, gear: i8) -> f64 {
        let final_for = |i: usize| self.finals[i.min(self.finals.len() - 1)];
        match gear {
            0 => 0.0,
            g if g < 0 => -self.reverse * final_for(0),
            g => {
                let i = (g as usize - 1).min(self.ratios.len() - 1);
                self.ratios[i] * final_for(i)
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Diff {
    Open,
    Lsd {
        preload: f64,
        power: f64,
        coast: f64,
    },
}

impl Diff {
    fn from_spec(spec: Option<&DiffSpec>) -> Self {
        match spec {
            Some(d) if d.kind == DiffKind::Lsd => Diff::Lsd {
                preload: d.preload_nm,
                power: d.power_ramp,
                coast: d.coast_ramp,
            },
            _ => Diff::Open,
        }
    }

    /// Locking torque capacity for input torque `input` (positive = driving).
    fn capacity(self, input: f64) -> f64 {
        match self {
            Diff::Open => 0.0,
            Diff::Lsd {
                preload,
                power,
                coast,
            } => preload + input.abs() * if input >= 0.0 { power } else { coast },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Drivetrain {
    pub engine: Engine,
    pub gearbox: Gearbox,
    /// Share of gearbox output torque each wheel gets through open
    /// differentials (FL, FR, RL, RR); also the kinematic weights of the
    /// gearbox output speed.
    pub weights: [f64; 4],
    front: Diff,
    rear: Diff,
    center: Diff,
}

/// Drivetrain state carried between steps.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DriveState {
    pub engine_omega: f64,
    /// -1 reverse, 0 neutral, 1.. forward.
    pub gear: i8,
    /// Time since the last gear change.
    pub since_shift: f64,
    /// Fuel cut by the rev limiter, held until the engine drops below it.
    pub limiter_cut: bool,
}

/// What one drivetrain substep transmitted, for telemetry.
#[derive(Clone, Copy, Debug, Default)]
pub struct DriveOutput {
    pub engine_torque: f64,
    pub clutch_torque: f64,
}

fn smoothstep(edge0: f64, edge1: f64, x: f64) -> f64 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Drivetrain {
    pub fn from_spec(spec: &CarSpec) -> Self {
        let e = &spec.estimated;
        let curve = e
            .engine
            .torque_curve
            .iter()
            .map(|&[rpm, nm]| (rpm * RPM_TO_RAD_S, nm))
            .collect();
        let front_split = match spec.driveline.layout {
            Layout::Fwd => 1.0,
            Layout::Rwd => 0.0,
            Layout::Awd => e.differentials.center.map_or(0.5, |c| c.front_split),
        };
        Drivetrain {
            engine: Engine {
                curve,
                idle: e.engine.idle_rpm * RPM_TO_RAD_S,
                limiter: spec.engine.limiter_rpm * RPM_TO_RAD_S,
                launch: e.engine.launch_rpm * RPM_TO_RAD_S,
                inertia: e.engine.inertia_kgm2,
                drag: e.engine.drag_nm,
            },
            gearbox: Gearbox {
                ratios: spec.gearbox.ratios.clone(),
                finals: spec.gearbox.final_drive.clone(),
                reverse: spec.gearbox.reverse,
                shift_time: e.gearbox.shift_time_s,
                upshift: e.gearbox.upshift_rpm.map(|r| r * RPM_TO_RAD_S),
                downshift: e.gearbox.downshift_rpm * RPM_TO_RAD_S,
                clutch_max: e.gearbox.clutch_max_torque_nm,
                efficiency: e.gearbox.efficiency,
            },
            weights: [
                front_split / 2.0,
                front_split / 2.0,
                (1.0 - front_split) / 2.0,
                (1.0 - front_split) / 2.0,
            ],
            front: Diff::from_spec(e.differentials.front.as_ref()),
            rear: Diff::from_spec(e.differentials.rear.as_ref()),
            center: Diff::from_spec(e.differentials.center.as_ref()),
        }
    }

    pub fn initial_state(&self) -> DriveState {
        DriveState {
            engine_omega: self.engine.idle,
            gear: 1,
            since_shift: f64::INFINITY,
            limiter_cut: false,
        }
    }

    /// Gearbox output speed implied by the wheel speeds.
    pub fn output_speed(&self, omega: &[f64; 4]) -> f64 {
        self.weights.iter().zip(omega).map(|(w, o)| w * o).sum()
    }

    /// Changes gear by `delta`, refusing to engage reverse while rolling forward.
    pub fn shift(&self, state: &mut DriveState, delta: i8, forward_speed: f64) {
        let gear = (state.gear + delta).clamp(-1, self.gearbox.forward_gears());
        if gear == state.gear || (gear < 0 && forward_speed > 1.0) || (state.gear < 0 && gear > 0) {
            return;
        }
        state.gear = gear;
        state.since_shift = 0.0;
    }

    /// Automatic gear selection among the forward gears.
    pub fn auto_shift(
        &self,
        state: &mut DriveState,
        omega: &[f64; 4],
        throttle: f64,
        forward_speed: f64,
    ) {
        let gb = &self.gearbox;
        if state.gear < 1 || state.since_shift < 2.0 * gb.shift_time {
            return;
        }
        let rpm = |gear| gb.ratio(gear) * self.output_speed(omega);
        let up = gb.upshift[1] + (gb.upshift[0] - gb.upshift[1]) * throttle;
        let down = gb.downshift + (0.55 * gb.upshift[0] - gb.downshift) * throttle * throttle;
        if state.gear < gb.forward_gears() && rpm(state.gear) >= up {
            self.shift(state, 1, forward_speed);
        } else if state.gear > 1 && rpm(state.gear) < down && rpm(state.gear - 1) < 0.92 * up {
            self.shift(state, -1, forward_speed);
        }
    }

    /// Advances engine and wheel speeds by `dt`.
    ///
    /// `wheel_torque` holds the external torque on each wheel (tire reaction),
    /// `brake` the friction torque that can stop but never reverse a wheel,
    /// `torque_limit` the traction-control torque factor, `clutch_pedal` 0
    /// (released) ..= 1 (pressed). `launch` engages launch control: clutch
    /// open, engine held at the launch speed.
    #[allow(clippy::too_many_arguments)]
    pub fn substep(
        &self,
        state: &mut DriveState,
        omega: &mut [f64; 4],
        inertia: &[f64; 4],
        mut wheel_torque: [f64; 4],
        brake: &[f64; 4],
        throttle: f64,
        torque_limit: f64,
        clutch_pedal: f64,
        launch: bool,
        dt: f64,
    ) -> DriveOutput {
        let eng = &self.engine;
        let gb = &self.gearbox;
        let we = state.engine_omega;
        if we > eng.limiter {
            state.limiter_cut = true;
        } else if we < eng.limiter - 150.0 * RPM_TO_RAD_S {
            state.limiter_cut = false;
        }
        let g = gb.ratio(state.gear);
        let out = self.output_speed(omega);
        // While a shift completes the gearbox matches engine speed to the new
        // gear: ignition cut on upshifts, a throttle blip on downshifts, with
        // the clutch carrying about the engine's torque so drive continues.
        let shifting = state.since_shift < gb.shift_time && g != 0.0;
        // Pulling away (clutch slipping below the launch speed): traction
        // control trims the clutch torque while the engine is held near the
        // launch speed. Otherwise traction control trims the engine.
        let driveline = g.abs() * out.abs();
        let pulling_away = driveline < eng.launch && we > driveline + 100.0 * RPM_TO_RAD_S;
        let throttle = if launch {
            ((eng.launch - we) / (300.0 * RPM_TO_RAD_S)).clamp(0.0, throttle)
        } else if shifting {
            if we > g * out { 0.0 } else { 1.0 }
        } else if pulling_away {
            throttle.min(
                ((eng.launch + 300.0 * RPM_TO_RAD_S - we) / (300.0 * RPM_TO_RAD_S)).clamp(0.0, 1.0),
            )
        } else {
            throttle * torque_limit
        };
        // Idle governor keeps the engine running; the limiter cuts fuel.
        let idle_throttle = ((eng.idle - we) / (250.0 * RPM_TO_RAD_S)).clamp(0.0, 1.0);
        let throttle = if state.limiter_cut {
            0.0
        } else {
            throttle.max(idle_throttle)
        };
        let te = eng.torque(we, throttle);

        // Automatic clutch: slips up to the launch speed from rest and locks
        // once the driveline turns the engine above idle; the pedal opens it.
        let engagement = smoothstep(eng.idle, eng.launch, we).max(smoothstep(
            eng.idle,
            eng.idle + 400.0 * RPM_TO_RAD_S,
            g.abs() * out.abs(),
        ));
        let mut capacity = gb.clutch_max * engagement * (1.0 - clutch_pedal.clamp(0.0, 1.0));
        if shifting {
            capacity = capacity.min(eng.full_torque(we));
        }
        // Below the launch speed the clutch is modulated to carry about the
        // engine's torque, holding engine speed instead of dumping it.
        if pulling_away {
            capacity = capacity.min(te.max(0.0) * torque_limit + 0.1 * gb.clutch_max);
        }
        if launch {
            capacity = 0.0;
        }

        let w = &self.weights;
        let mut tc = 0.0;
        let mut eta = gb.efficiency;
        if g != 0.0 && capacity > 0.0 {
            let a: f64 = (0..4).map(|i| w[i] * wheel_torque[i] / inertia[i]).sum();
            let b: f64 = (0..4).map(|i| w[i] * w[i] / inertia[i]).sum();
            let slip = we - g * out + dt * (te / eng.inertia - g * a);
            // Losses come off the power flow: when the wheels drive the
            // engine (tc < 0, same sign as `slip`) they supply 1/eta of it.
            if slip < 0.0 {
                eta = 1.0 / gb.efficiency;
            }
            tc = slip / (dt * (1.0 / eng.inertia + eta * g * g * b));
            tc = tc.clamp(-capacity, capacity);
            for i in 0..4 {
                wheel_torque[i] += eta * g * w[i] * tc;
            }
        }
        // Positive input torque drives the car (power side of an LSD ramp),
        // negative is engine braking (coast side).
        let input = eta * g * tc * out.signum();
        let (front_in, rear_in) = (input * (w[0] + w[1]), input * (w[2] + w[3]));
        if w[0] > 0.0 && w[2] > 0.0 {
            lock(
                omega,
                inertia,
                &mut wheel_torque,
                &[0, 1],
                &[2, 3],
                self.center.capacity(input),
                dt,
            );
        }
        if w[0] > 0.0 {
            lock(
                omega,
                inertia,
                &mut wheel_torque,
                &[0],
                &[1],
                self.front.capacity(front_in),
                dt,
            );
        }
        if w[2] > 0.0 {
            lock(
                omega,
                inertia,
                &mut wheel_torque,
                &[2],
                &[3],
                self.rear.capacity(rear_in),
                dt,
            );
        }

        for i in 0..4 {
            let o = omega[i] + dt * wheel_torque[i] / inertia[i];
            let stop = dt * brake[i] / inertia[i];
            omega[i] = if o.abs() <= stop {
                0.0
            } else {
                o - stop * o.signum()
            };
        }
        state.engine_omega =
            (we + dt * (te - tc) / eng.inertia).clamp(0.5 * eng.idle, 1.1 * eng.limiter);
        state.since_shift += dt;
        DriveOutput {
            engine_torque: te,
            clutch_torque: tc,
        }
    }
}

/// Applies the torque (clamped to `capacity`) that brings wheel groups `a`
/// and `b` to the same mean speed at the end of the substep.
fn lock(
    omega: &[f64; 4],
    inertia: &[f64; 4],
    torque: &mut [f64; 4],
    a: &[usize],
    b: &[usize],
    capacity: f64,
    dt: f64,
) {
    if capacity <= 0.0 {
        return;
    }
    let group = |set: &[usize]| {
        let n = set.len() as f64;
        let speed = set.iter().map(|&i| omega[i]).sum::<f64>() / n;
        let inertia_sum: f64 = set.iter().map(|&i| inertia[i]).sum();
        let tq: f64 = set.iter().map(|&i| torque[i]).sum();
        (speed, inertia_sum, tq)
    };
    let (wa, ia, ta) = group(a);
    let (wb, ib, tb) = group(b);
    let t = ((wa - wb + dt * (ta / ia - tb / ib)) / (dt * (1.0 / ia + 1.0 / ib)))
        .clamp(-capacity, capacity);
    for &i in a {
        torque[i] -= t / a.len() as f64;
    }
    for &i in b {
        torque[i] += t / b.len() as f64;
    }
}
