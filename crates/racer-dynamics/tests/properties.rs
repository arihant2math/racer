//! Property-based tests (QA, adversarial edge cases): arbitrary chassis
//! states, contacts, controls and internal states fed to `Vehicle::step`,
//! and invariants that must hold for all of them. See
//! `docs/qa/adversarial-edge-cases.md` for the findings.
//!
//! Each property runs a small number of cases by default so `cargo test`
//! stays fast; set `QA_EDGE_CASES=<n>` for a deeper run. Properties that
//! currently fail are `#[ignore = "QA-EDGE-N"]` regression tests with the
//! shrunk counterexample hard-coded.

use std::sync::LazyLock;

use proptest::prelude::*;
use racer_dynamics::{
    Assists, ChassisState, Controls, DQuat, DVec3, Surface, Vehicle, VehicleState, WheelContact,
    chassis_dt,
    spec::{CARS, CarSpec, load_car},
    testbed::{Flat, Sim},
    vehicle::GRAVITY,
};

fn config(default_cases: u32) -> ProptestConfig {
    let cases = std::env::var("QA_EDGE_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default_cases);
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

static VEHICLES: LazyLock<Vec<Vehicle>> = LazyLock::new(|| {
    CARS.iter()
        .map(|id| Vehicle::from_spec(&load_car(id).unwrap()).unwrap())
        .collect()
});

const SURFACES: [Surface; 8] = [
    Surface::DryAsphalt,
    Surface::WetAsphalt,
    Surface::PaintedLine,
    Surface::Cobbles,
    Surface::MetalPlate,
    Surface::Curb,
    Surface::Grass,
    Surface::Dirt,
];

fn assists() -> impl Strategy<Value = Assists> {
    (any::<bool>(), any::<bool>(), any::<bool>()).prop_map(|(abs, tcs, esc)| Assists {
        abs,
        tcs,
        esc,
    })
}

/// Controls inside their documented ranges.
fn controls() -> impl Strategy<Value = Controls> {
    (
        (0.0..=1.0f64, 0.0..=1.0f64, -1.0..=1.0f64, 0.0..=1.0f64),
        (0.0..=1.0f64, any::<bool>(), any::<bool>(), any::<bool>()),
        assists(),
    )
        .prop_map(
            |((throttle, brake, steer, handbrake), (clutch, up, down, automatic), assists)| {
                Controls {
                    throttle,
                    brake,
                    steer,
                    handbrake,
                    clutch,
                    shift_up: up,
                    shift_down: down,
                    automatic,
                    assists,
                    hold: false,
                }
            },
        )
}

fn contact() -> impl Strategy<Value = Option<WheelContact>> {
    prop::option::weighted(
        0.85,
        (-0.1..0.6f64, 0.0..1.0f64, -3.2..3.2f64, 0usize..8).prop_map(
            |(distance, tilt, azimuth, s)| {
                let normal = DVec3::new(
                    tilt.sin() * azimuth.cos(),
                    tilt.cos(),
                    tilt.sin() * azimuth.sin(),
                );
                WheelContact {
                    distance,
                    normal,
                    surface: SURFACES[s],
                }
            },
        ),
    )
}

fn chassis() -> impl Strategy<Value = ChassisState> {
    (
        (-0.4..0.4f64, -0.4..0.4f64, -3.2..3.2f64),
        (-20.0..75.0f64, -15.0..15.0f64, -5.0..5.0f64),
        (-2.0..2.0f64, -3.0..3.0f64, -2.0..2.0f64),
    )
        .prop_map(|((roll, pitch, yaw), (fwd, lat, vert), w)| {
            let rotation = DQuat::from_rotation_y(yaw)
                * DQuat::from_rotation_x(pitch)
                * DQuat::from_rotation_z(roll);
            ChassisState {
                position: DVec3::new(0.0, 0.5, 0.0),
                rotation,
                linear_velocity: rotation * DVec3::new(lat, vert, -fwd),
                angular_velocity: DVec3::new(w.0, w.1, w.2),
            }
        })
}

/// A plausible mid-race internal state for `vehicle`.
fn vehicle_state(car: usize) -> impl Strategy<Value = VehicleState> {
    let v = &VEHICLES[car];
    let gears = v.drivetrain.gearbox.forward_gears();
    let (idle, limiter) = (v.drivetrain.engine.idle, v.drivetrain.engine.limiter);
    (
        prop::array::uniform4((
            -250.0..250.0f64,
            -1.5..1.5f64,
            -1.5..1.5f64,
            0.05..=1.0f64,
            -8000.0..8000.0f64,
        )),
        (idle..limiter, -1i8..=gears, 0.0..1.0f64, 0.0..=1.0f64),
    )
        .prop_map(move |(wheels, (engine, gear, since, tcs))| {
            let mut s = VEHICLES[car].initial_state();
            for (w, (omega, kappa, alpha, abs, lat)) in s.wheels.iter_mut().zip(wheels) {
                w.omega = omega;
                w.slip_ratio = kappa;
                w.slip_lat = alpha;
                w.abs = abs;
                w.lateral_force = lat;
            }
            s.drive.engine_omega = engine;
            s.drive.gear = gear;
            s.drive.since_shift = since;
            s.tcs = tcs;
            s
        })
}

/// (car, state, chassis, controls, contacts)
fn step_input() -> impl Strategy<
    Value = (
        usize,
        VehicleState,
        ChassisState,
        Controls,
        [Option<WheelContact>; 4],
    ),
> {
    (0usize..CARS.len()).prop_flat_map(|car| {
        (
            Just(car),
            vehicle_state(car),
            chassis(),
            controls(),
            prop::array::uniform4(contact()),
        )
    })
}

fn state_is_finite(s: &VehicleState) -> bool {
    s.wheels.iter().all(|w| {
        w.omega.is_finite()
            && w.slip_ratio.is_finite()
            && w.slip_lat.is_finite()
            && w.abs.is_finite()
            && w.lateral_force.is_finite()
    }) && s.drive.engine_omega.is_finite()
        && s.tcs.is_finite()
}

proptest! {
    #![proptest_config(config(256))]

    /// One step from any plausible state stays finite, keeps the engine in
    /// its clamp, the assist factors in 0..=1, and every tire force inside
    /// its friction ellipse.
    #[test]
    fn step_is_finite_bounded_and_inside_the_friction_ellipse(
        (car, mut state, chassis, controls, contacts) in step_input()
    ) {
        let v = &VEHICLES[car];
        let out = v.step(&mut state, &chassis, &controls, &contacts, chassis_dt());
        prop_assert!(out.force.is_finite() && out.torque.is_finite(), "{out:?}");
        prop_assert!(state_is_finite(&state), "{state:?}");
        let e = &v.drivetrain.engine;
        prop_assert!(state.drive.engine_omega >= 0.5 * e.idle - 1e-9);
        prop_assert!(state.drive.engine_omega <= 1.1 * e.limiter + 1e-9);
        prop_assert!((0.0..=1.0).contains(&state.tcs));
        for (i, w) in out.telemetry.wheels.iter().enumerate() {
            prop_assert!((0.0..=1.0).contains(&state.wheels[i].abs));
            if !w.contact {
                continue;
            }
            prop_assert!(w.load >= 0.0 && w.load <= 20.0 * v.mass * GRAVITY / 4.0 + 1e-6);
            if w.load <= 0.0 {
                continue;
            }
            let grip = contacts[i].unwrap().surface.grip();
            let (mx, my) = v.tire(i).mu(w.load);
            let ellipse = (w.fx / (mx * grip * w.load)).powi(2) + (w.fy / (my * grip * w.load)).powi(2);
            prop_assert!(ellipse <= 1.0 + 1e-6, "wheel {i}: ellipse {ellipse} {w:?}");
        }
    }

    /// The step is a pure function: the same inputs give bit-identical output
    /// and state.
    #[test]
    fn step_is_deterministic((car, state, chassis, controls, contacts) in step_input()) {
        let v = &VEHICLES[car];
        let (mut a, mut b) = (state, state);
        let oa = v.step(&mut a, &chassis, &controls, &contacts, chassis_dt());
        let ob = v.step(&mut b, &chassis, &controls, &contacts, chassis_dt());
        prop_assert_eq!(a, b);
        prop_assert_eq!(oa.force.to_array().map(f64::to_bits), ob.force.to_array().map(f64::to_bits));
        prop_assert_eq!(oa.torque.to_array().map(f64::to_bits), ob.torque.to_array().map(f64::to_bits));
    }

    /// The cars are left/right symmetric: mirroring the chassis state, the
    /// contacts, the wheel states and the steering mirrors the force and the
    /// torque (a pseudovector).
    #[test]
    fn mirrored_inputs_give_mirrored_outputs(
        (car, state, chassis, controls, contacts) in step_input()
    ) {
        let v = &VEHICLES[car];
        let m = |x: DVec3| DVec3::new(-x.x, x.y, x.z);
        let mp = |x: DVec3| DVec3::new(x.x, -x.y, -x.z);
        let q = chassis.rotation;
        let mirrored_chassis = ChassisState {
            position: m(chassis.position),
            rotation: DQuat::from_xyzw(q.x, -q.y, -q.z, q.w),
            linear_velocity: m(chassis.linear_velocity),
            angular_velocity: mp(chassis.angular_velocity),
        };
        let swap = [1, 0, 3, 2];
        let mirrored_contacts = swap.map(|j| {
            contacts[j].map(|c| WheelContact { normal: m(c.normal), ..c })
        });
        let mut ms = state;
        for (i, &j) in swap.iter().enumerate() {
            let w = state.wheels[j];
            ms.wheels[i] = racer_dynamics::vehicle::WheelState {
                slip_lat: -w.slip_lat,
                lateral_force: -w.lateral_force,
                ..w
            };
        }
        let mc = Controls { steer: -controls.steer, ..controls };
        let mut s1 = state;
        let a = v.step(&mut s1, &chassis, &controls, &contacts, chassis_dt());
        let b = v.step(&mut ms, &mirrored_chassis, &mc, &mirrored_contacts, chassis_dt());
        let close = |x: DVec3, y: DVec3| {
            (x - y).abs().max_element() <= 1e-6 * x.abs().max_element().max(y.abs().max_element()) + 1e-6
        };
        prop_assert!(close(m(a.force), b.force), "force {} vs mirrored {}", a.force, b.force);
        prop_assert!(close(mp(a.torque), b.torque), "torque {} vs mirrored {}", a.torque, b.torque);
        for (i, &j) in swap.iter().enumerate() {
            let (x, y) = (s1.wheels[j].omega, ms.wheels[i].omega);
            prop_assert!((x - y).abs() <= 1e-9 * x.abs().max(1.0), "wheel {i}: {x} vs {y}");
        }
    }

    /// Airborne with the clutch pressed, no wheel can gain spin energy: the
    /// brakes only take energy out and the differentials only move it between
    /// wheels (lossily).
    #[test]
    fn airborne_wheels_never_gain_energy(
        car in 0usize..CARS.len(),
        omegas in prop::array::uniform4(-250.0..250.0f64),
        inputs in prop::collection::vec(controls(), 1..60),
    ) {
        let v = &VEHICLES[car];
        let mut s = v.initial_state();
        for (w, o) in s.wheels.iter_mut().zip(omegas) {
            w.omega = o;
        }
        let ke = |s: &VehicleState| (0..4).map(|i| 0.5 * v.tire(i).inertia * s.wheels[i].omega.powi(2)).sum::<f64>();
        let chassis = ChassisState {
            position: DVec3::ZERO,
            rotation: DQuat::IDENTITY,
            linear_velocity: DVec3::ZERO,
            angular_velocity: DVec3::ZERO,
        };
        let mut e = ke(&s);
        for c in inputs {
            let c = Controls { clutch: 1.0, ..c };
            v.step(&mut s, &chassis, &c, &[None; 4], chassis_dt());
            let now = ke(&s);
            prop_assert!(now <= e * (1.0 + 1e-12) + 1e-9, "wheel energy rose {e} -> {now}");
            e = now;
        }
    }

    /// Coasting in neutral on flat ground (no engine work) the car's total
    /// mechanical energy never rises above where it started.
    #[test]
    fn coasting_in_neutral_never_gains_energy(
        car in 0usize..CARS.len(),
        speed in 0.0..60.0f64,
        inputs in prop::collection::vec((controls(), 1usize..120), 1..10),
    ) {
        let v = &VEHICLES[car];
        let mut sim = Sim::new(v, &Flat);
        sim.launch_at(speed);
        sim.state.drive.gear = 0;
        let energy = |sim: &Sim| {
            let b = &sim.body;
            let w = b.rotation.inverse() * b.angular_velocity;
            0.5 * v.mass * b.linear_velocity.length_squared()
                + 0.5 * w.dot(v.inertia * w)
                + v.mass * GRAVITY * b.position.y
                + (0..4).map(|i| 0.5 * v.tire(i).inertia * sim.state.wheels[i].omega.powi(2)).sum::<f64>()
        };
        let e0 = energy(&sim);
        let mut peak = e0;
        for (c, steps) in inputs {
            let c = Controls { throttle: 0.0, shift_up: false, shift_down: false, automatic: false, ..c };
            for _ in 0..steps {
                sim.step(&c);
                peak = peak.max(energy(&sim));
            }
        }
        // Spring energy and the body settling are the only other stores.
        prop_assert!(peak <= e0 + 0.002 * e0 + 500.0, "energy {e0:.0} J rose to {peak:.0} J");
    }

    /// Splitting a step: one step of 2 dt and two steps of dt from the same
    /// state (chassis held fixed) end with nearly the same wheel and engine
    /// speeds, for a car driving on flat ground with its wheels near rolling
    /// and the clutch locked. (Outside that envelope -- a locked wheel at
    /// speed, a slipping clutch, a wheel braked by ESC in the air -- the
    /// coupled wheel/clutch/tire system is stiff and the 4-substep integrator
    /// legitimately differs by tens of percent between dt and 2 dt; that is
    /// first-order integration error, not a defect, see the QA note.)
    #[test]
    fn two_half_steps_match_one_full_step(
        car in 0usize..CARS.len(),
        (yaw, speed, lateral, yaw_rate) in (-3.2..3.2f64, 5.0..70.0f64, -3.0..3.0f64, -0.5..0.5f64),
        slips in prop::array::uniform4(-0.05..0.05f64),
        alphas in prop::array::uniform4(-0.1..0.1f64),
        travel in prop::array::uniform4(0.05..0.2f64),
        gear in 1i8..=6,
        controls in controls(),
    ) {
        let v = &VEHICLES[car];
        let rotation = DQuat::from_rotation_y(yaw);
        let chassis = ChassisState {
            position: DVec3::ZERO,
            rotation,
            linear_velocity: rotation * DVec3::new(lateral, 0.0, -speed),
            angular_velocity: DVec3::Y * yaw_rate,
        };
        let mut state = v.initial_state();
        for (i, w) in state.wheels.iter_mut().enumerate() {
            w.omega = speed / v.wheel_geometry(i).radius * (1.0 + slips[i]);
            w.slip_ratio = slips[i];
            w.slip_lat = alphas[i];
        }
        let d = &v.drivetrain;
        let out = d.output_speed(&state.wheels.map(|w| w.omega));
        // The requested gear, or the nearest one that keeps the engine in its range.
        let fits = |g: i8| (d.engine.idle..0.95 * d.engine.limiter).contains(&(d.gearbox.ratio(g) * out));
        let gear = gear.min(d.gearbox.forward_gears());
        let gear = (1..=d.gearbox.forward_gears())
            .filter(|&g| fits(g))
            .min_by_key(|&g| (g - gear).abs());
        prop_assume!(gear.is_some());
        state.drive.gear = gear.unwrap();
        let geared = d.gearbox.ratio(state.drive.gear) * out;
        state.drive.engine_omega = geared;
        let contacts = travel.map(|distance| Some(WheelContact {
            distance,
            normal: DVec3::Y,
            surface: Surface::DryAsphalt,
        }));
        // Brakes and ESC off: a wheel locking under a brake is a discontinuity.
        let controls = Controls {
            brake: 0.0,
            handbrake: 0.0,
            shift_up: false,
            shift_down: false,
            automatic: false,
            assists: Assists { esc: false, ..controls.assists },
            ..controls
        };
        let dt = chassis_dt();
        let mut one = state;
        v.step(&mut one, &chassis, &controls, &contacts, 2.0 * dt);
        let mut two = state;
        v.step(&mut two, &chassis, &controls, &contacts, dt);
        v.step(&mut two, &chassis, &controls, &contacts, dt);
        for i in 0..4 {
            let (a, b) = (one.wheels[i].omega, two.wheels[i].omega);
            prop_assert!((a - b).abs() <= 0.02 * a.abs().max(b.abs()) + 0.5, "wheel {i}: {a} vs {b}");
        }
        let (a, b) = (one.drive.engine_omega, two.drive.engine_omega);
        prop_assert!((a - b).abs() <= 0.02 * a.max(b) + 5.0, "engine {a} vs {b}");
    }

    /// Restoring a snapshot and replaying arbitrary inputs on the testbed is
    /// bit-identical (rollback).
    #[test]
    fn replay_after_restore_is_bit_identical(
        car in 0usize..CARS.len(),
        speed in 0.0..50.0f64,
        inputs in prop::collection::vec(controls(), 2..200),
        split in 0.0..1.0f64,
    ) {
        let v = &VEHICLES[car];
        let mut sim = Sim::new(v, &Flat);
        sim.launch_at(speed);
        let at = ((inputs.len() as f64 * split) as usize).min(inputs.len() - 1);
        for c in &inputs[..at] {
            sim.step(c);
        }
        let snapshot = (sim.state, sim.body);
        for c in &inputs[at..] {
            sim.step(c);
        }
        let first = (sim.state, sim.body);
        (sim.state, sim.body) = snapshot;
        for c in &inputs[at..] {
            sim.step(c);
        }
        prop_assert_eq!(first, (sim.state, sim.body));
    }
}

fn some_step(controls: Controls) -> VehicleState {
    let v = &VEHICLES[0];
    let mut sim = Sim::new(v, &Flat);
    sim.launch_at(20.0);
    let contacts = sim.contacts();
    let mut state = sim.state;
    v.step(&mut state, &sim.body, &controls, &contacts, chassis_dt());
    // Five more ordinary steps: does the state recover?
    for _ in 0..5 {
        v.step(
            &mut state,
            &sim.body,
            &Controls::default(),
            &contacts,
            chassis_dt(),
        );
    }
    state
}

proptest! {
    #![proptest_config(config(64))]

    /// Controls outside their ranges (an over-driven axis, a buggy AI) are
    /// clamped: the state stays finite.
    #[test]
    fn out_of_range_finite_controls_keep_the_state_finite(
        throttle in -10.0..10.0f64, brake in -10.0..10.0f64, steer in -10.0..10.0f64,
        handbrake in -10.0..10.0f64, clutch in -10.0..10.0f64,
    ) {
        let s = some_step(Controls { throttle, brake, steer, handbrake, clutch, ..Controls::default() });
        prop_assert!(state_is_finite(&s), "{s:?}");
    }
}

/// QA-EDGE-3: one non-finite control value (NaN steer from an AI fed a NaN
/// pose, or a malformed network input on the planned authoritative server)
/// panics the step within a few calls: `clamp` passes NaN through, it
/// reaches the drivetrain, and `lock`'s `clamp(-capacity, capacity)` with a
/// NaN capacity panics ("min > max, or either was NaN"), in release builds
/// too. Before that, the NaN poisons `VehicleState` (rollback snapshots).
#[test]
fn qa_edge_3_nan_control_poisons_the_vehicle_state() {
    for controls in [
        Controls {
            steer: f64::NAN,
            ..Controls::default()
        },
        Controls {
            throttle: f64::NAN,
            ..Controls::default()
        },
        Controls {
            brake: f64::NAN,
            ..Controls::default()
        },
        Controls {
            clutch: f64::NAN,
            ..Controls::default()
        },
    ] {
        let s = some_step(controls);
        assert!(
            state_is_finite(&s),
            "{controls:?} left a non-finite state: {s:?}"
        );
    }
}

fn spec_with(edit: impl Fn(&mut CarSpec)) -> CarSpec {
    let mut spec = load_car(CARS[0]).unwrap();
    edit(&mut spec);
    spec
}

/// Runs `from_spec` and a second of the testbed; `Err` is fine, a panic or a
/// non-finite state is not.
fn spec_outcome(spec: &CarSpec) -> Result<(), String> {
    let spec = spec.clone();
    std::panic::catch_unwind(move || {
        let Ok(v) = Vehicle::from_spec(&spec) else {
            return Ok(());
        };
        let mut sim = Sim::new(&v, &Flat);
        sim.run(1.0, |_| Controls {
            throttle: 1.0,
            automatic: true,
            ..Controls::default()
        });
        if state_is_finite(&sim.state) && sim.body.position.is_finite() {
            Ok(())
        } else {
            Err(format!("non-finite state {:?}", sim.state))
        }
    })
    .unwrap_or_else(|e| {
        Err(format!(
            "panicked: {}",
            e.downcast_ref::<String>()
                .cloned()
                .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
                .unwrap_or_default()
        ))
    })
}

/// QA-EDGE-4: `Vehicle::from_spec` validates nothing. Each of these
/// one-field edits of the shipped Golf GTI `car.toml` is accepted (`Ok`)
/// and then panics or produces NaN within a second of driving, instead of
/// being rejected with an error naming the field.
#[test]
fn qa_edge_4_invalid_car_specs_are_accepted() {
    let cases: Vec<(&str, CarSpec)> = vec![
        (
            "gearbox.ratios = []",
            spec_with(|s| s.gearbox.ratios.clear()),
        ),
        (
            "gearbox.final_drive = []",
            spec_with(|s| s.gearbox.final_drive.clear()),
        ),
        (
            "estimated.engine.torque_curve = []",
            spec_with(|s| s.estimated.engine.torque_curve.clear()),
        ),
        (
            "chassis.curb_mass_kg = -payload (zero mass)",
            spec_with(|s| s.chassis.curb_mass_kg = -s.estimated.chassis.payload_kg),
        ),
        (
            "estimated.tires.front.inertia_kgm2 = 0",
            spec_with(|s| s.estimated.tires.front.inertia_kgm2 = 0.0),
        ),
        (
            "estimated.tires.front.relaxation_long_m = 0",
            spec_with(|s| s.estimated.tires.front.relaxation_long_m = 0.0),
        ),
        (
            "estimated.engine.inertia_kgm2 = 0",
            spec_with(|s| s.estimated.engine.inertia_kgm2 = 0.0),
        ),
    ];
    let failures: Vec<String> = cases
        .iter()
        .filter_map(|(what, spec)| spec_outcome(spec).err().map(|e| format!("{what}: {e}")))
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
}

/// A Magic Formula shape factor below 1 cannot peak, so the curve fit
/// would give a tire with (almost) no grip: the spec is rejected instead.
/// Also part of QA-EDGE-4.
#[test]
fn qa_edge_4_shape_factor_below_one_is_rejected() {
    let spec = spec_with(|s| s.estimated.tires.front.shape_lat = 0.9);
    let err = Vehicle::from_spec(&spec).expect_err("C < 1 accepted");
    assert!(err.contains("shape_lat"), "{err}");
}
