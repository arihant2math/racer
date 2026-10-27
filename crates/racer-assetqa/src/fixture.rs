//! Synthetic calibration fixtures: a minimal GLB writer and a box car built to
//! the asset contract from a [`CarSpec`], with variants that each break one
//! rule. `cargo test` proves every gate passes the good car and catches its
//! fault; `racer-assetqa fixture` writes them for manual runs.

use glam::{Mat4, Vec2, Vec3};
use serde_json::{Value, json};

use crate::{raster::View, spec::CarSpec};

/// A triangle list ready for export (local space of its node).
#[derive(Clone, Debug, Default)]
pub struct PrimData {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    pub uvs: Option<Vec<Vec2>>,
    pub indices: Vec<u32>,
    pub material: String,
}

impl PrimData {
    fn new(material: &str) -> PrimData {
        PrimData {
            material: material.to_string(),
            uvs: Some(Vec::new()),
            ..PrimData::default()
        }
    }

    fn vertex(&mut self, p: Vec3, n: Vec3, uv: Vec2) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        if let Some(uvs) = &mut self.uvs {
            uvs.push(uv);
        }
        self.positions.len() as u32 - 1
    }

    fn flip(&mut self) {
        for t in self.indices.chunks_exact_mut(3) {
            t.swap(1, 2);
        }
        for n in &mut self.normals {
            *n = -*n;
        }
    }
}

/// Builds a binary glTF.
#[derive(Default)]
pub struct Glb {
    nodes: Vec<Value>,
    meshes: Vec<Value>,
    accessors: Vec<Value>,
    views: Vec<Value>,
    materials: Vec<String>,
    bin: Vec<u8>,
    roots: Vec<usize>,
}

impl Glb {
    fn material(&mut self, name: &str) -> usize {
        if let Some(i) = self.materials.iter().position(|m| m == name) {
            return i;
        }
        self.materials.push(name.to_string());
        self.materials.len() - 1
    }

    fn view(&mut self, bytes: &[u8], target: u32) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        self.views.push(json!({
            "buffer": 0, "byteOffset": self.bin.len(), "byteLength": bytes.len(), "target": target
        }));
        self.bin.extend_from_slice(bytes);
        self.views.len() - 1
    }

    fn accessor_f32(&mut self, data: &[f32], kind: &str, count: usize, minmax: bool) -> usize {
        let bytes: Vec<u8> = data.iter().flat_map(|f| f.to_le_bytes()).collect();
        let view = self.view(&bytes, 34962);
        let mut a = json!({
            "bufferView": view, "componentType": 5126, "count": count, "type": kind
        });
        if minmax {
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for c in data.chunks_exact(3) {
                for k in 0..3 {
                    min[k] = min[k].min(c[k]);
                    max[k] = max[k].max(c[k]);
                }
            }
            a["min"] = json!(min);
            a["max"] = json!(max);
        }
        self.accessors.push(a);
        self.accessors.len() - 1
    }

    pub fn mesh(&mut self, name: &str, prims: &[PrimData]) -> usize {
        let mut out = Vec::new();
        for p in prims {
            let pos: Vec<f32> = p.positions.iter().flat_map(|v| v.to_array()).collect();
            let nrm: Vec<f32> = p.normals.iter().flat_map(|v| v.to_array()).collect();
            let mut attrs = json!({
                "POSITION": self.accessor_f32(&pos, "VEC3", p.positions.len(), true),
                "NORMAL": self.accessor_f32(&nrm, "VEC3", p.normals.len(), false),
            });
            if let Some(uvs) = &p.uvs {
                let uv: Vec<f32> = uvs.iter().flat_map(|v| v.to_array()).collect();
                attrs["TEXCOORD_0"] = json!(self.accessor_f32(&uv, "VEC2", uvs.len(), false));
            }
            let idx: Vec<u8> = p.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
            let view = self.view(&idx, 34963);
            self.accessors.push(json!({
                "bufferView": view, "componentType": 5125, "count": p.indices.len(), "type": "SCALAR"
            }));
            let indices = self.accessors.len() - 1;
            let material = self.material(&p.material);
            out.push(json!({"attributes": attrs, "indices": indices, "material": material}));
        }
        self.meshes.push(json!({"name": name, "primitives": out}));
        self.meshes.len() - 1
    }

    /// Adds a node; `root` puts it in the scene's root list.
    pub fn node(
        &mut self,
        name: &str,
        translation: Vec3,
        mesh: Option<usize>,
        children: Vec<usize>,
        root: bool,
    ) -> usize {
        let mut n = json!({"name": name});
        if translation != Vec3::ZERO {
            n["translation"] = json!(translation.to_array());
        }
        if let Some(m) = mesh {
            n["mesh"] = json!(m);
        }
        if !children.is_empty() {
            n["children"] = json!(children);
        }
        self.nodes.push(n);
        let i = self.nodes.len() - 1;
        if root {
            self.roots.push(i);
        }
        i
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let materials: Vec<Value> = self
            .materials
            .iter()
            .map(|m| {
                let colour = match m.as_str() {
                    "glass" => [0.1, 0.12, 0.15, 0.4],
                    "rubber" | "trim_black" => [0.03, 0.03, 0.03, 1.0],
                    "rim" | "chrome" => [0.8, 0.8, 0.82, 1.0],
                    "light_emissive_front" => [1.0, 1.0, 0.95, 1.0],
                    "light_emissive_rear" => [0.8, 0.02, 0.02, 1.0],
                    "interior" => [0.2, 0.15, 0.1, 1.0],
                    _ => [0.75, 0.05, 0.05, 1.0],
                };
                json!({"name": m, "pbrMetallicRoughness": {"baseColorFactor": colour, "roughnessFactor": 0.4}})
            })
            .collect();
        let doc = json!({
            "asset": {"version": "2.0", "generator": "racer-assetqa fixture"},
            "scene": 0,
            "scenes": [{"nodes": self.roots}],
            "nodes": self.nodes,
            "meshes": self.meshes,
            "materials": materials,
            "accessors": self.accessors,
            "bufferViews": self.views,
            "buffers": [{"byteLength": self.bin.len()}],
        });
        let mut json = serde_json::to_vec(&doc).expect("json");
        while !json.len().is_multiple_of(4) {
            json.push(b' ');
        }
        let mut bin = self.bin.clone();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + json.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(json.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&json);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
}

/// Texture coordinates per metre on the fixture (uniform texel density).
const UV_SCALE: f32 = 0.25;

/// `min + (max - min) * i / n`, exact at both ends so shared edges weld.
fn lerp(min: f32, max: f32, i: u32, n: u32) -> f32 {
    if i == 0 {
        min
    } else if i == n {
        max
    } else {
        min + (max - min) * i as f32 / n as f32
    }
}

/// Appends an axis-aligned box with `n` × `n` quads per face, wound outwards.
pub fn add_box(p: &mut PrimData, min: Vec3, max: Vec3, n: u32) {
    // (normal axis, sign, u axis, v axis) with u × v = sign · normal.
    const FACES: [(usize, f32, usize, usize); 6] = [
        (0, 1.0, 1, 2),
        (0, -1.0, 2, 1),
        (1, 1.0, 2, 0),
        (1, -1.0, 0, 2),
        (2, 1.0, 0, 1),
        (2, -1.0, 1, 0),
    ];
    for (axis, sign, ua, va) in FACES {
        let mut normal = Vec3::ZERO;
        normal[axis] = sign;
        let base = p.positions.len() as u32;
        for j in 0..=n {
            for i in 0..=n {
                let mut v = Vec3::ZERO;
                v[axis] = if sign > 0.0 { max[axis] } else { min[axis] };
                v[ua] = lerp(min[ua], max[ua], i, n);
                v[va] = lerp(min[va], max[va], j, n);
                p.vertex(v, normal, Vec2::new(v[ua], v[va]) * UV_SCALE);
            }
        }
        for j in 0..n {
            for i in 0..n {
                let a = base + j * (n + 1) + i;
                let (b, c, d) = (a + 1, a + n + 2, a + n + 1);
                p.indices.extend([a, b, c, a, c, d]);
            }
        }
    }
}

/// A wheel (axis X) centred on the origin: rubber tread and two rim caps.
pub fn wheel_prims(radius: f32, width: f32, segments: u32, rings: u32) -> Vec<PrimData> {
    let hw = width / 2.0;
    let angle = |i: u32| (i % segments) as f32 / segments as f32 * std::f32::consts::TAU;
    let ring = |i: u32, r: f32, x: f32| {
        let a = angle(i);
        Vec3::new(x, r * a.cos(), r * a.sin())
    };
    let mut tread = PrimData::new("rubber");
    for j in 0..=rings {
        let x = lerp(-hw, hw, j, rings);
        for i in 0..=segments {
            let p = ring(i, radius, x);
            let n = Vec3::new(0.0, p.y, p.z).normalize();
            tread.vertex(p, n, Vec2::new(angle(i) * radius, x) * UV_SCALE);
        }
    }
    for j in 0..rings {
        for i in 0..segments {
            let a = j * (segments + 1) + i;
            let (b, c, d) = (a + 1, a + segments + 2, a + segments + 1);
            tread.indices.extend([a, b, c, a, c, d]);
        }
    }
    let mut caps = PrimData::new("rim");
    let cap_rings = (rings / 3).max(1);
    for side in [1.0f32, -1.0] {
        let x = side * hw;
        let normal = Vec3::X * side;
        let centre = caps.vertex(Vec3::new(x, 0.0, 0.0), normal, Vec2::ZERO);
        let mut prev: Option<u32> = None;
        for k in 1..=cap_rings {
            let r = lerp(0.0, radius, k, cap_rings);
            let base = caps.positions.len() as u32;
            for i in 0..segments {
                let p = ring(i, r, x);
                caps.vertex(p, normal, Vec2::new(p.y, p.z) * UV_SCALE);
            }
            for i in 0..segments {
                let (i0, i1) = (base + i, base + (i + 1) % segments);
                let tri = |a: u32, b: u32, c: u32| if side > 0.0 { [a, b, c] } else { [a, c, b] };
                match prev {
                    None => caps.indices.extend(tri(centre, i0, i1)),
                    Some(pb) => {
                        let (p0, p1) = (pb + i, pb + (i + 1) % segments);
                        caps.indices.extend(tri(p0, i0, i1));
                        caps.indices.extend(tri(p0, i1, p1));
                    }
                }
            }
            prev = Some(base);
        }
    }
    vec![tread, caps]
}

/// One fault per variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fault {
    None,
    /// Exported in centimetres.
    WrongScale,
    /// Rear wheels (and their sockets) left out.
    MissingWheels,
    /// Body wound inside out with inverted normals.
    FlippedNormals,
    /// An internal partition welded into the body box.
    NonManifoldBody,
    /// Rotated 180° about Y: the car faces +Z.
    WrongOrientation,
    /// Z-up export (no glTF axis conversion).
    ZUp,
    /// Body exported without texture coordinates.
    NoUvs,
    /// Collision hull with a dent in its roof.
    ConcaveCollision,
    /// Everything 8 % too long (wheelbase included).
    Stretched,
}

impl Fault {
    pub const ALL: [Fault; 10] = [
        Fault::None,
        Fault::WrongScale,
        Fault::MissingWheels,
        Fault::FlippedNormals,
        Fault::NonManifoldBody,
        Fault::WrongOrientation,
        Fault::ZUp,
        Fault::NoUvs,
        Fault::ConcaveCollision,
        Fault::Stretched,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Fault::None => "good",
            Fault::WrongScale => "wrong-scale",
            Fault::MissingWheels => "missing-wheels",
            Fault::FlippedNormals => "flipped-normals",
            Fault::NonManifoldBody => "non-manifold-body",
            Fault::WrongOrientation => "wrong-orientation",
            Fault::ZUp => "z-up",
            Fault::NoUvs => "no-uvs",
            Fault::ConcaveCollision => "concave-collision",
            Fault::Stretched => "stretched",
        }
    }

    /// Transform baked into every position and node translation.
    fn transform(self) -> Mat4 {
        match self {
            Fault::WrongScale => Mat4::from_scale(Vec3::splat(100.0)),
            Fault::WrongOrientation => Mat4::from_rotation_y(std::f32::consts::PI),
            Fault::ZUp => Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Fault::Stretched => Mat4::from_scale(Vec3::new(1.0, 1.0, 1.08)),
            _ => Mat4::IDENTITY,
        }
    }
}

/// Shape of a fixture part, in car space.
#[derive(Clone, Copy, Debug)]
pub enum Shape {
    Box(Vec3, Vec3),
    Wheel { hub: Vec3, radius: f32, width: f32 },
}

#[derive(Clone, Debug)]
pub struct Part {
    pub name: String,
    pub material: &'static str,
    pub shapes: Vec<Shape>,
    /// Quads per box face at LOD0.
    pub detail: u32,
}

/// The box car's parts, laid out to `spec` (front overhang 0.94 m).
pub fn parts(spec: &CarSpec) -> Vec<Part> {
    let hw = spec.width / 2.0;
    let zf = -spec.wheelbase / 2.0 - 0.94;
    let zr = zf + spec.length;
    let b = |x0: f32, y0: f32, z0: f32, x1: f32, y1: f32, z1: f32| {
        Shape::Box(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1))
    };
    let part = |name: &str, material: &'static str, shapes: Vec<Shape>, detail: u32| Part {
        name: name.to_string(),
        material,
        shapes,
        detail,
    };
    let mut parts = vec![
        part(
            "body",
            "car_paint",
            vec![
                b(-hw + 0.01, 0.15, zf + 0.20, hw - 0.01, 0.80, zr - 0.18),
                b(-0.70, 0.80, -0.70, 0.70, spec.height, 1.30),
            ],
            64,
        ),
        part(
            "bumper_front",
            "car_paint",
            vec![b(-0.90, 0.12, zf, 0.90, 0.62, zf + 0.20)],
            12,
        ),
        part(
            "bumper_rear",
            "car_paint",
            vec![b(-0.90, 0.15, zr - 0.18, 0.90, 0.62, zr)],
            12,
        ),
        part(
            "hood",
            "car_paint",
            vec![b(-0.70, 0.80, zf + 0.25, 0.70, 0.82, -0.75)],
            12,
        ),
        part(
            "door_l",
            "car_paint",
            vec![b(-hw, 0.30, -0.60, -hw + 0.01, 0.78, 0.50)],
            12,
        ),
        part(
            "door_r",
            "car_paint",
            vec![b(hw - 0.01, 0.30, -0.60, hw, 0.78, 0.50)],
            12,
        ),
        part(
            "mirror_l",
            "chrome",
            vec![b(-1.012, 0.86, -0.62, -hw, 0.95, -0.50)],
            12,
        ),
        part(
            "mirror_r",
            "chrome",
            vec![b(hw, 0.86, -0.62, 1.012, 0.95, -0.50)],
            12,
        ),
        part(
            "spoiler",
            "trim_black",
            vec![b(-0.60, 0.80, 1.95, 0.60, 0.85, zr - 0.20)],
            12,
        ),
        part(
            "glass",
            "glass",
            vec![b(-0.66, 0.86, -0.71, 0.66, 1.24, -0.70)],
            12,
        ),
        part(
            "lights_front",
            "light_emissive_front",
            vec![
                b(-0.80, 0.40, zf - 0.005, -0.55, 0.52, zf + 0.02),
                b(0.55, 0.40, zf - 0.005, 0.80, 0.52, zf + 0.02),
            ],
            12,
        ),
        part(
            "lights_rear",
            "light_emissive_rear",
            vec![b(-0.85, 0.45, zr - 0.02, 0.85, 0.55, zr + 0.005)],
            12,
        ),
        part(
            "interior",
            "interior",
            vec![b(-0.60, 0.50, -0.50, 0.60, 1.15, 1.00)],
            12,
        ),
    ];
    for w in ["fl", "fr", "rl", "rr"] {
        let tyre = spec.tyre(w);
        parts.push(part(
            &format!("wheel_{w}"),
            "rubber",
            vec![Shape::Wheel {
                hub: Vec3::from(spec.hub(w)),
                radius: tyre.radius,
                width: tyre.width,
            }],
            0,
        ));
    }
    parts
}

fn part_prims(part: &Part, divisor: u32) -> Vec<PrimData> {
    let mut out = Vec::new();
    for shape in &part.shapes {
        match *shape {
            Shape::Box(min, max) => {
                let mut p = PrimData::new(part.material);
                add_box(&mut p, min, max, (part.detail / divisor).max(1));
                out.push(p);
            }
            Shape::Wheel { radius, width, .. } => {
                out.extend(wheel_prims(
                    radius,
                    width,
                    256 / divisor,
                    (24 / divisor).max(3),
                ));
            }
        }
    }
    out
}

fn transform_prims(prims: &mut [PrimData], m: Mat4) {
    for p in prims {
        for v in &mut p.positions {
            *v = m.transform_point3(*v);
        }
        for n in &mut p.normals {
            *n = m.transform_vector3(*n).normalize();
        }
    }
}

/// Builds the box car with the given fault.
pub fn box_car(spec: &CarSpec, fault: Fault) -> Vec<u8> {
    let m = fault.transform();
    let mut glb = Glb::default();
    let parts = parts(spec);
    for (lod, divisor) in [(0u8, 1u32), (1, 2), (2, 4)] {
        for part in &parts {
            if fault == Fault::MissingWheels && part.name.starts_with("wheel_r") {
                continue;
            }
            let hub = match part.shapes[0] {
                Shape::Wheel { hub, .. } => Some(hub),
                Shape::Box(..) => None,
            };
            let mut prims = part_prims(part, divisor);
            if part.name == "body" {
                if fault == Fault::FlippedNormals {
                    prims.iter_mut().for_each(PrimData::flip);
                }
                if fault == Fault::NoUvs {
                    prims.iter_mut().for_each(|p| p.uvs = None);
                }
                if fault == Fault::NonManifoldBody
                    && let Shape::Box(min, max) = part.shapes[0]
                {
                    // A wall at x = 0 sharing the body's grid lines.
                    let n = part.detail / divisor;
                    let mut wall = PrimData::new("car_paint");
                    for j in 0..=n {
                        for i in 0..=n {
                            let v =
                                Vec3::new(0.0, lerp(min.y, max.y, j, n), lerp(min.z, max.z, i, n));
                            wall.vertex(v, Vec3::X, Vec2::new(v.z, v.y) * UV_SCALE);
                        }
                    }
                    for j in 0..n {
                        for i in 0..n {
                            let a = j * (n + 1) + i;
                            wall.indices
                                .extend([a, a + 1, a + n + 2, a, a + n + 2, a + n + 1]);
                        }
                    }
                    prims.push(wall);
                }
            }
            let name = if lod == 0 {
                part.name.clone()
            } else {
                format!("{}_LOD{lod}", part.name)
            };
            match hub {
                Some(h) => {
                    // Wheels are modelled around their hub and placed by the
                    // node. The fault transform is linear, so it applies to the
                    // geometry and to the translation alike.
                    let lin = Mat4::from_mat3(glam::Mat3::from_mat4(m));
                    transform_prims(&mut prims, lin);
                    let mesh = glb.mesh(&name, &prims);
                    glb.node(&name, m.transform_point3(h), Some(mesh), vec![], true);
                }
                None => {
                    transform_prims(&mut prims, m);
                    let mesh = glb.mesh(&name, &prims);
                    glb.node(&name, Vec3::ZERO, Some(mesh), vec![], true);
                }
            }
        }
    }

    // Collision: one convex box around the body (dented when broken).
    let hw = spec.width / 2.0;
    let zf = -spec.wheelbase / 2.0 - 0.94;
    let (cmin, cmax) = (
        Vec3::new(-hw, 0.12, zf),
        Vec3::new(hw, spec.height, zf + spec.length),
    );
    let mut hull = PrimData::new("car_paint");
    hull.uvs = None;
    add_box(
        &mut hull,
        cmin,
        cmax,
        if fault == Fault::ConcaveCollision {
            2
        } else {
            1
        },
    );
    if fault == Fault::ConcaveCollision {
        let mid = (cmin + cmax) / 2.0;
        for v in &mut hull.positions {
            if v.y == cmax.y && v.x == mid.x && v.z == mid.z {
                v.y -= 0.3;
            }
        }
    }
    transform_prims(std::slice::from_mut(&mut hull), m);
    let mesh = glb.mesh("collision", &[hull]);
    glb.node("collision", Vec3::ZERO, Some(mesh), vec![], true);

    let zr = zf + spec.length;
    let mut sockets: Vec<(String, Vec3)> = ["fl", "fr", "rl", "rr"]
        .iter()
        .filter(|w| !(fault == Fault::MissingWheels && w.starts_with('r')))
        .map(|w| (format!("socket_wheel_{w}"), Vec3::from(spec.hub(w))))
        .collect();
    sockets.extend([
        (
            "socket_light_front_l".to_string(),
            Vec3::new(-0.67, 0.46, zf),
        ),
        (
            "socket_light_front_r".to_string(),
            Vec3::new(0.67, 0.46, zf),
        ),
        ("socket_light_rear_l".to_string(), Vec3::new(-0.7, 0.5, zr)),
        ("socket_light_rear_r".to_string(), Vec3::new(0.7, 0.5, zr)),
        ("socket_exhaust_l".to_string(), Vec3::new(-0.4, 0.25, zr)),
        ("socket_exhaust_r".to_string(), Vec3::new(0.4, 0.25, zr)),
        ("socket_camera_hood".to_string(), Vec3::new(0.0, 0.95, -0.9)),
        (
            "socket_camera_cockpit".to_string(),
            Vec3::new(-0.35, 1.05, 0.2),
        ),
    ]);
    for (name, p) in sockets {
        glb.node(&name, m.transform_point3(p), None, vec![], true);
    }
    // Make sure every contract material exists even if unused by geometry.
    for mat in crate::structure::MATERIALS {
        glb.material(mat);
    }
    glb.to_bytes()
}

/// Whether a view-plane point lies inside the analytic silhouette of the
/// given parts (used to draw synthetic reference blueprints of the box car).
pub fn analytic_inside(parts: &[Part], view: View, p: Vec2) -> bool {
    parts
        .iter()
        .flat_map(|part| &part.shapes)
        .any(|s| match *s {
            Shape::Box(min, max) => {
                let (a, b) = (view.project(min), view.project(max));
                let (lo, hi) = (a.min(b), a.max(b));
                p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y
            }
            Shape::Wheel { hub, radius, width } => {
                let c = view.project(hub);
                if view == View::Side || view == View::SideRight {
                    (p - c.truncate()).length() <= radius
                } else {
                    let half = view.project(Vec3::new(width / 2.0, radius, radius)).abs();
                    (p.x - c.x).abs() <= half.x && (p.y - c.y).abs() <= half.y
                }
            }
        })
}
