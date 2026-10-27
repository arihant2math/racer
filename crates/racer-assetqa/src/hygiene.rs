//! Gate 3: mesh hygiene (budgets, LODs, topology, normals, UVs, collision).

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use crate::{
    model::{Model, Prim},
    raster::{self, Grid, View},
    report::Gate,
    thresholds::Thresholds,
};

/// Positions closer than this are welded into one vertex for topology checks
/// (glTF splits vertices at UV and normal seams).
const WELD: f32 = 1e-5;

/// Geometry of a part with seam-split vertices welded by position.
pub struct Welded {
    pub positions: Vec<Vec3>,
    pub tris: Vec<[u32; 3]>,
}

impl Welded {
    pub fn new<'a>(prims: impl IntoIterator<Item = &'a Prim>) -> Welded {
        let mut map: HashMap<[i64; 3], u32> = HashMap::new();
        let mut positions = Vec::new();
        let mut tris = Vec::new();
        for p in prims {
            let ids: Vec<u32> = p
                .positions
                .iter()
                .map(|v| {
                    let key = (*v / WELD).round().as_i64vec3().to_array();
                    *map.entry(key).or_insert_with(|| {
                        positions.push(*v);
                        positions.len() as u32 - 1
                    })
                })
                .collect();
            tris.extend(p.tris.iter().map(|t| t.map(|v| ids[v as usize])));
        }
        Welded { positions, tris }
    }

    pub fn tri(&self, t: &[u32; 3]) -> [Vec3; 3] {
        t.map(|v| self.positions[v as usize])
    }

    /// Triangles that collapse when welded or have (almost) no area.
    pub fn degenerate(&self, min_area: f32) -> usize {
        self.tris
            .iter()
            .filter(|t| {
                let [a, b, c] = self.tri(t);
                t[0] == t[1] || t[1] == t[2] || t[0] == t[2] || area(a, b, c) < min_area
            })
            .count()
    }

    pub fn edges(&self) -> EdgeStats {
        // Edge (lo, hi) -> (uses, orientation sum).
        let mut map: HashMap<(u32, u32), (u32, i32)> = HashMap::new();
        for t in &self.tris {
            if t[0] == t[1] || t[1] == t[2] || t[0] == t[2] {
                continue;
            }
            for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])] {
                let e = map.entry((a.min(b), a.max(b))).or_default();
                e.0 += 1;
                e.1 += if a < b { 1 } else { -1 };
            }
        }
        let mut s = EdgeStats {
            edges: map.len(),
            ..EdgeStats::default()
        };
        for (&(a, b), &(uses, orient)) in &map {
            match uses {
                1 => {
                    s.boundary += 1;
                    s.boundary_length +=
                        self.positions[a as usize].distance(self.positions[b as usize]);
                }
                2 if orient != 0 => s.inconsistent += 1,
                2 => {}
                _ => s.non_manifold += 1,
            }
        }
        s
    }

    /// Triangles grouped into connected pieces (sharing a vertex).
    pub fn components(&self) -> Vec<Vec<usize>> {
        let mut parent: Vec<u32> = (0..self.positions.len() as u32).collect();
        fn find(p: &mut [u32], mut x: u32) -> u32 {
            while p[x as usize] != x {
                p[x as usize] = p[p[x as usize] as usize];
                x = p[x as usize];
            }
            x
        }
        for t in &self.tris {
            let r0 = find(&mut parent, t[0]);
            for &v in &t[1..] {
                let r = find(&mut parent, v);
                parent[r as usize] = r0;
            }
        }
        let mut groups: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, t) in self.tris.iter().enumerate() {
            groups.entry(find(&mut parent, t[0])).or_default().push(i);
        }
        let mut out: Vec<Vec<usize>> = groups.into_values().collect();
        out.sort_by_key(|g| g[0]);
        out
    }

    /// Signed volume of the given triangles (positive if wound outwards).
    pub fn volume(&self, tris: &[usize]) -> f32 {
        tris.iter()
            .map(|&i| {
                let [a, b, c] = self.tri(&self.tris[i]);
                a.dot(b.cross(c)) / 6.0
            })
            .sum()
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EdgeStats {
    pub edges: usize,
    pub boundary: usize,
    pub non_manifold: usize,
    /// Manifold edges whose two triangles traverse it in the same direction.
    pub inconsistent: usize,
    pub boundary_length: f32,
}

pub fn area(a: Vec3, b: Vec3, c: Vec3) -> f32 {
    (b - a).cross(c - a).length() / 2.0
}

fn pct(x: f64) -> String {
    format!("{:.3} %", x * 100.0)
}

/// Singular values (max, min) of the surface→UV map of a triangle.
fn uv_singular_values(p: [Vec3; 3], uv: [Vec2; 3]) -> Option<(f32, f32)> {
    let e1 = p[1] - p[0];
    let e2 = p[2] - p[0];
    let l1 = e1.length();
    let x = e1 / l1;
    let n = e1.cross(e2);
    if n.length_squared() < 1e-24 {
        return None;
    }
    let y = n.cross(e1).normalize();
    // Triangle in its own 2D frame: (0,0), (l1, 0), (px, py).
    let (px, py) = (e2.dot(x), e2.dot(y));
    let q1 = uv[1] - uv[0];
    let q2 = uv[2] - uv[0];
    // J = Q · P⁻¹ with P = [[l1, px], [0, py]].
    let inv = glam::Mat2::from_cols(Vec2::new(l1, 0.0), Vec2::new(px, py)).inverse();
    let j = glam::Mat2::from_cols(q1, q2) * inv;
    let (a, b, c, d) = (j.x_axis.x, j.y_axis.x, j.x_axis.y, j.y_axis.y);
    let s1 = a * a + b * b + c * c + d * d;
    let det = a * d - b * c;
    let disc = (s1 * s1 - 4.0 * det * det).max(0.0).sqrt();
    let smax = ((s1 + disc) / 2.0).sqrt();
    let smin = ((s1 - disc) / 2.0).max(0.0).sqrt();
    Some((smax, smin))
}

/// Area-weighted median.
fn weighted_median(mut v: Vec<(f32, f32)>) -> f32 {
    v.sort_by(|a, b| a.0.total_cmp(&b.0));
    let total: f32 = v.iter().map(|x| x.1).sum();
    let mut acc = 0.0;
    for (value, w) in &v {
        acc += w;
        if acc >= total / 2.0 {
            return *value;
        }
    }
    v.last().map(|x| x.0).unwrap_or(0.0)
}

/// Side silhouette of a LOD on a fixed 1 cm grid.
fn side_silhouette(model: &Model, lod: u8, grid: &Grid) -> raster::Mask {
    let mut tris = Vec::new();
    model.for_each_triangle(model.render_nodes(lod), |t| tris.push(t));
    raster::silhouette(grid, View::Side, tris)
}

pub fn hygiene(model: &Model, t: &Thresholds) -> Gate {
    let mut g = Gate::new(3, "Mesh hygiene");
    let lod0: Vec<usize> = model.render_nodes(0).collect();
    let tris0 = model.triangles(lod0.iter().copied());
    g.check(
        "lod0_budget",
        "LOD0 triangles",
        tris0.to_string(),
        format!("{} – {}", t.lod0_triangles.0, t.lod0_triangles.1),
        tris0 >= t.lod0_triangles.0 && tris0 <= t.lod0_triangles.1,
    );

    // LOD chain.
    let mut prev = tris0;
    let bounds = model.bounds(lod0.iter().copied());
    let grid = (!bounds.is_empty()).then(|| {
        Grid::fit(
            glam::Vec2::new(bounds.min.z, bounds.min.y),
            glam::Vec2::new(bounds.max.z, bounds.max.y),
            grid_mpp(bounds.size().max_element()),
            4,
        )
    });
    let base = grid.as_ref().map(|grid| side_silhouette(model, 0, grid));
    for lod in [1u8, 2] {
        let n = model.triangles(model.render_nodes(lod));
        let ratio = if prev > 0 {
            n as f32 / prev as f32
        } else {
            0.0
        };
        g.check(
            &format!("lod{lod}_ratio"),
            format!("LOD{lod} triangles / LOD{}", lod - 1),
            format!("{n} ({ratio:.3})"),
            format!("present, {} – {}", t.lod_ratio.0, t.lod_ratio.1),
            n > 0 && ratio >= t.lod_ratio.0 && ratio <= t.lod_ratio.1,
        );
        if let (Some(grid), Some(base)) = (&grid, &base)
            && n > 0
        {
            let iou = raster::iou(base, &side_silhouette(model, lod, grid));
            g.check(
                &format!("lod{lod}_silhouette"),
                format!("LOD{lod} side silhouette IoU vs LOD0"),
                format!("{iou:.4}"),
                format!("≥ {}", t.lod_silhouette_iou),
                iou >= t.lod_silhouette_iou,
            );
        }
        prev = n;
    }
    let other: Vec<u8> = model.lods().into_iter().filter(|l| *l > 2).collect();
    if !other.is_empty() {
        g.info("lods_extra", "further LOD levels", format!("{other:?}"));
    }

    // Topology per LOD0 part (each node's own primitives).
    let mut edges = EdgeStats::default();
    let mut degenerate = 0usize;
    let mut degenerate_parts: Vec<(usize, String)> = Vec::new();
    let mut loose = 0usize;
    let mut loose_parts = Vec::new();
    let mut inconsistent_parts = Vec::new();
    let mut nm_parts = Vec::new();
    let mut open_parts = Vec::new();
    for &i in &lod0 {
        let node = &model.nodes[i];
        let w = Welded::new(&node.prims);
        let e = w.edges();
        edges.edges += e.edges;
        edges.boundary += e.boundary;
        edges.non_manifold += e.non_manifold;
        edges.inconsistent += e.inconsistent;
        edges.boundary_length += e.boundary_length;
        if e.non_manifold > 0 {
            nm_parts.push(format!("{} {}", node.name, e.non_manifold));
        }
        if e.inconsistent > 0 {
            inconsistent_parts.push(format!("{} {}", node.name, e.inconsistent));
        }
        if e.boundary > 0 {
            open_parts.push(format!("{} {:.2} m", node.name, e.boundary_length));
        }
        let d = w.degenerate(t.degenerate_area);
        if d > 0 {
            degenerate_parts.push((d, node.name.clone()));
        }
        degenerate += d;
        let small: usize = w
            .components()
            .iter()
            .filter(|c| c.len() <= t.loose_max_triangles)
            .map(Vec::len)
            .sum();
        if small > 0 {
            loose_parts.push(format!("{} {small}", node.name));
        }
        loose += small;
    }
    let nm = edges.non_manifold as f64 / edges.edges.max(1) as f64;
    g.check(
        "non_manifold",
        "LOD0 edges shared by > 2 triangles",
        format!(
            "{} of {} ({}) {}",
            edges.non_manifold,
            edges.edges,
            pct(nm),
            list(&nm_parts)
        ),
        format!("≤ {}", pct(t.non_manifold_fraction as f64)),
        nm <= t.non_manifold_fraction as f64,
    );
    g.info(
        "open_edges",
        "LOD0 open (boundary) edges, all parts",
        format!(
            "{} edges, {:.2} m {}",
            edges.boundary,
            edges.boundary_length,
            list(&open_parts)
        ),
    );
    g.info(
        "winding_inconsistent",
        "LOD0 manifold edges whose triangles disagree on winding",
        format!("{} {}", edges.inconsistent, list(&inconsistent_parts)),
    );

    // Closed parts: whole subtree welded together.
    let mut closed = Vec::new();
    let mut closed_ok = true;
    for name in &t.closed_parts {
        let nodes: Vec<usize> = if name == "collision" {
            model.collision_nodes().collect()
        } else {
            match model.find(name) {
                Some(i) => model
                    .subtree(i)
                    .filter(|&j| model.nodes[j].lod == 0 && !model.nodes[j].collision)
                    .collect(),
                None => continue,
            }
        };
        if nodes.is_empty() {
            continue;
        }
        let w = Welded::new(nodes.iter().flat_map(|&j| &model.nodes[j].prims));
        let e = w.edges();
        let vol = w.volume(&(0..w.tris.len()).collect::<Vec<_>>());
        let ok = e.boundary_length <= t.closed_boundary_length && e.non_manifold == 0 && vol > 0.0;
        closed_ok &= ok;
        closed.push(format!(
            "{name}: open {:.3} m, non-manifold {}, volume {:+.4} m³",
            e.boundary_length, e.non_manifold, vol
        ));
    }
    g.check(
        "closed_parts",
        "closed parts watertight and wound outwards (positive volume)",
        closed.join("; "),
        format!(
            "open edges ≤ {} m, 0 non-manifold, volume > 0",
            t.closed_boundary_length
        ),
        closed_ok,
    );

    let deg = degenerate as f64 / tris0.max(1) as f64;
    g.check(
        "degenerate",
        format!(
            "LOD0 triangles with area < {:.0e} m² or collapsed",
            t.degenerate_area
        ),
        format!(
            "{degenerate} ({}) {}",
            pct(deg),
            top_parts(degenerate_parts)
        ),
        format!("≤ {}", pct(t.degenerate_fraction as f64)),
        deg <= t.degenerate_fraction as f64,
    );
    let lf = loose as f64 / tris0.max(1) as f64;
    g.check(
        "loose",
        format!(
            "LOD0 triangles in pieces of ≤ {} triangles",
            t.loose_max_triangles
        ),
        format!("{loose} ({}) {}", pct(lf), list(&loose_parts)),
        format!("≤ {}", pct(t.loose_fraction as f64)),
        lf <= t.loose_fraction as f64,
    );

    // Stored normals vs winding.
    let (mut mismatch, mut total) = (0.0f64, 0.0f64);
    let mut no_normals = 0;
    for &i in &lod0 {
        for p in &model.nodes[i].prims {
            let Some(normals) = &p.normals else {
                no_normals += 1;
                continue;
            };
            for tri in &p.tris {
                let [a, b, c] = tri.map(|v| p.positions[v as usize]);
                let n = (b - a).cross(c - a);
                let ar = n.length() as f64 / 2.0;
                let vn: Vec3 = tri.iter().map(|&v| normals[v as usize]).sum();
                total += ar;
                if n.dot(vn) < 0.0 {
                    mismatch += ar;
                }
            }
        }
    }
    let mf = mismatch / total.max(1e-12);
    g.check(
        "normals_vs_winding",
        "LOD0 area whose stored normals point against the winding",
        format!(
            "{}{}",
            pct(mf),
            if no_normals > 0 {
                format!(", {no_normals} primitives without normals")
            } else {
                String::new()
            }
        ),
        format!("≤ {}", pct(t.normal_mismatch_fraction as f64)),
        mf <= t.normal_mismatch_fraction as f64,
    );

    // Back faces visible from outside.
    if !bounds.is_empty() {
        let mut single = Vec::new();
        let mut double = Vec::new();
        for p in lod0.iter().flat_map(|&i| &model.nodes[i].prims) {
            for k in 0..p.tris.len() {
                if p.double_sided {
                    double.push(p.triangle(k));
                } else {
                    single.push(p.triangle(k));
                }
            }
        }
        let mut worst = 0.0f64;
        let mut parts = Vec::new();
        for view in [
            View::Front,
            View::Rear,
            View::Side,
            View::SideRight,
            View::Top,
            View::Bottom,
        ] {
            let corners = corners(bounds.min, bounds.max).map(|c| view.project(c));
            let min = corners
                .iter()
                .fold(Vec2::splat(f32::INFINITY), |a, c| a.min(c.truncate()));
            let max = corners
                .iter()
                .fold(Vec2::splat(f32::NEG_INFINITY), |a, c| a.max(c.truncate()));
            let grid = Grid::fit(min, max, grid_mpp(bounds.size().max_element()), 2);
            // Double-sided surfaces count as front-facing from both sides.
            let tagged = single
                .iter()
                .map(|t| (*t, false))
                .chain(double.iter().map(|t| (*t, true)));
            let f = facing_with_double(&grid, view, tagged);
            let covered = f.iter().filter(|x| x.is_some()).count();
            let back = f.iter().filter(|x| **x == Some(false)).count();
            let frac = back as f64 / covered.max(1) as f64;
            if view != View::Bottom {
                worst = worst.max(frac);
            }
            parts.push(format!("{} {}", view.name(), pct(frac)));
        }
        g.check(
            "backfaces",
            "visible pixels showing a single-sided back face (worst of front, rear, sides, top; bottom reported)",
            parts.join(", "),
            format!("≤ {}", pct(t.backface_fraction as f64)),
            worst <= t.backface_fraction as f64,
        );
    }

    uv_checks(model, t, &mut g);
    collision_checks(model, t, &mut g, bounds);
    g
}

/// 1 cm pixels, coarser for absurdly large (mis-scaled) assets so a view
/// stays under ~2000 px.
fn grid_mpp(extent: f32) -> f32 {
    (extent / 2000.0).max(0.01)
}

/// Largest contributing parts with their share (%) of the LOD0 area.
fn share_by_part(model: &Model, by: HashMap<usize, f64>, total: f64) -> String {
    let parts = top_parts(
        by.into_iter()
            .map(|(i, a)| {
                (
                    (a / total * 1000.0).round() / 10.0,
                    model.nodes[i].name.clone(),
                )
            })
            .collect(),
    );
    if parts.is_empty() {
        parts
    } else {
        format!("largest parts, % of LOD0 area: {parts}")
    }
}

/// "name count" for the largest contributors, largest first.
fn top_parts<T: PartialOrd + std::fmt::Display>(mut parts: Vec<(T, String)>) -> String {
    parts.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    list(
        &parts
            .into_iter()
            .map(|(v, n)| format!("{n} {v}"))
            .collect::<Vec<_>>(),
    )
}

fn list(items: &[String]) -> String {
    if items.is_empty() {
        String::new()
    } else {
        let shown: Vec<&str> = items.iter().take(8).map(String::as_str).collect();
        format!(
            "[{}{}]",
            shown.join(", "),
            if items.len() > 8 { ", …" } else { "" }
        )
    }
}

fn corners(min: Vec3, max: Vec3) -> [Vec3; 8] {
    std::array::from_fn(|i| {
        Vec3::new(
            if i & 1 == 0 { min.x } else { max.x },
            if i & 2 == 0 { min.y } else { max.y },
            if i & 4 == 0 { min.z } else { max.z },
        )
    })
}

/// Like [`raster::facing`], but double-sided triangles always face the camera.
fn facing_with_double(
    grid: &Grid,
    view: View,
    tris: impl Iterator<Item = ([Vec3; 3], bool)>,
) -> Vec<Option<bool>> {
    let mut single = Vec::new();
    let mut double = Vec::new();
    for (t, d) in tris {
        if d {
            // Re-wind towards the camera.
            let n = (t[1] - t[0]).cross(t[2] - t[0]);
            double.push(if n.dot(view.dir()) < 0.0 {
                t
            } else {
                [t[0], t[2], t[1]]
            });
        } else {
            single.push(t);
        }
    }
    raster::facing(grid, view, single.into_iter().chain(double))
}

fn uv_checks(model: &Model, t: &Thresholds, g: &mut Gate) {
    let render: Vec<usize> = model
        .lods()
        .into_iter()
        .flat_map(|l| model.render_nodes(l).collect::<Vec<_>>())
        .collect();
    let mut missing = Vec::new();
    for &i in &render {
        let n = &model.nodes[i];
        if n.prims.iter().any(|p| p.uvs.is_none()) {
            missing.push(n.name.clone());
        }
    }
    g.check(
        "uv_present",
        "render primitives (all LODs) with TEXCOORD_0",
        if missing.is_empty() {
            "all".to_string()
        } else {
            format!("{} nodes without UVs {}", missing.len(), list(&missing))
        },
        "all",
        missing.is_empty(),
    );

    // Texel density and stretch on LOD0, per material.
    // (density, area, node) per material.
    let mut per_material: HashMap<String, Vec<(f32, f32, usize)>> = HashMap::new();
    let mut stretched_by: HashMap<usize, f64> = HashMap::new();
    let (mut total, mut zero_uv, mut stretched) = (0.0f64, 0.0f64, 0.0f64);
    for i in model.render_nodes(0) {
        for p in &model.nodes[i].prims {
            let Some(uvs) = &p.uvs else { continue };
            let mat = p.material.clone().unwrap_or_default();
            for tri in &p.tris {
                let pos = tri.map(|v| p.positions[v as usize]);
                let uv = tri.map(|v| uvs[v as usize]);
                let a3 = area(pos[0], pos[1], pos[2]);
                if a3 < t.degenerate_area {
                    continue;
                }
                total += a3 as f64;
                let auv = ((uv[1] - uv[0]).perp_dot(uv[2] - uv[0])).abs() / 2.0;
                let Some((smax, smin)) = uv_singular_values(pos, uv) else {
                    continue;
                };
                if auv < 1e-12 || smin <= smax * 1e-4 {
                    zero_uv += a3 as f64;
                    continue;
                }
                if smax / smin > t.uv_stretch {
                    stretched += a3 as f64;
                    *stretched_by.entry(i).or_default() += a3 as f64;
                }
                per_material
                    .entry(mat.clone())
                    .or_default()
                    .push(((auv / a3).sqrt(), a3, i));
            }
        }
    }
    let total = total.max(1e-12);
    g.check(
        "uv_degenerate",
        "LOD0 area mapped to zero UV area",
        pct(zero_uv / total),
        format!("≤ {}", pct(t.uv_stretch_fraction as f64)),
        zero_uv / total <= t.uv_stretch_fraction as f64,
    );
    g.check(
        "uv_stretch",
        format!("LOD0 area with UV stretch σmax/σmin > {}", t.uv_stretch),
        format!(
            "{} {}",
            pct(stretched / total),
            share_by_part(model, stretched_by, total)
        ),
        format!("≤ {}", pct(t.uv_stretch_fraction as f64)),
        stretched / total <= t.uv_stretch_fraction as f64,
    );
    let mut outliers = 0.0f64;
    let mut outliers_by: HashMap<usize, f64> = HashMap::new();
    let mut medians = Vec::new();
    let mut mats: Vec<_> = per_material.into_iter().collect();
    mats.sort_by(|a, b| a.0.cmp(&b.0));
    for (mat, v) in mats {
        let med = weighted_median(v.iter().map(|x| (x.0, x.1)).collect());
        medians.push(format!("{mat} {med:.3}/m"));
        let r = t.texel_density_ratio;
        for (d, a, i) in &v {
            if *d < med / r || *d > med * r {
                outliers += *a as f64;
                *outliers_by.entry(*i).or_default() += *a as f64;
            }
        }
    }
    g.check(
        "texel_density",
        format!(
            "LOD0 area whose texel density is outside ×1/{0}…×{0} of its material's median",
            t.texel_density_ratio
        ),
        format!(
            "{} {} (medians, UV units per metre: {})",
            pct(outliers / total),
            share_by_part(model, outliers_by, total),
            medians.join(", ")
        ),
        format!("≤ {}", pct(t.texel_outlier_fraction as f64)),
        outliers / total <= t.texel_outlier_fraction as f64,
    );
}

fn collision_checks(model: &Model, t: &Thresholds, g: &mut Gate, lod0: crate::model::Aabb) {
    let nodes: Vec<usize> = model.collision_nodes().collect();
    if nodes.is_empty() {
        g.check(
            "collision",
            "collision node with geometry",
            "missing",
            "present",
            false,
        );
        return;
    }
    let mut pieces = 0usize;
    let mut worst_out = 0.0f32;
    let mut worst_tris = 0usize;
    let mut open = 0.0f32;
    for &i in &nodes {
        let w = Welded::new(&model.nodes[i].prims);
        open += w.edges().boundary_length;
        for comp in w.components() {
            pieces += 1;
            worst_tris = worst_tris.max(comp.len());
            let sign = if w.volume(&comp) >= 0.0 { 1.0 } else { -1.0 };
            let verts: Vec<Vec3> = {
                let mut v: Vec<u32> = comp.iter().flat_map(|&k| w.tris[k]).collect();
                v.sort_unstable();
                v.dedup();
                v.into_iter().map(|k| w.positions[k as usize]).collect()
            };
            for &k in &comp {
                let [a, b, c] = w.tri(&w.tris[k]);
                let n = (b - a).cross(c - a);
                if n.length_squared() < 1e-16 {
                    continue;
                }
                let n = n.normalize() * sign;
                for v in &verts {
                    worst_out = worst_out.max(n.dot(*v - a));
                }
            }
        }
    }
    g.check(
        "collision_pieces",
        "collision pieces (connected components)",
        pieces.to_string(),
        format!("1 – {}", t.collision_max_pieces),
        pieces >= 1 && pieces <= t.collision_max_pieces,
    );
    g.check(
        "collision_convex",
        "largest distance of a hull vertex outside a face plane of its own piece",
        format!("{worst_out:.4} m"),
        format!("≤ {} m", t.collision_convex_tolerance),
        worst_out <= t.collision_convex_tolerance,
    );
    g.check(
        "collision_triangles",
        "triangles in the largest collision piece",
        worst_tris.to_string(),
        format!("≤ {}", t.collision_max_triangles),
        worst_tris <= t.collision_max_triangles,
    );
    g.info(
        "collision_open",
        "collision open-edge length",
        format!("{open:.3} m"),
    );
    let cb = model.bounds(nodes.iter().copied());
    let ratio = cb.size() / lod0.size().max(Vec3::splat(1e-6));
    let (lo, hi) = t.collision_coverage;
    g.check(
        "collision_coverage",
        "collision bounding box / LOD0 bounding box per axis",
        format!(
            "x {:.3}, y {:.3}, z {:.3} (collision y {:.3}..{:.3}, x ±{:.3}; LOD0 y {:.3}..{:.3}, x ±{:.3})",
            ratio.x,
            ratio.y,
            ratio.z,
            cb.min.y,
            cb.max.y,
            cb.size().x / 2.0,
            lod0.min.y,
            lod0.max.y,
            lod0.size().x / 2.0
        ),
        format!("{lo} – {hi}"),
        ratio.min_element() >= lo && ratio.max_element() <= hi,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uv_stretch_of_isotropic_and_stretched_maps() {
        let p = [Vec3::ZERO, Vec3::X, Vec3::Y];
        let (a, b) = uv_singular_values(p, [Vec2::ZERO, Vec2::X * 2.0, Vec2::Y * 2.0]).unwrap();
        assert!((a - 2.0).abs() < 1e-5 && (b - 2.0).abs() < 1e-5);
        let (a, b) = uv_singular_values(p, [Vec2::ZERO, Vec2::X * 8.0, Vec2::Y]).unwrap();
        assert!((a / b - 8.0).abs() < 1e-3);
    }

    #[test]
    fn weld_and_edges_of_a_tetrahedron() {
        let v = [Vec3::ZERO, Vec3::X, Vec3::Y, Vec3::Z];
        // Outward-wound faces, each with its own (seam-split) vertices.
        let faces = [[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]];
        let mut prim = Prim::default();
        for f in faces {
            let base = prim.positions.len() as u32;
            prim.positions.extend(f.map(|i| v[i]));
            prim.tris.push([base, base + 1, base + 2]);
        }
        let w = Welded::new([&prim]);
        assert_eq!(w.positions.len(), 4);
        let e = w.edges();
        assert_eq!(
            (e.edges, e.boundary, e.non_manifold, e.inconsistent),
            (6, 0, 0, 0)
        );
        assert!((w.volume(&[0, 1, 2, 3]) - 1.0 / 6.0).abs() < 1e-6);
        assert_eq!(w.components().len(), 1);
    }
}
