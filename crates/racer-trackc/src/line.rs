//! Racing line: the minimum-curvature path through the corridor the road
//! leaves for a car (road width minus the car's half width and a margin,
//! with the wheels allowed onto the curbs).
//!
//! The line is the centreline offset sideways by `offset` at each sample. It
//! minimises the discrete integral of curvature squared (turning angle
//! squared over length at each point) within the offset bounds, by
//! coordinate descent with a Newton step per offset, from a coarse to a fine
//! sample stride so long stretches settle quickly.

use glam::{Vec2, Vec3};

use crate::geometry::{Geometry, Sample};

/// Distance kept between the car's side and the road edge (m).
pub const EDGE_MARGIN: f32 = 0.3;
/// How far a wheel may run over a curb (m).
pub const CURB_USE: f32 = 0.5;
/// Half the width of the widest car (m).
pub const CAR_HALF_WIDTH: f32 = 0.96;

#[derive(Clone, Copy, Debug)]
pub struct LinePoint {
    /// Centreline distance of the sample this point is offset from.
    pub s: f32,
    /// Sideways offset from the centreline (m, positive right).
    pub offset: f32,
    pub pos: Vec3,
    /// Distance along the line from the start line.
    pub distance: f32,
    /// Plan curvature of the line (1/m, positive right).
    pub curvature: f32,
}

#[derive(Clone, Debug)]
pub struct RacingLine {
    pub points: Vec<LinePoint>,
    /// Closed length of the line.
    pub length: f32,
}

/// Offset bounds (left, right) at a sample for a car of `half_width`.
pub fn bounds(sample: &Sample, half_width: f32) -> (f32, f32) {
    let edge =
        sample.half_width - half_width - EDGE_MARGIN + if sample.curbs { CURB_USE } else { 0.0 };
    (-edge, edge)
}

fn plan(v: Vec3) -> Vec2 {
    Vec2::new(v.x, v.z)
}

/// Builds a line from per-sample offsets.
pub fn from_offsets(geometry: &Geometry, offsets: &[f32]) -> RacingLine {
    let samples = &geometry.samples;
    let n = samples.len();
    let pos: Vec<Vec3> = samples
        .iter()
        .zip(offsets)
        .map(|(s, &o)| s.at(o, 0.0))
        .collect();
    let mut distance = 0.0;
    let mut points: Vec<LinePoint> = (0..n)
        .map(|k| {
            let p = LinePoint {
                s: samples[k].s,
                offset: offsets[k],
                pos: pos[k],
                distance,
                curvature: curvature(
                    plan(pos[(k + n - 3) % n]),
                    plan(pos[k]),
                    plan(pos[(k + 3) % n]),
                ),
            };
            distance += pos[k].distance(pos[(k + 1) % n]);
            p
        })
        .collect();
    // Light smoothing of the three-point curvature.
    let raw: Vec<f32> = points.iter().map(|p| p.curvature).collect();
    for k in 0..n {
        points[k].curvature = (raw[(k + n - 1) % n] + 2.0 * raw[k] + raw[(k + 1) % n]) / 4.0;
    }
    RacingLine {
        points,
        length: distance,
    }
}

/// Signed curvature of the circle through three plan points (positive right).
pub fn curvature(a: Vec2, b: Vec2, c: Vec2) -> f32 {
    let cross = (b - a).perp_dot(c - b);
    let denom = a.distance(b) * b.distance(c) * c.distance(a);
    if denom < 1e-6 {
        0.0
    } else {
        2.0 * cross / denom
    }
}

/// Minimum-curvature line for cars up to `half_width` wide.
pub fn minimum_curvature(geometry: &Geometry, half_width: f32) -> RacingLine {
    let samples = &geometry.samples;
    let n = samples.len();
    let center: Vec<Vec2> = samples.iter().map(|s| plan(s.pos)).collect();
    let normal: Vec<Vec2> = samples.iter().map(|s| plan(s.right).normalize()).collect();
    let limits: Vec<(f32, f32)> = samples.iter().map(|s| bounds(s, half_width)).collect();
    let mut offset = vec![0.0f32; n];
    for stride in [32usize, 16, 8, 4, 2, 1] {
        let idx: Vec<usize> = (0..n).step_by(stride).collect();
        let m = idx.len();
        if m < 8 {
            continue;
        }
        let point = |offset: &[f32], j: usize| {
            let k = idx[j % m];
            center[k] + normal[k] * offset[k]
        };
        // Turning angle squared over length at point j: the discrete
        // integral of curvature squared, independent of how the line's
        // points are spaced.
        let bend = |offset: &[f32], j: usize| {
            let (a, b, c) = (
                point(offset, j + m - 1),
                point(offset, j),
                point(offset, j + 1),
            );
            let (u, v) = (b - a, c - b);
            let theta = u.perp_dot(v).atan2(u.dot(v));
            theta * theta / ((u.length() + v.length()) / 2.0).max(1e-3)
        };
        let local = |offset: &[f32], j: usize| {
            bend(offset, j + m - 1) + bend(offset, j) + bend(offset, j + 1)
        };
        let eps = 0.02 * stride as f32;
        let sweeps = if stride >= 8 { 3000 } else { 1500 };
        for _ in 0..sweeps {
            for (j, &k) in idx.iter().enumerate() {
                let x = offset[k];
                let f0 = local(&offset, j);
                offset[k] = x + eps;
                let fp = local(&offset, j);
                offset[k] = x - eps;
                let fm = local(&offset, j);
                let slope = (fp - fm) / (2.0 * eps);
                let curve = (fp - 2.0 * f0 + fm) / (eps * eps);
                let max_step = 0.25 * stride as f32;
                let step = if curve > 1e-9 {
                    -slope / curve
                } else {
                    -slope.signum() * eps
                };
                let next = (x + step.clamp(-max_step, max_step)).clamp(limits[k].0, limits[k].1);
                offset[k] = next;
                if local(&offset, j) > f0 {
                    offset[k] = x;
                }
            }
        }
        // Interpolate the coarse solution onto the next stride.
        if stride > 1 {
            for j in 0..m {
                let (a, b) = (idx[j], idx[(j + 1) % m]);
                let span = if b > a { b - a } else { n - a + b };
                for t in 1..span {
                    let k = (a + t) % n;
                    let w = t as f32 / span as f32;
                    offset[k] =
                        (offset[a] * (1.0 - w) + offset[b] * w).clamp(limits[k].0, limits[k].1);
                }
            }
        }
    }
    from_offsets(geometry, &offset)
}

impl RacingLine {
    /// Index of the point nearest `pos` within `window` points of `hint`.
    pub fn nearest(&self, pos: Vec3, hint: usize, window: usize) -> usize {
        let n = self.points.len();
        (0..=2 * window)
            .map(|i| (hint + n + i - window) % n)
            .min_by(|&a, &b| {
                self.points[a]
                    .pos
                    .distance_squared(pos)
                    .total_cmp(&self.points[b].pos.distance_squared(pos))
            })
            .unwrap_or(0)
    }
}
