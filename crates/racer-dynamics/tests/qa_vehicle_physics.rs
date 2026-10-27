//! QA repro tests for vehicle-physics bugs (docs/qa/vehicle-physics.md).
//! Each test fails on the current code and is ignored so the suite stays
//! green; run them with `cargo test -p racer-dynamics -- --ignored QA_PHYS`.

use std::panic::{AssertUnwindSafe, catch_unwind};

use racer_dynamics::{
    CarSpec, Controls, DVec3, Vehicle, load_car,
    testbed::{Flat, Sim, auto_controls},
};

fn gti() -> CarSpec {
    load_car("vw-golf-gti-mk8").unwrap()
}

/// What happens when a spec is turned into a vehicle and driven for 2 s:
/// `Err` from `from_spec` (the wanted outcome for bad data), a panic, or a
/// short report of the end state.
fn outcome(spec: &CarSpec) -> String {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let vehicle = match Vehicle::from_spec(spec) {
            Ok(v) => v,
            Err(e) => return format!("Err({e})"),
        };
        let mut sim = Sim::new(&vehicle, &Flat);
        sim.run(2.0, |_| Controls {
            throttle: 1.0,
            ..auto_controls()
        });
        format!(
            "Ok: finite {}, speed {:.2} m/s",
            sim.body.position.is_finite(),
            sim.speed()
        )
    }));
    result.unwrap_or_else(|e| {
        let msg = e
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default();
        format!("PANIC: {msg}")
    })
}

fn assert_all_rejected(cases: Vec<(&str, CarSpec)>) {
    let accepted: Vec<String> = cases
        .iter()
        .filter_map(|(name, spec)| {
            let o = outcome(spec);
            (!o.starts_with("Err")).then(|| format!("{name}: {o}"))
        })
        .collect();
    assert!(
        accepted.is_empty(),
        "invalid specs accepted by Vehicle::from_spec:\n{}",
        accepted.join("\n")
    );
}

#[test]
fn qa_phys_1_degenerate_parameters_are_rejected() {
    let mut cases = Vec::new();
    let mut case = |name, edit: &dyn Fn(&mut CarSpec)| {
        let mut s = gti();
        edit(&mut s);
        cases.push((name, s));
    };
    case("zero mass", &|s| {
        s.chassis.curb_mass_kg = 0.0;
        s.estimated.chassis.payload_kg = 0.0;
    });
    case("negative mass", &|s| s.chassis.curb_mass_kg = -1500.0);
    case("NaN mass", &|s| s.chassis.curb_mass_kg = f64::NAN);
    case("zero yaw gyration", &|s| {
        s.estimated.chassis.gyration_yaw_m = 0.0
    });
    case("zero wheel inertia", &|s| {
        s.estimated.tires.front.inertia_kgm2 = 0.0;
        s.estimated.tires.rear.inertia_kgm2 = 0.0;
    });
    case("zero engine inertia", &|s| {
        s.estimated.engine.inertia_kgm2 = 0.0
    });
    case("zero relaxation length", &|s| {
        s.estimated.tires.front.relaxation_lat_m = 0.0
    });
    case("zero peak slip ratio", &|s| {
        s.estimated.tires.front.peak_slip_ratio = 0.0
    });
    case("zero front track", &|s| s.chassis.track_front_m = 0.0);
    case("zero ride frequency", &|s| {
        s.estimated.suspension.front.ride_frequency_hz = 0.0
    });
    case("shape factor C < 1 (zero grip)", &|s| {
        s.estimated.tires.front.shape_long = 0.9;
        s.estimated.tires.rear.shape_long = 0.9;
    });
    case("curvature E >= 1", &|s| {
        s.estimated.tires.front.curvature = 1.5;
        s.estimated.tires.rear.curvature = 1.5;
    });
    case("front weight fraction > 1", &|s| {
        s.chassis.front_weight_fraction = 1.2
    });
    case("limiter below idle", &|s| s.engine.limiter_rpm = 500.0);
    case("zero driveline efficiency", &|s| {
        s.estimated.gearbox.efficiency = 0.0
    });
    case("zero steering ratio", &|s| s.steering.ratio = 0.0);
    assert_all_rejected(cases);
}

#[test]
fn qa_phys_1_nan_and_inf_in_car_toml_are_rejected() {
    let text =
        std::fs::read_to_string(racer_dynamics::spec::cars_dir().join("vw-golf-gti-mk8/car.toml"))
            .unwrap();
    for bad in ["nan", "inf", "-inf"] {
        let t = text.replace("curb_mass_kg = 1431.0", &format!("curb_mass_kg = {bad}"));
        let rejected = match CarSpec::parse(&t) {
            Err(_) => true,
            Ok(spec) => Vehicle::from_spec(&spec).is_err(),
        };
        assert!(rejected, "curb_mass_kg = {bad} accepted");
    }
}

#[test]
fn qa_phys_2_empty_gear_and_torque_tables_are_rejected() {
    let mut cases = Vec::new();
    let mut s = gti();
    s.gearbox.ratios.clear();
    cases.push(("no forward gears", s));
    let mut s = gti();
    s.gearbox.final_drive.clear();
    cases.push(("no final drive", s));
    let mut s = gti();
    s.estimated.engine.torque_curve.clear();
    cases.push(("empty torque curve", s));
    assert_all_rejected(cases);
}

#[test]
#[ignore = "QA-PHYS-3"]
fn qa_phys_3_car_stopped_after_a_sideways_slide_stays_put() {
    let v = Vehicle::from_spec(&gti()).unwrap();
    let mut sim = Sim::new(&v, &Flat);
    // Sliding sideways (to the right) at 12 m/s, as after a spin; brakes and handbrake on.
    sim.body.linear_velocity = DVec3::new(12.0, 0.0, 0.0);
    let held = Controls {
        brake: 1.0,
        handbrake: 1.0,
        ..auto_controls()
    };
    let mut furthest: f64 = 0.0;
    sim.run(12.0, |s| {
        furthest = furthest.max(s.body.position.x);
        held
    });
    let back = furthest - sim.body.position.x;
    assert!(
        back < 0.1,
        "after stopping at x = {furthest:.2} m the braked car crept {back:.2} m back sideways"
    );
}

#[test]
fn qa_phys_6_zero_dt_step_is_a_no_op() {
    let v = Vehicle::from_spec(&gti()).unwrap();
    let mut sim = Sim::new(&v, &Flat);
    sim.launch_at(20.0);
    let contacts = sim.contacts();
    let before = sim.state;
    let c = Controls {
        throttle: 1.0,
        ..auto_controls()
    };
    for dt in [0.0, -1.0 / 240.0, f64::NAN, f64::INFINITY] {
        let mut state = before;
        let r = catch_unwind(AssertUnwindSafe(|| {
            let out = v.step(&mut state, &sim.body, &c, &contacts, dt);
            (state, out.force)
        }));
        let (after, force) = r.unwrap_or_else(|_| panic!("Vehicle::step panicked with dt = {dt}"));
        assert_eq!(after, before, "dt = {dt} changed the state");
        assert_eq!(force, DVec3::ZERO, "dt = {dt} produced a force");
    }
}

#[test]
fn qa_phys_7_nan_controls_do_not_panic_or_poison_the_state() {
    let v = Vehicle::from_spec(&gti()).unwrap();
    let r = catch_unwind(AssertUnwindSafe(|| {
        let mut sim = Sim::new(&v, &Flat);
        sim.run(0.5, |_| Controls {
            throttle: f64::NAN,
            brake: f64::NAN,
            steer: f64::NAN,
            ..auto_controls()
        });
        sim.run(0.5, |_| auto_controls());
        sim.body.position
    }));
    let p = r.expect("NaN controls panicked inside Vehicle::step");
    assert!(p.is_finite(), "state poisoned: {p:?}");
}

#[test]
#[ignore = "QA-PHYS-8"]
fn qa_phys_8_engine_revs_freely_in_neutral() {
    let v = Vehicle::from_spec(&gti()).unwrap();
    let mut sim = Sim::new(&v, &Flat);
    let manual = Controls::default();
    sim.step(&Controls {
        shift_down: true,
        ..manual
    });
    assert_eq!(sim.state.drive.gear, 0);
    sim.run(2.0, |_| Controls {
        throttle: 1.0,
        ..manual
    });
    let rpm = sim.last.unwrap().telemetry.rpm;
    // Free revving in neutral should reach the limiter (6700 rpm).
    assert!(rpm > 6000.0, "full throttle in neutral holds {rpm:.0} rpm");
}

#[test]
#[ignore = "QA-PHYS-9"]
fn qa_phys_9_automatic_gearbox_engages_drive_from_neutral() {
    let v = Vehicle::from_spec(&gti()).unwrap();
    let mut sim = Sim::new(&v, &Flat);
    let auto = auto_controls();
    sim.step(&Controls {
        shift_down: true,
        ..auto
    });
    sim.run(3.0, |_| Controls {
        throttle: 1.0,
        ..auto
    });
    assert!(
        sim.speed() > 5.0,
        "automatic in gear {} after one shift-down: full throttle for 3 s gives {:.2} m/s",
        sim.state.drive.gear,
        sim.speed()
    );
}
