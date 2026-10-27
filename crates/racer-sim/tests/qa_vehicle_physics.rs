//! QA repro tests for vehicle-physics bugs in the Avian integration
//! (docs/qa/vehicle-physics.md). Each test fails on the current code and is
//! ignored so the suite stays green; run them with
//! `cargo test -p racer-sim --test qa_vehicle_physics -- --ignored`.

use avian3d::prelude::*;
use bevy::prelude::*;
use racer_dynamics::{Controls, Surface, load_car};
use racer_sim::{
    finish_headless, headless_app,
    track::{GroundSurface, Layer, SKIDPAD_CENTER, spawn_track, test_track},
    vehicle::{Car, CarControls, CarTelemetry, spawn_car},
};

fn app() -> App {
    let mut app = headless_app();
    finish_headless(&mut app);
    app
}

fn controls(app: &mut App, car: Entity, c: Controls) {
    app.world_mut().get_mut::<CarControls>(car).unwrap().0 = Controls {
        automatic: true,
        ..c
    };
}

fn telemetry(app: &App, car: Entity) -> racer_dynamics::Telemetry {
    app.world().get::<CarTelemetry>(car).unwrap().telemetry
}

/// A car parked, then rolling at 1 m/s, on the test track's grass (the
/// 4000 m ground) keeps all four wheels on the ground, with steady loads.
#[test]
fn qa_phys_4_wheels_keep_contact_on_the_grass_box() {
    let track = test_track();
    // A grass spot well away from the road and the skidpad.
    let spot = (0..40)
        .flat_map(|i| {
            (0..40).map(move |j| Vec3::new(-400.0 + 20.0 * i as f32, 0.0, -400.0 + 20.0 * j as f32))
        })
        .find(|p| {
            p.distance(SKIDPAD_CENTER) > 120.0
                && track
                    .centerline
                    .iter()
                    .all(|c| c.with_y(0.0).distance(*p) > 40.0)
        })
        .unwrap();
    let mut lost = Vec::new();
    for id in ["vw-golf-gti-mk8", "bmw-m3-competition-g80"] {
        let spec = load_car(id).unwrap();
        let mut app = app();
        let car = {
            let world = app.world_mut();
            let mut commands = world.commands();
            spawn_track(&mut commands, &track);
            let car =
                spawn_car(&mut commands, &spec, Transform::from_translation(spot), ()).unwrap();
            world.flush();
            car
        };
        for _ in 0..240 {
            app.update();
        }
        let mut dropouts = 0;
        let mut max_load: f64 = 0.0;
        for step in 0..240 * 4 {
            let rolling = step >= 240 * 2;
            let slow = telemetry(&app, car).speed < 1.0;
            controls(
                &mut app,
                car,
                Controls {
                    brake: if rolling { 0.0 } else { 1.0 },
                    throttle: if rolling && slow { 0.4 } else { 0.0 },
                    ..default()
                },
            );
            app.update();
            let t = telemetry(&app, car);
            dropouts += t.wheels.iter().filter(|w| !w.contact).count();
            max_load = t.wheels.iter().map(|w| w.load).fold(max_load, f64::max);
        }
        if dropouts > 0 {
            lost.push(format!(
                "{id}: {dropouts} of {} wheel-steps without contact",
                4 * 240 * 4
            ));
        }
        // Static corner loads are 3.7-4.6 kN; the misses swung them to 16.6 kN.
        if max_load > 6000.0 {
            lost.push(format!("{id}: wheel load peaked at {max_load:.0} N"));
        }
    }
    assert!(
        lost.is_empty(),
        "wheels lost contact on flat grass at {spot}:\n{}",
        lost.join("\n")
    );
}

/// Rolling at 1 m/s onto an 8 cm step (a box with a vertical edge, like the
/// test track's curbs and pad): the wheel should climb the edge over several
/// centimetres of travel, not jump to the bump stop.
#[test]
#[ignore = "QA-PHYS-5"]
fn qa_phys_5_wheel_climbs_a_step_edge_smoothly() {
    let spec = load_car("vw-golf-gti-mk8").unwrap();
    let mut app = app();
    let step_h = 0.08;
    let car = {
        let world = app.world_mut();
        let mut commands = world.commands();
        for (y, size, z) in [(-0.5, 60.0, 0.0), (step_h - 0.5, 34.0, -19.5)] {
            commands.spawn((
                Transform::from_xyz(0.0, y, z),
                RigidBody::Static,
                Collider::cuboid(20.0, 1.0, size),
                GroundSurface(Surface::DryAsphalt),
                Friction::new(0.6),
                CollisionLayers::new(Layer::World, LayerMask::ALL),
            ));
        }
        let car = spawn_car(&mut commands, &spec, Transform::IDENTITY, ()).unwrap();
        world.flush();
        car
    };
    for _ in 0..240 {
        app.update();
    }
    let hardpoint = app
        .world()
        .get::<Car>(car)
        .unwrap()
        .vehicle
        .wheel_geometry(0)
        .hardpoint;
    let static_load = telemetry(&app, car).wheels[0].load;
    let (mut max_jump, mut max_load) = (0.0f64, 0.0f64);
    let mut prev = telemetry(&app, car).wheels[0].travel;
    for _ in 0..240 * 4 {
        let slow = telemetry(&app, car).speed < 1.0;
        controls(
            &mut app,
            car,
            Controls {
                throttle: if slow { 0.3 } else { 0.0 },
                ..default()
            },
        );
        app.update();
        let tr = *app.world().get::<Transform>(car).unwrap();
        let wheel_z = (tr.translation + tr.rotation * hardpoint.as_vec3()).z;
        let t = telemetry(&app, car).wheels[0];
        // Only while the front-left wheel is at the edge (z = -2.5).
        if (-2.9..-2.1).contains(&wheel_z) {
            max_jump = max_jump.max(prev - t.travel);
            max_load = max_load.max(t.load);
        }
        prev = t.travel;
    }
    assert!(
        max_jump < 0.02 && max_load < 3.0 * static_load,
        "at the step edge travel dropped {max_jump:.3} m in one step, load peaked at \
         {max_load:.0} N ({:.1}x static {static_load:.0} N)",
        max_load / static_load
    );
}
