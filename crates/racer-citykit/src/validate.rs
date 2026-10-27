//! Asset validation: triangle budgets, naming, UV ranges, finiteness,
//! manifoldness of parts meant to be closed and LOD ratios.
//!
//! `racer-citykit validate` runs these over the whole kit; the unit tests run
//! them too, so a generator change that breaks a budget fails `cargo test`.

use std::collections::HashMap;

use bevy::math::Vec3;

use crate::mesh::MeshData;

/// Edge statistics of a mesh after welding coincident positions.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ManifoldReport {
    /// Edges used by exactly one triangle.
    pub boundary_edges: usize,
    /// Edges used by more than two triangles.
    pub non_manifold_edges: usize,
    /// Edges whose two triangles traverse them in the same direction.
    pub inconsistent_edges: usize,
    /// Triangles with (near) zero area.
    pub degenerate_triangles: usize,
    /// Volume by the divergence theorem (positive when outward-facing).
    pub signed_volume: f32,
}

impl ManifoldReport {
    /// Watertight, consistently oriented 2-manifold.
    pub fn is_closed(&self) -> bool {
        self.boundary_edges == 0 && self.non_manifold_edges == 0 && self.inconsistent_edges == 0
    }
}

/// Welds positions to 0.1 mm and counts edge usage.
pub fn manifold_report(m: &MeshData) -> ManifoldReport {
    manifold_report_of(m, &m.indices)
}

/// [`manifold_report`] over a subset of the triangles (`indices`).
pub fn manifold_report_of(m: &MeshData, indices: &[u32]) -> ManifoldReport {
    let key = |p: Vec3| {
        let q = (p * 1e4).round();
        (q.x as i64, q.y as i64, q.z as i64)
    };
    let mut ids: HashMap<(i64, i64, i64), u32> = HashMap::new();
    let welded: Vec<u32> = m
        .positions
        .iter()
        .map(|p| {
            let n = ids.len() as u32;
            *ids.entry(key(*p)).or_insert(n)
        })
        .collect();
    let mut edges: HashMap<(u32, u32), (u32, u32)> = HashMap::new(); // (a<b) -> (forward, backward)
    let mut report = ManifoldReport::default();
    for t in indices.chunks_exact(3) {
        let (pa, pb, pc) = (
            m.positions[t[0] as usize],
            m.positions[t[1] as usize],
            m.positions[t[2] as usize],
        );
        let cross = (pb - pa).cross(pc - pa);
        if cross.length() < 1e-9 {
            report.degenerate_triangles += 1;
            continue;
        }
        report.signed_volume += pa.dot(pb.cross(pc)) / 6.0;
        let w = [welded[t[0] as usize], welded[t[1] as usize], welded[t[2] as usize]];
        for k in 0..3 {
            let (a, b) = (w[k], w[(k + 1) % 3]);
            if a == b {
                continue;
            }
            let e = edges.entry((a.min(b), a.max(b))).or_default();
            if a < b {
                e.0 += 1;
            } else {
                e.1 += 1;
            }
        }
    }
    for (f, b) in edges.values() {
        match f + b {
            1 => report.boundary_edges += 1,
            2 if *f != 1 => report.inconsistent_edges += 1,
            2 => {}
            _ => report.non_manifold_edges += 1,
        }
    }
    report
}

/// How a part's UVs are meant to be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UvPolicy {
    /// Metre-scale UVs on a tiling material: finite, within ±10 km.
    Tiling,
    /// Atlas UVs: inside [0, 1].
    Atlas,
}

/// One mesh in an asset, with the material slot it uses.
#[derive(Clone, Debug)]
pub struct Part {
    pub material: crate::kit::Slot,
    pub mesh: MeshData,
    /// Index ranges of pieces that must each be watertight (closed and
    /// consistently oriented).
    pub closed: Vec<std::ops::Range<usize>>,
}

/// Validation limits for one asset category.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    pub max_triangles: usize,
    /// Each LOD must have at most this fraction of the previous one's triangles.
    pub lod_ratio: f32,
}

/// Problems found in one asset.
#[derive(Clone, Debug, Default)]
pub struct Findings {
    pub errors: Vec<String>,
}

/// Lowercase snake case with a known category prefix, e.g. `furn_lamp_double`.
pub fn valid_name(name: &str) -> bool {
    const PREFIXES: [&str; 5] = ["road_", "race_", "furn_", "bldg_", "decal_"];
    PREFIXES.iter().any(|p| name.starts_with(p))
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
        && !name.ends_with('_')
}

/// Checks one asset (its LOD chain, LOD0 first).
pub fn check_asset(name: &str, lods: &[Vec<Part>], budget: Budget) -> Findings {
    let mut f = Findings::default();
    if !valid_name(name) {
        f.errors.push(format!("{name}: invalid name"));
    }
    if lods.is_empty() || lods[0].is_empty() {
        f.errors.push(format!("{name}: no geometry"));
        return f;
    }
    let mut prev: Option<usize> = None;
    for (level, parts) in lods.iter().enumerate() {
        let tris: usize = parts.iter().map(|p| p.mesh.triangle_count()).sum();
        if level == 0 && tris > budget.max_triangles {
            f.errors.push(format!(
                "{name}: {tris} triangles over budget {}",
                budget.max_triangles
            ));
        }
        if let Some(p) = prev
            && tris as f32 > p as f32 * budget.lod_ratio
        {
            f.errors.push(format!(
                "{name}: LOD{level} has {tris} triangles, more than {:.0}% of LOD{} ({p})",
                budget.lod_ratio * 100.0,
                level - 1
            ));
        }
        prev = Some(tris);
        for (i, part) in parts.iter().enumerate() {
            let m = &part.mesh;
            let tag = format!("{name} LOD{level} part {i} ({:?})", part.material);
            if m.positions.len() != m.normals.len() || m.positions.len() != m.uvs.len() {
                f.errors.push(format!("{tag}: attribute lengths differ"));
                continue;
            }
            if m.indices.iter().any(|i| *i as usize >= m.positions.len()) {
                f.errors.push(format!("{tag}: index out of range"));
                continue;
            }
            if m.positions.iter().any(|p| !p.is_finite())
                || m.normals.iter().any(|n| !n.is_finite() || (n.length() - 1.0).abs() > 1e-3)
            {
                f.errors.push(format!("{tag}: non-finite position or bad normal"));
            }
            let atlas = part.material.uv_policy() == UvPolicy::Atlas;
            let bad_uv = m.uvs.iter().any(|uv| {
                !uv.is_finite()
                    || if atlas {
                        uv.min_element() < -1e-4 || uv.max_element() > 1.0 + 1e-4
                    } else {
                        uv.abs().max_element() > 1e4
                    }
            });
            if bad_uv {
                f.errors.push(format!("{tag}: UVs out of range for {:?}", part.material.uv_policy()));
            }
            for (k, range) in part.closed.iter().enumerate() {
                let r = manifold_report_of(m, &m.indices[range.clone()]);
                if !r.is_closed() {
                    f.errors.push(format!("{tag} piece {k}: not watertight: {r:?}"));
                } else if r.signed_volume <= 0.0 {
                    f.errors.push(format!("{tag} piece {k}: inside out (volume {})", r.signed_volume));
                }
            }
        }
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(valid_name("furn_lamp_double"));
        assert!(valid_name("race_barrier_jersey_lod1"));
        assert!(!valid_name("Lamp"));
        assert!(!valid_name("prop_lamp"));
        assert!(!valid_name("furn_lamp_"));
    }

    #[test]
    fn open_quad_has_boundary() {
        let mut m = MeshData::new();
        m.quad(
            [Vec3::ZERO, Vec3::X, Vec3::new(1.0, 0.0, -1.0), Vec3::NEG_Z],
            [bevy::math::Vec2::ZERO; 4],
        );
        let r = manifold_report(&m);
        assert_eq!(r.boundary_edges, 4);
        assert!(!r.is_closed());
    }
}
