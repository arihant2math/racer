//! Soak tests: random extreme inputs over curbs, jumps and slopes, plus
//! jitter checks at rest and when cruising. The simulation must stay finite,
//! bounded in energy and quiet at 240 Hz.

use racer_dynamics::{
    Assists, Controls, DVec3, Surface, Vehicle, chassis_dt,
    drivetrain::RPM_TO_RAD_S,
    spec::{CARS, load_car},
    testbed::{Flat, Ground, Sim, auto_controls},
    vehicle::GRAVITY,
};

/// Repeating obstacle course: curb strips every 40 m along X, a 1 m kicker
/// ramp (rising towards -Z) ending in a drop every 120 m along Z, gentle
/// undulation, grass patches.
struct Course;

impl Ground for Course {
    fn height(&self, x: f64, z: f64) -> f64 {
        let curb = if x.rem_euclid(40.0) < 1.2 { 0.08 } else { 0.0 };
        let zr = z.rem_euclid(120.0);
        let ramp = if zr < 10.0 { 0.1 * (10.0 - zr) } else { 0.0 };
        curb + ramp + 0.15 * (0.05 * x).sin() * (0.04 * z).cos()
    }

    fn surface(&self, x: f64, z: f64) -> Surface {
        if (x + z).rem_euclid(90.0) < 15.0 {
            Surface::Grass
        } else {
            Surface::DryAsphalt
        }
    }
}

/// xorshift64*: deterministic randomness without a dependency.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> f64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 11) as f64 / (1u64 << 53) as f64
    }

    fn chance(&mut self, p: f64) -> bool {
        self.next() < p
    }
}

fn random_controls(rng: &mut Rng) -> Controls {
    let extreme = |rng: &mut Rng| {
        if rng.chance(0.5) {
            rng.next().round()
        } else {
            rng.next()
        }
    };
    Controls {
        throttle: extreme(rng),
        brake: if rng.chance(0.4) { extreme(rng) } else { 0.0 },
        steer: 2.0 * extreme(rng) - 1.0,
        handbrake: if rng.chance(0.15) { 1.0 } else { 0.0 },
        clutch: if rng.chance(0.1) { 1.0 } else { 0.0 },
        shift_up: rng.chance(0.2),
        shift_down: rng.chance(0.25),
        automatic: rng.chance(0.5),
        assists: Assists {
            abs: rng.chance(0.5),
            tcs: rng.chance(0.5),
            esc: rng.chance(0.5),
        },
        hold: false,
    }
}

/// Mechanical energy of the chassis, wheels and engine.
fn energy(sim: &Sim) -> f64 {
    let (b, v) = (&sim.body, sim.vehicle);
    let w = b.rotation.inverse() * b.angular_velocity;
    let wheels: f64 = (0..4)
        .map(|i| 0.5 * v.tire(i).inertia * sim.state.wheels[i].omega.powi(2))
        .sum();
    0.5 * v.mass * b.linear_velocity.length_squared()
        + 0.5 * w.dot(v.inertia * w)
        + v.mass * GRAVITY * b.position.y
        + wheels
        + 0.5 * v.drivetrain.engine.inertia * sim.state.drive.engine_omega.powi(2)
}

#[test]
fn random_extreme_inputs_stay_finite_and_bounded() {
    for (n, id) in CARS.iter().enumerate() {
        let spec = load_car(id).unwrap();
        let vehicle = Vehicle::from_spec(&spec).unwrap();
        for seed in 0..4u64 {
            let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ (seed * 7 + n as u64 + 1));
            let mut sim = Sim::new(&vehicle, &Course);
            if seed % 2 == 1 {
                sim.launch_at(70.0); // start fast: high-speed curbs and jumps
            }
            let e0 = energy(&sim);
            // Positive work done by the engine's combustion torque.
            let mut work = 0.0;
            let mut controls = auto_controls();
            let mut next_change = 0.0;
            let (mut airborne, mut reversed) = (0, 0);
            while sim.time < 60.0 {
                let mut c = controls;
                if sim.time >= next_change {
                    controls = random_controls(&mut rng);
                    c = controls;
                    next_change = sim.time + 0.1 + 0.9 * rng.next();
                } else {
                    c.shift_up = false;
                    c.shift_down = false;
                }
                sim.step(&c);
                let b = &sim.body;
                let t = sim.last.unwrap().telemetry;
                work += (t.engine_torque * t.rpm * RPM_TO_RAD_S).max(0.0) * chassis_dt();
                let finite = b.position.is_finite()
                    && b.linear_velocity.is_finite()
                    && b.angular_velocity.is_finite()
                    && b.rotation.is_finite()
                    && sim
                        .state
                        .wheels
                        .iter()
                        .all(|w| w.omega.is_finite() && w.slip_ratio.is_finite())
                    && sim.state.drive.engine_omega.is_finite();
                assert!(
                    finite,
                    "{id} seed {seed}: non-finite state at t={:.3}",
                    sim.time
                );
                assert!(
                    b.linear_velocity.length() < 110.0,
                    "{id} seed {seed}: runaway speed at t={:.3}",
                    sim.time
                );
                assert!(
                    b.angular_velocity.length() < 25.0,
                    "{id} seed {seed}: spinning up at t={:.3}",
                    sim.time
                );
                assert!(
                    sim.state.wheels.iter().all(|w| w.omega.abs() < 500.0),
                    "{id}: wheel spin {:?}",
                    sim.state.wheels
                );
                // Energy can only come from the engine (and nothing else).
                assert!(
                    energy(&sim) <= e0 + 1.02 * work + 2.0e4,
                    "{id} seed {seed}: energy {:.0} J from {e0:.0} J after {:.1} s",
                    energy(&sim),
                    sim.time
                );
                airborne += usize::from(t.wheels.iter().all(|w| !w.contact));
                reversed += usize::from(t.gear < 0 && t.speed < -1.0);
            }
            println!("{id} seed {seed}: airborne steps {airborne}, reversing steps {reversed}");
        }
    }
}

/// Jumping off the kicker at speed lands without bouncing back into the air.
#[test]
fn jump_landing_settles() {
    let vehicle = Vehicle::from_spec(&load_car("vw-golf-gti-mk8").unwrap()).unwrap();
    let mut sim = Sim::new(&vehicle, &Course);
    // 25 m before the kicker that rises from z = 130 to the drop at z = 120.
    sim.body.position += DVec3::new(10.0, 0.0, 155.0);
    sim.launch_at(25.0);
    let (mut airborne, mut takeoffs, mut was_flying) = (0, 0, false);
    // 3 s at 25 m/s: over the kicker after 1 s, then onto flat ground.
    sim.run(3.0, |s| {
        if let Some(out) = s.last {
            let flying = out.telemetry.wheels.iter().all(|w| !w.contact);
            airborne += usize::from(flying);
            takeoffs += usize::from(flying && !was_flying);
            was_flying = flying;
        }
        let (throttle, brake) = racer_dynamics::testbed::driver::hold_speed(s, 25.0);
        Controls {
            throttle,
            brake,
            ..auto_controls()
        }
    });
    assert!(airborne > 24, "never left the ground ({airborne} steps)");
    assert!(takeoffs <= 2, "bounced: {takeoffs} separate flights");
    // After a few seconds the car is back on four wheels, level and quiet.
    // (Held on the brakes as race control does: the automatic would
    // otherwise engage reverse once the car is stopped.)
    sim.run(4.0, |_| Controls {
        brake: 1.0,
        hold: true,
        ..auto_controls()
    });
    let up = sim.body.rotation * DVec3::Y;
    assert!(up.y > 0.95, "car not upright: {up}");
    assert!(
        sim.body.linear_velocity.length() < 0.5,
        "{:?}",
        sim.body.linear_velocity
    );
}

/// At rest on flat ground the car settles completely: no creeping or buzzing.
/// The brake held at rest is race control's hold, which never engages the
/// automatic's reverse.
#[test]
fn rest_is_quiet() {
    for id in CARS {
        let vehicle = Vehicle::from_spec(&load_car(id).unwrap()).unwrap();
        for brake in [0.0, 1.0] {
            let mut sim = Sim::new(&vehicle, &Flat);
            sim.run(5.0, |_| Controls {
                brake,
                hold: true,
                ..auto_controls()
            });
            let mut max_v: f64 = 0.0;
            let mut max_w: f64 = 0.0;
            sim.run(2.0, |s| {
                max_v = max_v.max(s.body.linear_velocity.length());
                max_w = max_w.max(s.body.angular_velocity.length());
                Controls {
                    brake,
                    hold: true,
                    ..auto_controls()
                }
            });
            assert!(
                max_v < 2e-3 && max_w < 2e-3,
                "{id} brake {brake}: v {max_v:e} w {max_w:e}"
            );
        }
    }
}

/// Cruising straight at 120 km/h the body does not jitter vertically.
#[test]
fn cruise_is_smooth() {
    for id in CARS {
        let vehicle = Vehicle::from_spec(&load_car(id).unwrap()).unwrap();
        let mut sim = Sim::new(&vehicle, &Flat);
        sim.launch_at(120.0 / 3.6);
        let hold = |s: &Sim| {
            let (throttle, brake) = racer_dynamics::testbed::driver::hold_speed(s, 120.0 / 3.6);
            Controls {
                throttle,
                brake,
                ..auto_controls()
            }
        };
        sim.run(3.0, hold);
        let mut vertical = Vec::new();
        sim.run(2.0, |s| {
            vertical.push(s.accel.y);
            hold(s)
        });
        let mean = vertical.iter().sum::<f64>() / vertical.len() as f64;
        let std = (vertical.iter().map(|a| (a - mean).powi(2)).sum::<f64>()
            / vertical.len() as f64)
            .sqrt();
        assert!(std < 0.05, "{id}: vertical acceleration std {std} m/s^2");
    }
}

/// Rollback: restoring a copied state and replaying the same inputs
/// reproduces the run bit for bit.
#[test]
fn replay_from_a_snapshot_is_bit_identical() {
    let vehicle = Vehicle::from_spec(&load_car("bmw-m3-competition-g80").unwrap()).unwrap();
    let mut sim = Sim::new(&vehicle, &Course);
    sim.launch_at(30.0);
    let mut rng = Rng(42);
    let inputs: Vec<Controls> = (0..1200).map(|_| random_controls(&mut rng)).collect();
    for c in &inputs[..600] {
        sim.step(c);
    }
    let snapshot = (sim.state, sim.body, sim.time);
    let run = |sim: &mut Sim| {
        for c in &inputs[600..] {
            sim.step(c);
        }
        (sim.state, sim.body)
    };
    let first = run(&mut sim);
    (sim.state, sim.body, sim.time) = snapshot;
    assert_eq!(run(&mut sim), first);
}
