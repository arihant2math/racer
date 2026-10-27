//! Prop generators: racing furniture ([`racing`]) and street furniture
//! ([`street`]), plus shared shape helpers.
//!
//! Prop space: origin on the ground at the prop's footprint centre, +Y up,
//! local −Z facing the road (lamp arms and signal mast arms reach along −Z).
//! Linear furniture (barriers, fences, curbs) is generated directly along a
//! [`Path`](crate::path::Path) in world space.

pub mod racing;
pub mod street;

use bevy::math::{Affine3A, Quat, Vec2, Vec3};

use crate::{
    mesh::{Frame, MeshData},
    path::Path,
};

/// Radial segments for a level of detail.
pub fn segments(lod: usize, lod0: u32) -> u32 {
    match lod {
        0 => lod0,
        1 => (lod0 / 2).max(5),
        _ => (lod0 / 4).max(4),
    }
}

/// Box from `min` to `max` (alias for readability in the generators).
pub fn bx(min: Vec3, max: Vec3) -> MeshData {
    MeshData::cuboid(min, max)
}

/// Box of `size` centred at `c`.
pub fn bxc(c: Vec3, size: Vec3) -> MeshData {
    MeshData::cuboid(c - size / 2.0, c + size / 2.0)
}

/// Extrudes a counter-clockwise profile in the (z, y) plane along X from
/// `-w/2` to `w/2`, capped (watertight when the profile is star-shaped).
pub fn extrude_x(profile: &[Vec2], w: f32) -> MeshData {
    let frames: Vec<Frame> = [-w / 2.0, w / 2.0]
        .iter()
        .map(|x| Frame {
            pos: Vec3::X * *x,
            forward: Vec3::X,
            right: Vec3::Z,
            up: Vec3::Y,
            s: *x + w / 2.0,
        })
        .collect();
    MeshData::sweep(&frames, profile, true, true)
}

/// Frames along the path between `s0` and `s1` at a lateral offset, spaced
/// at most `step` metres (straights get only their ends unless `dense`).
pub fn run_frames(path: &Path, s0: f32, s1: f32, offset: f32, step: f32) -> Vec<Frame> {
    let n = ((s1 - s0) / step).ceil().max(1.0) as usize;
    let st: Vec<f32> = (0..=n).map(|k| s0 + (s1 - s0) * k as f32 / n as f32).collect();
    path.frames(&st, offset)
}

/// Thin disc of radius `r` facing +Z with UVs mapped into `rect` (atlas).
pub fn disc_z(r: f32, thickness: f32, seg: u32, rect: (Vec2, Vec2), back: bool) -> MeshData {
    let mut m = MeshData::new();
    let (uv0, uv1) = rect;
    let uv = |x: f32, y: f32| {
        Vec2::new(
            uv0.x + (uv1.x - uv0.x) * (0.5 + 0.5 * x / r),
            uv0.y + (uv1.y - uv0.y) * (0.5 - 0.5 * y / r),
        )
    };
    let c = m.vertex(Vec3::Z * thickness, Vec3::Z, uv(0.0, 0.0));
    let first = m.positions.len() as u32;
    for k in 0..seg {
        let a = k as f32 / seg as f32 * std::f32::consts::TAU;
        let (y, x) = a.sin_cos();
        m.vertex(Vec3::new(x * r, y * r, thickness), Vec3::Z, uv(x * r, y * r));
    }
    for k in 0..seg {
        m.tri(c, first + k, first + (k + 1) % seg);
    }
    if back {
        let rim = MeshData::frustum(r, r, thickness, seg, false)
            .transformed(Affine3A::from_rotation_x(std::f32::consts::FRAC_PI_2));
        m.append(&rim);
    }
    m
}

/// Quad facing +Z spanning `min..max` in (x, y) at z, UVs into `rect`.
pub fn panel_z(min: Vec2, max: Vec2, z: f32, rect: (Vec2, Vec2)) -> MeshData {
    let mut m = MeshData::new();
    let (a, b) = rect;
    m.quad_n(
        [
            Vec3::new(min.x, min.y, z),
            Vec3::new(max.x, min.y, z),
            Vec3::new(max.x, max.y, z),
            Vec3::new(min.x, max.y, z),
        ],
        [Vec2::new(a.x, b.y), Vec2::new(b.x, b.y), Vec2::new(b.x, a.y), Vec2::new(a.x, a.y)],
        Vec3::Z,
    );
    m
}

/// Seven-segment glyph boxes for `text` (digits and a few letters), each
/// character `h` tall, laid out from `origin` along +X on the plane z = 0
/// (boxes `depth` thick towards +Z).
pub fn seven_segment(text: &str, origin: Vec3, h: f32, depth: f32) -> MeshData {
    // Segments a..g as (x0, y0, x1, y1) in a 0.6 × 1.0 cell.
    const SEG: [(f32, f32, f32, f32); 7] = [
        (0.08, 0.9, 0.52, 1.0),
        (0.5, 0.52, 0.6, 0.95),
        (0.5, 0.05, 0.6, 0.48),
        (0.08, 0.0, 0.52, 0.1),
        (0.0, 0.05, 0.1, 0.48),
        (0.0, 0.52, 0.1, 0.95),
        (0.08, 0.45, 0.52, 0.55),
    ];
    let mut m = MeshData::new();
    for (i, ch) in text.chars().enumerate() {
        let mask: u8 = match ch.to_ascii_uppercase() {
            '0' => 0b0111111,
            '1' => 0b0000110,
            '2' => 0b1011011,
            '3' => 0b1001111,
            '4' => 0b1100110,
            '5' | 'S' => 0b1101101,
            '6' => 0b1111101,
            '7' => 0b0000111,
            '8' => 0b1111111,
            '9' => 0b1101111,
            'P' => 0b1110011,
            'F' => 0b1110001,
            'E' => 0b1111001,
            'L' => 0b0111000,
            'A' => 0b1110111,
            'T' => 0b1111000,
            '-' => 0b1000000,
            _ => 0,
        };
        let x0 = origin.x + i as f32 * 0.8 * h;
        for (k, (a, b, c, d)) in SEG.iter().enumerate() {
            if mask & (1 << k) != 0 {
                m.append(&bx(
                    Vec3::new(x0 + a * h, origin.y + b * h, origin.z),
                    Vec3::new(x0 + c * h, origin.y + d * h, origin.z + depth),
                ));
            }
        }
    }
    m
}

/// Width of [`seven_segment`] text.
pub fn seven_segment_width(text: &str, h: f32) -> f32 {
    (text.chars().count() as f32 * 0.8 - 0.2) * h
}

/// Square box truss from `a` to `b` (`w` wide) with four chords and zigzag
/// diagonals on each face (diagonals only when `seg` > 4, i.e. LOD0).
pub fn truss(a: Vec3, b: Vec3, w: f32, chord_r: f32, seg: u32) -> MeshData {
    let mut m = MeshData::new();
    let d = b - a;
    let len = d.length();
    let dir = d / len;
    let side = if dir.y.abs() > 0.9 { Vec3::X } else { dir.cross(Vec3::Y).normalize() };
    let up = side.cross(dir).normalize();
    let corners = [
        (side + up) * w / 2.0,
        (side - up) * w / 2.0,
        (-side - up) * w / 2.0,
        (-side + up) * w / 2.0,
    ];
    for c in corners {
        m.append(&MeshData::tube(a + c, b + c, chord_r, seg, true));
    }
    let n = if seg > 4 { (len / w).round().max(1.0) as usize } else { 0 };
    for k in 0..n {
        let (p0, p1) = (a + dir * (len * k as f32 / n as f32), a + dir * (len * (k + 1) as f32 / n as f32));
        for f in 0..4 {
            let (c0, c1) = (corners[f], corners[(f + 1) % 4]);
            let (from, to) = if k % 2 == 0 { (p0 + c0, p1 + c1) } else { (p0 + c1, p1 + c0) };
            m.append(&MeshData::tube(from, to, chord_r * 0.5, seg.min(5), false));
        }
    }
    m
}

/// Rotation that turns prop space (−Z towards the road) to face `dir`.
pub fn facing(dir: Vec3) -> Quat {
    let d = Vec3::new(dir.x, 0.0, dir.z).normalize_or(Vec3::NEG_Z);
    Quat::from_rotation_arc(Vec3::NEG_Z, d)
}
