//! Loads a GLB into a flat, world-space description: nodes with names,
//! transforms, LOD level and triangle geometry.

use std::path::Path;

use glam::{Mat3, Mat4, Vec2, Vec3};
use gltf::buffer::Source;

/// One triangle list in world space.
#[derive(Clone, Debug, Default)]
pub struct Prim {
    pub positions: Vec<Vec3>,
    pub normals: Option<Vec<Vec3>>,
    pub uvs: Option<Vec<Vec2>>,
    pub tris: Vec<[u32; 3]>,
    pub material: Option<String>,
    /// The material is double-sided (no back-face culling in Bevy).
    pub double_sided: bool,
}

impl Prim {
    pub fn triangle(&self, t: usize) -> [Vec3; 3] {
        let [a, b, c] = self.tris[t];
        [
            self.positions[a as usize],
            self.positions[b as usize],
            self.positions[c as usize],
        ]
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    /// Name as exported.
    pub name: String,
    /// Name without a LOD suffix (`body_LOD1` → `body`).
    pub base: String,
    pub parent: Option<usize>,
    /// One past the last descendant (nodes are stored depth first).
    pub end: usize,
    pub world: Mat4,
    pub prims: Vec<Prim>,
    /// LOD level from the node's or an ancestor's name (`lod1`, `LOD2`), else 0.
    pub lod: u8,
    /// The node or an ancestor is named `collision…`.
    pub collision: bool,
}

impl Node {
    pub fn translation(&self) -> Vec3 {
        self.world.w_axis.truncate()
    }

    pub fn has_mesh(&self) -> bool {
        !self.prims.is_empty()
    }

    pub fn triangle_count(&self) -> usize {
        self.prims.iter().map(|p| p.tris.len()).sum()
    }

    /// Sockets are empties named `socket_…`.
    pub fn is_socket(&self) -> bool {
        self.base.starts_with("socket_")
    }
}

#[derive(Clone, Debug, Default)]
pub struct Model {
    pub nodes: Vec<Node>,
    pub materials: Vec<String>,
    pub warnings: Vec<String>,
    pub generator: Option<String>,
    pub extensions_used: Vec<String>,
}

/// Axis-aligned bounding box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub const EMPTY: Aabb = Aabb {
        min: Vec3::splat(f32::INFINITY),
        max: Vec3::splat(f32::NEG_INFINITY),
    };

    pub fn add(&mut self, p: Vec3) {
        self.min = self.min.min(p);
        self.max = self.max.max(p);
    }

    pub fn is_empty(&self) -> bool {
        self.min.x > self.max.x
    }

    pub fn size(&self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            self.max - self.min
        }
    }

    pub fn centre(&self) -> Vec3 {
        (self.min + self.max) / 2.0
    }
}

impl Model {
    pub fn load(path: &Path) -> Result<Model, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Model::from_glb(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn from_glb(bytes: &[u8]) -> Result<Model, String> {
        let gltf = gltf::Gltf::from_slice(bytes).map_err(|e| format!("invalid glTF: {e}"))?;
        let doc = &gltf.document;
        let blob = gltf.blob.as_deref();
        let mut model = Model {
            materials: doc
                .materials()
                .map(|m| m.name().unwrap_or("").to_string())
                .collect(),
            extensions_used: doc.extensions_used().map(str::to_string).collect(),
            ..Model::default()
        };
        if let Some(required) = doc.extensions_required().next() {
            model.warnings.push(format!(
                "requires extension {required}; geometry may be unreadable"
            ));
        }
        let scene = doc
            .default_scene()
            .or_else(|| doc.scenes().next())
            .ok_or("no scene")?;
        for root in scene.nodes() {
            visit(&mut model, blob, root, None, Mat4::IDENTITY, 0, false);
        }
        Ok(model)
    }

    /// Indices of `i` and its descendants.
    pub fn subtree(&self, i: usize) -> std::ops::Range<usize> {
        i..self.nodes[i].end
    }

    /// The first LOD0 node whose base name is `name`.
    pub fn find(&self, name: &str) -> Option<usize> {
        self.nodes
            .iter()
            .position(|n| n.lod == 0 && !n.collision && n.base == name)
    }

    /// Renderable mesh nodes of a LOD level (collision excluded).
    pub fn render_nodes(&self, lod: u8) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len())
            .filter(move |&i| self.nodes[i].lod == lod && !self.nodes[i].collision)
            .filter(|&i| self.nodes[i].has_mesh())
    }

    pub fn collision_nodes(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.nodes.len()).filter(|&i| self.nodes[i].collision && self.nodes[i].has_mesh())
    }

    pub fn lods(&self) -> Vec<u8> {
        let mut lods: Vec<u8> = self
            .nodes
            .iter()
            .filter(|n| n.has_mesh() && !n.collision)
            .map(|n| n.lod)
            .collect();
        lods.sort_unstable();
        lods.dedup();
        lods
    }

    pub fn triangles(&self, nodes: impl IntoIterator<Item = usize>) -> usize {
        nodes
            .into_iter()
            .map(|i| self.nodes[i].triangle_count())
            .sum()
    }

    /// Bounding box of the geometry of `nodes`.
    pub fn bounds(&self, nodes: impl IntoIterator<Item = usize>) -> Aabb {
        let mut b = Aabb::EMPTY;
        for i in nodes {
            for p in &self.nodes[i].prims {
                for t in &p.tris {
                    for &v in t {
                        b.add(p.positions[v as usize]);
                    }
                }
            }
        }
        b
    }

    /// Bounding box of the geometry under node `i` (same LOD, no collision).
    pub fn subtree_bounds(&self, i: usize) -> Aabb {
        let lod = self.nodes[i].lod;
        self.bounds(
            self.subtree(i)
                .filter(|&j| self.nodes[j].lod == lod && !self.nodes[j].collision),
        )
    }

    /// Calls `f` for every triangle of `nodes`.
    pub fn for_each_triangle(
        &self,
        nodes: impl IntoIterator<Item = usize>,
        mut f: impl FnMut([Vec3; 3]),
    ) {
        for i in nodes {
            for p in &self.nodes[i].prims {
                for t in 0..p.tris.len() {
                    f(p.triangle(t));
                }
            }
        }
    }
}

/// Splits a name into lowercase alphanumeric tokens.
fn tokens(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|t| !t.is_empty())
        .map(str::to_ascii_lowercase)
}

/// LOD level named by a `lodN` token.
pub fn lod_of(name: &str) -> Option<u8> {
    tokens(name).find_map(|t| t.strip_prefix("lod").and_then(|n| n.parse().ok()))
}

/// Strips a trailing `_LODn`, `.lodn` or `-LODn` suffix.
pub fn strip_lod(name: &str) -> &str {
    if let Some(pos) = name.rfind(['_', '.', '-']) {
        let suffix = name[pos + 1..].to_ascii_lowercase();
        if suffix
            .strip_prefix("lod")
            .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
        {
            return &name[..pos];
        }
    }
    name
}

fn visit(
    model: &mut Model,
    blob: Option<&[u8]>,
    node: gltf::Node,
    parent: Option<usize>,
    parent_world: Mat4,
    parent_lod: u8,
    parent_collision: bool,
) {
    let name = node.name().unwrap_or("").to_string();
    let base = strip_lod(&name).to_string();
    let world = parent_world * Mat4::from_cols_array_2d(&node.transform().matrix());
    let lod = lod_of(&name).unwrap_or(parent_lod);
    let collision = parent_collision || base.to_ascii_lowercase().starts_with("collision");
    let mut prims = Vec::new();
    if let Some(mesh) = node.mesh() {
        let normal_matrix = Mat3::from_mat4(world).inverse().transpose();
        for prim in mesh.primitives() {
            if prim.mode() != gltf::mesh::Mode::Triangles {
                model.warnings.push(format!(
                    "node {name:?}: primitive mode {:?} ignored (only triangle lists are checked)",
                    prim.mode()
                ));
                continue;
            }
            let reader = prim.reader(|b| match b.source() {
                Source::Bin => blob,
                Source::Uri(_) => None,
            });
            let Some(positions) = reader.read_positions() else {
                model.warnings.push(format!(
                    "node {name:?}: primitive without readable positions"
                ));
                continue;
            };
            let positions: Vec<Vec3> = positions
                .map(|p| world.transform_point3(Vec3::from(p)))
                .collect();
            let normals = reader.read_normals().map(|n| {
                n.map(|n| (normal_matrix * Vec3::from(n)).normalize_or_zero())
                    .collect()
            });
            let uvs = reader
                .read_tex_coords(0)
                .map(|t| t.into_f32().map(Vec2::from).collect());
            let indices: Vec<u32> = match reader.read_indices() {
                Some(i) => i.into_u32().collect(),
                None => (0..positions.len() as u32).collect(),
            };
            let n = positions.len() as u32;
            let tris = indices
                .chunks_exact(3)
                .map(|c| [c[0], c[1], c[2]])
                .filter(|t| t.iter().all(|&v| v < n))
                .collect();
            prims.push(Prim {
                positions,
                normals,
                uvs,
                tris,
                material: prim.material().name().map(str::to_string),
                double_sided: prim.material().double_sided(),
            });
        }
    }
    let index = model.nodes.len();
    model.nodes.push(Node {
        name,
        base,
        parent,
        end: index + 1,
        world,
        prims,
        lod,
        collision,
    });
    for child in node.children() {
        visit(model, blob, child, Some(index), world, lod, collision);
    }
    model.nodes[index].end = model.nodes.len();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lod_names() {
        assert_eq!(lod_of("body_LOD1"), Some(1));
        assert_eq!(lod_of("LOD2"), Some(2));
        assert_eq!(lod_of("body"), None);
        assert_eq!(lod_of("lodge"), None);
        assert_eq!(strip_lod("wheel_fl_LOD2"), "wheel_fl");
        assert_eq!(strip_lod("body.lod1"), "body");
        assert_eq!(strip_lod("body"), "body");
        assert_eq!(strip_lod("door_l"), "door_l");
    }
}
