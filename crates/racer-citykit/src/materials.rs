//! Bevy materials for every kit slot, built from the procedural textures.
//!
//! [`KitMaterials`] is created once (textures take well under a second to
//! generate in an optimised build) and holds a material per slot. Buildings
//! get tinted copies through [`KitMaterials::tinted`]; roads get their own
//! surface material through [`KitMaterials::road`] because their markings
//! (lanes, crossings) are per-road uniforms.

use std::collections::HashMap;

use bevy::{math::Affine2, prelude::*};
use racer_render::surface::{Surface, SurfaceMaterial, SurfaceParams};

use crate::{
    kit::{Slot, kelvin_to_rgb},
    props::street::STREET_LIGHT_K,
    texture::{self, Encoding, PbrTex, tile},
};

/// Time-of-day lighting preset; drives emissive strengths and exposure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeOfDay {
    Day,
    Golden,
    Night,
}

/// Either material type used by the kit.
#[derive(Clone, Debug)]
pub enum Mat {
    Std(Handle<StandardMaterial>),
    Surface(Handle<SurfaceMaterial>),
}

/// Handles of one PBR texture set.
#[derive(Clone, Debug)]
pub struct PbrHandles {
    pub albedo: Handle<Image>,
    pub normal: Handle<Image>,
    pub orm: Handle<Image>,
}

#[derive(Resource)]
pub struct KitMaterials {
    pub time: TimeOfDay,
    pub wetness: f32,
    pub by_slot: HashMap<Slot, Mat>,
    pub asphalt: PbrHandles,
    tinted: HashMap<(Slot, [u32; 3]), Handle<StandardMaterial>>,
}

fn upload(images: &mut Assets<Image>, t: &PbrTex) -> PbrHandles {
    PbrHandles {
        albedo: images.add(t.albedo.to_image(Encoding::Srgb)),
        normal: images.add(t.normal.to_image(Encoding::Normal)),
        orm: images.add(t.orm.to_image(Encoding::Linear)),
    }
}

fn textured(h: &PbrHandles, tile: Vec2) -> StandardMaterial {
    StandardMaterial {
        base_color_texture: Some(h.albedo.clone()),
        normal_map_texture: Some(h.normal.clone()),
        metallic_roughness_texture: Some(h.orm.clone()),
        occlusion_texture: Some(h.orm.clone()),
        perceptual_roughness: 1.0,
        metallic: 1.0,
        uv_transform: Affine2::from_scale(Vec2::ONE / tile),
        ..default()
    }
}

fn plain(r: f32, g: f32, b: f32, rough: f32, metal: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: Color::srgb(r, g, b),
        perceptual_roughness: rough,
        metallic: metal,
        ..default()
    }
}

fn emissive(base: Color, light: LinearRgba, rough: f32) -> StandardMaterial {
    StandardMaterial {
        base_color: base,
        emissive: light,
        perceptual_roughness: rough,
        ..default()
    }
}

/// Emissive luminance (nits) of lamp lenses, signals, windows and signs.
pub struct Glow {
    pub lamp: f32,
    pub signal: f32,
    pub window: f32,
    pub shop: f32,
    pub sign: f32,
    pub lightbox: f32,
    pub led: f32,
}

impl TimeOfDay {
    pub fn glow(self) -> Glow {
        match self {
            // Interiors are emissive at every time of day (they are lit rooms
            // seen from outside, not sunlit albedo); their luminance only
            // looks different because the exposure changes.
            TimeOfDay::Day => Glow { lamp: 0.0, signal: 6000.0, window: 300.0, shop: 900.0, sign: 0.0, lightbox: 1500.0, led: 5000.0 },
            TimeOfDay::Golden => Glow { lamp: 3000.0, signal: 4000.0, window: 150.0, shop: 450.0, sign: 60.0, lightbox: 800.0, led: 3000.0 },
            TimeOfDay::Night => Glow { lamp: 25000.0, signal: 150.0, window: 2.2, shop: 6.0, sign: 7.0, lightbox: 15.0, led: 100.0 },
        }
    }
}

impl KitMaterials {
    /// Generates the textures and builds every slot's material.
    pub fn new(
        time: TimeOfDay,
        wetness: f32,
        images: &mut Assets<Image>,
        std_mats: &mut Assets<StandardMaterial>,
        surf: &mut Assets<SurfaceMaterial>,
    ) -> Self {
        let glow = time.glow();
        let asphalt = upload(images, &texture::asphalt(1024, 11));
        let paving = upload(images, &texture::paving(1024, 12));
        let concrete = upload(images, &texture::concrete(512, 13, 0.34));
        let kerb = upload(images, &texture::concrete(512, 14, 0.33));
        let brick = upload(images, &texture::brick(512, 15));
        let plaster = upload(images, &texture::plaster(512, 16));
        let roof = upload(images, &texture::roof(512, 17));
        let curb = upload(images, &texture::curb(512, 18));
        let decals = upload(images, &texture::decal_atlas(1024, 19));
        let fence = images.add(texture::fence(256).to_image(Encoding::Srgb));
        let windows = images.add(texture::window_atlas(512, 20).to_image(Encoding::Srgb));
        let signs = images.add(texture::sign_atlas(512, 21).to_image(Encoding::Srgb));
        let road_signs = images.add(texture::road_sign_atlas(512).to_image(Encoding::Srgb));
        let chequer = images.add(texture::chequer(64).to_image(Encoding::Srgb));
        let skyline = images.add(texture::skyline(512, 22, false).to_image(Encoding::Srgb));
        let skyline_lit = images.add(texture::skyline(512, 22, true).to_image(Encoding::Srgb));

        let wet = wetness;
        let mut by_slot = HashMap::new();
        let mut std = |slot: Slot, mut m: StandardMaterial| {
            // Emissive values are physical luminances (nits): let exposure apply.
            m.emissive_exposure_weight = 1.0;
            // Rain darkens and smooths exposed, non-emissive materials.
            if wet > 0.0 && m.emissive == LinearRgba::BLACK && !matches!(slot, Slot::Window | Slot::CurtainGlass | Slot::Glass | Slot::CarGlass) {
                m.base_color = m.base_color.mix(&(m.base_color.to_linear() * 0.6).into(), wet);
                m.perceptual_roughness *= 1.0 - 0.45 * wet;
            }
            by_slot.insert(slot, Mat::Std(std_mats.add(m)));
        };
        let lamp_rgb = kelvin_to_rgb(STREET_LIGHT_K);
        let lamp = LinearRgba::from(Color::srgb(lamp_rgb.x, lamp_rgb.y, lamp_rgb.z)) * glow.lamp;

        std(Slot::Concrete, textured(&concrete, Vec2::splat(tile::CONCRETE)));
        std(Slot::Galvanised, plain(0.62, 0.63, 0.64, 0.42, 0.9));
        std(
            Slot::Fence,
            StandardMaterial {
                base_color_texture: Some(fence),
                alpha_mode: AlphaMode::AlphaToCoverage,
                double_sided: true,
                cull_mode: None,
                metallic: 0.8,
                perceptual_roughness: 0.5,
                uv_transform: Affine2::from_scale(Vec2::splat(1.0 / tile::FENCE)),
                ..default()
            },
        );
        std(Slot::Rubber, plain(0.025, 0.025, 0.025, 0.85, 0.0));
        std(
            Slot::Chequer,
            StandardMaterial {
                base_color_texture: Some(chequer),
                perceptual_roughness: 0.6,
                uv_transform: Affine2::from_scale(Vec2::splat(1.0 / 0.8)),
                ..default()
            },
        );
        std(Slot::BoardWhite, plain(0.85, 0.85, 0.83, 0.55, 0.0));
        std(Slot::BoardDark, plain(0.025, 0.027, 0.03, 0.45, 0.0));
        std(Slot::Led, emissive(Color::srgb(0.3, 0.2, 0.05), LinearRgba::rgb(1.0, 0.45, 0.05) * glow.led, 0.4));
        std(Slot::SeatBlue, plain(0.04, 0.17, 0.55, 0.4, 0.0));
        std(Slot::DarkMetal, plain(0.035, 0.045, 0.04, 0.45, 0.2));
        std(Slot::PaintYellow, plain(0.85, 0.6, 0.05, 0.45, 0.0));
        std(Slot::PaintRed, plain(0.6, 0.05, 0.04, 0.45, 0.0));
        std(Slot::PaintWhite, plain(0.85, 0.85, 0.85, 0.35, 0.0));
        std(Slot::Wood, plain(0.33, 0.21, 0.12, 0.75, 0.0));
        std(Slot::LampLens, emissive(Color::srgb(0.9, 0.9, 0.85), lamp, 0.2));
        std(Slot::SignalRed, emissive(Color::srgb(0.5, 0.02, 0.02), LinearRgba::rgb(1.0, 0.03, 0.01) * glow.signal, 0.15));
        std(Slot::SignalAmber, emissive(Color::srgb(0.5, 0.3, 0.02), LinearRgba::rgb(1.0, 0.4, 0.02) * glow.signal, 0.15));
        std(Slot::SignalGreen, emissive(Color::srgb(0.02, 0.4, 0.3), LinearRgba::rgb(0.02, 1.0, 0.55) * glow.signal, 0.15));
        std(Slot::SignalOff, plain(0.02, 0.02, 0.02, 0.1, 0.0));
        std(
            Slot::RoadSign,
            StandardMaterial {
                base_color_texture: Some(road_signs),
                alpha_mode: AlphaMode::Mask(0.5),
                perceptual_roughness: 0.35,
                // Retroreflective sheeting glows faintly in headlights/streetlights.
                emissive: if time == TimeOfDay::Night { LinearRgba::gray(0.4) } else { LinearRgba::BLACK },
                ..default()
            },
        );
        std(
            Slot::Glass,
            StandardMaterial {
                base_color: Color::srgba(0.55, 0.65, 0.65, 0.22),
                alpha_mode: AlphaMode::Blend,
                perceptual_roughness: 0.05,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
        );
        std(
            Slot::Lightbox,
            StandardMaterial {
                base_color_texture: Some(signs.clone()),
                emissive_texture: Some(signs.clone()),
                emissive: LinearRgba::gray(glow.lightbox),
                perceptual_roughness: 0.1,
                ..default()
            },
        );
        std(Slot::CarPaint, racer_render::car_paint(Color::WHITE));
        std(Slot::CarGlass, plain(0.015, 0.018, 0.02, 0.05, 0.0));
        std(Slot::Foliage, plain(0.09, 0.17, 0.05, 0.8, 0.0));
        std(Slot::Bark, plain(0.11, 0.085, 0.065, 0.9, 0.0));
        std(Slot::Brick, textured(&brick, Vec2::splat(tile::BRICK)));
        std(Slot::Plaster, textured(&plaster, Vec2::splat(tile::PLASTER)));
        std(Slot::Stone, textured(&plaster, Vec2::splat(2.0)));
        std(
            Slot::Window,
            StandardMaterial {
                base_color_texture: Some(windows.clone()),
                base_color: Color::srgb(0.04, 0.04, 0.04),
                emissive_texture: Some(windows.clone()),
                emissive: LinearRgba::gray(glow.window),
                perceptual_roughness: 0.06,
                reflectance: 0.5,
                ..default()
            },
        );
        std(
            Slot::ShopGlass,
            StandardMaterial {
                base_color_texture: Some(windows.clone()),
                base_color: Color::srgb(0.04, 0.04, 0.04),
                emissive_texture: Some(windows),
                emissive: LinearRgba::gray(glow.shop),
                perceptual_roughness: 0.08,
                // Shop windows mostly reflect the dark street canyon, which the
                // sky environment map does not know about; tone them down.
                reflectance: 0.25,
                ..default()
            },
        );
        std(
            Slot::ShopSign,
            StandardMaterial {
                base_color_texture: Some(signs.clone()),
                emissive_texture: Some(signs),
                emissive: LinearRgba::gray(glow.sign),
                perceptual_roughness: 0.3,
                ..default()
            },
        );
        std(
            Slot::CurtainGlass,
            StandardMaterial {
                // Tinted per building (district glass palette).
                base_color: Color::WHITE,
                metallic: 0.65,
                perceptual_roughness: 0.04,
                ..default()
            },
        );
        std(
            Slot::Skyline,
            StandardMaterial {
                base_color_texture: Some(skyline),
                emissive_texture: Some(skyline_lit),
                emissive: LinearRgba::gray(glow.window * 0.8),
                perceptual_roughness: 0.15,
                metallic: 0.3,
                uv_transform: Affine2::from_scale(Vec2::splat(1.0 / 32.0)),
                ..default()
            },
        );
        std(Slot::Mullion, plain(0.16, 0.17, 0.18, 0.35, 0.8));
        std(Slot::Roof, textured(&roof, Vec2::splat(tile::ROOF)));
        std(Slot::RoofMetal, plain(0.5, 0.51, 0.5, 0.5, 0.7));
        std(
            Slot::Awning,
            StandardMaterial {
                base_color: Color::srgb(0.5, 0.1, 0.1),
                perceptual_roughness: 0.85,
                double_sided: true,
                cull_mode: None,
                ..default()
            },
        );

        // Ground surfaces share the wet surface extension.
        let surface = |base: StandardMaterial, mut params: SurfaceParams| {
            params.wetness = wet;
            SurfaceMaterial { base, extension: Surface { params } }
        };
        let mut s = |slot: Slot, base: StandardMaterial, params: SurfaceParams| {
            by_slot.insert(slot, Mat::Surface(surf.add(surface(base, params))));
        };
        s(Slot::RoadPlain, textured(&asphalt, Vec2::splat(tile::ASPHALT)), SurfaceParams::plain());
        s(Slot::Sidewalk, textured(&paving, Vec2::splat(tile::PAVING)), SurfaceParams::plain());
        s(Slot::Kerb, textured(&kerb, Vec2::splat(1.0)), SurfaceParams::plain());
        s(Slot::CurbPaint, textured(&curb, Vec2::new(1.0, tile::CURB)), SurfaceParams::plain());
        let mut decal = textured(&decals, Vec2::ONE);
        decal.alpha_mode = AlphaMode::Blend;
        decal.depth_bias = 50.0;
        s(Slot::Decal, decal, SurfaceParams::decal());
        let road = textured(&asphalt, Vec2::splat(tile::ASPHALT));
        s(Slot::Road, road, SurfaceParams::road(2, 3.4, racer_render::surface::flags::ALL));

        Self { time, wetness, by_slot, asphalt, tinted: HashMap::new() }
    }

    pub fn get(&self, slot: Slot) -> Mat {
        self.by_slot[&slot].clone()
    }

    /// A road surface material with its own marking parameters.
    pub fn road(&self, params: SurfaceParams, surf: &mut Assets<SurfaceMaterial>) -> Handle<SurfaceMaterial> {
        let Mat::Surface(base) = &self.by_slot[&Slot::Road] else {
            unreachable!("road is a surface material")
        };
        let mut m = surf.get(base).expect("road material exists").clone();
        m.extension.params = SurfaceParams { wetness: self.wetness, ..params };
        surf.add(m)
    }

    /// A copy of `slot`'s standard material with its base colour multiplied
    /// by `tint` (sRGB 0..1), cached per (slot, tint).
    pub fn tinted(&mut self, slot: Slot, tint: [f32; 3], std_mats: &mut Assets<StandardMaterial>) -> Mat {
        let key = (slot, tint.map(f32::to_bits));
        if let Some(h) = self.tinted.get(&key) {
            return Mat::Std(h.clone());
        }
        let Mat::Std(base) = &self.by_slot[&slot] else {
            return self.get(slot);
        };
        let mut m = std_mats.get(base).expect("slot material exists").clone();
        let t = Color::srgb(tint[0], tint[1], tint[2]).to_linear();
        let b = m.base_color.to_linear();
        m.base_color = Color::LinearRgba(LinearRgba::new(b.red * t.red, b.green * t.green, b.blue * t.blue, b.alpha));
        let h = std_mats.add(m);
        self.tinted.insert(key, h.clone());
        Mat::Std(h)
    }
}
