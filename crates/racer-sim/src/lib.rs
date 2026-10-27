//! Gameplay shared by the client and the dedicated server.

pub mod ai;
pub mod autopilot;
pub mod race;
pub mod rules;
pub mod track;
pub mod vehicle;

use std::time::Duration;

use bevy::prelude::*;

/// Length of one simulation tick, shared by client and server.
pub fn tick_duration() -> Duration {
    Duration::from_secs_f64(racer_dynamics::chassis_dt())
}

/// Configures the fixed simulation step shared by client and server.
pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_duration(tick_duration()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::f32::consts::FRAC_PI_2;

    use bevy::time::TimeUpdateStrategy;
    use racer_dynamics::load_car;

    use crate::{
        track::{spawn_track, test_track},
        vehicle::{CarControls, CarTelemetry, VehiclePlugin, spawn_car},
    };

    /// Headless app with physics that advances exactly one tick per update.
    fn physics_app() -> App {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, TransformPlugin, SimPlugin, VehiclePlugin))
            .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()));
        app.finish();
        app.cleanup();
        app
    }

    #[test]
    fn car_settles_on_the_track_and_drives_off() {
        let mut app = physics_app();
        let spec = load_car("bmw-m3-competition-g80").unwrap();
        let world = app.world_mut();
        let track = test_track();
        let car = {
            let mut commands = world.commands();
            spawn_track(&mut commands, &track);
            spawn_car(&mut commands, &spec, track.start, ()).unwrap()
        };
        world.flush();
        let mut max_speed: f32 = 0.0;
        for k in 0..960 {
            app.update();
            if k >= 720 {
                let v = app
                    .world()
                    .get::<avian3d::prelude::LinearVelocity>(car)
                    .unwrap()
                    .0;
                max_speed = max_speed.max(v.length());
            }
        }
        assert!(max_speed < 0.03, "not settled: {max_speed} m/s");
        let ride = app.world().get::<Transform>(car).unwrap().translation.y;
        let t = app.world().get::<CarTelemetry>(car).unwrap().telemetry;
        assert!(t.wheels.iter().all(|w| w.contact), "{t:?}");
        assert!((0.4..0.6).contains(&ride), "ride height {ride}");
        let weight: f64 = t.wheels.iter().map(|w| w.load).sum();
        assert!(
            (weight / (1810.0 * 9.81) - 1.0).abs() < 0.03,
            "wheel loads {weight}"
        );

        app.world_mut()
            .get_mut::<CarControls>(car)
            .unwrap()
            .0
            .throttle = 1.0;
        for _ in 0..480 {
            app.update();
        }
        let t = app.world().get::<CarTelemetry>(car).unwrap().telemetry;
        let z = app.world().get::<Transform>(car).unwrap().translation.z;
        assert!(t.speed > 12.0, "speed after 2 s full throttle: {}", t.speed);
        assert!(z < -10.0, "drove forward (-Z): {z}");
    }

    /// Random extreme inputs on the Avian track (crest, curbs, grass): the
    /// coupled simulation stays finite and bounded.
    #[test]
    fn random_inputs_on_the_track_stay_finite() {
        use avian3d::prelude::{AngularVelocity, LinearVelocity};
        use racer_dynamics::{Assists, Controls, spec::CARS};

        let mut seed = 0x2545_f491_4f6c_dd1du64;
        let mut rand = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            (seed >> 11) as f64 / (1u64 << 53) as f64
        };
        for id in CARS {
            let mut app = physics_app();
            let spec = load_car(id).unwrap();
            let track = test_track();
            let car = {
                let world = app.world_mut();
                let mut commands = world.commands();
                spawn_track(&mut commands, &track);
                // Start on the back straight, just before the crest.
                let at = Transform::from_xyz(230.0, 0.02, 25.0)
                    .with_rotation(Quat::from_rotation_y(FRAC_PI_2));
                let car = spawn_car(&mut commands, &spec, at, ()).unwrap();
                world.flush();
                car
            };
            let mut controls = Controls::default();
            for step in 0..240 * 20 {
                if step % 60 == 0 {
                    controls = Controls {
                        throttle: if rand() < 0.6 { 1.0 } else { rand() },
                        brake: if rand() < 0.25 { 1.0 } else { 0.0 },
                        steer: 2.0 * rand() - 1.0,
                        handbrake: if rand() < 0.1 { 1.0 } else { 0.0 },
                        shift_down: rand() < 0.1,
                        automatic: rand() < 0.7,
                        assists: Assists {
                            abs: rand() < 0.5,
                            tcs: rand() < 0.5,
                            esc: rand() < 0.5,
                        },
                        ..default()
                    };
                } else {
                    controls.shift_down = false;
                }
                app.world_mut().get_mut::<CarControls>(car).unwrap().0 = controls;
                app.update();
                let w = app.world();
                let (p, v, av) = (
                    w.get::<Transform>(car).unwrap().translation,
                    w.get::<LinearVelocity>(car).unwrap().0,
                    w.get::<AngularVelocity>(car).unwrap().0,
                );
                assert!(
                    p.is_finite() && v.is_finite() && av.is_finite(),
                    "{id}: non-finite at step {step}"
                );
                assert!(
                    v.length() < 100.0 && av.length() < 30.0,
                    "{id}: v {v} w {av} at step {step}"
                );
            }
        }
    }

    #[test]
    fn fixed_step_matches_chassis_rate() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.update();
        let fixed = app.world().resource::<Time<Fixed>>();
        assert_eq!(fixed.timestep(), tick_duration());
        assert_eq!(fixed.timestep().as_micros(), 4166);
    }
}
