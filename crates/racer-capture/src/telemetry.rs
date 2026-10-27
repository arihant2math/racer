//! Headless telemetry runs: drives a car through scripted maneuvers on the
//! Bevy/Avian test track (no renderer) and writes one CSV per maneuver.
//! `tools/plot_telemetry.py` turns them into PNG plots.

use std::{fmt::Write as _, path::Path};

use avian3d::prelude::{AngularVelocity, LinearVelocity};
use bevy::{prelude::*, time::TimeUpdateStrategy};
use racer_dynamics::{CarSpec, Controls, load_car};
use racer_sim::{
    SimPlugin,
    autopilot::Autopilot,
    tick_duration,
    track::{SKIDPAD_CENTER, SKIDPAD_RADIUS, skidpad_start, spawn_track, test_track},
    vehicle::{Car, CarControls, CarTelemetry, VehiclePlugin, spawn_car},
};

/// Maneuvers written by [`run`], one `<car>-<maneuver>.csv` each.
pub const MANEUVERS: [&str; 3] = ["lap", "skidpad", "launch-brake"];

/// Samples are written at 60 Hz (every 4th tick).
const SAMPLE_EVERY: u32 = 4;

/// One simulation with a single car, advanced tick by tick.
struct Run {
    app: App,
    car: Entity,
    csv: String,
    ticks: u32,
}

impl Run {
    fn new(spec: &CarSpec, at: Transform) -> Self {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, SimPlugin, VehiclePlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
        app.finish();
        app.cleanup();
        let world = app.world_mut();
        let mut commands = world.commands();
        spawn_track(&mut commands, &test_track());
        let car = spawn_car(&mut commands, spec, at).expect("valid car");
        world.flush();
        let mut csv = String::from(
            "t,x,y,z,speed_kmh,long_g,lat_g,yaw_rate,sideslip_deg,roll_deg,pitch_deg,steer,throttle,brake,rpm,gear",
        );
        for w in ["fl", "fr", "rl", "rr"] {
            write!(csv, ",load_{w},slip_ratio_{w},slip_angle_{w}").unwrap();
        }
        csv.push('\n');
        Run {
            app,
            car,
            csv,
            ticks: 0,
        }
    }

    fn time(&self) -> f32 {
        self.ticks as f32 * tick_duration().as_secs_f32()
    }

    fn transform(&self) -> Transform {
        *self.app.world().get::<Transform>(self.car).unwrap()
    }

    fn velocity(&self) -> Vec3 {
        self.app.world().get::<LinearVelocity>(self.car).unwrap().0
    }

    fn speed(&self) -> f32 {
        self.velocity().dot(*self.transform().forward())
    }

    fn car(&self) -> &Car {
        self.app.world().get::<Car>(self.car).unwrap()
    }

    fn step(&mut self, controls: Controls) {
        self.app
            .world_mut()
            .get_mut::<CarControls>(self.car)
            .unwrap()
            .0 = controls;
        self.app.update();
        self.ticks += 1;
        if self.ticks.is_multiple_of(SAMPLE_EVERY) {
            self.sample(&controls);
        }
    }

    fn sample(&mut self, c: &Controls) {
        let world = self.app.world();
        let t = world.get::<CarTelemetry>(self.car).unwrap();
        let tr = self.transform();
        let v = self.velocity();
        let yaw = world.get::<AngularVelocity>(self.car).unwrap().0.y;
        let sideslip = if v.length() > 2.0 {
            tr.forward().cross(v.normalize()).y.asin().to_degrees()
        } else {
            0.0
        };
        // Roll positive = right side down, pitch positive = nose up.
        let roll = tr.right().y.asin() * -1.0;
        let pitch = tr.forward().y.asin();
        let tel = &t.telemetry;
        let csv = &mut self.csv;
        write!(
            csv,
            "{:.4},{:.2},{:.3},{:.2},{:.2},{:.4},{:.4},{:.4},{:.3},{:.3},{:.3},{:.4},{:.3},{:.3},{:.0},{}",
            self.ticks as f32 * tick_duration().as_secs_f32(),
            tr.translation.x,
            tr.translation.y,
            tr.translation.z,
            tel.speed * 3.6,
            t.acceleration.dot(*tr.forward()) / 9.81,
            t.acceleration.dot(*tr.right()) / 9.81,
            yaw,
            sideslip,
            roll.to_degrees(),
            pitch.to_degrees(),
            c.steer,
            tel.throttle,
            c.brake,
            tel.rpm,
            tel.gear
        )
        .unwrap();
        for w in &tel.wheels {
            write!(
                csv,
                ",{:.1},{:.4},{:.3}",
                w.load,
                w.slip_ratio,
                w.slip_angle.to_degrees()
            )
            .unwrap();
        }
        csv.push('\n');
    }
}

/// Runs every maneuver for car `id` and writes the CSVs into `out_dir`.
pub fn run(id: &str, out_dir: &Path) -> Result<Vec<std::path::PathBuf>, String> {
    let spec = load_car(id)?;
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let grip = 0.85 * spec.reference.skidpad_g.value as f32 * 9.81;
    let mut written = Vec::new();
    for maneuver in MANEUVERS {
        let run = match maneuver {
            "lap" => lap(&spec, grip),
            "skidpad" => skidpad(&spec),
            _ => launch_brake(&spec),
        };
        let path = out_dir.join(format!("{id}-{maneuver}.csv"));
        std::fs::write(&path, &run.csv).map_err(|e| format!("{}: {e}", path.display()))?;
        info!(
            "{}: {} samples",
            path.display(),
            run.csv.lines().count() - 1
        );
        written.push(path);
    }
    Ok(written)
}

/// One lap of the test track behind the autopilot, from a standing start.
fn lap(spec: &CarSpec, grip: f32) -> Run {
    let track = test_track();
    let pilot = Autopilot::new(track.centerline, grip, 0.75 * grip, 70.0);
    let mut run = Run::new(spec, track.start);
    let mut left_start = false;
    while run.time() < 150.0 {
        let tr = run.transform();
        let car = run.car();
        let (wheelbase, max_steer) = (car.vehicle.wheelbase as f32, car.vehicle.max_steer as f32);
        let c = pilot.drive(&tr, run.velocity(), wheelbase, max_steer);
        run.step(c);
        // Done when back across the start line after leaving its area.
        let p = run.transform().translation;
        left_start |= p.z < -100.0;
        if left_start && p.x.abs() < 10.0 && p.z > -5.0 {
            break;
        }
    }
    run
}

/// Constant-radius circle on the skidpad (left turn), speed raised slowly to the limit.
fn skidpad(spec: &CarSpec) -> Run {
    let mut run = Run::new(spec, skidpad_start());
    let mut target = 10.0f32;
    while run.time() < 90.0 {
        let tr = run.transform();
        let rel = tr.translation - SKIDPAD_CENTER;
        let radial = Vec2::new(rel.x, rel.z).length() - SKIDPAD_RADIUS;
        if radial > 8.0 {
            break;
        }
        let ahead = (0.6 * run.speed()).max(6.0) / SKIDPAD_RADIUS;
        let angle = rel.z.atan2(rel.x) - ahead;
        let aim = SKIDPAD_CENTER + Vec3::new(angle.cos(), 0.0, angle.sin()) * SKIDPAD_RADIUS;
        let local = tr.rotation.inverse() * (aim - tr.translation);
        let car = run.car();
        let alpha = (-local.x).atan2(-local.z);
        let delta =
            (2.0 * car.vehicle.wheelbase as f32 * alpha.sin() / local.length().max(1.0)).atan();
        let steer = (-delta / car.vehicle.max_steer as f32).clamp(-1.0, 1.0);
        let e = target - run.speed();
        run.step(Controls {
            throttle: (0.25 + 0.5 * e).clamp(0.0, 1.0) as f64,
            steer: steer as f64,
            automatic: true,
            assists: racer_dynamics::Assists::OFF,
            ..default()
        });
        target += 0.12 * tick_duration().as_secs_f32();
    }
    run
}

/// Launch-control start to 120 km/h on the main straight, then a full ABS stop.
fn launch_brake(spec: &CarSpec) -> Run {
    let mut run = Run::new(spec, test_track().start);
    let auto = Controls {
        automatic: true,
        ..default()
    };
    for _ in 0..240 {
        run.step(Controls {
            throttle: 1.0,
            brake: 1.0,
            ..auto
        });
    }
    let hold_line = |run: &Run| {
        let tr = run.transform();
        let heading = tr.forward().x;
        (-1.5 * heading - 0.05 * tr.translation.x) as f64
    };
    while run.speed() < 120.0 / 3.6 && run.time() < 30.0 {
        let steer = hold_line(&run);
        run.step(Controls {
            throttle: 1.0,
            steer,
            ..auto
        });
    }
    while run.speed() > 0.2 && run.time() < 45.0 {
        let steer = hold_line(&run);
        run.step(Controls {
            brake: 1.0,
            steer,
            ..auto
        });
    }
    for _ in 0..120 {
        run.step(Controls { brake: 1.0, ..auto });
    }
    run
}
