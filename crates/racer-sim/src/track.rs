//! Greybox driving areas: a closed test track (long straight with a crest,
//! hairpin, chicane with curbs, fast sweeper) and a skidpad, built from
//! boxes with colliders. Rendering adds meshes for each [`TrackPiece`].

use avian3d::prelude::*;
use bevy::prelude::*;
use racer_dynamics::Surface;

/// Radius of the skidpad circle (the magazines' 300-ft pad).
pub const SKIDPAD_RADIUS: f32 = 45.72;
/// Centre of the skidpad, beside the track.
pub const SKIDPAD_CENTER: Vec3 = Vec3::new(-150.0, 0.0, -130.0);
pub const ROAD_WIDTH: f32 = 12.0;
/// Road surface height above the surrounding grass.
const ROAD_TOP: f32 = 0.02;
const ROAD_THICKNESS: f32 = 0.3;

/// What a piece is, for rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Grass,
    Road,
    /// Curb segment; `true` for the red ones.
    Curb(bool),
    /// Painted line, visual only.
    Marking,
}

/// A box of the track: centre transform and full size.
#[derive(Component, Clone, Copy, Debug)]
pub struct TrackPiece {
    pub kind: PieceKind,
    pub size: Vec3,
}

/// Grip of a collider's surface for the wheel contacts.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct GroundSurface(pub Surface);

#[derive(Clone, Debug)]
pub struct Track {
    pub pieces: Vec<(TrackPiece, Transform)>,
    /// Closed racing line through the middle of the road, about 2 m spacing.
    pub centerline: Vec<Vec3>,
    pub start: Transform,
}

/// Turtle that walks the centreline: heading 0 faces -Z, positive turns right.
struct Turtle {
    pos: Vec2,
    heading: f32,
    points: Vec<(Vec2, bool)>,
}

impl Turtle {
    fn dir(&self) -> Vec2 {
        Vec2::new(self.heading.sin(), -self.heading.cos())
    }

    fn straight(&mut self, length: f32) {
        let n = (length / 2.0).ceil().max(1.0) as usize;
        for _ in 0..n {
            self.pos += self.dir() * (length / n as f32);
            self.points.push((self.pos, false));
        }
    }

    /// Arc of `degrees` (positive = right) and `radius`; `curbs` lines both edges.
    fn arc(&mut self, degrees: f32, radius: f32, curbs: bool) {
        let angle = degrees.to_radians();
        let n = (angle.abs() * radius / 2.0).ceil().max(1.0) as usize;
        let right = Vec2::new(-self.dir().y, self.dir().x) * angle.signum();
        let center = self.pos + right * radius;
        let start = self.pos - center;
        for i in 1..=n {
            let a = angle * i as f32 / n as f32;
            self.pos = center + Vec2::from_angle(a).rotate(start);
            self.points.push((self.pos, curbs));
        }
        self.heading += angle;
    }
}

fn piece(kind: PieceKind, size: Vec3, transform: Transform) -> (TrackPiece, Transform) {
    (TrackPiece { kind, size }, transform)
}

/// The test track and the skidpad in one world.
pub fn test_track() -> Track {
    let mut t = Turtle {
        pos: Vec2::ZERO,
        heading: 0.0,
        points: vec![(Vec2::ZERO, false)],
    };
    t.straight(260.0);
    t.arc(180.0, 15.0, true); // hairpin
    t.straight(40.0);
    t.arc(-90.0, 30.0, false);
    t.straight(30.0);
    t.arc(-20.0, 40.0, true); // chicane
    t.arc(40.0, 40.0, true);
    t.arc(-20.0, 40.0, true);
    t.straight(40.0);
    t.arc(90.0, 80.0, false); // fast sweeper
    // Close the loop: south, right to west, west, right to north at the start.
    let (r1, r2) = (30.0, 25.0);
    t.straight(-t.pos.y - r1 + r2);
    t.arc(90.0, r1, false);
    t.straight(t.pos.x - r2);
    t.arc(90.0, r2, false);

    let to3 = |p: Vec2| Vec3::new(p.x, ROAD_TOP, p.y);
    let mut pieces = vec![piece(
        PieceKind::Grass,
        Vec3::new(4000.0, 1.0, 4000.0),
        Transform::from_xyz(0.0, -0.5, 0.0),
    )];
    let pts = &t.points;
    let mut curb_red = false;
    for w in pts.windows(2) {
        let ((a, curbs), (b, _)) = (w[0], w[1]);
        let d = b - a;
        let len = d.length();
        if len < 1e-3 {
            continue;
        }
        let rot = Quat::from_rotation_y((-d.x).atan2(-d.y));
        let mid = to3((a + b) / 2.0);
        pieces.push(piece(
            PieceKind::Road,
            Vec3::new(ROAD_WIDTH, ROAD_THICKNESS, len + 0.6),
            Transform::from_translation(mid - Vec3::Y * ROAD_THICKNESS / 2.0).with_rotation(rot),
        ));
        if curbs {
            curb_red = !curb_red;
            for side in [-1.0, 1.0] {
                let offset = rot * Vec3::X * side * (ROAD_WIDTH / 2.0 - 0.45);
                pieces.push(piece(
                    PieceKind::Curb(curb_red),
                    Vec3::new(0.9, 0.06, len + 0.05),
                    Transform::from_translation(mid + offset + Vec3::Y * 0.03).with_rotation(rot),
                ));
            }
        }
    }
    // Crest on the main straight: 12 m up to 0.9 m, 12 m down.
    let (rise, run) = (0.9f32, 12.0f32);
    let slope = (rise / run).atan();
    let ramp_len = rise.hypot(run);
    for (z, sign) in [(-120.0 - run / 2.0, 1.0f32), (-120.0 - run * 1.5, -1.0)] {
        pieces.push(piece(
            PieceKind::Road,
            Vec3::new(ROAD_WIDTH, ROAD_THICKNESS, ramp_len),
            Transform::from_xyz(0.0, ROAD_TOP + rise / 2.0 - ROAD_THICKNESS / 2.0 / slope.cos(), z)
                .with_rotation(Quat::from_rotation_x(sign * slope)),
        ));
    }
    // Start/finish line.
    pieces.push(piece(
        PieceKind::Marking,
        Vec3::new(ROAD_WIDTH, 0.01, 0.5),
        Transform::from_xyz(0.0, ROAD_TOP + 0.005, -5.0),
    ));

    // Skidpad: square asphalt pad with the circle painted on it.
    pieces.push(piece(
        PieceKind::Road,
        Vec3::new(130.0, ROAD_THICKNESS, 130.0),
        Transform::from_translation(SKIDPAD_CENTER + Vec3::Y * (ROAD_TOP - ROAD_THICKNESS / 2.0)),
    ));
    let n = 96;
    for i in 0..n {
        let a = std::f32::consts::TAU * i as f32 / n as f32;
        let p = SKIDPAD_CENTER + Vec3::new(a.cos(), 0.0, a.sin()) * SKIDPAD_RADIUS;
        let chord = std::f32::consts::TAU * SKIDPAD_RADIUS / n as f32;
        pieces.push(piece(
            PieceKind::Marking,
            Vec3::new(0.2, 0.01, chord * 0.6),
            Transform::from_translation(p + Vec3::Y * (ROAD_TOP + 0.005)).with_rotation(Quat::from_rotation_y(-a)),
        ));
    }

    Track {
        pieces,
        centerline: pts.iter().map(|&(p, _)| to3(p)).collect(),
        start: Transform::from_xyz(0.0, ROAD_TOP, 0.0),
    }
}

/// Start pose on the skidpad circle, driving anticlockwise seen from above (left turn).
pub fn skidpad_start() -> Transform {
    Transform::from_translation(SKIDPAD_CENTER + Vec3::new(SKIDPAD_RADIUS, ROAD_TOP, 0.0))
}

/// Spawns every piece with its collider (markings have none).
pub fn spawn_track(commands: &mut Commands, track: &Track) {
    for &(piece, transform) in &track.pieces {
        let mut e = commands.spawn((piece, transform, RigidBody::Static));
        let surface = match piece.kind {
            PieceKind::Grass => Surface::Grass,
            PieceKind::Road => Surface::DryAsphalt,
            PieceKind::Curb(_) => Surface::Curb,
            PieceKind::Marking => continue,
        };
        e.insert((
            Collider::cuboid(piece.size.x, piece.size.y, piece.size.z),
            GroundSurface(surface),
            Friction::new(0.6),
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centerline_is_a_closed_loop() {
        let t = test_track();
        let (first, last) = (t.centerline[0], *t.centerline.last().unwrap());
        assert!(first.distance(last) < 0.5, "gap {}", first.distance(last));
        let length: f32 = t.centerline.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!((1000.0..1500.0).contains(&length), "length {length}");
        assert!(t.centerline.windows(2).all(|w| w[0].distance(w[1]) < 2.5));
    }
}
