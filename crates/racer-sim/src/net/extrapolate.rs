//! Remote cars on a client: extrapolated from their last snapshot with the
//! inputs the server last applied to them.
//!
//! Throttle, brake and steering tell where a car is going much better than
//! its velocity alone (dead reckoning): a car braking for a corner slows and
//! turns in. The model is the point-mass car the AI plans with (drive,
//! braking and grip per car) plus a kinematic bicycle for the yaw rate, which
//! is cheap enough to re-run from every new snapshot for every remote car.

use bevy::prelude::*;
use racer_dynamics::Controls;
use racer_trackc::profile::CarModel;

use super::protocol::Body;
use crate::ai::CarParams;

/// What the extrapolation knows about a car.
#[derive(Clone, Debug)]
pub struct Model {
    pub wheelbase: f32,
    pub max_steer: f32,
    /// Brake deceleration at full pedal without the grip limit (m/s^2).
    pub brake_gain: f32,
    pub car: CarModel,
}

impl From<&CarParams> for Model {
    fn from(p: &CarParams) -> Self {
        Model {
            wheelbase: p.wheelbase,
            max_steer: p.max_steer,
            brake_gain: p.brake_gain,
            car: p.model.clone(),
        }
    }
}

/// Time constants (s): yaw rate towards the steering's, sideways sliding
/// and pitch/roll rates dying out.
const YAW_RESPONSE: f32 = 0.12;
const SLIDE_DECAY: f32 = 0.4;
const ROCK_DECAY: f32 = 0.15;

/// Advances `body` by `dt` under `controls`.
pub fn step(body: &mut Body, controls: &Controls, model: &Model, dt: f32) {
    let up = body.rotation * Vec3::Y;
    let forward = body.rotation * Vec3::NEG_Z;
    let v = body.velocity;
    let along = v.dot(forward);
    let vertical = v.dot(up);
    let side = v - forward * along - up * vertical;
    let speed = along.abs();

    let car = &model.car;
    let drive = controls.throttle as f32 * car.drive_at(speed);
    let brake = (controls.brake as f32 * model.brake_gain).min(car.braking)
        + controls.handbrake as f32 * 0.5 * car.braking;
    // Brakes and drag slow the car towards rest, never through it.
    let mut along_next = along + (drive - car.drag * along * along.abs()) * dt;
    let slow = brake * dt;
    along_next = if along_next > 0.0 {
        (along_next - slow).max(0.0)
    } else {
        (along_next + slow).min(0.0)
    };

    // Kinematic bicycle yaw rate, capped by the grip.
    let steer = controls.steer as f32 * model.max_steer;
    let wanted = along * steer.tan() / model.wheelbase;
    let cap = car.lateral / speed.max(1.0);
    let wanted = -wanted.clamp(-cap, cap);
    let w = body.angular_velocity;
    let yaw = w.dot(up);
    let rock = w - up * yaw;
    let yaw = yaw + (wanted - yaw) * (dt / YAW_RESPONSE).min(1.0);
    let rock = rock * (1.0 - (dt / ROCK_DECAY).min(1.0));
    let side = side * (1.0 - (dt / SLIDE_DECAY).min(1.0));

    let turn = Quat::from_axis_angle(up, yaw * dt);
    body.rotation = (turn * body.rotation).normalize();
    let forward = body.rotation * Vec3::NEG_Z;
    body.velocity = forward * along_next + turn * side + up * vertical;
    body.angular_velocity = up * yaw + rock;
    body.position += (v + body.velocity) * 0.5 * dt;
}

/// Constant velocity and angular velocity, for comparison.
pub fn dead_reckon(body: &mut Body, dt: f32) {
    body.position += body.velocity * dt;
    let w = body.angular_velocity;
    if w.length_squared() > 0.0 {
        body.rotation = (Quat::from_scaled_axis(w * dt) * body.rotation).normalize();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model() -> Model {
        Model {
            wheelbase: 2.8,
            max_steer: 0.6,
            brake_gain: 20.0,
            car: CarModel {
                id: "test".into(),
                mass: 1500.0,
                lateral: 10.0,
                braking: 11.0,
                traction: 9.0,
                drag: 0.0004,
                lift: 0.0,
                top_speed: 70.0,
                drive: (0..80).map(|v| 8.0 - v as f32 * 0.08).collect(),
            },
        }
    }

    fn moving(speed: f32) -> Body {
        Body {
            velocity: Vec3::NEG_Z * speed,
            ..default()
        }
    }

    #[test]
    fn braking_slows_to_rest_and_stays() {
        let mut b = moving(30.0);
        let c = Controls {
            brake: 1.0,
            ..default()
        };
        for _ in 0..240 * 4 {
            step(&mut b, &c, &model(), 1.0 / 240.0);
        }
        assert!(b.velocity.length() < 1e-3, "{:?}", b.velocity);
        // 30 m/s at 11 m/s^2 stops in about 41 m.
        assert!((-45.0..-38.0).contains(&b.position.z), "{}", b.position.z);
    }

    #[test]
    fn throttle_accelerates_and_steering_turns_right() {
        let mut b = moving(20.0);
        let c = Controls {
            throttle: 1.0,
            steer: 0.2,
            ..default()
        };
        for _ in 0..240 {
            step(&mut b, &c, &model(), 1.0 / 240.0);
        }
        assert!(b.velocity.length() > 25.0);
        // Right of the start heading (-Z) is +X.
        assert!(b.position.x > 1.0, "{:?}", b.position);
        // The yaw rate respects the grip: v * r <= lateral.
        let r = b.angular_velocity.y.abs();
        assert!(r * b.velocity.length() <= 10.5, "{r}");
    }

    #[test]
    fn a_car_at_rest_pulls_away() {
        let mut b = Body::default();
        let c = Controls {
            throttle: 1.0,
            ..default()
        };
        for _ in 0..240 {
            step(&mut b, &c, &model(), 1.0 / 240.0);
        }
        assert!(b.position.z < -2.0, "{:?}", b.position);
    }
}
