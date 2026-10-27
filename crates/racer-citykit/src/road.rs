//! Road sweep: a cross-section (lanes with camber, gutter pan, kerbs,
//! sidewalks) swept along a [`Path`], plus scattered road decals.
//!
//! UV conventions: the carriageway's UV0 is (lateral offset u, centreline
//! arc length s) in metres, which the surface shader uses for markings;
//! kerbs and sidewalks use (profile length, arc length along their own
//! offset line) in metres for tiling textures.

use bevy::math::{Vec2, Vec3};

use crate::{
    mesh::{Frame, MeshData},
    noise::Rng,
    path::Path,
    texture::DecalCell,
};

/// Street cross-section, symmetric about the centreline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RoadProfile {
    pub lanes_per_side: u32,
    pub lane_width: f32,
    /// Concrete gutter pan between the outer lane and the kerb.
    pub gutter: f32,
    /// Cross fall of the carriageway from the crown (m/m).
    pub camber: f32,
    pub kerb_height: f32,
    pub kerb_width: f32,
    pub sidewalk: f32,
}

impl Default for RoadProfile {
    /// Four-lane downtown avenue: 2 × 3.4 m lanes per side, 0.5 m gutter pans,
    /// 150 mm kerbs, 4.5 m sidewalks: 14.6 m between kerbs, 24.2 m between
    /// building lines (a typical downtown arterial).
    fn default() -> Self {
        Self {
            lanes_per_side: 2,
            lane_width: 3.4,
            gutter: 0.5,
            camber: 0.02,
            kerb_height: 0.15,
            kerb_width: 0.3,
            sidewalk: 4.5,
        }
    }
}

impl RoadProfile {
    /// Centreline to the outer edge of the last lane.
    pub fn carriage_half(&self) -> f32 {
        self.lanes_per_side as f32 * self.lane_width
    }

    /// Centreline to the kerb face.
    pub fn road_half(&self) -> f32 {
        self.carriage_half() + self.gutter
    }

    /// Centreline to the back of the sidewalk (building line).
    pub fn frontage(&self) -> f32 {
        self.road_half() + self.kerb_width + self.sidewalk
    }

    /// Surface height of the carriageway at lateral offset `u`.
    pub fn road_height(&self, u: f32) -> f32 {
        let au = u.abs();
        let ch = self.carriage_half();
        -self.camber * au.min(ch) - 0.04 * (au - ch).clamp(0.0, self.gutter)
    }

    /// Height of the kerb top and sidewalk front edge.
    pub fn kerb_top(&self) -> f32 {
        self.road_height(self.road_half()) + self.kerb_height
    }

    /// Sidewalk height at lateral offset `u` (rises 1.5 % towards the buildings).
    pub fn sidewalk_height(&self, u: f32) -> f32 {
        self.kerb_top() + 0.015 * (u.abs() - self.road_half() - self.kerb_width).max(0.0)
    }

    /// Surface height anywhere across the street.
    pub fn height(&self, u: f32) -> f32 {
        if u.abs() <= self.road_half() {
            self.road_height(u)
        } else {
            self.sidewalk_height(u)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn sign(self) -> f32 {
        match self {
            Side::Left => -1.0,
            Side::Right => 1.0,
        }
    }
}

/// An opening in the kerb and sidewalk (a side street or driveway) between
/// centreline arc lengths `s0..s1`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    pub side: Side,
    pub s0: f32,
    pub s1: f32,
}

/// Meshes of one road, one per material.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RoadMeshes {
    /// Carriageway incl. gutter pans (surface shader, road mode).
    pub carriageway: MeshData,
    /// Flat asphalt filling side-street mouths (plain mode).
    pub mouths: MeshData,
    pub kerbs: MeshData,
    pub sidewalks: MeshData,
}

/// Arc-length intervals of `[s0, s1]` not covered by `gaps` on `side`.
pub fn solid_intervals(s0: f32, s1: f32, side: Side, gaps: &[Gap]) -> Vec<(f32, f32)> {
    let mut gs: Vec<(f32, f32)> = gaps
        .iter()
        .filter(|g| g.side == side && g.s1 > s0 && g.s0 < s1)
        .map(|g| (g.s0.max(s0), g.s1.min(s1)))
        .collect();
    gs.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out = Vec::new();
    let mut cur = s0;
    for (a, b) in gs {
        if a > cur + 1e-3 {
            out.push((cur, a));
        }
        cur = cur.max(b);
    }
    if s1 > cur + 1e-3 {
        out.push((cur, s1));
    }
    out
}

/// Surface strip across the given `(u, y)` points, UV = (u, s).
fn strip(frames: &[Frame], pts: &[(f32, f32)]) -> MeshData {
    let mut m = MeshData::new();
    for w in pts.windows(2) {
        let ((u0, y0), (u1, y1)) = (w[0], w[1]);
        let base = m.positions.len() as u32;
        for f in frames {
            let a = f.at(Vec2::new(u0, y0));
            let b = f.at(Vec2::new(u1, y1));
            let n = (f.up * (u1 - u0) - f.right * (y1 - y0)).normalize();
            m.vertex(a, n, Vec2::new(u0, f.s));
            m.vertex(b, n, Vec2::new(u1, f.s));
        }
        for i in 0..frames.len() as u32 - 1 {
            let a = base + 2 * i;
            m.tri(a, a + 1, a + 3);
            m.tri(a, a + 3, a + 2);
        }
    }
    m
}

/// Mirrors a right-side profile to the left, keeping counter-clockwise order.
fn mirror(profile: &[Vec2]) -> Vec<Vec2> {
    profile.iter().rev().map(|p| Vec2::new(-p.x, p.y)).collect()
}

/// Builds the road between centreline arc lengths `s0..s1`.
pub fn build_road(path: &Path, p: &RoadProfile, gaps: &[Gap], s0: f32, s1: f32) -> RoadMeshes {
    let mut out = RoadMeshes::default();
    let stations = path.stations(s0, s1, 1.0);
    let frames = path.frames_centre_s(&stations, 0.0);
    let (ch, rh) = (p.carriage_half(), p.road_half());
    let cross: Vec<(f32, f32)> = [-rh, -ch, 0.0, ch, rh]
        .iter()
        .map(|u| (*u, p.road_height(*u)))
        .collect();
    out.carriageway = strip(&frames, &cross);

    let kerb_y0 = p.road_height(rh);
    let kt = p.kerb_top();
    for side in [Side::Left, Side::Right] {
        let sg = side.sign();
        // Side-street mouths: flat asphalt from the kerb line to the frontage.
        for g in gaps.iter().filter(|g| g.side == side) {
            let st = path.stations(g.s0.max(s0), g.s1.min(s1), 1.0);
            let fr = path.frames_centre_s(&st, 0.0);
            let pts = if sg > 0.0 {
                [(rh, kerb_y0), (p.frontage() + 0.5, kerb_y0)]
            } else {
                [(-p.frontage() - 0.5, kerb_y0), (-rh, kerb_y0)]
            };
            out.mouths.append(&strip(&fr, &pts));
        }
        for (a, b) in solid_intervals(s0, s1, side, gaps) {
            let st = path.stations(a, b, 1.0);
            // Kerb stone: buried 0.25 m, 25 mm chamfer on the road-side arris.
            let kf = path.frames(&st, sg * rh);
            let kw = p.kerb_width;
            let kerb = [
                Vec2::new(0.0, kerb_y0 - 0.25),
                Vec2::new(kw, kerb_y0 - 0.25),
                Vec2::new(kw, kt),
                Vec2::new(0.025, kt),
                Vec2::new(0.0, kt - 0.025),
            ];
            let kerb = if sg > 0.0 { kerb.to_vec() } else { mirror(&kerb) };
            out.kerbs.append(&MeshData::sweep(&kf, &kerb, true, true));
            // Sidewalk slab, top surface rising 1.5 % towards the buildings.
            let sf = path.frames(&st, sg * (rh + kw));
            let w = p.sidewalk;
            let rise = 0.015 * w;
            let slab = [
                Vec2::new(w + 0.3, kt + rise),
                Vec2::new(0.0, kt),
                Vec2::new(0.0, kt - 0.3),
                Vec2::new(w + 0.3, kt - 0.3),
            ];
            // Counter-clockwise from the back-top corner, so the first edge
            // (UV u from 0) is the walking surface.
            let slab = if sg > 0.0 { slab.to_vec() } else { mirror(&slab) };
            out.sidewalks.append(&MeshData::sweep(&sf, &slab, true, true));
        }
    }
    out
}

/// One decal on the road surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Decal {
    pub cell: DecalCell,
    /// Centre in road coordinates (u, s).
    pub at: Vec2,
    /// Size across (u) and along (s) the road, metres.
    pub size: Vec2,
}

/// Where decals go: braking zones get skid marks, lanes get patches,
/// manholes and cracks, gutters get drains.
#[derive(Clone, Debug, PartialEq)]
pub struct DecalRules {
    pub seed: u64,
    /// Utility patches per 100 m.
    pub patches: f32,
    pub cracks: f32,
    pub manholes: f32,
    /// Gutter drain spacing (m) on each side.
    pub drain_spacing: f32,
    pub oil: f32,
    /// Braking zones `(s0, s1)` that get rubber skid marks.
    pub braking_zones: Vec<(f32, f32)>,
}

impl Default for DecalRules {
    fn default() -> Self {
        Self {
            seed: 1,
            patches: 3.0,
            cracks: 6.0,
            manholes: 2.5,
            drain_spacing: 35.0,
            oil: 4.0,
            braking_zones: Vec::new(),
        }
    }
}

/// Scatters decals over `s0..s1`, deterministically from `rules.seed`.
pub fn scatter_decals(p: &RoadProfile, rules: &DecalRules, s0: f32, s1: f32) -> Vec<Decal> {
    let mut rng = Rng::new(rules.seed);
    let len = s1 - s0;
    let ch = p.carriage_half();
    let lw = p.lane_width;
    let lanes = p.lanes_per_side as i32;
    let lane_centre = |rng: &mut Rng| {
        let k = rng.index((2 * lanes) as usize) as i32 - lanes;
        (k as f32 + 0.5) * lw
    };
    let mut out = Vec::new();
    let count = |rate: f32, rng: &mut Rng| (rate * len / 100.0 + rng.f32()).floor() as usize;
    for _ in 0..count(rules.patches, &mut rng) {
        // Trench repairs: across one or two lanes or full width.
        let s = rng.range(s0 + 3.0, s1 - 3.0);
        let w = [lw, 2.0 * lw, 2.0 * ch][rng.weighted(&[0.5, 0.3, 0.2])];
        let u = if w >= 2.0 * ch { 0.0 } else { lane_centre(&mut rng) + rng.range(-0.3, 0.3) };
        out.push(Decal { cell: DecalCell::Patch, at: Vec2::new(u, s), size: Vec2::new(w, rng.range(1.2, 3.5)) });
    }
    for _ in 0..count(rules.cracks, &mut rng) {
        let cell = if rng.chance(0.5) { DecalCell::Crack } else { DecalCell::CrackBranch };
        let s = rng.range(s0 + 2.0, s1 - 2.0);
        // Cracks follow lane joints (longitudinal) - rotate by sizing along s.
        let k = rng.index((2 * lanes + 1) as usize) as i32 - lanes;
        let size = rng.range(2.0, 5.0);
        out.push(Decal { cell, at: Vec2::new(k as f32 * lw + rng.range(-0.2, 0.2), s), size: Vec2::new(size * 0.4, size) });
    }
    for _ in 0..count(rules.manholes, &mut rng) {
        let cell = if rng.chance(0.75) { DecalCell::Manhole } else { DecalCell::ManholeRect };
        let s = rng.range(s0 + 2.0, s1 - 2.0);
        out.push(Decal { cell, at: Vec2::new(lane_centre(&mut rng) + rng.range(-0.5, 0.5), s), size: Vec2::splat(0.75) });
    }
    for _ in 0..count(rules.oil, &mut rng) {
        let s = rng.range(s0 + 2.0, s1 - 2.0);
        out.push(Decal { cell: DecalCell::OilStain, at: Vec2::new(lane_centre(&mut rng), s), size: Vec2::splat(rng.range(0.8, 1.6)) });
    }
    if rules.drain_spacing > 0.0 {
        for side in [-1.0, 1.0] {
            let mut s = s0 + rng.range(5.0, rules.drain_spacing);
            while s < s1 - 2.0 {
                out.push(Decal { cell: DecalCell::Drain, at: Vec2::new(side * (ch + p.gutter * 0.5), s), size: Vec2::new(0.45, 0.9) });
                s += rules.drain_spacing * rng.range(0.8, 1.2);
            }
        }
    }
    for &(a, b) in &rules.braking_zones {
        // Rubber laid down on the racing line, densest at the end of the zone.
        let n = ((b - a) / 6.0) as usize;
        for i in 0..n {
            let t = (i as f32 + rng.f32()) / n as f32;
            let s = a + (b - a) * t;
            let u = rng.range(-ch + 1.2, ch - 1.2);
            if rng.chance(0.3 + 0.6 * t) {
                out.push(Decal { cell: DecalCell::Skid, at: Vec2::new(u, s), size: Vec2::new(1.9, rng.range(4.0, 9.0)) });
            }
        }
    }
    out
}

/// Builds decal quads following the road surface (subdivided across the
/// camber), 15 mm above it, with UVs in the decal atlas cells.
pub fn build_decals(path: &Path, p: &RoadProfile, decals: &[Decal]) -> MeshData {
    let mut m = MeshData::new();
    for d in decals {
        let (uv0, uv1) = d.cell.uv_rect();
        // Keep a half-texel border so mips do not bleed between cells.
        let inset = Vec2::splat(0.004);
        let (uv0, uv1) = (uv0 + inset, uv1 - inset);
        let nu = ((d.size.x / 1.5).ceil() as usize).clamp(1, 12);
        let ns = ((d.size.y / 2.0).ceil() as usize).clamp(1, 12);
        let base = m.positions.len() as u32;
        for j in 0..=ns {
            let tv = j as f32 / ns as f32;
            let s = d.at.y + (tv - 0.5) * d.size.y;
            let pose = path.pose(s);
            let right = Vec3::new(pose.right().x, 0.0, pose.right().y);
            for i in 0..=nu {
                let tu = i as f32 / nu as f32;
                let u = d.at.x + (tu - 0.5) * d.size.x;
                let pos = path.point(s, u, p.height(u) + 0.015);
                let slope = if u.abs() < p.carriage_half() { p.camber } else { 0.04 } * -u.signum();
                let n = (Vec3::Y - right * slope).normalize();
                m.vertex(pos, n, Vec2::new(uv0.x + (uv1.x - uv0.x) * tu, uv0.y + (uv1.y - uv0.y) * tv));
            }
        }
        let row = nu as u32 + 1;
        for j in 0..ns as u32 {
            for i in 0..nu as u32 {
                let a = base + j * row + i;
                m.tri(a, a + 1, a + row + 1);
                m.tri(a, a + row + 1, a + row);
            }
        }
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::manifold_report;

    fn corner() -> Path {
        Path::new(Vec2::ZERO, 0.0).straight(60.0).arc(25.0, 90.0).straight(40.0)
    }

    #[test]
    fn profile_dimensions() {
        let p = RoadProfile::default();
        assert!((p.frontage() - 12.1).abs() < 1e-4);
        assert!(p.road_height(0.0) == 0.0 && p.road_height(5.0) < 0.0);
        assert!((p.kerb_top() - p.road_height(p.road_half()) - 0.15).abs() < 1e-5);
    }

    #[test]
    fn intervals_skip_gaps() {
        let gaps = [Gap { side: Side::Right, s0: 10.0, s1: 20.0 }, Gap { side: Side::Left, s0: 0.0, s1: 5.0 }];
        assert_eq!(solid_intervals(0.0, 50.0, Side::Right, &gaps), vec![(0.0, 10.0), (20.0, 50.0)]);
        assert_eq!(solid_intervals(0.0, 50.0, Side::Left, &gaps), vec![(5.0, 50.0)]);
    }

    #[test]
    fn road_meshes_are_finite_and_kerbs_watertight() {
        let path = corner();
        let gaps = [Gap { side: Side::Left, s0: 20.0, s1: 34.0 }];
        let r = build_road(&path, &RoadProfile::default(), &gaps, 0.0, path.length());
        for m in [&r.carriageway, &r.mouths, &r.kerbs, &r.sidewalks] {
            assert!(!m.is_empty());
            assert!(m.positions.iter().all(|p| p.is_finite()));
            assert!(m.normals.iter().all(|n| (n.length() - 1.0).abs() < 1e-4));
        }
        // Carriageway faces up everywhere.
        assert!(r.carriageway.normals.iter().all(|n| n.y > 0.99));
        for m in [&r.kerbs, &r.sidewalks] {
            let rep = manifold_report(m);
            assert!(rep.is_closed(), "{rep:?}");
            assert!(rep.signed_volume > 0.0);
        }
        // UV u of the carriageway is the lateral offset in metres.
        let half = RoadProfile::default().road_half();
        assert!(r.carriageway.uvs.iter().all(|uv| uv.x.abs() <= half + 1e-4));
    }

    #[test]
    fn decals_are_deterministic_and_on_the_road() {
        let p = RoadProfile::default();
        let rules = DecalRules { braking_zones: vec![(30.0, 60.0)], ..Default::default() };
        let a = scatter_decals(&p, &rules, 0.0, 200.0);
        assert_eq!(a, scatter_decals(&p, &rules, 0.0, 200.0));
        assert!(a.len() > 20);
        assert!(a.iter().all(|d| d.at.x.abs() <= p.road_half() && (0.0..=200.0).contains(&d.at.y)));
        let m = build_decals(&corner(), &p, &a);
        assert!(m.uvs.iter().all(|uv| (0.0..=1.0).contains(&uv.x) && (0.0..=1.0).contains(&uv.y)));
    }
}
