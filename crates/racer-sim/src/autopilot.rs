//! Scripted test driver: follows a closed centreline with pure-pursuit
//! steering and a speed profile from the line's curvature. Used for
//! telemetry runs and captures; the real AI drivers come in M2.

use bevy::prelude::*;
use racer_dynamics::Controls;

/// Speed and steering targets along a closed line.
#[derive(Clone, Debug)]
pub struct Autopilot {
    points: Vec<Vec3>,
    /// Target speed at each point (m/s).
    speeds: Vec<f32>,
}

impl Autopilot {
    /// `grip` is the lateral acceleration (m/s^2) to corner at, `brake` the
    /// deceleration to plan braking with, `top` the speed cap.
    pub fn new(points: Vec<Vec3>, grip: f32, brake: f32, top: f32) -> Self {
        let n = points.len();
        let at = |i: usize| points[i % n];
        // Corner speed from the circle through each point and its neighbours 5 points away.
        let mut speeds: Vec<f32> = (0..n)
            .map(|i| {
                let (a, b, c) = (at(i + n - 5), at(i), at(i + 5));
                let area2 = (b - a).cross(c - a).length();
                let radius =
                    (a.distance(b) * b.distance(c) * c.distance(a)) / (2.0 * area2).max(1e-6);
                (grip * radius).sqrt().min(top)
            })
            .collect();
        // Braking zones: walk backwards (twice around) limiting each speed by the next.
        for k in (0..2 * n).rev() {
            let (i, j) = (k % n, (k + 1) % n);
            let d = points[i].distance(points[j]);
            speeds[i] = speeds[i].min((speeds[j] * speeds[j] + 2.0 * brake * d).sqrt());
        }
        Autopilot { points, speeds }
    }

    fn nearest(&self, position: Vec3) -> usize {
        (0..self.points.len())
            .min_by(|&a, &b| {
                let da = self.points[a].distance_squared(position);
                let db = self.points[b].distance_squared(position);
                da.total_cmp(&db)
            })
            .unwrap_or(0)
    }

    /// Target speed (m/s) at the point nearest `position`.
    pub fn target_speed(&self, position: Vec3) -> f32 {
        self.speeds[self.nearest(position)]
    }

    /// Controls for a car at `transform` moving at `velocity`, given its
    /// wheelbase and maximum road-wheel angle (rad).
    pub fn drive(
        &self,
        transform: &Transform,
        velocity: Vec3,
        wheelbase: f32,
        max_steer: f32,
    ) -> Controls {
        let n = self.points.len();
        let speed = velocity.dot(*transform.forward());
        let i = self.nearest(transform.translation);
        // Look ahead about 0.8 s, at least 8 m.
        let lookahead = (0.8 * speed.abs()).max(8.0);
        let mut j = i;
        while self.points[j].distance(transform.translation) < lookahead && (j + n - i) % n < n / 2
        {
            j = (j + 1) % n;
        }
        let local = transform.rotation.inverse() * (self.points[j] - transform.translation);
        let alpha = (-local.x).atan2(-local.z);
        let delta = (2.0 * wheelbase * alpha.sin() / lookahead).atan();
        // Longitudinal: the deceleration needed to meet the slowest target
        // within the next ~1 s (at least 10 m), as pedal feed-forward.
        let horizon = (speed.abs().max(10.0) / 2.0) as usize + 1;
        let (mut accel, mut dist) = (f32::MAX, 0.0);
        for k in 1..horizon {
            dist += self.points[(i + k - 1) % n].distance(self.points[(i + k) % n]);
            let v = self.speeds[(i + k) % n];
            accel = accel.min((v * v - speed * speed) / (2.0 * dist));
        }
        let (throttle, brake) = if accel >= -0.5 {
            ((0.3 + accel / 3.0).clamp(0.0, 1.0), 0.0)
        } else {
            (0.0, (-accel / 10.0).clamp(0.0, 1.0))
        };
        Controls {
            throttle: throttle as f64,
            brake: brake as f64,
            steer: (-delta / max_steer).clamp(-1.0, 1.0) as f64,
            automatic: true,
            ..default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::track::test_track;

    #[test]
    fn slows_for_the_hairpin_and_runs_flat_out_on_the_straight() {
        let line = test_track().centerline;
        let pilot = Autopilot::new(line.clone(), 8.0, 7.0, 60.0);
        let straight = pilot.target_speed(Vec3::new(0.0, 0.0, -60.0));
        let hairpin = pilot.target_speed(Vec3::new(15.0, 0.0, -275.0));
        assert!(straight > 40.0, "{straight}");
        assert!(hairpin < 13.0, "{hairpin}");
    }
}
