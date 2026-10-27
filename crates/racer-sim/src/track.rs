//! Greybox driving areas: the M1 test track (long straight, hairpin,
//! chicane with curbs, fast sweeper, crest on the back straight) with the
//! skidpad, and city tracks compiled by `racer-trackc`. Roads, aprons, curbs
//! and walls are triangle meshes (no seams for the chassis to catch on); the
//! test track's curbs and pad are boxes, and the ground collides as a
//! half-space. Rendering adds meshes for each [`TrackPiece`].

use std::sync::Arc;

use avian3d::prelude::*;
use bevy::prelude::*;
use racer_dynamics::Surface;
pub use racer_trackc::geometry::TriMesh;
use racer_trackc::geometry::{GROUND_Y, Geometry};

/// Radius of the skidpad circle (the magazines' 300-ft pad).
pub const SKIDPAD_RADIUS: f32 = 45.72;
/// Centre of the skidpad, beside the track.
pub const SKIDPAD_CENTER: Vec3 = Vec3::new(-150.0, 0.0, -130.0);
pub const ROAD_WIDTH: f32 = 12.0;
/// Road surface height above the surrounding grass.
const ROAD_TOP: f32 = 0.02;
const PAD_THICKNESS: f32 = 1.0;
/// Crest on the back straight: rises `CREST_RISE` over `CREST_RUN`, then falls.
const CREST_RISE: f32 = 0.9;
const CREST_RUN: f32 = 12.0;

/// What a piece is, for rendering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PieceKind {
    Grass,
    Road,
    /// Flat run-off between the road and the walls.
    Apron,
    /// Curb segment; `true` for the red ones.
    Curb(bool),
    Wall,
    /// Painted line, visual only.
    Marking,
    /// Greybox building, visual only.
    Building,
}

#[derive(Clone, Debug)]
pub enum Shape {
    /// Box of this full size, centred on the piece's transform.
    Box(Vec3),
    /// Triangle mesh in the piece's frame, counter-clockwise seen from above.
    Mesh(Arc<TriMesh>),
}

/// One piece of the track.
#[derive(Component, Clone, Debug)]
pub struct TrackPiece {
    pub kind: PieceKind,
    pub shape: Shape,
}

/// Collision layers: the ground, walls and cars. Wheels cast against the
/// ground only (a wheel never stands on a wall top); car bodies collide with
/// everything, other cars only unless ghosted.
#[derive(PhysicsLayer, Clone, Copy, Debug, Default)]
pub enum Layer {
    #[default]
    World,
    Car,
    Wall,
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
    (
        TrackPiece {
            kind,
            shape: Shape::Box(size),
        },
        transform,
    )
}

/// Road surface strip along a closed centreline, with heights per point.
fn road_mesh(points: &[Vec3]) -> TriMesh {
    let n = points.len();
    let mut vertices = Vec::with_capacity(2 * n);
    for i in 0..n {
        let tangent = (points[(i + 1) % n] - points[(i + n - 1) % n])
            .with_y(0.0)
            .normalize();
        let right = tangent.cross(Vec3::Y) * (ROAD_WIDTH / 2.0);
        vertices.push(points[i] - right);
        vertices.push(points[i] + right);
    }
    let indices = (0..n as u32)
        .flat_map(|i| {
            let j = (i + 1) % n as u32;
            let (l0, r0, l1, r1) = (2 * i, 2 * i + 1, 2 * j, 2 * j + 1);
            [[l0, r0, l1], [r0, r1, l1]]
        })
        .collect();
    TriMesh { vertices, indices }
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
    let back_straight = t.pos.x - r2;
    t.straight(60.0);
    let (crest_at, crest_dir) = (t.pos, t.dir());
    t.straight(CREST_RUN);
    t.straight(CREST_RUN);
    t.straight(back_straight - 60.0 - 2.0 * CREST_RUN);
    t.arc(90.0, r2, false);

    // The turtle ends where it started; drop the duplicate closing point.
    let pts = &t.points[..t.points.len() - 1];
    let crest = |p: Vec2| {
        let along = (p - crest_at).dot(crest_dir);
        let across = (p - crest_at).perp_dot(crest_dir).abs();
        if across < 1.0 && (0.0..=2.0 * CREST_RUN).contains(&along) {
            CREST_RISE * (1.0 - (along - CREST_RUN).abs() / CREST_RUN)
        } else {
            0.0
        }
    };
    let centerline: Vec<Vec3> = pts
        .iter()
        .map(|&(p, _)| Vec3::new(p.x, ROAD_TOP + crest(p), p.y))
        .collect();
    let to3 = |p: Vec2| Vec3::new(p.x, ROAD_TOP, p.y);
    let mut pieces = vec![
        piece(
            PieceKind::Grass,
            Vec3::new(4000.0, 1.0, 4000.0),
            Transform::from_xyz(0.0, -0.5, 0.0),
        ),
        (
            TrackPiece {
                kind: PieceKind::Road,
                shape: Shape::Mesh(Arc::new(road_mesh(&centerline))),
            },
            Transform::IDENTITY,
        ),
    ];
    // Curbs line both edges of flagged segments, following the road edges.
    let n = pts.len();
    let edge = |i: usize, side: f32| {
        let (prev, next) = (pts[(i + n - 1) % n].0, pts[(i + 1) % n].0);
        let tangent = (next - prev).normalize();
        to3(pts[i].0 + Vec2::new(-tangent.y, tangent.x) * side * (ROAD_WIDTH / 2.0 - 0.45))
    };
    for (i, _) in pts.iter().enumerate().filter(|(_, (_, curbs))| *curbs) {
        for side in [-1.0, 1.0] {
            let (a, b) = (edge((i + n - 1) % n, side), edge(i, side));
            let d = b - a;
            pieces.push(piece(
                PieceKind::Curb(i % 2 == 0),
                Vec3::new(0.9, 0.06, d.length()),
                Transform::from_translation((a + b) / 2.0 + Vec3::Y * 0.03)
                    .with_rotation(Quat::from_rotation_y((-d.x).atan2(-d.z))),
            ));
        }
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
        Vec3::new(130.0, PAD_THICKNESS, 130.0),
        Transform::from_translation(SKIDPAD_CENTER + Vec3::Y * (ROAD_TOP - PAD_THICKNESS / 2.0)),
    ));
    let n = 96;
    for i in 0..n {
        let a = std::f32::consts::TAU * i as f32 / n as f32;
        let p = SKIDPAD_CENTER + Vec3::new(a.cos(), 0.0, a.sin()) * SKIDPAD_RADIUS;
        let chord = std::f32::consts::TAU * SKIDPAD_RADIUS / n as f32;
        pieces.push(piece(
            PieceKind::Marking,
            Vec3::new(0.2, 0.01, chord * 0.6),
            Transform::from_translation(p + Vec3::Y * (ROAD_TOP + 0.005))
                .with_rotation(Quat::from_rotation_y(-a)),
        ));
    }

    Track {
        pieces,
        centerline,
        start: Transform::from_xyz(0.0, ROAD_TOP, 0.0),
    }
}

/// Start pose on the skidpad circle, driving anticlockwise seen from above (left turn).
pub fn skidpad_start() -> Transform {
    Transform::from_translation(SKIDPAD_CENTER + Vec3::new(SKIDPAD_RADIUS, ROAD_TOP, 0.0))
}

/// A city track's pieces: ground, road, aprons, curbs, walls, markings and buildings.
pub fn city_track(geometry: &Geometry) -> Track {
    let mesh = |kind, mesh: &TriMesh| {
        (
            TrackPiece {
                kind,
                shape: Shape::Mesh(Arc::new(mesh.clone())),
            },
            Transform::IDENTITY,
        )
    };
    let (lo, hi) = geometry.bounds;
    let center = (lo + hi) / 2.0;
    let size = hi - lo + Vec2::splat(800.0);
    let mut pieces = vec![
        piece(
            PieceKind::Grass,
            Vec3::new(size.x, 1.0, size.y),
            Transform::from_xyz(center.x, GROUND_Y - 0.5, center.y),
        ),
        mesh(PieceKind::Road, &geometry.road),
        mesh(PieceKind::Apron, &geometry.aprons),
        mesh(PieceKind::Curb(true), &geometry.curbs[0]),
        mesh(PieceKind::Curb(false), &geometry.curbs[1]),
        mesh(PieceKind::Wall, &geometry.walls),
        mesh(PieceKind::Marking, &geometry.markings),
    ];
    for b in &geometry.buildings {
        pieces.push(piece(
            PieceKind::Building,
            b.size,
            Transform::from_translation(b.base + Vec3::Y * b.size.y / 2.0)
                .with_rotation(Quat::from_rotation_y(b.yaw)),
        ));
    }
    let start = geometry.grid[0];
    Track {
        pieces,
        centerline: geometry.samples.iter().map(|s| s.pos).collect(),
        start: Transform::from_translation(start.position).with_rotation(start.rotation),
    }
}

/// Spawns every piece with its collider (markings and buildings have none).
pub fn spawn_track(commands: &mut Commands, track: &Track) {
    for (piece, transform) in &track.pieces {
        let (surface, friction, layer) = match piece.kind {
            PieceKind::Grass => (Surface::Grass, 0.6, Layer::World),
            PieceKind::Road => (Surface::DryAsphalt, 0.6, Layer::World),
            PieceKind::Apron => (Surface::Cobbles, 0.6, Layer::World),
            PieceKind::Curb(_) => (Surface::Curb, 0.6, Layer::World),
            // Low friction so a car scraping a wall slides along it.
            PieceKind::Wall => (Surface::DryAsphalt, 0.2, Layer::Wall),
            PieceKind::Marking | PieceKind::Building => {
                commands.spawn((piece.clone(), *transform));
                continue;
            }
        };
        let physics = (
            GroundSurface(surface),
            Friction::new(friction),
            CollisionLayers::new(layer, LayerMask::ALL),
        );
        let mut entity = commands.spawn((piece.clone(), *transform, RigidBody::Static));
        match &piece.shape {
            // The ground box is kilometres wide, too big for GJK in f32: its
            // support points lie far beyond the cast's absolute tolerance,
            // and about a quarter of the wheel casts miss. The ground is
            // unbounded for driving anyway, so it collides as the half-space
            // under the box's top face, which parry solves exactly.
            Shape::Box(size) if piece.kind == PieceKind::Grass => {
                entity.with_child((
                    Collider::half_space(Vec3::Y),
                    Transform::from_xyz(0.0, size.y / 2.0, 0.0),
                    physics,
                ));
            }
            Shape::Box(size) => {
                entity.insert((Collider::cuboid(size.x, size.y, size.z), physics));
            }
            Shape::Mesh(mesh) => {
                entity.insert((
                    Collider::trimesh_with_config(
                        mesh.vertices.clone(),
                        mesh.indices.clone(),
                        TrimeshFlags::FIX_INTERNAL_EDGES,
                    ),
                    physics,
                ));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centerline_is_a_closed_loop() {
        let t = test_track();
        let (first, last) = (t.centerline[0], *t.centerline.last().unwrap());
        assert!(first.distance(last) < 2.5, "gap {}", first.distance(last));
        let top = t.centerline.iter().map(|p| p.y).fold(0.0, f32::max);
        assert!((top - ROAD_TOP - CREST_RISE).abs() < 1e-3, "crest {top}");
        let length: f32 = t.centerline.windows(2).map(|w| w[0].distance(w[1])).sum();
        assert!((1000.0..1500.0).contains(&length), "length {length}");
        assert!(t.centerline.windows(2).all(|w| w[0].distance(w[1]) < 2.5));
    }
}
