//! Static batching: merges generated parts into per-cell, per-material
//! meshes (one draw per cell × material × LOD) and spawns them with
//! visibility ranges for LOD switching.

use std::collections::BTreeMap;

use bevy::{
    camera::visibility::VisibilityRange,
    light::NotShadowCaster,
    math::Affine3A,
    prelude::*,
};
use racer_render::surface::SurfaceMaterial;

use crate::{
    kit::{Parts, Slot},
    materials::{KitMaterials, Mat},
    mesh::MeshData,
};

/// Material variant of a bin: plain slot, a tint, or an explicit material.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Variant {
    Slot,
    Tint([u32; 3]),
    /// Index into [`Batcher::custom`].
    Custom(usize),
}

/// Distance band a bin is visible in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Band {
    /// Always visible (roads, buildings).
    All,
    /// LOD0: up to `NEAR` metres.
    Near,
    /// LOD1: from `NEAR` to `FAR` metres.
    Far,
    /// Small props: only up to `NEAR` metres, nothing beyond.
    NearOnly,
}

pub const NEAR: f32 = 70.0;
pub const FAR: f32 = 450.0;

type Key = (i32, i32, Band, Slot, Variant);

/// Accumulates world-space meshes into grid cells.
#[derive(Default)]
pub struct Batcher {
    pub cell: f32,
    bins: BTreeMap<Key, MeshData>,
    pub custom: Vec<Mat>,
    /// Slots that should not cast shadows.
    pub no_shadow: Vec<Slot>,
}

/// Counts of what was spawned.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BatchStats {
    pub entities: usize,
    pub triangles_lod0: usize,
    pub triangles_far: usize,
}

impl Batcher {
    pub fn new(cell: f32) -> Self {
        Self {
            cell,
            no_shadow: vec![Slot::Road, Slot::RoadPlain, Slot::Decal, Slot::Sidewalk, Slot::CurbPaint, Slot::Window, Slot::ShopGlass, Slot::ShopSign],
            ..default()
        }
    }

    fn cell_of(&self, p: Vec3) -> (i32, i32) {
        ((p.x / self.cell).floor() as i32, (p.z / self.cell).floor() as i32)
    }

    /// Adds `parts` transformed by `t`, binned by the transformed origin.
    pub fn add(&mut self, parts: &Parts, t: Affine3A, band: Band, variant: impl Fn(Slot) -> Variant) {
        let (cx, cz) = self.cell_of(t.translation.into());
        for (slot, mesh) in &parts.by_slot {
            let m = mesh.transformed(t);
            self.bins.entry((cx, cz, band, *slot, variant(*slot))).or_default().append(&m);
        }
    }

    /// Adds world-space parts, binning each slot mesh by its bounds centre
    /// (for long sweeps such as barriers, call per segment).
    pub fn add_world(&mut self, parts: &Parts, band: Band, variant: impl Fn(Slot) -> Variant) {
        for (slot, mesh) in &parts.by_slot {
            let Some((lo, hi)) = mesh.bounds() else { continue };
            let (cx, cz) = self.cell_of((lo + hi) / 2.0);
            self.bins.entry((cx, cz, band, *slot, variant(*slot))).or_default().append(mesh);
        }
    }

    /// Adds one mesh with an explicit material.
    pub fn add_mesh(&mut self, mesh: MeshData, slot: Slot, mat: Mat, band: Band) {
        let Some((lo, hi)) = mesh.bounds() else { return };
        let (cx, cz) = self.cell_of((lo + hi) / 2.0);
        self.custom.push(mat);
        let v = Variant::Custom(self.custom.len() - 1);
        self.bins.entry((cx, cz, band, slot, v)).or_default().append(&mesh);
    }

    pub fn stats(&self) -> BatchStats {
        let mut s = BatchStats::default();
        for ((_, _, band, _, _), m) in &self.bins {
            s.entities += 1;
            if *band != Band::Far {
                s.triangles_lod0 += m.triangle_count();
            }
            if matches!(band, Band::All | Band::Far) {
                s.triangles_far += m.triangle_count();
            }
        }
        s
    }

    /// Spawns one entity per bin.
    pub fn spawn(
        self,
        commands: &mut Commands,
        meshes: &mut Assets<Mesh>,
        kit: &mut KitMaterials,
        std_mats: &mut Assets<StandardMaterial>,
        parent: Option<Entity>,
    ) -> BatchStats {
        let stats = self.stats();
        for ((_, _, band, slot, variant), mesh) in self.bins {
            if mesh.is_empty() {
                continue;
            }
            let mat = match variant {
                Variant::Slot => kit.get(slot),
                Variant::Tint(t) => kit.tinted(slot, t.map(f32::from_bits), std_mats),
                Variant::Custom(i) => self.custom[i].clone(),
            };
            // Mesh relative to its bounds centre so visibility ranges
            // measure from the right place.
            let (lo, hi) = mesh.bounds().expect("non-empty");
            let centre = (lo + hi) / 2.0;
            let handle = meshes.add(mesh.translated(-centre).to_bevy());
            let mut e = commands.spawn((Mesh3d(handle), Transform::from_translation(centre)));
            match mat {
                Mat::Std(h) => e.insert(MeshMaterial3d::<StandardMaterial>(h)),
                Mat::Surface(h) => e.insert(MeshMaterial3d::<SurfaceMaterial>(h)),
            };
            match band {
                Band::All => {}
                Band::Near => {
                    e.insert(VisibilityRange { start_margin: 0.0..0.0, end_margin: NEAR..NEAR + 8.0, use_aabb: false });
                }
                Band::Far => {
                    e.insert(VisibilityRange { start_margin: NEAR..NEAR + 8.0, end_margin: FAR..FAR + 20.0, use_aabb: false });
                }
                Band::NearOnly => {
                    e.insert(VisibilityRange { start_margin: 0.0..0.0, end_margin: NEAR + 40.0..NEAR + 50.0, use_aabb: false });
                }
            }
            if self.no_shadow.contains(&slot) {
                e.insert(NotShadowCaster);
            }
            if let Some(p) = parent {
                e.insert(ChildOf(p));
            }
        }
        stats
    }
}

/// Packs an sRGB tint for [`Variant::Tint`].
pub fn tint(c: [f32; 3]) -> Variant {
    Variant::Tint(c.map(f32::to_bits))
}
