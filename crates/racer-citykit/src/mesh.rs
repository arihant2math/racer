//! Triangle mesh builder used by every generator in the kit.
//!
//! Meshes are plain data (positions, normals, UVs, indices) so the generators
//! can be tested without a renderer; [`MeshData::to_bevy`] converts at the end.
//! Units are metres, +Y up. UVs of tiling surfaces are in metres; materials
//! scale them with their `uv_transform`.

use bevy::{
    asset::RenderAssetUsages,
    math::{Affine3A, Vec2, Vec3, Vec4},
    mesh::{Indices, Mesh, PrimitiveTopology},
};

/// A point on a path with its local frame. `right` and `up` span the
/// cross-section plane; `s` is the arc length along the (offset) path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Frame {
    pub pos: Vec3,
    pub forward: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub s: f32,
}

impl Frame {
    /// Point at `(x, y)` in the cross-section plane (x right, y up).
    pub fn at(&self, p: Vec2) -> Vec3 {
        self.pos + self.right * p.x + self.up * p.y
    }

    /// Affine transform mapping local +X right, +Y up, −Z forward to this frame.
    pub fn affine(&self) -> Affine3A {
        Affine3A::from_cols(
            self.right.into(),
            self.up.into(),
            (-self.forward).into(),
            self.pos.into(),
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshData {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Vec<Vec2>,
    pub indices: Vec<u32>,
}

impl MeshData {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty()
    }

    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    pub fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        self.uvs.push(uv);
        (self.positions.len() - 1) as u32
    }

    /// Triangle whose front face points along `n` (winding fixed up if needed).
    pub fn tri(&mut self, a: u32, b: u32, c: u32) {
        let (pa, pb, pc) = (
            self.positions[a as usize],
            self.positions[b as usize],
            self.positions[c as usize],
        );
        let n = self.normals[a as usize] + self.normals[b as usize] + self.normals[c as usize];
        if (pb - pa).cross(pc - pa).dot(n) >= 0.0 {
            self.indices.extend([a, b, c]);
        } else {
            self.indices.extend([a, c, b]);
        }
    }

    /// Planar quad `p0 p1 p2 p3` (in order around the rim) facing `n`.
    pub fn quad_n(&mut self, p: [Vec3; 4], uv: [Vec2; 4], n: Vec3) {
        let i: Vec<u32> = (0..4).map(|k| self.vertex(p[k], n, uv[k])).collect();
        self.tri(i[0], i[1], i[2]);
        self.tri(i[0], i[2], i[3]);
    }

    /// Quad with the normal taken from its winding (`p0 p1 p2` counter-clockwise
    /// seen from the front).
    pub fn quad(&mut self, p: [Vec3; 4], uv: [Vec2; 4]) {
        let n = (p[1] - p[0]).cross(p[2] - p[0]).normalize_or(Vec3::Y);
        self.quad_n(p, uv, n);
    }

    pub fn append(&mut self, other: &MeshData) {
        let base = self.positions.len() as u32;
        self.positions.extend_from_slice(&other.positions);
        self.normals.extend_from_slice(&other.normals);
        self.uvs.extend_from_slice(&other.uvs);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }

    pub fn append_transformed(&mut self, other: &MeshData, t: Affine3A) {
        self.append(&other.transformed(t));
    }

    /// Copy transformed by `t` (normals by the inverse transpose).
    pub fn transformed(&self, t: Affine3A) -> MeshData {
        let normal_matrix = t.matrix3.inverse().transpose();
        let flip = t.matrix3.determinant() < 0.0;
        let mut indices = self.indices.clone();
        if flip {
            for tri in indices.chunks_mut(3) {
                tri.swap(1, 2);
            }
        }
        MeshData {
            positions: self
                .positions
                .iter()
                .map(|p| t.transform_point3(*p))
                .collect(),
            normals: self
                .normals
                .iter()
                .map(|n| Vec3::from(normal_matrix * bevy::math::Vec3A::from(*n)).normalize_or(Vec3::Y))
                .collect(),
            uvs: self.uvs.clone(),
            indices,
        }
    }

    pub fn translated(&self, d: Vec3) -> MeshData {
        self.transformed(Affine3A::from_translation(d))
    }

    /// Maps every UV through `f` (used to place parts in an atlas cell).
    pub fn map_uvs(mut self, f: impl Fn(Vec2) -> Vec2) -> MeshData {
        for uv in &mut self.uvs {
            *uv = f(*uv);
        }
        self
    }

    /// Axis-aligned box from `min` to `max`, UVs in metres on each face.
    pub fn cuboid(min: Vec3, max: Vec3) -> MeshData {
        let mut m = MeshData::new();
        m.add_cuboid(min, max);
        m
    }

    pub fn add_cuboid(&mut self, min: Vec3, max: Vec3) {
        let (a, b) = (min, max);
        let uv = |u0: f32, v0: f32, u1: f32, v1: f32| {
            [
                Vec2::new(u0, v1),
                Vec2::new(u1, v1),
                Vec2::new(u1, v0),
                Vec2::new(u0, v0),
            ]
        };
        // +X, -X
        self.quad_n(
            [
                Vec3::new(b.x, a.y, b.z),
                Vec3::new(b.x, a.y, a.z),
                Vec3::new(b.x, b.y, a.z),
                Vec3::new(b.x, b.y, b.z),
            ],
            uv(-b.z, -b.y, -a.z, -a.y),
            Vec3::X,
        );
        self.quad_n(
            [
                Vec3::new(a.x, a.y, a.z),
                Vec3::new(a.x, a.y, b.z),
                Vec3::new(a.x, b.y, b.z),
                Vec3::new(a.x, b.y, a.z),
            ],
            uv(a.z, -b.y, b.z, -a.y),
            Vec3::NEG_X,
        );
        // +Z, -Z
        self.quad_n(
            [
                Vec3::new(a.x, a.y, b.z),
                Vec3::new(b.x, a.y, b.z),
                Vec3::new(b.x, b.y, b.z),
                Vec3::new(a.x, b.y, b.z),
            ],
            uv(a.x, -b.y, b.x, -a.y),
            Vec3::Z,
        );
        self.quad_n(
            [
                Vec3::new(b.x, a.y, a.z),
                Vec3::new(a.x, a.y, a.z),
                Vec3::new(a.x, b.y, a.z),
                Vec3::new(b.x, b.y, a.z),
            ],
            uv(-b.x, -b.y, -a.x, -a.y),
            Vec3::NEG_Z,
        );
        // +Y, -Y
        self.quad_n(
            [
                Vec3::new(a.x, b.y, b.z),
                Vec3::new(b.x, b.y, b.z),
                Vec3::new(b.x, b.y, a.z),
                Vec3::new(a.x, b.y, a.z),
            ],
            uv(a.x, a.z, b.x, b.z),
            Vec3::Y,
        );
        self.quad_n(
            [
                Vec3::new(a.x, a.y, a.z),
                Vec3::new(b.x, a.y, a.z),
                Vec3::new(b.x, a.y, b.z),
                Vec3::new(a.x, a.y, b.z),
            ],
            uv(a.x, a.z, b.x, b.z),
            Vec3::NEG_Y,
        );
    }

    /// Frustum (cylinder when `r0 == r1`) along +Y from `y = 0` to `h`, with
    /// optional caps. UVs: u around in metres, v up in metres.
    pub fn frustum(r0: f32, r1: f32, h: f32, segments: u32, caps: bool) -> MeshData {
        let mut m = MeshData::new();
        let n = segments.max(3);
        let slope = (r0 - r1) / h;
        let circumference = std::f32::consts::TAU * r0.max(r1);
        for k in 0..=n {
            let a = k as f32 / n as f32 * std::f32::consts::TAU;
            let (s, c) = a.sin_cos();
            let dir = Vec3::new(c, 0.0, -s);
            let normal = (dir + Vec3::Y * slope).normalize();
            let u = k as f32 / n as f32 * circumference;
            m.vertex(dir * r0, normal, Vec2::new(u, h));
            m.vertex(dir * r1 + Vec3::Y * h, normal, Vec2::new(u, 0.0));
        }
        for k in 0..n {
            let i = 2 * k;
            m.tri(i, i + 2, i + 3);
            m.tri(i, i + 3, i + 1);
        }
        if caps {
            for (y, r, normal) in [(0.0, r0, Vec3::NEG_Y), (h, r1, Vec3::Y)] {
                if r <= 0.0 {
                    continue;
                }
                let centre = m.vertex(Vec3::Y * y, normal, Vec2::ZERO);
                let first = m.positions.len() as u32;
                for k in 0..n {
                    let a = k as f32 / n as f32 * std::f32::consts::TAU;
                    let (s, c) = a.sin_cos();
                    m.vertex(
                        Vec3::new(c * r, y, -s * r),
                        normal,
                        Vec2::new(c * r, s * r),
                    );
                }
                for k in 0..n {
                    m.tri(centre, first + k, first + (k + 1) % n);
                }
            }
        }
        m
    }

    /// Cylinder from `a` to `b` (any direction).
    pub fn tube(a: Vec3, b: Vec3, r: f32, segments: u32, caps: bool) -> MeshData {
        let d = b - a;
        let len = d.length();
        let dir = d / len.max(1e-6);
        let rot = bevy::math::Quat::from_rotation_arc(Vec3::Y, dir);
        MeshData::frustum(r, r, len, segments, caps)
            .transformed(Affine3A::from_rotation_translation(rot, a))
    }

    /// Sweeps a closed or open 2D profile (x right, y up, counter-clockwise
    /// for outward normals) along `frames`. Each profile edge is flat-shaded
    /// across the profile and smooth along the path. UV u = profile length,
    /// v = arc length (metres). With `caps`, a closed profile gets end caps
    /// (ear-clipped, so concave profiles work).
    pub fn sweep(frames: &[Frame], profile: &[Vec2], closed: bool, caps: bool) -> MeshData {
        let mut m = MeshData::new();
        if frames.len() < 2 || profile.len() < 2 {
            return m;
        }
        let edges = if closed {
            profile.len()
        } else {
            profile.len() - 1
        };
        let mut u0 = 0.0;
        for e in 0..edges {
            let (p0, p1) = (profile[e], profile[(e + 1) % profile.len()]);
            let d = p1 - p0;
            let len = d.length();
            if len < 1e-6 {
                continue;
            }
            let n2 = Vec2::new(d.y, -d.x) / len;
            let base = m.positions.len() as u32;
            for f in frames {
                let n = (f.right * n2.x + f.up * n2.y).normalize();
                m.vertex(f.at(p0), n, Vec2::new(u0, f.s));
                m.vertex(f.at(p1), n, Vec2::new(u0 + len, f.s));
            }
            for i in 0..frames.len() as u32 - 1 {
                let a = base + 2 * i;
                m.tri(a, a + 1, a + 3);
                m.tri(a, a + 3, a + 2);
            }
            u0 += len;
        }
        if caps && closed {
            let tris = triangulate(profile);
            for (f, n) in [
                (frames[0], -frames[0].forward),
                (frames[frames.len() - 1], frames[frames.len() - 1].forward),
            ] {
                let first = m.positions.len() as u32;
                for p in profile {
                    m.vertex(f.at(*p), n, *p);
                }
                for t in &tris {
                    m.tri(first + t[0] as u32, first + t[1] as u32, first + t[2] as u32);
                }
            }
        }
        m
    }

    /// Axis-aligned bounds, or `None` when empty.
    pub fn bounds(&self) -> Option<(Vec3, Vec3)> {
        let first = *self.positions.first()?;
        Some(
            self.positions
                .iter()
                .fold((first, first), |(lo, hi), p| (lo.min(*p), hi.max(*p))),
        )
    }

    /// Per-vertex tangents (xyz + handedness in w) from UV derivatives.
    pub fn tangents(&self) -> Vec<Vec4> {
        let n = self.positions.len();
        let mut tan = vec![Vec3::ZERO; n];
        let mut bit = vec![Vec3::ZERO; n];
        for t in self.indices.chunks_exact(3) {
            let [a, b, c] = [t[0] as usize, t[1] as usize, t[2] as usize];
            let (e1, e2) = (
                self.positions[b] - self.positions[a],
                self.positions[c] - self.positions[a],
            );
            let (d1, d2) = (self.uvs[b] - self.uvs[a], self.uvs[c] - self.uvs[a]);
            let det = d1.x * d2.y - d2.x * d1.y;
            if det.abs() < 1e-12 {
                continue;
            }
            let r = 1.0 / det;
            let sdir = (e1 * d2.y - e2 * d1.y) * r;
            let tdir = (e2 * d1.x - e1 * d2.x) * r;
            for v in [a, b, c] {
                tan[v] += sdir;
                bit[v] += tdir;
            }
        }
        (0..n)
            .map(|i| {
                let nrm = self.normals[i];
                let mut t = tan[i] - nrm * nrm.dot(tan[i]);
                if t.length_squared() < 1e-12 {
                    t = nrm.any_orthonormal_vector();
                }
                let t = t.normalize();
                let w = if nrm.cross(t).dot(bit[i]) < 0.0 {
                    -1.0
                } else {
                    1.0
                };
                t.extend(w)
            })
            .collect()
    }

    /// Bevy mesh with positions, normals, UVs and tangents.
    pub fn to_bevy(&self) -> Mesh {
        let to = |v: &[Vec3]| v.iter().map(|p| p.to_array()).collect::<Vec<_>>();
        Mesh::new(PrimitiveTopology::TriangleList, RenderAssetUsages::default())
            .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, to(&self.positions))
            .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, to(&self.normals))
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_UV_0,
                self.uvs.iter().map(|u| u.to_array()).collect::<Vec<_>>(),
            )
            .with_inserted_attribute(
                Mesh::ATTRIBUTE_TANGENT,
                self.tangents()
                    .iter()
                    .map(|t| t.to_array())
                    .collect::<Vec<_>>(),
            )
            .with_inserted_indices(Indices::U32(self.indices.clone()))
    }

    /// Order-independent-of-nothing content hash, for determinism tests.
    pub fn content_hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |x: u32| {
            h ^= x as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        };
        for p in &self.positions {
            p.to_array().iter().for_each(|v| eat(v.to_bits()));
        }
        for p in &self.normals {
            p.to_array().iter().for_each(|v| eat(v.to_bits()));
        }
        for p in &self.uvs {
            p.to_array().iter().for_each(|v| eat(v.to_bits()));
        }
        self.indices.iter().for_each(|i| eat(*i));
        h
    }
}

/// Ear-clipping triangulation of a simple counter-clockwise polygon.
pub fn triangulate(poly: &[Vec2]) -> Vec<[usize; 3]> {
    let cross = |a: Vec2, b: Vec2, c: Vec2| (b - a).perp_dot(c - a);
    let mut idx: Vec<usize> = (0..poly.len()).collect();
    // Accept either orientation: work counter-clockwise.
    let area: f32 = (0..poly.len())
        .map(|i| poly[i].perp_dot(poly[(i + 1) % poly.len()]))
        .sum();
    if area < 0.0 {
        idx.reverse();
    }
    let mut out = Vec::new();
    let mut guard = 0;
    while idx.len() > 3 && guard < 10_000 {
        guard += 1;
        let n = idx.len();
        let mut clipped = false;
        for i in 0..n {
            let (ia, ib, ic) = (idx[(i + n - 1) % n], idx[i], idx[(i + 1) % n]);
            let (a, b, c) = (poly[ia], poly[ib], poly[ic]);
            if cross(a, b, c) <= 1e-9 {
                continue;
            }
            let inside = idx.iter().any(|&j| {
                j != ia && j != ib && j != ic && {
                    let p = poly[j];
                    cross(a, b, p) >= 0.0 && cross(b, c, p) >= 0.0 && cross(c, a, p) >= 0.0
                }
            });
            if !inside {
                out.push([ia, ib, ic]);
                idx.remove(i);
                clipped = true;
                break;
            }
        }
        if !clipped {
            break;
        }
    }
    if idx.len() == 3 {
        out.push([idx[0], idx[1], idx[2]]);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::validate::manifold_report;

    #[test]
    fn concave_profile_sweeps_watertight() {
        let l = [Vec2::new(0.0, 0.0), Vec2::new(1.0, 0.0), Vec2::new(1.0, 0.2), Vec2::new(0.2, 0.2), Vec2::new(0.2, 1.0), Vec2::new(0.0, 1.0)];
        assert_eq!(triangulate(&l).len(), 4);
        let frames: Vec<Frame> = [0.0, 2.0]
            .iter()
            .map(|z| Frame { pos: Vec3::Z * -*z, forward: Vec3::NEG_Z, right: Vec3::X, up: Vec3::Y, s: *z })
            .collect();
        let r = manifold_report(&MeshData::sweep(&frames, &l, true, true));
        assert!(r.is_closed() && (r.signed_volume - 0.72).abs() < 1e-4, "{r:?}");
    }

    #[test]
    fn cuboid_is_closed_and_outward() {
        let m = MeshData::cuboid(Vec3::new(-1.0, 0.0, -2.0), Vec3::new(1.0, 3.0, 2.0));
        assert_eq!(m.triangle_count(), 12);
        let r = manifold_report(&m);
        assert!(r.is_closed(), "{r:?}");
        assert!(r.signed_volume > 0.0 && (r.signed_volume - 24.0).abs() < 1e-3);
    }

    #[test]
    fn capped_frustum_and_tube_are_closed() {
        let m = MeshData::frustum(0.5, 0.3, 2.0, 12, true);
        assert!(manifold_report(&m).is_closed());
        let t = MeshData::tube(Vec3::ZERO, Vec3::new(1.0, 2.0, -0.5), 0.1, 8, true);
        let r = manifold_report(&t);
        assert!(r.is_closed() && r.signed_volume > 0.0, "{r:?}");
    }

    #[test]
    fn mirrored_transform_keeps_outward_winding() {
        let m = MeshData::cuboid(Vec3::ZERO, Vec3::ONE)
            .transformed(Affine3A::from_scale(Vec3::new(-1.0, 1.0, 1.0)));
        assert!(manifold_report(&m).signed_volume > 0.0);
    }
}
