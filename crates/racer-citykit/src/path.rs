//! Road centrelines: chains of straights and circular arcs on the ground
//! plane with an optional linear grade, sampled into [`Frame`]s.
//!
//! This is the shape `racer-trackc` will hand the kit: a centreline that can
//! be evaluated at any arc length and offset sideways. Headings use the scene
//! convention: forward is −Z at heading 0, right is +X.

use bevy::math::{Vec2, Vec3};

use crate::mesh::Frame;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Segment {
    Straight {
        length: f32,
    },
    /// Circular arc; positive `angle` (radians) turns right.
    Arc {
        radius: f32,
        angle: f32,
    },
}

impl Segment {
    pub fn length(&self) -> f32 {
        match *self {
            Segment::Straight { length } => length,
            Segment::Arc { radius, angle } => radius * angle.abs(),
        }
    }
}

/// A centreline starting at `start` (x, z) with `heading` (radians, clockwise
/// from −Z seen from above, so a right turn increases it).
#[derive(Clone, Debug, PartialEq)]
pub struct Path {
    pub start: Vec2,
    pub heading: f32,
    pub elevation: f32,
    pub segments: Vec<Segment>,
}

/// Ground-plane pose on a path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose2 {
    pub pos: Vec2,
    pub heading: f32,
    /// Signed curvature (1/m, positive turning right).
    pub curvature: f32,
}

impl Pose2 {
    /// Unit forward in (x, z).
    pub fn forward(&self) -> Vec2 {
        Vec2::new(self.heading.sin(), -self.heading.cos())
    }

    /// Unit right in (x, z).
    pub fn right(&self) -> Vec2 {
        let f = self.forward();
        Vec2::new(-f.y, f.x)
    }
}

impl Path {
    pub fn new(start: Vec2, heading: f32) -> Self {
        Self {
            start,
            heading,
            elevation: 0.0,
            segments: Vec::new(),
        }
    }

    pub fn straight(mut self, length: f32) -> Self {
        self.segments.push(Segment::Straight { length });
        self
    }

    pub fn arc(mut self, radius: f32, angle_deg: f32) -> Self {
        self.segments.push(Segment::Arc {
            radius,
            angle: angle_deg.to_radians(),
        });
        self
    }

    pub fn length(&self) -> f32 {
        self.segments.iter().map(Segment::length).sum()
    }

    /// Pose at arc length `s` (clamped to the path).
    pub fn pose(&self, s: f32) -> Pose2 {
        let mut p = Pose2 {
            pos: self.start,
            heading: self.heading,
            curvature: 0.0,
        };
        let mut rest = s.clamp(0.0, self.length());
        for seg in &self.segments {
            let len = seg.length();
            let t = rest.min(len);
            match *seg {
                Segment::Straight { .. } => {
                    p.pos += p.forward() * t;
                    p.curvature = 0.0;
                }
                Segment::Arc { radius, angle } => {
                    let sign = angle.signum();
                    let centre = p.pos + p.right() * radius * sign;
                    let turn = t / radius * sign;
                    let rel = p.pos - centre;
                    // Turning right (clockwise seen from above, +heading) is a
                    // rotation by +turn in the (x, z) plane with z down-screen.
                    let (sn, cs) = turn.sin_cos();
                    p.pos = centre + Vec2::new(rel.x * cs - rel.y * sn, rel.x * sn + rel.y * cs);
                    p.heading += turn;
                    p.curvature = sign / radius;
                }
            }
            rest -= t;
            if rest <= 0.0 && t < len {
                break;
            }
        }
        p
    }

    /// Breakpoints between segments (arc lengths), including 0 and the end.
    pub fn breakpoints(&self) -> Vec<f32> {
        let mut out = vec![0.0];
        let mut s = 0.0;
        for seg in &self.segments {
            s += seg.length();
            out.push(s);
        }
        out
    }

    /// Arc lengths from `s0` to `s1` with spacing at most `max_step` on arcs
    /// (straights only need their ends), always including segment ends.
    pub fn stations(&self, s0: f32, s1: f32, max_step: f32) -> Vec<f32> {
        let mut out = vec![s0];
        let mut seg_start = 0.0;
        for seg in &self.segments {
            let seg_end = seg_start + seg.length();
            let (a, b) = (seg_start.max(s0), seg_end.min(s1));
            if b > a {
                let step = match seg {
                    Segment::Straight { .. } => max_step * 4.0,
                    Segment::Arc { .. } => max_step,
                };
                let n = ((b - a) / step).ceil().max(1.0) as usize;
                for k in 1..=n {
                    out.push(a + (b - a) * k as f32 / n as f32);
                }
            }
            seg_start = seg_end;
        }
        out.dedup_by(|a, b| (*a - *b).abs() < 1e-4);
        out
    }

    /// Frames offset `offset` metres to the right (negative: left) at the
    /// given stations; `s` of each frame is the arc length along the offset
    /// curve from the first station.
    pub fn frames(&self, stations: &[f32], offset: f32) -> Vec<Frame> {
        let mut out: Vec<Frame> = Vec::with_capacity(stations.len());
        for &s in stations {
            let p = self.pose(s);
            let pos2 = p.pos + p.right() * offset;
            let fwd = p.forward();
            let right = p.right();
            let arc = match out.last() {
                Some(prev) => {
                    prev.s + (Vec3::new(pos2.x, 0.0, pos2.y) - prev.pos.with_y(0.0)).length()
                }
                None => 0.0,
            };
            out.push(Frame {
                pos: Vec3::new(pos2.x, self.elevation, pos2.y),
                forward: Vec3::new(fwd.x, 0.0, fwd.y),
                right: Vec3::new(right.x, 0.0, right.y),
                up: Vec3::Y,
                s: arc,
            });
        }
        out
    }

    /// Like [`Path::frames`] but with each frame's `s` set to the centreline
    /// arc length (so markings line up across the cross-section).
    pub fn frames_centre_s(&self, stations: &[f32], offset: f32) -> Vec<Frame> {
        let mut f = self.frames(stations, offset);
        for (frame, s) in f.iter_mut().zip(stations) {
            frame.s = *s;
        }
        f
    }

    /// World position at arc length `s`, `offset` metres right, `height` up.
    pub fn point(&self, s: f32, offset: f32, height: f32) -> Vec3 {
        let p = self.pose(s);
        let q = p.pos + p.right() * offset;
        Vec3::new(q.x, self.elevation + height, q.y)
    }

    /// Distance along the offset curve between centreline stations `s0..s1`.
    pub fn offset_length(&self, s0: f32, s1: f32, offset: f32) -> f32 {
        let st = self.stations(s0, s1, 0.5);
        let f = self.frames(&st, offset);
        f.last().map_or(0.0, |l| l.s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_angle_corner_ends_where_expected() {
        let p = Path::new(Vec2::ZERO, 0.0)
            .straight(100.0)
            .arc(20.0, 90.0)
            .straight(50.0);
        let end = p.pose(p.length());
        // 100 m north (−z), turn right with radius 20, 50 m east (+x).
        assert!(
            (end.pos - Vec2::new(70.0, -120.0)).length() < 1e-3,
            "{:?}",
            end.pos
        );
        assert!((end.heading - std::f32::consts::FRAC_PI_2).abs() < 1e-5);
        assert!((end.forward() - Vec2::X).length() < 1e-5);
    }

    #[test]
    fn offset_arc_lengths_scale_with_radius() {
        let p = Path::new(Vec2::ZERO, 0.0).arc(20.0, 90.0);
        let inner = p.offset_length(0.0, p.length(), 5.0);
        let outer = p.offset_length(0.0, p.length(), -5.0);
        let q = std::f32::consts::FRAC_PI_2;
        assert!((inner - 15.0 * q).abs() < 0.05, "{inner}");
        assert!((outer - 25.0 * q).abs() < 0.05, "{outer}");
    }
}
