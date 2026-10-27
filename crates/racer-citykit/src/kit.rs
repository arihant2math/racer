//! Kit-wide types: material slots, assets with LODs and sockets.

use std::{collections::BTreeMap, ops::Range};

use bevy::math::Vec3;

use crate::{
    mesh::MeshData,
    validate::{Budget, Part, UvPolicy},
};

/// Material slots. Every generated mesh names one; the Bevy side maps each
/// slot to a material (see `materials.rs`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    // Ground (surface shader with wetness)
    Road,
    RoadPlain,
    Sidewalk,
    Kerb,
    Decal,
    CurbPaint,
    // Racing furniture
    Concrete,
    Galvanised,
    Fence,
    Rubber,
    Chequer,
    BoardWhite,
    BoardDark,
    Led,
    SeatBlue,
    /// Printed fence scrims (sign atlas, not emissive).
    Banner,
    /// Spectators (tinted per instance).
    Crowd,
    // Street furniture
    DarkMetal,
    PaintYellow,
    PaintRed,
    PaintWhite,
    Wood,
    LampLens,
    SignalRed,
    SignalAmber,
    SignalGreen,
    SignalOff,
    RoadSign,
    Glass,
    Lightbox,
    CarPaint,
    CarGlass,
    Foliage,
    /// Alpha-tested leaf cards.
    Leaves,
    Bark,
    // Buildings
    Brick,
    Plaster,
    Stone,
    Window,
    ShopSign,
    ShopGlass,
    CurtainGlass,
    Mullion,
    Roof,
    RoofMetal,
    Awning,
    /// Distant HLOD tower façades.
    Skyline,
}

impl Slot {
    pub fn uv_policy(self) -> UvPolicy {
        match self {
            Slot::Decal
            | Slot::Window
            | Slot::ShopSign
            | Slot::RoadSign
            | Slot::Lightbox
            | Slot::ShopGlass
            | Slot::Banner => UvPolicy::Atlas,
            _ => UvPolicy::Tiling,
        }
    }
}

/// Collects meshes per material slot. Pieces added with
/// [`Parts::add_closed`] are remembered (as index ranges) so the validator
/// can check each of them for watertightness on its own.
#[derive(Clone, Debug, Default)]
pub struct Parts {
    pub by_slot: BTreeMap<Slot, MeshData>,
    pub closed: BTreeMap<Slot, Vec<Range<usize>>>,
}

impl Parts {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds an open (not necessarily watertight) mesh.
    pub fn add(&mut self, slot: Slot, mesh: MeshData) -> &mut Self {
        self.by_slot.entry(slot).or_default().append(&mesh);
        self
    }

    /// Adds a mesh that must be watertight.
    pub fn add_closed(&mut self, slot: Slot, mesh: MeshData) -> &mut Self {
        let target = self.by_slot.entry(slot).or_default();
        let start = target.indices.len();
        target.append(&mesh);
        let end = target.indices.len();
        self.closed.entry(slot).or_default().push(start..end);
        self
    }

    pub fn merge(&mut self, other: &Parts, t: bevy::math::Affine3A) {
        for (slot, m) in &other.by_slot {
            let target = self.by_slot.entry(*slot).or_default();
            let base = target.indices.len();
            target.append(&m.transformed(t));
            if let Some(ranges) = other.closed.get(slot) {
                self.closed
                    .entry(*slot)
                    .or_default()
                    .extend(ranges.iter().map(|r| r.start + base..r.end + base));
            }
        }
    }

    pub fn triangles(&self) -> usize {
        self.by_slot.values().map(MeshData::triangle_count).sum()
    }

    pub fn into_parts(self) -> Vec<Part> {
        let mut closed = self.closed;
        self.by_slot
            .into_iter()
            .filter(|(_, m)| !m.is_empty())
            .map(|(material, mesh)| Part {
                material,
                closed: closed.remove(&material).unwrap_or_default(),
                mesh,
            })
            .collect()
    }
}

/// What a socket is for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SocketKind {
    /// Spot light aimed along the socket direction.
    SpotLight {
        outer_angle_deg: f32,
        range: f32,
        lumens: f32,
        kelvin: f32,
    },
    PointLight {
        range: f32,
        lumens: f32,
        kelvin: f32,
    },
    /// A light that is purely emissive (signal lens) - no Bevy light.
    Emissive,
}

/// Named attachment point in asset space.
#[derive(Clone, Debug, PartialEq)]
pub struct Socket {
    pub name: String,
    pub kind: SocketKind,
    pub pos: Vec3,
    pub dir: Vec3,
}

/// One kit asset: LOD chain (LOD0 first), sockets and its budget.
#[derive(Clone, Debug)]
pub struct Asset {
    pub name: String,
    pub lods: Vec<Vec<Part>>,
    /// Distances (m) at which LOD k+1 takes over from LOD k.
    pub lod_distances: Vec<f32>,
    pub sockets: Vec<Socket>,
    pub budget: Budget,
}

impl Asset {
    pub fn triangles(&self, lod: usize) -> usize {
        self.lods
            .get(lod)
            .map_or(0, |p| p.iter().map(|p| p.mesh.triangle_count()).sum())
    }
}

/// Approximate colour temperature to linear RGB (Tanner Helland's fit).
pub fn kelvin_to_rgb(k: f32) -> Vec3 {
    let t = k / 100.0;
    let r = if t <= 66.0 {
        1.0
    } else {
        (1.292_936_2 * (t - 60.0).powf(-0.133_204_76)).clamp(0.0, 1.0)
    };
    let g = if t <= 66.0 {
        (0.390_081_58 * t.ln() - 0.631_841_4).clamp(0.0, 1.0)
    } else {
        (1.129_890_9 * (t - 60.0).powf(-0.075_514_85)).clamp(0.0, 1.0)
    };
    let b = if t >= 66.0 {
        1.0
    } else if t <= 19.0 {
        0.0
    } else {
        (0.543_206_8 * (t - 10.0).ln() - 1.196_254_1).clamp(0.0, 1.0)
    };
    Vec3::new(r, g, b)
}
