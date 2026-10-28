//! Reversing: the automatic gearbox's brake-hold reverse (engage, drive,
//! disengage), the manual gearbox's reverse gear, and the bounds on both.

use racer_dynamics::{
    Controls, Vehicle,
    drivetrain::{REVERSE_HOLD, STANDSTILL},
    spec::{CARS, load_car},
    testbed::{Flat, Sim, auto_controls, driver},
    vehicle::REVERSE_TOP_SPEED,
};

fn vehicles() -> Vec<Vehicle> {
    CARS.iter()
        .map(|id| Vehicle::from_spec(&load_car(id).unwrap()).unwrap())
        .collect()
}

fn gear(sim: &Sim) -> i8 {
    sim.state.drive.gear
}

/// Runs `controls` for `seconds`, steered straight while rolling forward.
fn run(sim: &mut Sim, seconds: f64, controls: Controls) {
    let (origin, dir) = (sim.body.position, sim.forward());
    sim.run(seconds, |s| Controls {
        steer: if s.speed() < 0.0 {
            0.0
        } else {
            driver::hold_line(s, origin, dir)
        },
        ..controls
    });
}

/// Like [`run`], checking the gear every step.
fn lowest_gear(sim: &mut Sim, seconds: f64, controls: Controls) -> i8 {
    let mut lowest = gear(sim);
    let end = sim.time + seconds;
    while sim.time < end {
        sim.step(&controls);
        lowest = lowest.min(gear(sim));
    }
    lowest
}

#[test]
fn holding_the_brake_at_a_standstill_engages_reverse_and_drives_backwards() {
    for v in &vehicles() {
        let mut sim = Sim::new(v, &Flat);
        let brake = Controls {
            brake: 1.0,
            ..auto_controls()
        };
        // A short hold at a standstill keeps 1st.
        assert_eq!(
            lowest_gear(&mut sim, 0.8 * REVERSE_HOLD, brake),
            1,
            "{}",
            v.name
        );
        sim.run(0.25 * REVERSE_HOLD, |_| brake);
        assert_eq!(gear(&sim), -1, "{}", v.name);
        // The brake pedal is the reverse throttle now.
        run(&mut sim, 3.0, brake);
        assert!(sim.speed() < -3.0, "{}: {}", v.name, sim.speed());
        // Reverse speed is bounded.
        let mut fastest: f64 = 0.0;
        for _ in 0..240 * 10 {
            sim.step(&brake);
            fastest = fastest.max(-sim.speed());
        }
        assert!(
            fastest < REVERSE_TOP_SPEED + 0.5 && fastest > REVERSE_TOP_SPEED - 1.5,
            "{}: {fastest}",
            v.name
        );
        assert_eq!(gear(&sim), -1);
    }
}

#[test]
fn the_throttle_brakes_a_reversing_car_then_goes_back_to_first() {
    for v in &vehicles() {
        let mut sim = Sim::new(v, &Flat);
        let base = auto_controls();
        run(
            &mut sim,
            REVERSE_HOLD + 0.1,
            Controls { brake: 1.0, ..base },
        );
        run(&mut sim, 2.0, Controls { brake: 1.0, ..base });
        let reversing = sim.speed();
        assert!(reversing < -3.0, "{}: {reversing}", v.name);
        // Throttle while reversing: the car slows in reverse, then pulls away in 1st.
        let throttle = Controls {
            throttle: 1.0,
            ..base
        };
        sim.step(&throttle);
        assert_eq!(gear(&sim), -1, "{}", v.name);
        let mut stopped_in_reverse = false;
        let mut previous = sim.speed();
        while gear(&sim) < 0 && sim.time < 30.0 {
            sim.step(&throttle);
            if gear(&sim) < 0 {
                assert!(sim.speed() >= previous - 1e-3, "{}: speeding up", v.name);
                previous = sim.speed();
            } else {
                stopped_in_reverse = previous > -STANDSTILL - 0.1;
            }
        }
        assert!(stopped_in_reverse, "{}: left reverse at {previous}", v.name);
        assert_eq!(gear(&sim), 1, "{}", v.name);
        run(&mut sim, 2.0, throttle);
        assert!(sim.speed() > 5.0, "{}: {}", v.name, sim.speed());
    }
}

#[test]
fn no_reverse_when_stopping_hard_or_while_rolling_forward() {
    for v in &vehicles() {
        // Full ABS stop from 100 km/h, then the brake held a little longer,
        // as a driver waits at a standstill before pulling away again.
        let mut sim = Sim::new(v, &Flat);
        sim.launch_at(100.0 / 3.6);
        let brake = Controls {
            brake: 1.0,
            ..auto_controls()
        };
        let dir = sim.forward();
        let mut lowest = gear(&sim);
        // How far the car rolls back from the furthest point it reached (the
        // body rocks back on its springs at the stop, a few centimetres).
        let (mut furthest, mut back) = (f64::MIN, 0.0f64);
        let end = sim.time + 10.0;
        while sim.time < end {
            sim.step(&brake);
            lowest = lowest.min(gear(&sim));
            let along = sim.body.position.dot(dir);
            furthest = furthest.max(along);
            back = back.max(furthest - along);
            if sim.speed() < STANDSTILL && sim.state.drive.reverse_hold > 0.8 * REVERSE_HOLD {
                break;
            }
        }
        assert!(lowest >= 1, "{}: gear {lowest}", v.name);
        assert!(back < 0.1, "{}: rolled back {back} m", v.name);
        // Light braking while rolling slowly forward never selects reverse.
        let mut sim = Sim::new(v, &Flat);
        sim.launch_at(2.0);
        let light = Controls {
            brake: 0.25,
            ..auto_controls()
        };
        let mut lowest = gear(&sim);
        while sim.speed() > STANDSTILL + 0.05 {
            sim.step(&light);
            lowest = lowest.min(gear(&sim));
        }
        assert!(lowest >= 1, "{}", v.name);
        // Throttle and brake together (launch control) never selects reverse.
        let mut sim = Sim::new(v, &Flat);
        let launch = Controls {
            throttle: 1.0,
            brake: 1.0,
            ..auto_controls()
        };
        assert_eq!(lowest_gear(&mut sim, 3.0, launch), 1, "{}", v.name);
    }
}

#[test]
fn race_control_hold_never_engages_reverse() {
    for v in &vehicles() {
        let mut sim = Sim::new(v, &Flat);
        let held = Controls {
            brake: 1.0,
            hold: true,
            ..auto_controls()
        };
        assert_eq!(lowest_gear(&mut sim, 5.0, held), 1, "{}", v.name);
        assert!(sim.speed().abs() < 0.05);
        // After the start the driver's own brake counts from zero.
        let brake = Controls {
            hold: false,
            ..held
        };
        assert_eq!(
            lowest_gear(&mut sim, 0.8 * REVERSE_HOLD, brake),
            1,
            "{}",
            v.name
        );
    }
}

#[test]
fn launch_control_works_after_reverse_was_engaged_on_the_spot() {
    for v in &vehicles() {
        let mut sim = Sim::new(v, &Flat);
        let base = auto_controls();
        sim.run(REVERSE_HOLD + 0.1, |_| Controls { brake: 1.0, ..base });
        assert_eq!(gear(&sim), -1, "{}", v.name);
        // Throttle added to the held brake: back to 1st, launch control holds the revs.
        let launch = Controls {
            throttle: 1.0,
            brake: 1.0,
            ..base
        };
        sim.run(1.5, |_| launch);
        assert_eq!(gear(&sim), 1, "{}", v.name);
        let rpm = sim.last.unwrap().telemetry.rpm;
        let launch_rpm = v.drivetrain.engine.launch / racer_dynamics::drivetrain::RPM_TO_RAD_S;
        assert!(
            (rpm - launch_rpm).abs() < 400.0,
            "{}: {rpm} vs {launch_rpm}",
            v.name
        );
        assert!(sim.speed().abs() < 0.1, "{}", v.name);
        run(
            &mut sim,
            2.0,
            Controls {
                throttle: 1.0,
                ..base
            },
        );
        assert!(sim.speed() > 8.0, "{}: {}", v.name, sim.speed());
    }
}

#[test]
fn analog_brake_in_reverse_is_proportional_throttle() {
    for v in &vehicles() {
        let reverse_speed = |pedal: f64| {
            let mut sim = Sim::new(v, &Flat);
            let base = auto_controls();
            sim.run(REVERSE_HOLD + 0.05, |_| Controls { brake: 1.0, ..base });
            assert_eq!(gear(&sim), -1);
            sim.run(1.5, |_| Controls {
                brake: pedal,
                ..base
            });
            -sim.speed()
        };
        let (light, half, full) = (reverse_speed(0.25), reverse_speed(0.5), reverse_speed(1.0));
        assert!(
            0.3 < light && light < half && half < full,
            "{}: {light} {half} {full}",
            v.name
        );
    }
}

#[test]
fn manual_reverse_needs_a_standstill_and_the_throttle_drives_it() {
    for v in &vehicles() {
        let manual = Controls::default();
        let down = Controls {
            shift_down: true,
            ..manual
        };
        // Rolling forward: 1st to neutral, but never reverse.
        let mut sim = Sim::new(v, &Flat);
        sim.launch_at(5.0);
        sim.state.drive.gear = 1;
        sim.step(&down);
        sim.run(0.5, |_| manual);
        sim.step(&down);
        assert_eq!(gear(&sim), 0, "{}", v.name);
        // At a standstill: 1st, neutral, reverse.
        let mut sim = Sim::new(v, &Flat);
        sim.step(&down);
        sim.run(0.5, |_| manual);
        sim.step(&down);
        assert_eq!(gear(&sim), -1, "{}", v.name);
        sim.run(0.5, |_| manual);
        // The throttle drives backwards; the brake still brakes.
        sim.run(3.0, |_| Controls {
            throttle: 1.0,
            ..manual
        });
        assert!(sim.speed() < -3.0, "{}: {}", v.name, sim.speed());
        let mut fastest: f64 = 0.0;
        for _ in 0..240 * 10 {
            sim.step(&Controls {
                throttle: 1.0,
                ..manual
            });
            fastest = fastest.max(-sim.speed());
        }
        assert!(fastest < REVERSE_TOP_SPEED + 0.5, "{}: {fastest}", v.name);
        sim.run(3.0, |_| Controls {
            brake: 1.0,
            ..manual
        });
        assert!(sim.speed().abs() < 0.05, "{}: {}", v.name, sim.speed());
        assert_eq!(gear(&sim), -1, "manual stays in reverse");
    }
}
