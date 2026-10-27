//! Street furniture: lamps, traffic signals, road signs, bollards, bins,
//! benches, bus shelter, hydrants, parking meters, utility cabinets, trees
//! and parked-car placeholders. All in prop space (−Z towards the road).

use bevy::math::{Affine3A, Vec2, Vec3};

use super::{bx, bxc, disc_z, extrude_x, panel_z, segments};
use crate::{
    kit::{Parts, Slot, Socket, SocketKind},
    mesh::MeshData,
    noise::Rng,
    texture::RoadSignCell,
};

/// LED street light colour temperature (K) and output (lm).
pub const STREET_LIGHT_K: f32 = 4000.0;
pub const STREET_LIGHT_LM: f32 = 16_000.0;

/// Street lamp: 9 m tapered pole on a plinth, `arms` (1 or 2) outreach arms
/// of 2.2 m with flat LED luminaires; one spot-light socket per head.
pub fn street_lamp(arms: u32, lod: usize) -> (Parts, Vec<Socket>) {
    let seg = segments(lod, 12);
    let mut p = Parts::new();
    let h = 9.0;
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.16, 0.13, 0.7, seg, true),
    );
    p.add_closed(Slot::DarkMetal, MeshData::frustum(0.1, 0.065, h, seg, true));
    let mut sockets = Vec::new();
    let dirs: &[f32] = if arms >= 2 { &[-1.0, 1.0] } else { &[-1.0] };
    for &d in dirs {
        let a = Vec3::new(0.0, h - 0.5, 0.0);
        let b = Vec3::new(0.0, h + 0.05, d * 0.9);
        let c = Vec3::new(0.0, h + 0.15, d * 2.2);
        p.add_closed(
            Slot::DarkMetal,
            MeshData::tube(a, b, 0.045, seg.min(8), true),
        );
        p.add_closed(
            Slot::DarkMetal,
            MeshData::tube(b, c, 0.04, seg.min(8), true),
        );
        // Luminaire: tapered housing with a lens underneath.
        let z0 = d * 2.0;
        let z1 = d * 2.75;
        let (zmin, zmax) = (z0.min(z1), z0.max(z1));
        p.add_closed(
            Slot::DarkMetal,
            bx(
                Vec3::new(-0.18, h + 0.05, zmin),
                Vec3::new(0.18, h + 0.2, zmax),
            ),
        );
        p.add_closed(
            Slot::LampLens,
            bx(
                Vec3::new(-0.15, h + 0.02, zmin + 0.04),
                Vec3::new(0.15, h + 0.05, zmax - 0.04),
            ),
        );
        sockets.push(Socket {
            name: format!("light_{}", if d < 0.0 { "road" } else { "back" }),
            kind: SocketKind::SpotLight {
                outer_angle_deg: 62.0,
                range: 32.0,
                lumens: STREET_LIGHT_LM,
                kelvin: STREET_LIGHT_K,
            },
            pos: Vec3::new(0.0, h, (z0 + z1) / 2.0),
            // Tilted 10 degrees towards the far side of the street.
            dir: Vec3::new(0.0, -1.0, d * 0.18).normalize(),
        });
    }
    (p, sockets)
}

/// Which aspect a signal head shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Aspect {
    Red,
    Amber,
    Green,
}

/// Three-aspect signal head (0.35 × 1.0 m) with visors, lenses facing +X.
fn signal_head(aspect: Aspect, lod: usize) -> Parts {
    let mut p = Parts::new();
    let seg = segments(lod, 12);
    p.add_closed(Slot::DarkMetal, bxc(Vec3::ZERO, Vec3::new(0.25, 1.0, 0.35)));
    // Backplate with a thin yellow retroreflective border.
    p.add_closed(
        Slot::DarkMetal,
        bxc(Vec3::new(-0.13, 0.0, 0.0), Vec3::new(0.02, 1.25, 0.6)),
    );
    p.add_closed(
        Slot::PaintYellow,
        bxc(Vec3::new(-0.1225, 0.0, 0.0), Vec3::new(0.005, 1.27, 0.62)),
    );
    for (k, (a, slot)) in [
        (Aspect::Red, Slot::SignalRed),
        (Aspect::Amber, Slot::SignalAmber),
        (Aspect::Green, Slot::SignalGreen),
    ]
    .into_iter()
    .enumerate()
    {
        let y = 0.31 - k as f32 * 0.31;
        let lens = disc_z(0.11, 0.015, seg, (Vec2::ZERO, Vec2::ONE), false)
            .map_uvs(|_| Vec2::ZERO)
            .transformed(Affine3A::from_rotation_translation(
                bevy::math::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
                Vec3::new(0.125, y, 0.0),
            ));
        p.add(if a == aspect { slot } else { Slot::SignalOff }, lens);
        if lod == 0 {
            // Visor hood above each lens.
            p.add_closed(
                Slot::DarkMetal,
                bxc(Vec3::new(0.24, y + 0.13, 0.0), Vec3::new(0.22, 0.02, 0.26)),
            );
        }
    }
    p
}

/// Traffic signal mast: 6.5 m pole, mast arm reaching `reach` m over the
/// road (−Z) carrying two heads, plus one head on the pole. Heads face +X.
pub fn traffic_signal(reach: f32, aspect: Aspect, lod: usize) -> (Parts, Vec<Socket>) {
    let seg = segments(lod, 12);
    let mut p = Parts::new();
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.2, 0.18, 0.5, seg, true),
    );
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.13, 0.1, 6.5, seg, true),
    );
    p.add_closed(
        Slot::DarkMetal,
        MeshData::tube(
            Vec3::new(0.0, 6.0, 0.0),
            Vec3::new(0.0, 6.2, -reach),
            0.07,
            seg.min(8),
            true,
        ),
    );
    p.add_closed(
        Slot::DarkMetal,
        MeshData::tube(
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::new(0.0, 6.05, -reach * 0.35),
            0.035,
            6,
            true,
        ),
    );
    let mut sockets = Vec::new();
    let mut place = |at: Vec3, p: &mut Parts| {
        p.merge(&signal_head(aspect, lod), Affine3A::from_translation(at));
        sockets.push(Socket {
            name: format!("signal_{}", sockets.len()),
            kind: SocketKind::Emissive,
            pos: at,
            dir: Vec3::X,
        });
    };
    for z in [-reach * 0.55, -reach * 0.92] {
        place(Vec3::new(0.0, 5.55, z), &mut p);
        p.add_closed(
            Slot::DarkMetal,
            bxc(Vec3::new(0.0, 6.12, z), Vec3::new(0.06, 0.15, 0.06)),
        );
    }
    place(Vec3::new(0.15, 3.2, 0.0), &mut p);
    (p, sockets)
}

/// Road sign on a galvanised post: plate showing `cell` of the road-sign
/// atlas, facing +X (oncoming traffic), at 2.2 m.
pub fn road_sign(cell: RoadSignCell, lod: usize) -> Parts {
    let mut p = Parts::new();
    let seg = segments(lod, 20);
    p.add_closed(
        Slot::Galvanised,
        MeshData::frustum(0.035, 0.035, 2.9, 8, true),
    );
    let rect = cell.uv_rect();
    let (w, h) = cell.size();
    let to_x = Affine3A::from_rotation_translation(
        bevy::math::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
        Vec3::new(0.04, 2.3 + h / 2.0, 0.0),
    );
    if cell.round() {
        p.add(
            Slot::RoadSign,
            disc_z(w / 2.0, 0.0, seg, rect, false).transformed(to_x),
        );
        p.add(
            Slot::Galvanised,
            MeshData::frustum(w / 2.0, w / 2.0, 0.01, seg, true).transformed(
                Affine3A::from_rotation_translation(
                    bevy::math::Quat::from_rotation_z(-std::f32::consts::FRAC_PI_2),
                    Vec3::new(0.03, 2.3 + h / 2.0, 0.0),
                ),
            ),
        );
    } else {
        p.add(
            Slot::RoadSign,
            panel_z(
                Vec2::new(-w / 2.0, -h / 2.0),
                Vec2::new(w / 2.0, h / 2.0),
                0.0,
                rect,
            )
            .transformed(to_x),
        );
        p.add_closed(
            Slot::Galvanised,
            bxc(Vec3::new(0.03, 2.3 + h / 2.0, 0.0), Vec3::new(0.012, h, w)),
        );
    }
    p
}

/// Steel bollard (0.95 m) with a reflective band.
pub fn bollard(lod: usize) -> Parts {
    let seg = segments(lod, 12);
    let mut p = Parts::new();
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.1, 0.095, 0.95, seg, true),
    );
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.095, 0.03, 0.06, seg, true).translated(Vec3::Y * 0.95),
    );
    p.add_closed(
        Slot::PaintWhite,
        MeshData::frustum(0.102, 0.1, 0.07, seg, true).translated(Vec3::Y * 0.78),
    );
    p
}

/// Litter bin (0.95 m) with a domed lid.
pub fn bin(lod: usize) -> Parts {
    let seg = segments(lod, 14);
    let mut p = Parts::new();
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.27, 0.3, 0.9, seg, true),
    );
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.31, 0.2, 0.12, seg, true).translated(Vec3::Y * 0.9),
    );
    p.add_closed(
        Slot::Galvanised,
        MeshData::frustum(0.302, 0.302, 0.05, seg, true).translated(Vec3::Y * 0.75),
    );
    p
}

/// Bench, 1.8 m, cast legs with timber slats, seat facing +Z (away from the
/// road when placed with the default facing).
pub fn bench() -> Parts {
    let mut p = Parts::new();
    for x in [-0.75, 0.75] {
        p.add_closed(
            Slot::DarkMetal,
            bx(
                Vec3::new(x - 0.04, 0.0, -0.25),
                Vec3::new(x + 0.04, 0.45, 0.25),
            ),
        );
        p.add_closed(
            Slot::DarkMetal,
            bx(
                Vec3::new(x - 0.04, 0.45, -0.25),
                Vec3::new(x + 0.04, 0.85, -0.18),
            ),
        );
    }
    for k in 0..3 {
        let z = -0.2 + k as f32 * 0.15;
        p.add_closed(
            Slot::Wood,
            bx(Vec3::new(-0.9, 0.42, z), Vec3::new(0.9, 0.46, z + 0.12)),
        );
    }
    for k in 0..2 {
        let y = 0.55 + k as f32 * 0.15;
        p.add_closed(
            Slot::Wood,
            bx(Vec3::new(-0.9, y, -0.26), Vec3::new(0.9, y + 0.11, -0.22)),
        );
    }
    p
}

/// Bus shelter 4 × 1.6 m: posts, roof, glass back and ends, advertising
/// lightbox, bench, and a bus-stop flag sign.
pub fn bus_shelter(lod: usize) -> (Parts, Vec<Socket>) {
    let mut p = Parts::new();
    let (w, d, h) = (4.0, 1.6, 2.5);
    // Open side faces −Z (the road); back at +Z.
    for (x, z) in [
        (-w / 2.0, d / 2.0),
        (w / 2.0, d / 2.0),
        (-w / 2.0, -d / 2.0 + 0.3),
        (w / 2.0, -d / 2.0 + 0.3),
    ] {
        p.add_closed(
            Slot::DarkMetal,
            bxc(Vec3::new(x, h / 2.0, z), Vec3::new(0.08, h, 0.08)),
        );
    }
    p.add_closed(
        Slot::DarkMetal,
        bx(
            Vec3::new(-w / 2.0 - 0.1, h, -d / 2.0),
            Vec3::new(w / 2.0 + 0.1, h + 0.12, d / 2.0 + 0.1),
        ),
    );
    p.add_closed(
        Slot::Glass,
        bx(
            Vec3::new(-w / 2.0 + 0.05, 0.15, d / 2.0 - 0.01),
            Vec3::new(w / 2.0 - 1.3, h - 0.05, d / 2.0 + 0.01),
        ),
    );
    p.add_closed(
        Slot::Glass,
        bx(
            Vec3::new(-w / 2.0 - 0.01, 0.15, -d / 2.0 + 0.35),
            Vec3::new(-w / 2.0 + 0.01, h - 0.05, d / 2.0 - 0.05),
        ),
    );
    // Advertising lightbox (double-sided) at the right end of the back wall.
    p.add_closed(
        Slot::DarkMetal,
        bx(
            Vec3::new(w / 2.0 - 1.3, 0.1, d / 2.0 - 0.1),
            Vec3::new(w / 2.0 - 0.05, h - 0.1, d / 2.0 + 0.1),
        ),
    );
    let (a, b) = crate::texture::sign_cell_rect(5);
    for (z, flip) in [(d / 2.0 - 0.105, true), (d / 2.0 + 0.105, false)] {
        let mut panel = panel_z(
            Vec2::new(w / 2.0 - 1.22, 0.25),
            Vec2::new(w / 2.0 - 0.13, h - 0.25),
            0.0,
            (a, b),
        );
        if flip {
            panel = panel
                .transformed(Affine3A::from_rotation_y(std::f32::consts::PI))
                .translated(Vec3::new(w - 1.35, 0.0, 0.0));
        }
        p.add(Slot::Lightbox, panel.translated(Vec3::Z * z));
    }
    let mut bench = bench();
    if lod > 0 {
        bench = Parts::new();
    }
    p.merge(
        &bench,
        Affine3A::from_translation(Vec3::new(-0.6, 0.0, 0.35)),
    );
    let sign = road_sign(RoadSignCell::BusStop, lod);
    p.merge(
        &sign,
        Affine3A::from_translation(Vec3::new(-w / 2.0 - 0.8, 0.0, -d / 2.0 + 0.2)),
    );
    let sockets = vec![Socket {
        name: "shelter_light".into(),
        kind: SocketKind::PointLight {
            range: 8.0,
            lumens: 1500.0,
            kelvin: 4500.0,
        },
        pos: Vec3::new(0.0, h - 0.15, 0.0),
        dir: Vec3::NEG_Y,
    }];
    (p, sockets)
}

/// Fire hydrant (0.75 m) with cap, side outlets and flange.
pub fn hydrant(lod: usize) -> Parts {
    let seg = segments(lod, 12);
    let mut p = Parts::new();
    p.add_closed(
        Slot::PaintYellow,
        MeshData::frustum(0.17, 0.17, 0.06, seg, true),
    );
    p.add_closed(
        Slot::PaintYellow,
        MeshData::frustum(0.12, 0.11, 0.55, seg, true).translated(Vec3::Y * 0.06),
    );
    p.add_closed(
        Slot::PaintYellow,
        MeshData::frustum(0.14, 0.14, 0.04, seg, true).translated(Vec3::Y * 0.58),
    );
    p.add_closed(
        Slot::PaintRed,
        MeshData::frustum(0.13, 0.04, 0.14, seg, true).translated(Vec3::Y * 0.62),
    );
    for (a, b) in [
        (Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.0, 0.42, -0.2)),
        (Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.18, 0.42, 0.0)),
        (Vec3::new(0.0, 0.42, 0.0), Vec3::new(-0.18, 0.42, 0.0)),
    ] {
        p.add_closed(
            Slot::PaintYellow,
            MeshData::tube(a, b, 0.045, seg.min(8), true),
        );
    }
    p
}

/// Parking meter: post with a head.
pub fn parking_meter() -> Parts {
    let mut p = Parts::new();
    p.add_closed(
        Slot::DarkMetal,
        MeshData::frustum(0.04, 0.04, 1.15, 8, true),
    );
    p.add_closed(
        Slot::Galvanised,
        bxc(Vec3::new(0.0, 1.35, 0.0), Vec3::new(0.24, 0.42, 0.18)),
    );
    p.add_closed(
        Slot::BoardDark,
        bxc(Vec3::new(0.0, 1.42, -0.091), Vec3::new(0.14, 0.12, 0.01)),
    );
    p
}

/// Traffic-signal controller cabinet.
pub fn utility_cabinet() -> Parts {
    let mut p = Parts::new();
    p.add_closed(
        Slot::Concrete,
        bxc(Vec3::new(0.0, 0.05, 0.0), Vec3::new(0.9, 0.1, 0.6)),
    );
    p.add_closed(
        Slot::Galvanised,
        bxc(Vec3::new(0.0, 0.75, 0.0), Vec3::new(0.8, 1.3, 0.5)),
    );
    p
}

/// Street tree in a pit: tapered trunk, branching limbs and a canopy of
/// jittered low-poly blobs (deterministic per `seed`).
pub fn tree(seed: u64, lod: usize) -> Parts {
    let mut rng = Rng::new(seed);
    let mut p = Parts::new();
    let seg = segments(lod, 8);
    let h = rng.range(3.0, 3.8);
    p.add_closed(
        Slot::DarkMetal,
        bx(Vec3::new(-0.7, -0.02, -0.7), Vec3::new(0.7, 0.03, 0.7)),
    );
    p.add_closed(Slot::Bark, MeshData::frustum(0.14, 0.09, h, seg, true));
    let blobs = if lod == 0 { 9 } else { 4 };
    for k in 0..blobs {
        let a = k as f32 / blobs as f32 * std::f32::consts::TAU + rng.range(-0.3, 0.3);
        let r = if k == 0 { 0.0 } else { rng.range(0.8, 1.8) };
        let c = Vec3::new(a.cos() * r, h + rng.range(0.4, 2.6), a.sin() * r);
        if k > 0 && lod == 0 {
            p.add_closed(
                Slot::Bark,
                MeshData::tube(Vec3::Y * (h - 0.2), c, 0.05, 5, true),
            );
        }
        let rad = rng.range(1.0, 1.6);
        p.add_closed(Slot::Foliage, blob(c, rad, &mut rng, lod));
    }
    p
}

/// Jittered icosphere-like blob (closed).
fn blob(c: Vec3, r: f32, rng: &mut Rng, lod: usize) -> MeshData {
    let (rings, seg) = if lod == 0 { (6, 9) } else { (4, 6) };
    let mut pts = Vec::new();
    for i in 0..=rings {
        let phi = i as f32 / rings as f32 * std::f32::consts::PI;
        for j in 0..seg {
            let th = j as f32 / seg as f32 * std::f32::consts::TAU;
            let jitter = if i == 0 || i == rings {
                1.0
            } else {
                rng.range(0.8, 1.15)
            };
            pts.push(
                Vec3::new(phi.sin() * th.cos(), phi.cos() * 0.8, phi.sin() * th.sin()) * r * jitter,
            );
        }
    }
    let mut m = MeshData::new();
    let idx = |i: usize, j: usize| i * seg + j % seg;
    for i in 0..rings {
        for j in 0..seg {
            let (a, b, cc, d) = (
                pts[idx(i, j)],
                pts[idx(i, j + 1)],
                pts[idx(i + 1, j + 1)],
                pts[idx(i + 1, j)],
            );
            for tri in [[a, b, cc], [a, cc, d]] {
                let n = (tri[1] - tri[0]).cross(tri[2] - tri[0]);
                if n.length_squared() < 1e-10 {
                    continue;
                }
                let centroid = (tri[0] + tri[1] + tri[2]) / 3.0;
                let n = if n.dot(centroid) < 0.0 { -n } else { n }.normalize();
                let base = m.positions.len() as u32;
                for p in tri {
                    // Mostly radial normals: soft, leafy shading instead of facets.
                    let radial = p.normalize_or(Vec3::Y);
                    m.vertex(
                        p + c,
                        (radial * 0.75 + n * 0.25).normalize(),
                        Vec2::new(p.x + p.z, p.y),
                    );
                }
                m.tri(base, base + 1, base + 2);
            }
        }
    }
    m
}

/// Parked-car placeholder (sedan 4.7 × 1.85 m), facing −Z, body colour by
/// material. Extruded side profile, glasshouse, four wheels.
pub fn parked_car(lod: usize) -> Parts {
    let mut p = Parts::new();
    let seg = segments(lod, 14);
    // Side profile in (z, y), front at −z, counter-clockwise.
    let body: Vec<Vec2> = [
        (-2.35, 0.3),
        (-2.38, 0.55),
        (-2.25, 0.82),
        (-0.9, 0.95),
        (1.4, 0.98),
        (2.3, 0.95),
        (2.38, 0.6),
        (2.32, 0.3),
    ]
    .iter()
    .rev()
    .map(|(z, y)| Vec2::new(*z, *y))
    .collect();
    p.add_closed(Slot::CarPaint, extrude_x(&body, 1.85));
    let glass: Vec<Vec2> = [(-0.95, 0.95), (-0.25, 1.42), (0.95, 1.43), (1.5, 0.98)]
        .iter()
        .rev()
        .map(|(z, y)| Vec2::new(*z, *y))
        .collect();
    p.add_closed(Slot::CarGlass, extrude_x(&glass, 1.6));
    let roof: Vec<Vec2> = [(-0.2, 1.42), (-0.25, 1.46), (0.95, 1.47), (0.92, 1.43)]
        .iter()
        .rev()
        .map(|(z, y)| Vec2::new(*z, *y))
        .collect();
    p.add_closed(Slot::CarPaint, extrude_x(&roof, 1.5));
    for (x, z) in [(-0.82, -1.45), (0.82, -1.45), (-0.82, 1.4), (0.82, 1.4)] {
        let wheel = MeshData::frustum(0.33, 0.33, 0.24, seg, true).transformed(
            Affine3A::from_rotation_translation(
                bevy::math::Quat::from_rotation_z(std::f32::consts::FRAC_PI_2),
                Vec3::new(x + 0.12 * f32::signum(x), 0.33, z),
            ),
        );
        p.add_closed(Slot::Rubber, wheel);
    }
    // Lights: emissive-ish lamp boxes (tail red, head white).
    for x in [-0.65, 0.65] {
        p.add_closed(
            Slot::LampLens,
            bxc(Vec3::new(x, 0.75, -2.33), Vec3::new(0.35, 0.1, 0.06)),
        );
        p.add_closed(
            Slot::SignalRed,
            bxc(Vec3::new(x, 0.82, 2.36), Vec3::new(0.35, 0.08, 0.04)),
        );
    }
    p
}
