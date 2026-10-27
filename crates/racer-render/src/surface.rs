//! Wet-capable ground surface material: `StandardMaterial` extended with
//! procedural road markings and the global wetness model (PLAN.md section 6).
//!
//! The same extension shades roads ([`SurfaceMode::Road`]), kerbs and
//! sidewalks ([`SurfaceMode::Plain`]) and alpha-tested road decals
//! ([`SurfaceMode::Decal`]), so wetness and puddles stay consistent across
//! them. [`Wetness`] is a resource; changing it updates every surface.

use bevy::{
    asset::embedded_asset,
    pbr::{ExtendedMaterial, MaterialExtension},
    prelude::*,
    render::render_resource::{AsBindGroup, ShaderType},
    shader::ShaderRef,
};

/// Ground material: Bevy's PBR plus [`Surface`].
pub type SurfaceMaterial = ExtendedMaterial<StandardMaterial, Surface>;

/// Registers [`SurfaceMaterial`] and keeps it in sync with [`Wetness`].
pub struct SurfacePlugin;

impl Plugin for SurfacePlugin {
    fn build(&self, app: &mut App) {
        embedded_asset!(app, "surface.wgsl");
        app.add_plugins(MaterialPlugin::<SurfaceMaterial>::default())
            .init_resource::<Wetness>()
            .add_systems(PostUpdate, apply_wetness);
    }
}

/// Global surface wetness: 0 dry, 1 soaked. `puddles` (0..1) scales how much
/// of the ground standing water covers once wet.
#[derive(Resource, Clone, Copy, Debug, PartialEq)]
pub struct Wetness {
    pub wetness: f32,
    pub puddles: f32,
}

impl Default for Wetness {
    fn default() -> Self {
        Self {
            wetness: 0.0,
            puddles: 0.5,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SurfaceMode {
    /// Carriageway with markings; UV0 = (lateral offset, arc length) in metres.
    Road = 0,
    /// Kerbs, sidewalks, gutters: wetness only.
    Plain = 1,
    /// Alpha-tested decal on the road (patches, cracks, manholes).
    Decal = 2,
}

/// Marking flags for [`SurfaceParams::flags`].
pub mod flags {
    pub const CENTRE_LINE: u32 = 1;
    pub const LANE_LINES: u32 = 2;
    pub const EDGE_LINES: u32 = 4;
    pub const WHEEL_TRACKS: u32 = 8;
    pub const GUTTER: u32 = 16;
    pub const ALL: u32 = 31;
}

/// Kinds of transverse markings in [`SurfaceParams::crossings`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Crossing {
    Crosswalk = 1,
    StopLine = 2,
    Chequered = 3,
}

#[derive(ShaderType, Reflect, Clone, Copy, Debug, PartialEq)]
pub struct SurfaceParams {
    pub wetness: f32,
    pub puddles: f32,
    pub mode: u32,
    pub lanes_per_side: u32,
    pub lane_width: f32,
    /// Distance from the centreline to the carriageway edge (gutter starts).
    pub carriage_half: f32,
    pub flags: u32,
    pub seed: f32,
    /// Up to four transverse markings: (s0, s1, kind, half width).
    pub crossings: [Vec4; 4],
}

impl Default for SurfaceParams {
    fn default() -> Self {
        Self {
            wetness: 0.0,
            puddles: 0.5,
            mode: SurfaceMode::Plain as u32,
            lanes_per_side: 2,
            lane_width: 3.4,
            carriage_half: 6.8,
            flags: 0,
            seed: 0.0,
            crossings: [Vec4::ZERO; 4],
        }
    }
}

impl SurfaceParams {
    pub fn plain() -> Self {
        Self::default()
    }

    pub fn decal() -> Self {
        Self {
            mode: SurfaceMode::Decal as u32,
            ..default()
        }
    }

    pub fn road(lanes_per_side: u32, lane_width: f32, flags: u32) -> Self {
        Self {
            mode: SurfaceMode::Road as u32,
            lanes_per_side,
            lane_width,
            carriage_half: lanes_per_side as f32 * lane_width,
            flags,
            ..default()
        }
    }

    /// Adds a transverse marking between arc lengths `s0..s1` over
    /// `|u| < half_width`; returns false when all four slots are used.
    pub fn add_crossing(&mut self, kind: Crossing, s0: f32, s1: f32, half_width: f32) -> bool {
        match self.crossings.iter_mut().find(|c| c.z == 0.0) {
            Some(slot) => {
                *slot = Vec4::new(s0, s1, kind as u32 as f32, half_width);
                true
            }
            None => false,
        }
    }
}

#[derive(Asset, AsBindGroup, Reflect, Debug, Clone, Default)]
pub struct Surface {
    #[uniform(100)]
    pub params: SurfaceParams,
}

impl MaterialExtension for Surface {
    fn fragment_shader() -> ShaderRef {
        "embedded://racer_render/surface.wgsl".into()
    }
}

fn apply_wetness(wetness: Res<Wetness>, mut materials: ResMut<Assets<SurfaceMaterial>>) {
    if !wetness.is_changed() {
        return;
    }
    for (_, m) in materials.iter_mut() {
        m.extension.params.wetness = wetness.wetness;
        m.extension.params.puddles = wetness.puddles;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crossings_fill_four_slots() {
        let mut p = SurfaceParams::road(2, 3.4, flags::ALL);
        assert_eq!(p.carriage_half, 6.8);
        for i in 0..4 {
            assert!(p.add_crossing(Crossing::Crosswalk, i as f32, i as f32 + 3.0, 7.0));
        }
        assert!(!p.add_crossing(Crossing::StopLine, 0.0, 1.0, 7.0));
    }

    #[test]
    fn wetness_reaches_every_surface() {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .add_plugins(AssetPlugin::default())
            .init_asset::<SurfaceMaterial>()
            .init_resource::<Wetness>()
            .add_systems(PostUpdate, apply_wetness);
        let h = app
            .world_mut()
            .resource_mut::<Assets<SurfaceMaterial>>()
            .add(SurfaceMaterial {
                base: StandardMaterial::default(),
                extension: Surface::default(),
            });
        app.world_mut().resource_mut::<Wetness>().wetness = 0.8;
        app.update();
        let m = app.world().resource::<Assets<SurfaceMaterial>>();
        assert_eq!(m.get(&h).unwrap().extension.params.wetness, 0.8);
    }
}
