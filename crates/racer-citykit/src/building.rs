//! Modular façade kit and rule-based block assembler (PLAN.md section 7).
//!
//! A block is a convex footprint polygon (x, z) with flags for the edges that
//! face a street. [`subdivide_block`] cuts it into lots along its frontage;
//! [`build`] assembles each lot from façade modules chosen by the district
//! rules: ground-floor shopfronts (pilasters, stall risers, glazing, fascia
//! signs, awnings), punched-window bays with sills and lintels, curtain-wall
//! bays, string courses, cornices, parapets, fire escapes and rooftops (AC
//! units, bulkheads, water towers). Façades are built in façade space
//! (x along the edge, y up, z outward) and transformed into place.

use bevy::math::{Affine3A, Vec2, Vec3};

use crate::{
    district::{Archetype, District, Rgb, Style, Wall},
    kit::{Parts, Slot},
    mesh::MeshData,
    noise::Rng,
    props::{bx, extrude_x},
};

/// A lot (or block): convex footprint in (x, z) and per-edge street flags
/// (edge i runs from vertex i to i + 1).
#[derive(Clone, Debug, PartialEq)]
pub struct Lot {
    pub poly: Vec<Vec2>,
    pub street: Vec<bool>,
}

impl Lot {
    pub fn area(&self) -> f32 {
        let n = self.poly.len();
        (0..n)
            .map(|i| {
                let (a, b) = (self.poly[i], self.poly[(i + 1) % n]);
                a.x * b.y - b.x * a.y
            })
            .sum::<f32>()
            .abs()
            / 2.0
    }

    pub fn centroid(&self) -> Vec2 {
        self.poly.iter().copied().sum::<Vec2>() / self.poly.len() as f32
    }

    /// Total street frontage length.
    pub fn frontage(&self) -> f32 {
        let n = self.poly.len();
        (0..n)
            .filter(|i| self.street[*i])
            .map(|i| self.poly[i].distance(self.poly[(i + 1) % n]))
            .sum()
    }
}

/// Clips a convex polygon to the half-plane `dot(p - origin, normal) <= 0`.
pub fn clip(poly: &[Vec2], origin: Vec2, normal: Vec2) -> Vec<Vec2> {
    let mut out = Vec::new();
    let n = poly.len();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let (da, db) = ((a - origin).dot(normal), (b - origin).dot(normal));
        if da <= 0.0 {
            out.push(a);
        }
        if (da < 0.0) != (db < 0.0) && (da - db).abs() > 1e-9 {
            let t = da / (da - db);
            out.push(a + (b - a) * t);
        }
    }
    out.dedup_by(|a, b| a.distance(*b) < 1e-4);
    if out.len() > 1 && out[0].distance(out[out.len() - 1]) < 1e-4 {
        out.pop();
    }
    out
}

/// Insets a convex polygon by `d` (each edge moved inwards).
pub fn inset(poly: &[Vec2], d: f32) -> Vec<Vec2> {
    let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    let n = poly.len();
    let mut out = poly.to_vec();
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        let dir = (b - a).normalize_or_zero();
        let mut nrm = Vec2::new(dir.y, -dir.x);
        if nrm.dot(c - a) > 0.0 {
            nrm = -nrm;
        }
        out = clip(&out, a - nrm * d, nrm);
        if out.len() < 3 {
            return Vec::new();
        }
    }
    out
}

fn on_edge(p: Vec2, q: Vec2, a: Vec2, b: Vec2) -> bool {
    let d = b - a;
    let len2 = d.length_squared();
    let near = |x: Vec2| {
        let t = (x - a).dot(d) / len2;
        (-1e-3..=1.0 + 1e-3).contains(&t) && (a + d * t).distance(x) < 0.05
    };
    near(p) && near(q)
}

/// Cuts `block` into lots along its first street edge with frontages drawn
/// from `width` (the last lot absorbs the remainder).
pub fn subdivide_block(block: &Lot, width: [f32; 2], rng: &mut Rng) -> Vec<Lot> {
    let n = block.poly.len();
    let Some(front) = (0..n).max_by(|i, j| {
        let len = |k: usize| if block.street[k] { block.poly[k].distance(block.poly[(k + 1) % n]) } else { 0.0 };
        len(*i).total_cmp(&len(*j))
    }) else {
        return Vec::new();
    };
    let (a, b) = (block.poly[front], block.poly[(front + 1) % n]);
    let dir = (b - a).normalize();
    // Extent of the block along the front direction.
    let (lo, hi) = block
        .poly
        .iter()
        .map(|p| (*p - a).dot(dir))
        .fold((f32::MAX, f32::MIN), |(l, h), t| (l.min(t), h.max(t)));
    let mut cuts = Vec::new();
    let mut t = lo;
    loop {
        let w = rng.range(width[0], width[1]);
        if t + w + width[0] * 0.75 > hi {
            break;
        }
        t += w;
        cuts.push(t);
    }
    let mut lots = Vec::new();
    let mut rest = block.poly.clone();
    for c in cuts.iter().chain(std::iter::once(&f32::MAX)) {
        let piece = if *c == f32::MAX {
            rest.clone()
        } else {
            let origin = a + dir * *c;
            let piece = clip(&rest, origin, dir);
            rest = clip(&rest, origin, -dir);
            piece
        };
        if piece.len() < 3 {
            continue;
        }
        let m = piece.len();
        let street = (0..m)
            .map(|i| {
                let (p, q) = (piece[i], piece[(i + 1) % m]);
                (0..n).any(|e| block.street[e] && on_edge(p, q, block.poly[e], block.poly[(e + 1) % n]))
            })
            .collect();
        lots.push(Lot { poly: piece, street });
    }
    lots
}

/// Material tints of one building (sRGB 0..1).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tints {
    pub wall: Rgb,
    pub trim: Rgb,
    pub glass: Rgb,
    pub awning: Rgb,
}

/// One assembled building: meshes per slot plus its tints.
#[derive(Clone, Debug)]
pub struct Building {
    pub name: String,
    pub style: Style,
    pub wall: Wall,
    pub parts: Parts,
    pub tints: Tints,
    pub height: f32,
    pub footprint: Lot,
}

/// Window-atlas cell rectangle (lit cells are rows 0-2, dark row 3).
fn window_cell(rng: &mut Rng, lit_fraction: f32) -> (Vec2, Vec2) {
    let lit = rng.chance(lit_fraction);
    let (cx, cy) = (rng.index(4) as f32, if lit { rng.index(3) as f32 } else { 3.0 });
    let inset = 0.006;
    let min = Vec2::new(cx, cy) * 0.25 + Vec2::splat(inset);
    (min, min + Vec2::splat(0.25 - 2.0 * inset))
}

/// Quad in façade space facing +Z (outward) at depth `z`, UV (x, −y) metres.
fn wall_quad(m: &mut MeshData, x0: f32, x1: f32, y0: f32, y1: f32, z: f32) {
    if x1 - x0 < 1e-4 || y1 - y0 < 1e-4 {
        return;
    }
    m.quad_n(
        [Vec3::new(x0, y0, z), Vec3::new(x1, y0, z), Vec3::new(x1, y1, z), Vec3::new(x0, y1, z)],
        [Vec2::new(x0, -y0), Vec2::new(x1, -y0), Vec2::new(x1, -y1), Vec2::new(x0, -y1)],
        Vec3::Z,
    );
}

/// Quad facing +Z with explicit atlas UVs.
fn atlas_quad(m: &mut MeshData, x0: f32, x1: f32, y0: f32, y1: f32, z: f32, rect: (Vec2, Vec2)) {
    let (a, b) = rect;
    m.quad_n(
        [Vec3::new(x0, y0, z), Vec3::new(x1, y0, z), Vec3::new(x1, y1, z), Vec3::new(x0, y1, z)],
        [Vec2::new(a.x, b.y), Vec2::new(b.x, b.y), Vec2::new(b.x, a.y), Vec2::new(a.x, a.y)],
        Vec3::Z,
    );
}

/// A wall cell `x0..x1 × y0..y1` with a recessed opening; the reveals use
/// the wall slot, the back of the recess gets `glass` with atlas UVs.
#[allow(clippy::too_many_arguments)]
fn punched(
    p: &mut Parts,
    wall: Slot,
    cell: (f32, f32, f32, f32),
    hole: (f32, f32, f32, f32),
    depth: f32,
    glass: Slot,
    rect: (Vec2, Vec2),
    frame: bool,
) {
    let (x0, x1, y0, y1) = cell;
    let (hx0, hx1, hy0, hy1) = hole;
    let mut w = MeshData::new();
    wall_quad(&mut w, x0, hx0, y0, y1, 0.0);
    wall_quad(&mut w, hx1, x1, y0, y1, 0.0);
    wall_quad(&mut w, hx0, hx1, y0, hy0, 0.0);
    wall_quad(&mut w, hx0, hx1, hy1, y1, 0.0);
    // Reveals.
    let d = -depth;
    w.quad_n(
        [Vec3::new(hx0, hy0, 0.0), Vec3::new(hx0, hy0, d), Vec3::new(hx0, hy1, d), Vec3::new(hx0, hy1, 0.0)],
        [Vec2::new(0.0, -hy0), Vec2::new(depth, -hy0), Vec2::new(depth, -hy1), Vec2::new(0.0, -hy1)],
        Vec3::X,
    );
    w.quad_n(
        [Vec3::new(hx1, hy0, d), Vec3::new(hx1, hy0, 0.0), Vec3::new(hx1, hy1, 0.0), Vec3::new(hx1, hy1, d)],
        [Vec2::new(0.0, -hy0), Vec2::new(depth, -hy0), Vec2::new(depth, -hy1), Vec2::new(0.0, -hy1)],
        Vec3::NEG_X,
    );
    w.quad_n(
        [Vec3::new(hx0, hy0, d), Vec3::new(hx1, hy0, d), Vec3::new(hx1, hy0, 0.0), Vec3::new(hx0, hy0, 0.0)],
        [Vec2::new(hx0, 0.0), Vec2::new(hx1, 0.0), Vec2::new(hx1, depth), Vec2::new(hx0, depth)],
        Vec3::Y,
    );
    w.quad_n(
        [Vec3::new(hx0, hy1, 0.0), Vec3::new(hx1, hy1, 0.0), Vec3::new(hx1, hy1, d), Vec3::new(hx0, hy1, d)],
        [Vec2::new(hx0, 0.0), Vec2::new(hx1, 0.0), Vec2::new(hx1, depth), Vec2::new(hx0, depth)],
        Vec3::NEG_Y,
    );
    p.add(wall, w);
    let mut g = MeshData::new();
    atlas_quad(&mut g, hx0, hx1, hy0, hy1, d, rect);
    p.add(glass, g);
    if frame {
        // Window frame: mullion and transom bars proud of the glass.
        let (cx, ty) = ((hx0 + hx1) / 2.0, hy0 + (hy1 - hy0) * 0.68);
        p.add_closed(Slot::Mullion, bx(Vec3::new(cx - 0.035, hy0, d), Vec3::new(cx + 0.035, hy1, d + 0.06)));
        p.add_closed(Slot::Mullion, bx(Vec3::new(hx0, ty - 0.035, d), Vec3::new(hx1, ty + 0.035, d + 0.06)));
    }
}

/// Façade parameters drawn for one building.
struct Spec<'a> {
    arch: &'a Archetype,
    style: Style,
    wall: Slot,
    ground: f32,
    floor_h: f32,
    floors: u32,
    shopfront: bool,
    awning: bool,
    fire_escape: bool,
    lit: f32,
}

/// Street façade of `len` metres in façade space.
fn street_facade(p: &mut Parts, spec: &Spec, len: f32, y_top: f32, district: &District, rng: &mut Rng) {
    let g = spec.ground;
    // Ground floor.
    if spec.shopfront && len > 3.0 {
        let n = (len / rng.range(district.ground.bay_width[0], district.ground.bay_width[1])).round().max(1.0) as usize;
        let bay = len / n as f32;
        for k in 0..n {
            let (x0, x1) = (k as f32 * bay, (k + 1) as f32 * bay);
            shopfront_bay(p, spec, x0, x1, g, spec.awning, rng);
        }
    } else {
        let mut w = MeshData::new();
        wall_quad(&mut w, 0.0, len, 0.0, g, 0.0);
        p.add(spec.wall, w);
    }
    // String course over the ground floor.
    p.add_closed(Slot::Stone, bx(Vec3::new(-0.05, g - 0.05, 0.0), Vec3::new(len + 0.05, g + 0.3, 0.16)));

    match spec.style {
        Style::Curtain => curtain_bays(p, 0.0, len, g + 0.3, y_top, spec.floor_h, spec.lit, rng),
        Style::Masonry | Style::Loft => {
            let (bw0, bw1) = (spec.arch.bay_width[0], spec.arch.bay_width[1]);
            let margin = 0.7;
            let usable = (len - 2.0 * margin).max(0.0);
            let n = (usable / rng.range(bw0, bw1)).round() as usize;
            let mut w = MeshData::new();
            // Corner piers.
            wall_quad(&mut w, 0.0, margin.min(len), g + 0.3, y_top, 0.0);
            wall_quad(&mut w, (len - margin).max(0.0), len, g + 0.3, y_top, 0.0);
            if n == 0 {
                wall_quad(&mut w, margin, len - margin, g + 0.3, y_top, 0.0);
            }
            p.add(spec.wall, w);
            if n == 0 {
                return;
            }
            let bay = usable / n as f32;
            let [wf, hf] = spec.arch.window;
            let depth = if spec.style == Style::Loft { 0.28 } else { 0.22 };
            for f in 0..spec.floors.saturating_sub(1) {
                let y0 = g + 0.3 + f as f32 * spec.floor_h;
                let y1 = (y0 + spec.floor_h).min(y_top);
                if y1 - y0 < 1.0 {
                    let mut w = MeshData::new();
                    wall_quad(&mut w, margin, len - margin, y0, y1, 0.0);
                    p.add(spec.wall, w);
                    continue;
                }
                for k in 0..n {
                    let x0 = margin + k as f32 * bay;
                    let x1 = x0 + bay;
                    let ww = bay * wf;
                    let wh = spec.floor_h * hf;
                    let (hx0, hx1) = ((x0 + x1 - ww) / 2.0, (x0 + x1 + ww) / 2.0);
                    let hy0 = y0 + 0.85;
                    let hy1 = (hy0 + wh).min(y1 - 0.3);
                    let rect = window_cell(rng, spec.lit);
                    punched(p, spec.wall, (x0, x1, y0, y1), (hx0, hx1, hy0, hy1), depth, Slot::Window, rect, true);
                    // Sill and lintel.
                    p.add_closed(Slot::Stone, bx(Vec3::new(hx0 - 0.08, hy0 - 0.07, -0.02), Vec3::new(hx1 + 0.08, hy0, 0.07)));
                    let lintel_h = if spec.style == Style::Loft { 0.28 } else { 0.18 };
                    p.add_closed(Slot::Stone, bx(Vec3::new(hx0 - 0.1, hy1, -0.01), Vec3::new(hx1 + 0.1, hy1 + lintel_h, 0.04)));
                }
            }
            if spec.fire_escape && n >= 2 {
                let k = rng.index(n - 1);
                let (x0, x1) = (margin + k as f32 * bay + 0.2, margin + (k + 2) as f32 * bay - 0.2);
                fire_escape(p, x0, x1, g + 0.3, spec.floor_h, spec.floors.saturating_sub(1));
            }
        }
    }
}

/// One ground-floor shop bay: pilasters, stall riser, recessed glazing with
/// glazing bars, fascia with a sign board, optional awning.
fn shopfront_bay(p: &mut Parts, spec: &Spec, x0: f32, x1: f32, g: f32, awning: bool, rng: &mut Rng) {
    let pil = 0.22;
    p.add_closed(Slot::Stone, bx(Vec3::new(x0, 0.0, -0.05), Vec3::new(x0 + pil, g, 0.14)));
    p.add_closed(Slot::Stone, bx(Vec3::new(x1 - pil, 0.0, -0.05), Vec3::new(x1, g, 0.14)));
    let (gx0, gx1) = (x0 + pil, x1 - pil);
    let fascia_y0 = g - 1.15;
    let (rect_a, rect_b) = window_cell(rng, 0.85);
    // Brighter shop interiors: shift into the lit rows only.
    let rect = (rect_a, rect_b);
    p.add_closed(Slot::Stone, bx(Vec3::new(gx0, 0.0, -0.3), Vec3::new(gx1, 0.5, 0.02)));
    punched(p, spec.wall, (gx0, gx1, 0.5, fascia_y0), (gx0, gx1, 0.5, fascia_y0 - 0.12), 0.3, Slot::ShopGlass, rect, false);
    let mut w = MeshData::new();
    wall_quad(&mut w, gx0, gx1, fascia_y0 - 0.12, fascia_y0, 0.0);
    p.add(spec.wall, w);
    // Glazing bars every ~1.4 m.
    let nb = ((gx1 - gx0) / 1.4).round().max(1.0) as usize;
    for k in 1..nb {
        let x = gx0 + (gx1 - gx0) * k as f32 / nb as f32;
        p.add_closed(Slot::Mullion, bx(Vec3::new(x - 0.04, 0.5, -0.3), Vec3::new(x + 0.04, fascia_y0 - 0.12, -0.24)));
    }
    p.add_closed(Slot::Mullion, bx(Vec3::new(gx0, fascia_y0 - 0.55, -0.3), Vec3::new(gx1, fascia_y0 - 0.5, -0.24)));
    // Fascia board with a sign.
    p.add_closed(Slot::Stone, bx(Vec3::new(gx0, fascia_y0, 0.0), Vec3::new(gx1, g - 0.05, 0.12)));
    let cell = rng.index(16) as u32;
    let (a, b) = crate::texture::sign_cell_rect(cell);
    let mut s = MeshData::new();
    let sw = ((gx1 - gx0) * 0.8).min(5.0);
    let cx = (gx0 + gx1) / 2.0;
    atlas_quad(&mut s, cx - sw / 2.0, cx + sw / 2.0, fascia_y0 + 0.12, g - 0.17, 0.125, (a, b));
    p.add(Slot::ShopSign, s);
    if awning {
        let (y_top, out, drop) = (fascia_y0 - 0.05, 1.5, 0.7);
        let mut m = MeshData::new();
        let top = [
            Vec3::new(gx0, y_top, 0.0),
            Vec3::new(gx1, y_top, 0.0),
            Vec3::new(gx1, y_top - drop, out),
            Vec3::new(gx0, y_top - drop, out),
        ];
        m.quad([top[0], top[3], top[2], top[1]], [Vec2::new(0.0, 0.0), Vec2::new(0.0, 1.6), Vec2::new(gx1 - gx0, 1.6), Vec2::new(gx1 - gx0, 0.0)]);
        // Valance.
        m.quad_n(
            [Vec3::new(gx0, y_top - drop - 0.25, out), Vec3::new(gx1, y_top - drop - 0.25, out), Vec3::new(gx1, y_top - drop, out), Vec3::new(gx0, y_top - drop, out)],
            [Vec2::new(0.0, 0.25), Vec2::new(gx1 - gx0, 0.25), Vec2::new(gx1 - gx0, 0.0), Vec2::new(0.0, 0.0)],
            Vec3::Z,
        );
        p.add(Slot::Awning, m);
    }
}

/// Curtain wall: per floor a spandrel band and vision glass in 1.5 m bays
/// between projecting mullion fins; some bays show lit or dark interiors.
#[allow(clippy::too_many_arguments)]
fn curtain_bays(p: &mut Parts, x0: f32, x1: f32, y0: f32, y1: f32, floor_h: f32, lit: f32, rng: &mut Rng) {
    let n = ((x1 - x0) / 1.5).round().max(1.0) as usize;
    let bay = (x1 - x0) / n as f32;
    let floors = ((y1 - y0) / floor_h).floor().max(1.0) as usize;
    let mut glass = MeshData::new();
    let mut spandrel = MeshData::new();
    let mut interior = MeshData::new();
    for f in 0..floors {
        let fy0 = y0 + f as f32 * floor_h;
        let fy1 = if f + 1 == floors { y1 } else { fy0 + floor_h };
        wall_quad(&mut spandrel, x0, x1, fy0, fy0 + 0.9, -0.05);
        // Interior rows are drawn per floor so whole floors light up together.
        let floor_lit = rng.chance(0.55);
        for k in 0..n {
            let (bx0, bx1) = (x0 + k as f32 * bay, x0 + (k + 1) as f32 * bay);
            if rng.chance(if floor_lit { 0.6 } else { 0.12 }) {
                let rect = window_cell(rng, if floor_lit { lit.max(0.7) } else { 0.0 });
                atlas_quad(&mut interior, bx0, bx1, fy0 + 0.9, fy1, -0.06, rect);
            } else {
                wall_quad(&mut glass, bx0, bx1, fy0 + 0.9, fy1, -0.06);
            }
        }
        // Floor-edge transom.
        p.add_closed(Slot::Mullion, bx(Vec3::new(x0, fy0 + 0.86, -0.06), Vec3::new(x1, fy0 + 0.94, 0.04)));
    }
    p.add(Slot::CurtainGlass, glass);
    p.add(Slot::Mullion, spandrel);
    p.add(Slot::Window, interior);
    for k in 0..=n {
        let x = x0 + k as f32 * bay;
        p.add_closed(Slot::Mullion, bx(Vec3::new(x - 0.04, y0, -0.06), Vec3::new(x + 0.04, y1, 0.18)));
    }
}

/// Steel fire escape: landings with railings at each floor and stairs.
fn fire_escape(p: &mut Parts, x0: f32, x1: f32, y0: f32, floor_h: f32, floors: u32) {
    let depth = 1.2;
    for f in 0..floors {
        let y = y0 + f as f32 * floor_h + 0.05;
        p.add_closed(Slot::DarkMetal, bx(Vec3::new(x0, y, 0.0), Vec3::new(x1, y + 0.06, depth)));
        p.add_closed(Slot::DarkMetal, bx(Vec3::new(x0, y + 0.95, depth - 0.04), Vec3::new(x1, y + 1.0, depth)));
        let posts = ((x1 - x0) / 0.8).ceil() as usize;
        for k in 0..=posts {
            let x = x0 + (x1 - x0) * k as f32 / posts as f32;
            p.add_closed(Slot::DarkMetal, bx(Vec3::new(x - 0.02, y, depth - 0.04), Vec3::new(x + 0.02, y + 0.95, depth)));
        }
        if f + 1 < floors {
            let (sx0, sx1) = if f % 2 == 0 { (x0 + 0.3, x1 - 0.5) } else { (x1 - 0.3, x0 + 0.5) };
            p.add_closed(
                Slot::DarkMetal,
                MeshData::tube(Vec3::new(sx0, y + 0.06, depth * 0.5), Vec3::new(sx1, y + floor_h, depth * 0.5), 0.05, 4, true),
            );
        }
    }
}

/// Façade-space transform for the edge `a → b` with outward normal `n`.
/// The frame is always right-handed (x along, y up, z out), so text and
/// asymmetric modules are never mirrored; the edge is walked from whichever
/// end makes that true.
fn facade_frame(a: Vec2, b: Vec2, n: Vec2) -> (Affine3A, f32) {
    let len = a.distance(b);
    let mut d = (b - a).normalize();
    let mut a = a;
    // det(d, Y, n) = d · (Y × n); Y × (nx, 0, nz) = (nz, 0, -nx).
    if d.x * n.y - d.y * n.x < 0.0 {
        a = b;
        d = -d;
    }
    (
        Affine3A::from_cols(
            Vec3::new(d.x, 0.0, d.y).into(),
            Vec3::Y.into(),
            Vec3::new(n.x, 0.0, n.y).into(),
            Vec3::new(a.x, 0.0, a.y).into(),
        ),
        len,
    )
}

/// Outward normal of edge `i` of a convex polygon.
fn outward(poly: &[Vec2], i: usize) -> Vec2 {
    let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
    let d = (b - a).normalize_or_zero();
    let n = Vec2::new(d.y, -d.x);
    if n.dot((a + b) / 2.0 - c) < 0.0 { -n } else { n }
}

/// Roof slab over a convex polygon at `y`, UVs in metres.
fn roof_slab(poly: &[Vec2], y: f32) -> MeshData {
    let mut m = MeshData::new();
    let c = poly.iter().copied().sum::<Vec2>() / poly.len() as f32;
    let ci = m.vertex(Vec3::new(c.x, y, c.y), Vec3::Y, c);
    let first = m.positions.len() as u32;
    for p in poly {
        m.vertex(Vec3::new(p.x, y, p.y), Vec3::Y, *p);
    }
    let n = poly.len() as u32;
    for i in 0..n {
        m.tri(ci, first + i, first + (i + 1) % n);
    }
    m
}

/// Parapet and coping around the roof edge.
fn parapet(p: &mut Parts, wall: Slot, poly: &[Vec2], y: f32, h: f32) {
    for i in 0..poly.len() {
        let (t, len) = facade_frame(poly[i], poly[(i + 1) % poly.len()], outward(poly, i));
        let mut m = Parts::new();
        let mut w = MeshData::new();
        wall_quad(&mut w, 0.0, len, y, y + h, 0.0);
        m.add(wall, w);
        m.add_closed(Slot::Stone, bx(Vec3::new(-0.05, y + h, -0.3), Vec3::new(len + 0.05, y + h + 0.08, 0.04)));
        let mut inner = MeshData::new();
        wall_quad(&mut inner, 0.0, len, y, y + h, 0.0);
        let inner = inner.transformed(Affine3A::from_rotation_y(std::f32::consts::PI)).translated(Vec3::new(len, 0.0, -0.25));
        m.add(wall, inner);
        p.merge(&m, t);
    }
}

/// Classical cornice along an edge (profile extruded along the façade).
fn cornice(p: &mut Parts, poly: &[Vec2], i: usize, y: f32, big: bool) {
    let (t, len) = facade_frame(poly[i], poly[(i + 1) % poly.len()], outward(poly, i));
    let (proj, h) = if big { (0.75, 0.9) } else { (0.35, 0.5) };
    // Profile in (z, y): counter-clockwise seen from +X.
    let prof = [
        Vec2::new(0.0, y - h),
        Vec2::new(0.12, y - h),
        Vec2::new(0.12, y - h * 0.7),
        Vec2::new(proj * 0.55, y - h * 0.45),
        Vec2::new(proj, y - h * 0.2),
        Vec2::new(proj, y),
        Vec2::new(0.0, y),
    ];
    let m = extrude_x(&prof, len + 2.0 * proj).translated(Vec3::new(len / 2.0, 0.0, 0.0));
    p.add_closed(Slot::Stone, m.transformed(t));
}

/// Rooftop clutter: AC units, a stair/lift bulkhead, a water tower (lofts).
fn rooftop(p: &mut Parts, wall: Slot, poly: &[Vec2], y: f32, style: Style, district: &District, rng: &mut Rng) {
    let inner = inset(poly, 2.0);
    if inner.len() < 3 {
        return;
    }
    let lot = Lot { poly: inner.clone(), street: vec![false; inner.len()] };
    let area = lot.area();
    let (lo, hi) = inner.iter().fold((Vec2::MAX, Vec2::MIN), |(l, h), q| (l.min(*q), h.max(*q)));
    let inside = |q: Vec2| {
        let n = inner.len();
        (0..n).all(|i| (q - inner[i]).dot(outward(&inner, i)) <= 0.0)
    };
    let sample = |rng: &mut Rng| {
        for _ in 0..20 {
            let q = Vec2::new(rng.range(lo.x, hi.x), rng.range(lo.y, hi.y));
            if inside(q) {
                return Some(q);
            }
        }
        None
    };
    let units = (area / 100.0 * district.roof.ac_units_per_100m2).round() as usize;
    for _ in 0..units {
        if let Some(q) = sample(rng) {
            let (w, d, h) = (rng.range(1.0, 1.6), rng.range(1.6, 2.6), rng.range(0.9, 1.4));
            p.add_closed(Slot::RoofMetal, bx(Vec3::new(q.x - w / 2.0, y, q.y - d / 2.0), Vec3::new(q.x + w / 2.0, y + h, q.y + d / 2.0)));
            p.add_closed(Slot::DarkMetal, MeshData::frustum(0.35, 0.35, 0.05, 10, true).translated(Vec3::new(q.x, y + h, q.y)));
        }
    }
    if rng.chance(district.roof.bulkhead)
        && let Some(q) = sample(rng)
    {
        let (w, d, h) = (rng.range(3.0, 5.0), rng.range(3.0, 4.5), rng.range(2.8, 3.6));
        let b = bx(Vec3::new(q.x - w / 2.0, y, q.y - d / 2.0), Vec3::new(q.x + w / 2.0, y + h, q.y + d / 2.0));
        p.add_closed(if style == Style::Curtain { Slot::RoofMetal } else { wall }, b);
        p.add_closed(Slot::Roof, bx(Vec3::new(q.x - w / 2.0 - 0.1, y + h, q.y - d / 2.0 - 0.1), Vec3::new(q.x + w / 2.0 + 0.1, y + h + 0.1, q.y + d / 2.0 + 0.1)));
    }
    if style == Style::Loft
        && rng.chance(district.roof.water_tower)
        && let Some(q) = sample(rng)
    {
        let legs = 3.2;
        let r = rng.range(1.6, 2.2);
        for (dx, dz) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let base = Vec3::new(q.x + dx * r * 0.6, y, q.y + dz * r * 0.6);
            p.add_closed(Slot::DarkMetal, MeshData::tube(base, base + Vec3::Y * legs, 0.08, 6, true));
        }
        p.add_closed(Slot::DarkMetal, bx(Vec3::new(q.x - r, y + legs - 0.1, q.y - r), Vec3::new(q.x + r, y + legs, q.y + r)));
        let tank_h = rng.range(3.5, 4.5);
        p.add_closed(Slot::Wood, MeshData::frustum(r, r * 0.97, tank_h, 16, true).translated(Vec3::new(q.x, y + legs, q.y)));
        p.add_closed(Slot::DarkMetal, MeshData::frustum(r * 1.05, 0.15, r * 0.6, 16, true).translated(Vec3::new(q.x, y + legs + tank_h, q.y)));
        for k in 0..3 {
            let hy = y + legs + 0.6 + k as f32 * tank_h / 3.0;
            p.add_closed(Slot::DarkMetal, MeshData::frustum(r + 0.03, r + 0.03, 0.06, 16, true).translated(Vec3::new(q.x, hy, q.y)));
        }
    }
}

fn pick<T: Copy>(list: &[T], rng: &mut Rng) -> T {
    list[rng.index(list.len())]
}

/// Picks an archetype for a lot by weight, respecting `min_lot_width`.
pub fn pick_archetype<'a>(district: &'a District, frontage: f32, rng: &mut Rng) -> &'a Archetype {
    let weights: Vec<f32> = district
        .archetypes
        .iter()
        .map(|a| if frontage >= a.min_lot_width { a.weight } else { 0.0 })
        .collect();
    &district.archetypes[rng.weighted(&weights)]
}

/// Assembles one building on `lot` with archetype `arch`.
pub fn build(lot: &Lot, arch: &Archetype, district: &District, seed: u64, name: String) -> Building {
    let mut rng = Rng::new(seed);
    let floors = arch.floors[0] + rng.index((arch.floors[1] - arch.floors[0] + 1) as usize) as u32;
    let wall = match arch.wall {
        Wall::Brick => Slot::Brick,
        Wall::Plaster => Slot::Plaster,
    };
    let pal = &district.palette;
    let tints = Tints {
        wall: if arch.wall == Wall::Brick { pick(&pal.brick, &mut rng) } else { pick(&pal.plaster, &mut rng) },
        trim: pick(&pal.stone, &mut rng),
        glass: pick(&pal.glass, &mut rng),
        awning: pick(&pal.awning, &mut rng),
    };
    let lit = rng.range(0.35, 0.7);
    let mut parts = Parts::new();
    let poly = &lot.poly;
    let n = poly.len();
    let g = arch.ground_floor;
    let (podium_floors, tower) = if arch.style == Style::Curtain {
        let pf = arch.podium_floors[0] + rng.index((arch.podium_floors[1] - arch.podium_floors[0] + 1) as usize) as u32;
        (pf.max(1), true)
    } else {
        (floors, false)
    };
    // Base (whole building for masonry/loft, podium for towers).
    let base_style = if tower { Style::Masonry } else { arch.style };
    let base_floors = podium_floors;
    let base_top = g + 0.3 + (base_floors.saturating_sub(1)) as f32 * arch.floor_height;
    let spec = Spec {
        arch,
        style: base_style,
        wall,
        ground: g,
        floor_h: arch.floor_height,
        floors: base_floors,
        shopfront: rng.chance(district.ground.shopfront),
        awning: rng.chance(district.ground.awning),
        fire_escape: rng.chance(arch.fire_escapes),
        lit,
    };
    for i in 0..n {
        let (t, len) = facade_frame(poly[i], poly[(i + 1) % n], outward(poly, i));
        let mut f = Parts::new();
        if lot.street[i] {
            street_facade(&mut f, &spec, len, base_top, district, &mut rng);
        } else {
            let mut w = MeshData::new();
            wall_quad(&mut w, 0.0, len, 0.0, base_top, 0.0);
            f.add(wall, w);
        }
        parts.merge(&f, t);
        if lot.street[i] && !tower {
            cornice(&mut parts, poly, i, base_top, base_style == Style::Masonry);
        }
    }
    if tower {
        for i in (0..n).filter(|i| lot.street[*i]) {
            cornice(&mut parts, poly, i, base_top, false);
        }
    }
    parapet(&mut parts, wall, poly, base_top, if tower { 0.6 } else { 1.0 });
    parts.add(Slot::Roof, roof_slab(poly, base_top + 0.02));
    let mut height = base_top + 1.0;
    if tower {
        let tp = inset(poly, arch.setback);
        if tp.len() >= 3 {
            let top = base_top + (floors - podium_floors) as f32 * arch.floor_height;
            let m = tp.len();
            for i in 0..m {
                let (t, len) = facade_frame(tp[i], tp[(i + 1) % m], outward(&tp, i));
                let mut f = Parts::new();
                curtain_bays(&mut f, 0.0, len, base_top, top, arch.floor_height, lit, &mut rng);
                // Crown: a tall glass parapet band above the last floor.
                f.add_closed(Slot::Mullion, bx(Vec3::new(0.0, top, -0.2), Vec3::new(len, top + 2.5, 0.05)));
                parts.merge(&f, t);
            }
            parts.add(Slot::Roof, roof_slab(&tp, top + 0.05));
            rooftop(&mut parts, wall, &tp, top + 0.05, Style::Curtain, district, &mut rng);
            height = top + 2.5;
        }
    } else {
        rooftop(&mut parts, wall, poly, base_top + 0.02, arch.style, district, &mut rng);
    }
    Building { name, style: arch.style, wall: arch.wall, parts, tints, height, footprint: lot.clone() }
}

/// Subdivides `block` and builds every lot; deterministic per `seed`.
pub fn assemble_block(block: &Lot, district: &District, seed: u64, prefix: &str) -> Vec<Building> {
    let mut rng = Rng::new(seed ^ district.seed);
    let lots = subdivide_block(block, district.lot_width, &mut rng);
    let mut last: Option<String> = None;
    lots.iter()
        .enumerate()
        .map(|(i, lot)| {
            let mut arch = pick_archetype(district, lot.frontage(), &mut rng);
            if last.as_deref() == Some(arch.name.as_str()) {
                arch = pick_archetype(district, lot.frontage(), &mut rng);
            }
            last = Some(arch.name.clone());
            build(lot, arch, district, rng.next_u32() as u64, format!("bldg_{prefix}_{i}"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::{Budget, check_asset};

    fn block() -> Lot {
        Lot {
            poly: vec![Vec2::new(0.0, 0.0), Vec2::new(80.0, 0.0), Vec2::new(80.0, 30.0), Vec2::new(0.0, 30.0)],
            street: vec![true, false, false, true],
        }
    }

    #[test]
    fn clip_and_inset_convex() {
        let sq = block().poly;
        let half = clip(&sq, Vec2::new(40.0, 0.0), Vec2::X);
        let lot = Lot { street: vec![false; half.len()], poly: half };
        assert!((lot.area() - 1200.0).abs() < 1e-2);
        let small = inset(&sq, 5.0);
        let lot = Lot { street: vec![false; small.len()], poly: small };
        assert!((lot.area() - 70.0 * 20.0).abs() < 1e-2);
    }

    #[test]
    fn subdivision_covers_the_block() {
        let b = block();
        let d = District::downtown();
        let lots = subdivide_block(&b, d.lot_width, &mut Rng::new(3));
        assert!(lots.len() >= 3);
        let total: f32 = lots.iter().map(Lot::area).sum();
        assert!((total - b.area()).abs() < 0.5, "{total}");
        // Every lot keeps street frontage on the long edge.
        assert!(lots.iter().all(|l| l.frontage() > 5.0));
        // The corner lot also fronts the side street.
        assert!(lots[0].street.iter().filter(|s| **s).count() >= 2);
    }

    #[test]
    fn buildings_are_deterministic_finite_and_in_budget() {
        let d = District::downtown();
        let a = assemble_block(&block(), &d, 5, "t");
        let b = assemble_block(&block(), &d, 5, "t");
        assert_eq!(a.len(), b.len());
        for (x, y) in a.iter().zip(&b) {
            assert_eq!(x.height, y.height);
            for (s, m) in &x.parts.by_slot {
                assert_eq!(m.content_hash(), y.parts.by_slot[s].content_hash(), "{s:?}");
            }
            let f = check_asset(&x.name, &[x.parts.clone().into_parts()], Budget { max_triangles: 80_000, lod_ratio: 1.0 });
            assert!(f.errors.is_empty(), "{:?}", f.errors);
            assert!(x.height > 8.0 && x.height < 200.0);
        }
        let c = assemble_block(&block(), &d, 6, "t");
        assert!(a.len() != c.len() || a.iter().zip(&c).any(|(x, y)| x.height != y.height));
    }

    #[test]
    fn every_archetype_builds() {
        let d = District::downtown();
        let lot = Lot {
            poly: vec![Vec2::new(0.0, 0.0), Vec2::new(30.0, 0.0), Vec2::new(30.0, 25.0), Vec2::new(0.0, 25.0)],
            street: vec![true, false, false, true],
        };
        for (i, arch) in d.archetypes.iter().enumerate() {
            let b = build(&lot, arch, &d, i as u64, format!("bldg_test_{i}"));
            assert!(b.parts.triangles() > 500, "{} {}", arch.name, b.parts.triangles());
            for slot in [Slot::Roof, Slot::Stone] {
                assert!(b.parts.by_slot.contains_key(&slot), "{} lacks {slot:?}", arch.name);
            }
        }
    }
}
