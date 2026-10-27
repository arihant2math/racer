//! Reference maneuvers for every car against the published figures stored in
//! its `car.toml`. Tolerances are documented in docs/milestones/M1.md.

use racer_dynamics::{
    Assists, Vehicle,
    spec::{CARS, CarSpec, load_car},
    testbed,
};

/// Relative tolerance on 0-100 km/h time.
const ACCEL_TOL: f64 = 0.08;
/// Relative tolerance on 100-0 km/h braking distance.
const BRAKE_TOL: f64 = 0.08;
/// Absolute tolerance on skidpad lateral acceleration (g).
const SKIDPAD_TOL: f64 = 0.08;
/// Relative tolerance on top speed.
const TOP_SPEED_TOL: f64 = 0.04;
/// Radius of the 300-ft skidpad the magazines use (m).
const SKIDPAD_RADIUS: f64 = 45.72;

fn cars() -> Vec<(CarSpec, Vehicle)> {
    CARS.iter()
        .map(|id| {
            let spec = load_car(id).unwrap();
            let vehicle = Vehicle::from_spec(&spec).unwrap();
            (spec, vehicle)
        })
        .collect()
}

fn check(car: &str, what: &str, sim: f64, real: f64, ok: bool) -> bool {
    println!(
        "{car:30} {what:12} sim {sim:7.2}  real {real:7.2}  {}",
        if ok { "ok" } else { "FAIL" }
    );
    ok
}

fn relative(car: &str, what: &str, sim: f64, real: f64, tol: f64) -> bool {
    check(car, what, sim, real, ((sim - real) / real).abs() <= tol)
}

#[test]
fn zero_to_100() {
    let all = cars().iter().fold(true, |ok, (spec, v)| {
        let r = &spec.reference.zero_to_100_s;
        relative(
            &spec.id,
            "0-100 s",
            testbed::zero_to_100(v),
            r.value,
            ACCEL_TOL,
        ) && ok
    });
    assert!(all);
}

#[test]
fn braking_100_to_0() {
    let all = cars().iter().fold(true, |ok, (spec, v)| {
        let r = &spec.reference.braking_100_to_0_m;
        relative(
            &spec.id,
            "100-0 m",
            testbed::braking_100_to_0(v),
            r.value,
            BRAKE_TOL,
        ) && ok
    });
    assert!(all);
}

#[test]
fn skidpad() {
    let all = cars().iter().fold(true, |ok, (spec, v)| {
        let r = spec.reference.skidpad_g.value;
        let g = testbed::skidpad(v, SKIDPAD_RADIUS);
        check(&spec.id, "skidpad g", g, r, (g - r).abs() <= SKIDPAD_TOL) && ok
    });
    assert!(all);
}

#[test]
fn top_speed() {
    let all = cars().iter().fold(true, |ok, (spec, v)| {
        let r = &spec.reference.top_speed_kmh;
        relative(
            &spec.id,
            "top km/h",
            testbed::top_speed(v),
            r.value,
            TOP_SPEED_TOL,
        ) && ok
    });
    assert!(all);
}

/// A moderate steering step at 100 km/h settles to a steady yaw rate
/// without spinning, with and without stability control; a sharp step with
/// ESC on does not spin the car.
#[test]
fn step_steer_is_stable() {
    for (spec, v) in cars() {
        let speed = 100.0 / 3.6;
        // Steering for ~0.4 g on a neutral-steer car.
        let steer = (v.wheelbase * 0.4 * 9.81 / (speed * speed)).atan() / v.max_steer;
        for assists in [Assists::OFF, Assists::default()] {
            let r = testbed::step_steer(&v, speed, steer, assists);
            println!("{:30} step-steer esc={} {r:.3?}", spec.id, assists.esc);
            assert!(r.max_sideslip < 3f64.to_radians(), "{}: {r:?}", spec.id);
            assert!(
                r.final_variation < 0.01,
                "{}: yaw rate not steady: {r:?}",
                spec.id
            );
            assert!(
                r.peak_yaw_rate < 1.3 * r.steady_yaw_rate,
                "{}: overshoot {r:?}",
                spec.id
            );
            assert!((0.3..0.5).contains(&r.lateral_g), "{}: {r:?}", spec.id);
        }
        let r = testbed::step_steer(&v, speed, 4.0 * steer, Assists::default());
        println!("{:30} sharp step-steer {r:.3?}", spec.id);
        assert!(
            r.max_sideslip < 10f64.to_radians(),
            "{}: spun: {r:?}",
            spec.id
        );
    }
}
