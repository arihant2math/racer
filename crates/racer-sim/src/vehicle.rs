//! Vehicle plugin: Avian owns the chassis rigid body and its collisions,
//! wheels are shape casts against world colliders, and `racer-dynamics`
//! computes the chassis force and torque every fixed 240 Hz step.

use std::sync::Arc;

use avian3d::prelude::*;
use bevy::prelude::*;
use racer_dynamics::{
    CarSpec, ChassisState, Controls, DVec3, Telemetry, Vehicle, VehicleState, WheelContact,
    vehicle::WheelGeometry,
};

use crate::track::{GroundSurface, Layer};

/// Adds Avian physics and the vehicle step, and [`SimPlugin`](crate::SimPlugin)
/// if the app lacks it: the model must run at the 240 Hz chassis rate.
pub struct VehiclePlugin;

impl Plugin for VehiclePlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<crate::SimPlugin>() {
            app.add_plugins(crate::SimPlugin);
        }
        app.add_plugins(PhysicsPlugins::default())
            .configure_sets(FixedUpdate, ControlSystems.before(VehicleSystems))
            .add_systems(
                FixedUpdate,
                (drive_cars, record_acceleration)
                    .chain()
                    .in_set(VehicleSystems),
            );
    }
}

/// The vehicle step.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct VehicleSystems;

/// Systems that write [`CarControls`] (player input, AI drivers); they run
/// before the vehicle step.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub struct ControlSystems;

/// A car: immutable parameters shared between all cars of the same model.
#[derive(Component, Clone)]
pub struct Car {
    pub id: String,
    pub vehicle: Arc<Vehicle>,
    /// Steering-wheel to road-wheel ratio, for force feedback.
    pub steering_ratio: f64,
}

/// Internal state of the vehicle model (wheel spin, slips, gear, ...).
#[derive(Component, Clone, Copy, Debug)]
pub struct CarState(pub VehicleState);

/// Inputs applied at the next step.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CarControls(pub Controls);

/// Output of the last step plus the chassis accelerometer reading (world frame,
/// reads 1 g upwards at rest).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CarTelemetry {
    pub telemetry: Telemetry,
    pub acceleration: Vec3,
    last_velocity: Vec3,
}

/// Spawns a car resting at `transform` (the ground point below its centre of
/// mass), with `extra` components (rendering, player markers) on the body.
pub fn spawn_car(
    commands: &mut Commands,
    spec: &CarSpec,
    transform: Transform,
    extra: impl Bundle,
) -> Result<Entity, String> {
    let vehicle = Vehicle::from_spec(spec)?;
    let c = &spec.chassis;
    let ride = vehicle.ride_height() as f32;
    // Body box from 12 cm above the ground to 85 % of the roof height,
    // relative to the centre of mass.
    let (bottom, top) = (0.12, 0.85 * c.height_m as f32);
    let body = Vec3::new(c.width_m as f32, top - bottom, c.length_m as f32);
    let i = vehicle.inertia.as_vec3();
    let entity = commands
        .spawn((
            Name::new(spec.name.clone()),
            Transform::from_translation(transform.translation + transform.up() * ride)
                .with_rotation(transform.rotation),
            RigidBody::Dynamic,
            Mass(vehicle.mass as f32),
            AngularInertia::new(i),
            CenterOfMass::ZERO,
            (NoAutoMass, NoAutoAngularInertia, NoAutoCenterOfMass),
            SleepingDisabled,
            TransformInterpolation,
            CarState(vehicle.initial_state()),
            CarControls(Controls {
                automatic: true,
                ..default()
            }),
            CarTelemetry::default(),
            Car {
                id: spec.id.clone(),
                vehicle: Arc::new(vehicle),
                steering_ratio: spec.steering.ratio,
            },
            extra,
        ))
        .with_child((
            Collider::cuboid(body.x, body.y, body.z),
            ColliderDensity(0.0),
            Friction::new(0.4),
            Restitution::new(0.0),
            Transform::from_xyz(0.0, (bottom + top) / 2.0 - ride, 0.0),
            CarBody,
            CollisionLayers::new(Layer::Car, [Layer::World, Layer::Wall, Layer::Car]),
            CollidingEntities::default(),
        ))
        .id();
    Ok(entity)
}

/// The body collider of a car (a child of the [`Car`] entity).
#[derive(Component, Clone, Copy, Debug)]
pub struct CarBody;

/// Wheel-centre position in the car's local frame for a cast distance.
pub fn wheel_center(geometry: &WheelGeometry, travel: f64) -> Vec3 {
    (geometry.hardpoint - DVec3::Y * travel).as_vec3()
}

#[allow(clippy::type_complexity)]
fn drive_cars(
    time: Res<Time>,
    spatial: SpatialQuery,
    surfaces: Query<&GroundSurface>,
    mut cars: Query<(
        Entity,
        &Car,
        &mut CarState,
        &CarControls,
        &mut CarTelemetry,
        &Children,
        &RigidBody,
        Forces,
    )>,
) {
    let dt = time.delta_secs_f64();
    if dt <= 0.0 {
        return;
    }
    for (entity, car, mut state, controls, mut telemetry, children, body, mut forces) in &mut cars {
        // Cars moved by someone else (remote cars online) take no forces.
        if !body.is_dynamic() {
            continue;
        }
        let v = &car.vehicle;
        let rot = forces.rotation().0;
        let pos = forces.position().0;
        let chassis = ChassisState {
            position: pos.as_dvec3(),
            rotation: rot.as_dquat(),
            linear_velocity: forces.linear_velocity().as_dvec3(),
            angular_velocity: forces.angular_velocity().as_dvec3(),
        };
        let filter = SpatialQueryFilter::from_mask(Layer::World)
            .with_excluded_entities(std::iter::once(entity).chain(children.iter()));
        let down = Dir3::new(rot * Vec3::NEG_Y).unwrap_or(Dir3::NEG_Y);
        let contacts: [Option<WheelContact>; 4] = std::array::from_fn(|i| {
            let g = v.wheel_geometry(i);
            // Cylinder with its axis along the car's X (the axle).
            let wheel = Collider::cylinder(g.radius as f32, g.width as f32);
            let origin = pos + rot * g.hardpoint.as_vec3();
            let shape_rot = rot * Quat::from_rotation_z(std::f32::consts::FRAC_PI_2);
            let config = ShapeCastConfig::from_max_distance(g.max_distance as f32);
            let hit = spatial.cast_shape(&wheel, origin, shape_rot, down, &config, &filter)?;
            let up = -*down;
            // The cylinder cast finds where the wheel first touches, but on a
            // face its normal is unreliable (rim-edge features tilt it by
            // several degrees on flat ground) and its distance is off by
            // millimetres on big boxes (GJK in f32). A short ray at the
            // contact point gives the exact surface normal; the distance is
            // then solved against that plane.
            let ray_down = |p: Vec3| {
                spatial
                    .cast_ray(p + up * 0.05, down, 0.1, true, &filter)
                    .filter(|r| r.normal.dot(up) > 0.1)
                    .map(|r| (r.normal, p + up * 0.05 + *down * r.distance))
            };
            let plane = ray_down(hit.point1)
                .filter(|_| hit.distance > 0.0)
                .and_then(|(n, point)| {
                    let axle = rot * Vec3::X;
                    let an = axle.dot(n);
                    // Support extent of the cylinder along -n.
                    let extent = g.radius as f32 * (1.0 - an * an).max(0.0).sqrt()
                        + 0.5 * g.width as f32 * an.abs();
                    let distance = (n.dot(origin - point) - extent) / n.dot(up);
                    // The plane is the ground only if its face goes on under
                    // the wheel. At an edge (curb, step) the cylinder touches
                    // the corner and the plane of the face beyond it would
                    // lift the wheel onto that face at once; there the cast's
                    // own distance and normal (from the edge towards the
                    // wheel centre) are the contact.
                    let centre = origin + *down * distance;
                    let (_, under) = ray_down(centre - n * n.dot(centre - point))?;
                    (n.dot(under - point).abs() < 0.005).then_some((n, distance))
                });
            let (normal, distance) = match plane {
                Some(face) => face,
                None if hit.distance > 0.0 => (hit.normal1, hit.distance),
                None => (up, hit.distance),
            };
            Some(WheelContact {
                distance: f64::from(distance.clamp(0.0, g.max_distance as f32)),
                normal: normal.as_dvec3(),
                surface: surfaces.get(hit.entity).map(|s| s.0).unwrap_or_default(),
            })
        });
        let out = v.step(&mut state.0, &chassis, &controls.0, &contacts, dt);
        forces.apply_force(out.force.as_vec3());
        forces.apply_torque(out.torque.as_vec3());
        telemetry.telemetry = out.telemetry;
    }
}

/// Chassis acceleration from the velocity change over each physics step,
/// low-passed like a data logger's accelerometer (single solver steps add
/// millimetre-per-second blips that are invisible but noisy as acceleration).
fn record_acceleration(time: Res<Time>, mut cars: Query<(&LinearVelocity, &mut CarTelemetry)>) {
    const FILTER_TIME: f32 = 0.02;
    let dt = time.delta_secs();
    for (v, mut t) in &mut cars {
        if dt > 0.0 {
            let raw = (v.0 - t.last_velocity) / dt + Vec3::Y * 9.81;
            t.acceleration = t.acceleration.lerp(raw, (dt / FILTER_TIME).min(1.0));
        }
        t.last_velocity = v.0;
    }
}
