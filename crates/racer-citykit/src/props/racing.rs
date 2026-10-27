//! Racing furniture: curbs, concrete (jersey) barriers, Armco, tyre walls,
//! catch fencing, start/finish gantry, sector and braking boards and a
//! temporary grandstand.

use bevy::math::{Affine3A, Quat, Vec2, Vec3};

use super::{bx, bxc, run_frames, segments, seven_segment, seven_segment_width, truss};
use crate::{
    kit::{Parts, Slot, Socket, SocketKind},
    mesh::MeshData,
    path::Path,
};

/// New Jersey barrier profile (0.81 m tall, 0.61 m base, 0.15 m top),
/// centred, counter-clockwise, 20 mm buried.
pub const JERSEY: [Vec2; 8] = [
    Vec2::new(0.305, -0.02),
    Vec2::new(0.305, 0.075),
    Vec2::new(0.18, 0.33),
    Vec2::new(0.075, 0.81),
    Vec2::new(-0.075, 0.81),
    Vec2::new(-0.18, 0.33),
    Vec2::new(-0.305, 0.075),
    Vec2::new(-0.305, -0.02),
];

/// Jersey barrier units (3.8 m long, 20 mm joints) along the path at
/// `offset`, standing at height `y0` above the path.
pub fn jersey_run(path: &Path, s0: f32, s1: f32, offset: f32, y0: f32) -> Parts {
    let mut parts = Parts::new();
    let unit = 3.8;
    let n = ((s1 - s0) / unit).round().max(1.0) as usize;
    let len = (s1 - s0) / n as f32;
    let profile: Vec<Vec2> = JERSEY.iter().map(|p| *p + Vec2::Y * y0).collect();
    for k in 0..n {
        let a = s0 + k as f32 * len + 0.01;
        let b = a + len - 0.02;
        let frames = run_frames(path, a, b, offset, 1.0);
        parts.add_closed(Slot::Concrete, MeshData::sweep(&frames, &profile, true, true));
    }
    parts
}

/// Racing curb (rumble strip) on the road surface: 1.0 m wide, rising from
/// flush at the track edge to 50 mm, painted red/white in 1 m blocks.
/// `side` is +1 when the curb lies to the right of `offset` (outward).
pub fn curb_run(path: &Path, s0: f32, s1: f32, offset: f32, side: f32, y0: f32) -> Parts {
    let w = 1.0;
    let raw = [
        Vec2::new(0.0, -0.05),
        Vec2::new(w, -0.05),
        Vec2::new(w, 0.05),
        Vec2::new(0.25, 0.05),
        Vec2::new(0.0, 0.005),
    ];
    let profile: Vec<Vec2> = if side > 0.0 {
        raw.iter().map(|p| *p + Vec2::Y * y0).collect()
    } else {
        raw.iter().rev().map(|p| Vec2::new(-p.x, p.y + y0)).collect()
    };
    let frames = run_frames(path, s0, s1, offset, 0.5);
    let mut parts = Parts::new();
    parts.add_closed(Slot::CurbPaint, MeshData::sweep(&frames, &profile, true, true));
    parts
}

/// Double W-beam guardrail (Armco) with posts every 2 m. The beams face the
/// road on the `facing` side (+1: road to the right of `offset`).
pub fn armco_run(path: &Path, s0: f32, s1: f32, offset: f32, facing: f32, y0: f32) -> Parts {
    // W-beam: 310 mm tall, 83 mm deep, 4 mm steel (closed thin profile).
    let front = [(0.02, 0.0), (0.083, 0.07), (0.0, 0.155), (0.083, 0.24), (0.02, 0.31)];
    let mut beam: Vec<Vec2> = front.iter().map(|(x, y)| Vec2::new(*x, *y)).collect();
    beam.extend(front.iter().rev().map(|(x, y)| Vec2::new(x - 0.006, *y)));
    let mut parts = Parts::new();
    let frames = run_frames(path, s0, s1, offset, 1.0);
    for base in [0.42, 0.78] {
        let prof: Vec<Vec2> = if facing > 0.0 {
            beam.iter().map(|p| Vec2::new(p.x, p.y + base + y0)).collect()
        } else {
            beam.iter().rev().map(|p| Vec2::new(-p.x, p.y + base + y0)).collect()
        };
        parts.add_closed(Slot::Galvanised, MeshData::sweep(&frames, &prof, true, true));
    }
    let n = ((s1 - s0) / 2.0).floor() as usize;
    for k in 0..=n {
        let s = (s0 + k as f32 * 2.0).min(s1);
        let p = path.pose(s);
        let pos = path.point(s, offset - facing * 0.2, y0);
        let rot = Quat::from_rotation_y(-p.heading);
        let post = bx(Vec3::new(-0.05, -0.1, -0.075), Vec3::new(0.05, 1.15, 0.075));
        parts.add_closed(Slot::Galvanised, post.transformed(Affine3A::from_rotation_translation(rot, pos)));
    }
    parts
}

/// Tyre wall: stacks of four tyres, two rows deep, behind a conveyor-belt
/// facing; `facing` +1 when the road is to the right of `offset`.
pub fn tirewall_run(path: &Path, s0: f32, s1: f32, offset: f32, facing: f32, y0: f32, lod: usize) -> Parts {
    let seg = segments(lod, 12);
    let mut stack = MeshData::new();
    for k in 0..4 {
        // Each tyre bulges: two frustums meeting at the tread centre.
        let y = k as f32 * 0.2;
        stack.append(&MeshData::frustum(0.29, 0.31, 0.1, seg, false).translated(Vec3::Y * y));
        stack.append(&MeshData::frustum(0.31, 0.29, 0.1, seg, false).translated(Vec3::Y * (y + 0.1)));
    }
    // Top: rim hole and sidewall.
    stack.append(&MeshData::frustum(0.29, 0.17, 0.0001, seg, false).translated(Vec3::Y * 0.8));
    stack.append(&MeshData::frustum(0.17, 0.17, 0.0001, seg, true).translated(Vec3::Y * 0.74));
    let mut parts = Parts::new();
    let pitch = 0.62;
    let n = ((s1 - s0) / pitch).floor() as usize;
    for k in 0..n {
        let s = s0 + (k as f32 + 0.5) * pitch;
        for row in 0..2 {
            let d = offset - facing * (0.33 + row as f32 * 0.6);
            let pos = path.point(s, d, y0);
            parts.add(Slot::Rubber, stack.translated(pos));
        }
    }
    // Belt over the front face, 0.85 m tall, in 3 m panels.
    let belt = [
        Vec2::new(0.0, y0),
        Vec2::new(0.015, y0),
        Vec2::new(0.015, y0 + 0.85),
        Vec2::new(0.0, y0 + 0.85),
    ];
    let belt: Vec<Vec2> = if facing > 0.0 {
        belt.to_vec()
    } else {
        belt.iter().rev().map(|p| Vec2::new(-p.x, p.y)).collect()
    };
    let frames = run_frames(path, s0, s1, offset, 1.0);
    parts.add_closed(Slot::Rubber, MeshData::sweep(&frames, &belt, true, true));
    parts
}

/// Catch fence: galvanised posts every 3.8 m, chain-link panels (two-sided
/// alpha material) and three tension cables, from `y0` up to `y0 + h`. The
/// top 0.6 m cranks towards the road (`facing`).
pub fn fence_run(path: &Path, s0: f32, s1: f32, offset: f32, facing: f32, y0: f32, h: f32, lod: usize) -> Parts {
    let mut parts = Parts::new();
    let seg = segments(lod, 8);
    let n = ((s1 - s0) / 3.8).round().max(1.0) as usize;
    let crank = 0.6;
    for k in 0..=n {
        let s = s0 + (s1 - s0) * k as f32 / n as f32;
        let a = path.point(s, offset, y0);
        let b = path.point(s, offset, y0 + h - crank);
        let c = path.point(s, offset + facing * 0.35, y0 + h);
        parts.add_closed(Slot::Galvanised, MeshData::tube(a, b, 0.045, seg, true));
        parts.add_closed(Slot::Galvanised, MeshData::tube(b, c, 0.045, seg, true));
    }
    // Mesh panels (fine sampling so the panel follows curves).
    let frames = run_frames(path, s0, s1, offset, 1.0);
    let mut panel = MeshData::new();
    for w in frames.windows(2) {
        let (f0, f1) = (w[0], w[1]);
        let p = |f: &crate::mesh::Frame, x: f32, y: f32| f.at(Vec2::new(x, y));
        let (lo, mid, top) = (y0 + 0.05, y0 + h - crank, y0 + h);
        panel.quad(
            [p(&f0, 0.0, lo), p(&f1, 0.0, lo), p(&f1, 0.0, mid), p(&f0, 0.0, mid)],
            [Vec2::new(f0.s, -lo), Vec2::new(f1.s, -lo), Vec2::new(f1.s, -mid), Vec2::new(f0.s, -mid)],
        );
        panel.quad(
            [p(&f0, 0.0, mid), p(&f1, 0.0, mid), p(&f1, facing * 0.35, top), p(&f0, facing * 0.35, top)],
            [Vec2::new(f0.s, -mid), Vec2::new(f1.s, -mid), Vec2::new(f1.s, -mid - crank), Vec2::new(f0.s, -mid - crank)],
        );
    }
    parts.add(Slot::Fence, panel);
    if lod == 0 {
        for y in [y0 + 0.1, y0 + (h - crank) * 0.5, y0 + h - crank] {
            let f = run_frames(path, s0, s1, offset + facing * 0.03, 2.0);
            for w in f.windows(2) {
                parts.add(Slot::Galvanised, MeshData::tube(w[0].pos + Vec3::Y * y, w[1].pos + Vec3::Y * y, 0.008, 4, false));
            }
        }
    }
    parts
}

/// Start/finish gantry spanning `span` metres (centred on the origin across
/// local X), 6.5 m clearance: two truss towers, a truss beam, a chequered
/// banner and a five-column start-light unit facing local +Z (towards the
/// grid behind it), plus sockets for the lights.
pub fn gantry(span: f32, lod: usize) -> (Parts, Vec<Socket>) {
    let seg = segments(lod, 8);
    let mut parts = Parts::new();
    let half = span / 2.0;
    let clear = 6.5;
    let w = 0.6;
    for x in [-half, half] {
        parts.add(Slot::Galvanised, truss(Vec3::new(x, 0.0, 0.0), Vec3::new(x, clear + 1.2, 0.0), w, 0.035, seg));
        parts.add_closed(Slot::Concrete, bxc(Vec3::new(x, 0.2, 0.0), Vec3::new(1.2, 0.4, 1.2)));
    }
    parts.add(
        Slot::Galvanised,
        truss(Vec3::new(-half, clear + 0.6, 0.0), Vec3::new(half, clear + 0.6, 0.0), w * 1.4, 0.04, seg),
    );
    // Chequered banner on both faces of the beam.
    let banner_h = 1.4;
    for z in [0.5, -0.5] {
        parts.add_closed(
            Slot::Chequer,
            bx(Vec3::new(-half + 0.5, clear, z - 0.03), Vec3::new(half - 0.5, clear + banner_h, z + 0.03)),
        );
    }
    // Start lights: housing with 5 columns of 2 lights below the beam.
    let lw = 3.4;
    parts.add_closed(Slot::BoardDark, bx(Vec3::new(-lw / 2.0, clear - 1.0, -0.2), Vec3::new(lw / 2.0, clear, 0.2)));
    let mut sockets = Vec::new();
    for c in 0..5 {
        let x = -lw / 2.0 + 0.34 + c as f32 * (lw - 0.68) / 4.0;
        for (r, y) in [clear - 0.3, clear - 0.7].iter().enumerate() {
            let lens = super::disc_z(0.13, 0.02, segments(lod, 12), (Vec2::ZERO, Vec2::ONE), false)
                .map_uvs(|_| Vec2::ZERO)
                .translated(Vec3::new(x, *y, 0.2));
            parts.add(if r == 0 { Slot::SignalRed } else { Slot::SignalOff }, lens);
            sockets.push(Socket {
                name: format!("start_light_{c}_{r}"),
                kind: SocketKind::Emissive,
                pos: Vec3::new(x, *y, 0.22),
                dir: Vec3::Z,
            });
        }
    }
    (parts, sockets)
}

/// Sector/checkpoint board: dark LED panel with `label` (e.g. "S1") on two
/// legs, standing on a barrier top (`y0`), facing local +Z.
pub fn sector_board(label: &str, y0: f32) -> Parts {
    let mut parts = Parts::new();
    let (w, h) = (2.4, 1.3);
    let base = y0 + 1.2;
    parts.add_closed(Slot::BoardDark, bx(Vec3::new(-w / 2.0, base, -0.08), Vec3::new(w / 2.0, base + h, 0.08)));
    for x in [-w / 2.0 + 0.2, w / 2.0 - 0.2] {
        parts.add_closed(Slot::Galvanised, bx(Vec3::new(x - 0.04, y0, -0.04), Vec3::new(x + 0.04, base, 0.04)));
    }
    let gh = 0.9;
    let tw = seven_segment_width(label, gh);
    parts.add_closed(
        Slot::Led,
        seven_segment(label, Vec3::new(-tw / 2.0, base + (h - gh) / 2.0, 0.08), gh, 0.015),
    );
    parts
}

/// Braking marker board ("150", "100", "50"): white panel with black
/// numerals on a post, facing local +Z.
pub fn distance_board(label: &str, y0: f32) -> Parts {
    let mut parts = Parts::new();
    let (w, h) = (1.0, 0.75);
    let base = y0 + 0.9;
    parts.add_closed(Slot::BoardWhite, bx(Vec3::new(-w / 2.0, base, -0.03), Vec3::new(w / 2.0, base + h, 0.03)));
    parts.add_closed(Slot::Galvanised, bx(Vec3::new(-0.04, y0, -0.1), Vec3::new(0.04, base, -0.03)));
    let gh = 0.45;
    let tw = seven_segment_width(label, gh);
    parts.add_closed(
        Slot::BoardDark,
        seven_segment(label, Vec3::new(-tw / 2.0, base + (h - gh) / 2.0, 0.03), gh, 0.01),
    );
    parts
}

/// Temporary grandstand `length` long (along X) with `rows` tiers rising
/// away from the track (track at local −Z), scaffold frame, seats, front
/// railing and a canopy.
pub fn grandstand(length: f32, rows: u32, lod: usize) -> Parts {
    let mut parts = Parts::new();
    let (tread, rise, front_h) = (0.8, 0.42, 1.2);
    let half = length / 2.0;
    let depth = rows as f32 * tread;
    for r in 0..rows {
        let z0 = -depth / 2.0 + r as f32 * tread;
        let y = front_h + r as f32 * rise;
        // Deck plank and riser.
        parts.add_closed(Slot::Galvanised, bx(Vec3::new(-half, y - 0.06, z0), Vec3::new(half, y, z0 + tread)));
        parts.add_closed(Slot::BoardDark, bx(Vec3::new(-half, y, z0 + tread - 0.02), Vec3::new(half, y + rise, z0 + tread)));
        if lod == 0 {
            let n = (length / 0.5) as usize;
            for k in 0..n {
                let x = -half + 0.25 + k as f32 * 0.5;
                let mut seat = bx(Vec3::new(x - 0.2, y + 0.38, z0 + 0.3), Vec3::new(x + 0.2, y + 0.43, z0 + 0.7));
                seat.append(&bx(Vec3::new(x - 0.2, y + 0.43, z0 + 0.66), Vec3::new(x + 0.2, y + 0.8, z0 + 0.7)));
                seat.append(&bx(Vec3::new(x - 0.03, y, z0 + 0.45), Vec3::new(x + 0.03, y + 0.38, z0 + 0.55)));
                parts.add(Slot::SeatBlue, seat);
            }
        } else {
            parts.add_closed(Slot::SeatBlue, bx(Vec3::new(-half, y, z0 + 0.3), Vec3::new(half, y + 0.6, z0 + 0.7)));
        }
    }
    // Scaffold: verticals on a 2.5 m grid under the tiers, ledgers and bracing.
    let seg = segments(lod, 6);
    let nx = (length / 2.5).round() as usize;
    let top = front_h + rows as f32 * rise;
    for i in 0..=nx {
        let x = -half + length * i as f32 / nx as f32;
        for j in 0..=rows.div_ceil(2) {
            let z = -depth / 2.0 + (j as f32 * 2.0 * tread).min(depth);
            let r = ((z + depth / 2.0) / tread).floor() as u32;
            let y = front_h + r.min(rows - 1) as f32 * rise;
            parts.add(Slot::Galvanised, MeshData::tube(Vec3::new(x, 0.0, z), Vec3::new(x, y, z), 0.024, seg, false));
        }
        parts.add(
            Slot::Galvanised,
            MeshData::tube(Vec3::new(x, 0.0, depth / 2.0), Vec3::new(x, top + 3.0, depth / 2.0), 0.03, seg, false),
        );
        if i < nx {
            let x1 = -half + length * (i + 1) as f32 / nx as f32;
            parts.add(
                Slot::Galvanised,
                MeshData::tube(Vec3::new(x, 0.2, depth / 2.0), Vec3::new(x1, top, depth / 2.0), 0.02, seg, false),
            );
        }
    }
    // Front railing and fascia.
    parts.add_closed(Slot::BoardWhite, bx(Vec3::new(-half, 0.0, -depth / 2.0 - 0.05), Vec3::new(half, front_h, -depth / 2.0)));
    parts.add(
        Slot::Galvanised,
        MeshData::tube(Vec3::new(-half, front_h + 1.0, -depth / 2.0), Vec3::new(half, front_h + 1.0, -depth / 2.0), 0.025, seg, true),
    );
    // Canopy over the back half, on the rear posts, tilted.
    let roof = bx(Vec3::new(-half - 0.3, top + 3.0, -depth / 2.0 + 1.0), Vec3::new(half + 0.3, top + 3.08, depth / 2.0 + 0.3));
    parts.add_closed(Slot::BoardWhite, roof);
    parts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::manifold_report;

    #[test]
    fn jersey_units_are_watertight() {
        let p = Path::new(Vec2::ZERO, 0.0).straight(20.0).arc(15.0, 45.0);
        let parts = jersey_run(&p, 0.0, p.length(), 8.0, 0.0);
        let m = &parts.by_slot[&Slot::Concrete];
        let r = manifold_report(m);
        assert!(r.is_closed() && r.signed_volume > 0.0, "{r:?}");
        // Cross-section area of the profile × length along the offset line.
        let area = 0.304;
        let len = p.offset_length(0.0, p.length(), 8.0);
        assert!((r.signed_volume / (area * len) - 1.0).abs() < 0.03, "{}", r.signed_volume);
    }

    #[test]
    fn armco_and_curbs_face_outward_on_both_sides() {
        let p = Path::new(Vec2::ZERO, 0.0).arc(30.0, -60.0);
        for side in [-1.0, 1.0] {
            for parts in [armco_run(&p, 0.0, p.length(), 7.0 * side, side, 0.0), curb_run(&p, 0.0, p.length(), 6.0 * side, side, 0.0)] {
                for m in parts.by_slot.values() {
                    assert!(manifold_report(m).signed_volume > 0.0);
                }
            }
        }
    }
}
