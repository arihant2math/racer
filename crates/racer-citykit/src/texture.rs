//! Procedural PBR textures for the kit: asphalt, paving, concrete, brick,
//! curb stripes, fence mesh, window and sign atlases, road decals.
//!
//! All textures are generated from noise with fixed seeds (bit-identical on
//! every run), tile seamlessly and come with a full CPU-built mip chain.
//! Colours are computed in linear space and stored as sRGB.

use bevy::{
    asset::RenderAssetUsages,
    image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor},
    math::{Vec2, Vec3},
    render::render_resource::{Extent3d, TextureDimension, TextureFormat},
};

use crate::noise::{fbm, hash, hash3, unit, value, worley};

/// A square texture in linear floating point, RGBA.
#[derive(Clone, Debug, PartialEq)]
pub struct Tex {
    pub size: u32,
    pub px: Vec<[f32; 4]>,
}

/// How a texture is encoded for the GPU.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    /// Colour (albedo, emissive): sRGB.
    Srgb,
    /// Data (roughness/metallic): linear.
    Linear,
    /// Tangent-space normal map: linear, renormalised per mip.
    Normal,
}

impl Tex {
    pub fn new(size: u32, fill: [f32; 4]) -> Self {
        Self {
            size,
            px: vec![fill; (size * size) as usize],
        }
    }

    pub fn from_fn(size: u32, f: impl Fn(f32, f32) -> [f32; 4]) -> Self {
        let mut px = Vec::with_capacity((size * size) as usize);
        for y in 0..size {
            for x in 0..size {
                px.push(f(
                    (x as f32 + 0.5) / size as f32,
                    (y as f32 + 0.5) / size as f32,
                ));
            }
        }
        Self { size, px }
    }

    pub fn get(&self, x: i32, y: i32) -> [f32; 4] {
        let s = self.size as i32;
        self.px[(y.rem_euclid(s) * s + x.rem_euclid(s)) as usize]
    }

    /// Tangent-space normal map from the given channel used as height,
    /// `strength` = height units per texel slope scale.
    pub fn normal_from(&self, channel: usize, strength: f32) -> Tex {
        let s = self.size as i32;
        let mut out = Tex::new(self.size, [0.5, 0.5, 1.0, 1.0]);
        for y in 0..s {
            for x in 0..s {
                let h = |dx: i32, dy: i32| self.get(x + dx, y + dy)[channel];
                let dx =
                    (h(1, -1) + 2.0 * h(1, 0) + h(1, 1)) - (h(-1, -1) + 2.0 * h(-1, 0) + h(-1, 1));
                let dy =
                    (h(-1, 1) + 2.0 * h(0, 1) + h(1, 1)) - (h(-1, -1) + 2.0 * h(0, -1) + h(1, -1));
                // +v in the texture is down the image; glTF/Bevy normal maps are
                // +Y up (OpenGL convention), so a height rising downwards tilts -Y.
                let n = Vec3::new(-dx * strength, dy * strength, 1.0).normalize();
                out.px[(y * s + x) as usize] =
                    [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.0];
            }
        }
        out
    }

    fn downsample(&self, enc: Encoding) -> Tex {
        let half = (self.size / 2).max(1);
        let mut out = Tex::new(half, [0.0; 4]);
        for y in 0..half as i32 {
            for x in 0..half as i32 {
                let mut acc = [0.0f32; 4];
                for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                    let p = self.get(2 * x + dx, 2 * y + dy);
                    for c in 0..4 {
                        acc[c] += p[c] * 0.25;
                    }
                }
                if enc == Encoding::Normal {
                    let n = Vec3::new(acc[0] * 2.0 - 1.0, acc[1] * 2.0 - 1.0, acc[2] * 2.0 - 1.0)
                        .normalize_or(Vec3::Z);
                    acc = [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, acc[3]];
                }
                out.px[(y as u32 * half + x as u32) as usize] = acc;
            }
        }
        out
    }

    /// 8-bit RGBA bytes of this level.
    pub fn to_rgba8(&self, enc: Encoding) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.px.len() * 4);
        for p in &self.px {
            for (c, v) in p.iter().enumerate() {
                let v = v.clamp(0.0, 1.0);
                let e = if enc == Encoding::Srgb && c < 3 {
                    linear_to_srgb(v)
                } else {
                    v
                };
                out.push((e * 255.0 + 0.5) as u8);
            }
        }
        out
    }

    /// All mip levels down to 1×1, concatenated.
    pub fn mip_chain(&self, enc: Encoding) -> (Vec<u8>, u32) {
        let mut data = self.to_rgba8(enc);
        let mut level = self.clone();
        let mut count = 1;
        while level.size > 1 {
            level = level.downsample(enc);
            data.extend(level.to_rgba8(enc));
            count += 1;
        }
        (data, count)
    }

    /// GPU image with mips and a repeating, anisotropic sampler.
    pub fn to_image(&self, enc: Encoding) -> Image {
        let (data, mips) = self.mip_chain(enc);
        let format = if enc == Encoding::Srgb {
            TextureFormat::Rgba8UnormSrgb
        } else {
            TextureFormat::Rgba8Unorm
        };
        let mut image = Image::new_uninit(
            Extent3d {
                width: self.size,
                height: self.size,
                depth_or_array_layers: 1,
            },
            TextureDimension::D2,
            format,
            RenderAssetUsages::RENDER_WORLD,
        );
        image.texture_descriptor.mip_level_count = mips;
        image.data = Some(data);
        image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
            address_mode_u: ImageAddressMode::Repeat,
            address_mode_v: ImageAddressMode::Repeat,
            mag_filter: ImageFilterMode::Linear,
            min_filter: ImageFilterMode::Linear,
            mipmap_filter: ImageFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });
        image
    }

    /// Writes the top level as a PNG.
    pub fn save_png(&self, path: &std::path::Path, enc: Encoding) -> image::ImageResult<()> {
        image::RgbaImage::from_raw(self.size, self.size, self.to_rgba8(enc))
            .expect("buffer matches size")
            .save(path)
    }
}

pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn rgb(c: Vec3, a: f32) -> [f32; 4] {
    [c.x, c.y, c.z, a]
}

fn srgb(r: u8, g: u8, b: u8) -> Vec3 {
    Vec3::new(
        srgb_to_linear(r as f32 / 255.0),
        srgb_to_linear(g as f32 / 255.0),
        srgb_to_linear(b as f32 / 255.0),
    )
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// A PBR texture set: albedo (sRGB, alpha = coverage or height), normal map
/// and packed occlusion/roughness/metallic (glTF order: R occlusion,
/// G roughness, B metallic).
#[derive(Clone, Debug, PartialEq)]
pub struct PbrTex {
    pub albedo: Tex,
    pub normal: Tex,
    pub orm: Tex,
}

/// Physical size in metres covered by one tile of each tiling texture (the
/// material `uv_transform` scale is `1 / tile`).
pub mod tile {
    pub const ASPHALT: f32 = 4.0;
    pub const PAVING: f32 = 3.0;
    pub const CONCRETE: f32 = 2.0;
    pub const BRICK: f32 = 0.9;
    pub const PLASTER: f32 = 3.0;
    pub const CURB: f32 = 2.0;
    pub const FENCE: f32 = 0.5;
    pub const ROOF: f32 = 4.0;
}

/// Weathered asphalt: aggregate stones in a dark binder, large-scale tonal
/// blotches and bitumen stains. Alpha holds the height (for puddle masks).
pub fn asphalt(size: u32, seed: u32) -> PbrTex {
    let cells = (size / 4) as i32; // ~1.6 cm aggregate on a 4 m tile at 1024 px
    let mut height = Tex::new(size, [0.0; 4]);
    let mut albedo = Tex::new(size, [0.0; 4]);
    let mut orm = Tex::new(size, [1.0, 0.9, 0.0, 1.0]);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (x as f32 / size as f32, y as f32 / size as f32);
            let (d1, d2, id) = worley(u * cells as f32, v * cells as f32, cells, seed);
            let stone = smoothstep(0.02, 0.18, d2 - d1) * (1.0 - smoothstep(0.35, 0.55, d1));
            let fine = value(u * 512.0, v * 512.0, 512, seed ^ 0x55);
            let blotch = fbm(u * 4.0, v * 4.0, 4, 5, seed ^ 0xa1);
            let stain = fbm(u * 2.0 + 7.0, v * 2.0, 2, 4, seed ^ 0xb2);
            let tone = unit(id);
            // Binder ~0.035 linear, stones 0.08-0.22 (grey granite and a few
            // pale quartz pieces), so the mix reads as weathered asphalt.
            let stone_col = if tone > 0.93 {
                Vec3::splat(0.30)
            } else {
                Vec3::splat(0.07 + 0.12 * tone)
            };
            let binder = Vec3::splat(0.032 + 0.012 * fine);
            let mut c = binder.lerp(stone_col, stone * 0.85);
            c *= 0.88 + 0.26 * blotch;
            c *= 1.0 - 0.2 * smoothstep(0.55, 0.75, stain);
            let h = 0.55 * stone * (0.6 + 0.4 * unit(hash(id))) + 0.25 * fine + 0.2 * blotch;
            let i = (y * size + x) as usize;
            height.px[i] = [h, h, h, 1.0];
            albedo.px[i] = rgb(c, h);
            let rough = 0.93 - 0.12 * stone - 0.05 * smoothstep(0.55, 0.75, stain);
            orm.px[i] = [1.0 - 0.25 * (1.0 - h).powi(3), rough, 0.0, 1.0];
        }
    }
    PbrTex {
        normal: height.normal_from(0, 2.5),
        albedo,
        orm,
    }
}

/// Concrete sidewalk slabs (1.5 m, 2×2 per 3 m tile) with saw-cut joints,
/// per-slab tone, stains and gum spots.
pub fn paving(size: u32, seed: u32) -> PbrTex {
    let mut height = Tex::new(size, [0.0; 4]);
    let mut albedo = Tex::new(size, [0.0; 4]);
    let mut orm = Tex::new(size, [1.0, 0.85, 0.0, 1.0]);
    let joint_w = 0.012 / tile::PAVING; // 12 mm joints
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let (su, sv) = (u * 2.0, v * 2.0);
            let (fu, fv) = (su.fract(), sv.fract());
            let edge = (fu.min(1.0 - fu)).min(fv.min(1.0 - fv)) / 2.0; // in tile units
            let joint = 1.0 - smoothstep(joint_w * 0.4, joint_w, edge);
            let slab = hash3(su as i32, sv as i32, seed);
            let grain = value(u * 700.0, v * 700.0, 700, seed ^ 3);
            let cloud = fbm(u * 6.0, v * 6.0, 6, 5, seed ^ 9);
            let stain = fbm(u * 3.0 + 1.3, v * 3.0, 3, 4, seed ^ 11);
            let (gd, _, gid) = worley(u * 24.0, v * 24.0, 24, seed ^ 0x77);
            let gum = (1.0 - smoothstep(0.03, 0.06, gd)) * (unit(gid) > 0.8) as i32 as f32;
            let base = 0.30 + 0.06 * (unit(slab) - 0.5) + 0.05 * grain;
            let mut c = Vec3::new(base * 1.0, base * 0.98, base * 0.94);
            c *= 0.85 + 0.3 * cloud;
            c *= 1.0 - 0.35 * smoothstep(0.5, 0.75, stain);
            c = c.lerp(Vec3::splat(0.05), gum * 0.85);
            c = c.lerp(Vec3::splat(0.06), joint * 0.85);
            let h = 0.6 + 0.1 * grain - 0.6 * joint;
            let i = (y * size + x) as usize;
            height.px[i] = [h; 4];
            albedo.px[i] = rgb(c, h);
            orm.px[i] = [1.0 - 0.5 * joint, 0.82 + 0.1 * grain - 0.2 * gum, 0.0, 1.0];
        }
    }
    PbrTex {
        normal: height.normal_from(0, 3.0),
        albedo,
        orm,
    }
}

/// Generic cast concrete (barriers, kerbs, plinths): fine pores, formwork
/// tone variation and water streaks running down (+v).
pub fn concrete(size: u32, seed: u32, base: f32) -> PbrTex {
    let mut height = Tex::new(size, [0.0; 4]);
    let mut albedo = Tex::new(size, [0.0; 4]);
    let mut orm = Tex::new(size, [1.0, 0.85, 0.0, 1.0]);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let grain = value(u * 400.0, v * 400.0, 400, seed);
            let (pd, _, _) = worley(u * 90.0, v * 90.0, 90, seed ^ 5);
            let pore = 1.0 - smoothstep(0.05, 0.12, pd);
            let cloud = fbm(u * 3.0, v * 3.0, 3, 5, seed ^ 7);
            let streak = fbm(u * 24.0, v * 1.0, 24, 3, seed ^ 13);
            let mut c =
                Vec3::new(base, base * 0.985, base * 0.95) * (0.85 + 0.3 * cloud + 0.08 * grain);
            c *= 1.0 - 0.3 * smoothstep(0.55, 0.8, streak) * v;
            c *= 1.0 - 0.5 * pore;
            let h = 0.5 + 0.2 * grain - 0.5 * pore;
            let i = (y * size + x) as usize;
            height.px[i] = [h; 4];
            albedo.px[i] = rgb(c, 1.0);
            orm.px[i] = [1.0 - 0.3 * pore, 0.86 + 0.08 * grain, 0.0, 1.0];
        }
    }
    PbrTex {
        normal: height.normal_from(0, 2.0),
        albedo,
        orm,
    }
}

/// Neutral brick (light, tinted by the material colour per building):
/// 225 × 65 mm bricks with 10 mm mortar, running bond, 0.9 m tile.
pub fn brick(size: u32, seed: u32) -> PbrTex {
    let mut height = Tex::new(size, [0.0; 4]);
    let mut albedo = Tex::new(size, [0.0; 4]);
    let mut orm = Tex::new(size, [1.0, 0.85, 0.0, 1.0]);
    let (cols, rows) = (4.0, 12.0);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let row = (v * rows).floor();
            let shift = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
            let bu = u * cols + shift;
            let col = bu.floor().rem_euclid(cols);
            let (fu, fv) = (bu.fract(), (v * rows).fract());
            // Mortar is 10 mm of a 235 mm / 75 mm module.
            let mu = (fu.min(1.0 - fu)) * 0.235;
            let mv = (fv.min(1.0 - fv)) * 0.075;
            let mortar = 1.0 - smoothstep(0.003, 0.006, mu.min(mv));
            let id = hash3(col as i32, row as i32, seed);
            let grain = value(u * 300.0, v * 300.0, 300, seed ^ 1);
            let cloud = fbm(u * 2.0, v * 2.0, 2, 4, seed ^ 2);
            let burnt = unit(hash(id));
            let brick_c = Vec3::new(0.62, 0.42, 0.34)
                * (0.7 + 0.45 * unit(id))
                * (0.9 + 0.2 * grain)
                * if burnt > 0.85 { 0.6 } else { 1.0 };
            let mortar_c = Vec3::new(0.55, 0.53, 0.5) * (0.85 + 0.2 * grain);
            let c = brick_c.lerp(mortar_c, mortar) * (0.85 + 0.3 * cloud);
            let h = (1.0 - mortar) * (0.7 + 0.2 * grain);
            let i = (y * size + x) as usize;
            height.px[i] = [h; 4];
            albedo.px[i] = rgb(c, 1.0);
            orm.px[i] = [1.0 - 0.4 * mortar, 0.82 + 0.1 * grain, 0.0, 1.0];
        }
    }
    PbrTex {
        normal: height.normal_from(0, 4.0),
        albedo,
        orm,
    }
}

/// Painted render / limestone cladding: near-white, tinted per building,
/// soft dirt streaks below (+v), faint panel joints every 1.5 m.
pub fn plaster(size: u32, seed: u32) -> PbrTex {
    let mut height = Tex::new(size, [0.0; 4]);
    let mut albedo = Tex::new(size, [0.0; 4]);
    let mut orm = Tex::new(size, [1.0, 0.8, 0.0, 1.0]);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let grain = value(u * 350.0, v * 350.0, 350, seed);
            let cloud = fbm(u * 3.0, v * 3.0, 3, 5, seed ^ 4);
            let streak = fbm(u * 20.0, v * 1.5, 20, 3, seed ^ 8);
            let (pu, pv) = ((u * 2.0).fract(), (v * 2.0).fract());
            let joint = 1.0 - smoothstep(0.0, 0.004, pu.min(1.0 - pu).min(pv.min(1.0 - pv)));
            let mut c = Vec3::splat(0.72) * (0.9 + 0.12 * cloud + 0.05 * grain);
            c *= 1.0 - 0.25 * smoothstep(0.5, 0.8, streak);
            c *= 1.0 - 0.4 * joint;
            let h = 0.5 + 0.1 * grain - 0.3 * joint;
            let i = (y * size + x) as usize;
            height.px[i] = [h; 4];
            albedo.px[i] = rgb(c, 1.0);
            orm.px[i] = [1.0, 0.75 + 0.1 * grain, 0.0, 1.0];
        }
    }
    PbrTex {
        normal: height.normal_from(0, 1.5),
        albedo,
        orm,
    }
}

/// Roof membrane with gravel ballast, 4 m tile.
pub fn roof(size: u32, seed: u32) -> PbrTex {
    let mut height = Tex::new(size, [0.0; 4]);
    let mut albedo = Tex::new(size, [0.0; 4]);
    let orm = Tex::new(size, [1.0, 0.92, 0.0, 1.0]);
    let cells = (size / 3) as i32;
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let (d1, d2, id) = worley(u * cells as f32, v * cells as f32, cells, seed);
            let stone = smoothstep(0.0, 0.2, d2 - d1);
            let cloud = fbm(u * 4.0, v * 4.0, 4, 4, seed ^ 3);
            let c = Vec3::splat(0.18 + 0.12 * unit(id) * stone) * (0.8 + 0.4 * cloud);
            let i = (y * size + x) as usize;
            height.px[i] = [stone; 4];
            albedo.px[i] = rgb(c, 1.0);
        }
    }
    PbrTex {
        normal: height.normal_from(0, 1.5),
        albedo,
        orm,
    }
}

/// Red/white racing curb paint over concrete, one red and one white block per
/// 2 m tile along v, scuffed by tyres (black rubber marks) and worn at edges.
pub fn curb(size: u32, seed: u32) -> PbrTex {
    let mut height = Tex::new(size, [0.0; 4]);
    let mut albedo = Tex::new(size, [0.0; 4]);
    let mut orm = Tex::new(size, [1.0, 0.5, 0.0, 1.0]);
    let red = srgb(196, 28, 26);
    let white = srgb(232, 230, 222);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let paint = if v < 0.5 { red } else { white };
            let grain = value(u * 200.0, v * 200.0, 200, seed);
            let wear = fbm(u * 8.0, v * 8.0, 8, 5, seed ^ 2);
            let rubber = fbm(u * 3.0, v * 30.0, 3, 4, seed ^ 5) * smoothstep(0.2, 0.9, u);
            let worn = smoothstep(0.62, 0.75, wear);
            let mut c = paint.lerp(Vec3::splat(0.25), worn * 0.8) * (0.92 + 0.1 * grain);
            c = c.lerp(Vec3::splat(0.03), smoothstep(0.55, 0.85, rubber) * 0.7);
            let i = (y * size + x) as usize;
            height.px[i] = [0.5 + 0.2 * grain - 0.2 * worn; 4];
            albedo.px[i] = rgb(c, 1.0);
            orm.px[i] = [1.0, 0.45 + 0.4 * worn, 0.0, 1.0];
        }
    }
    PbrTex {
        normal: height.normal_from(0, 1.0),
        albedo,
        orm,
    }
}

/// Chain-link catch fence: 50 mm diamond mesh of 3 mm galvanised wire;
/// alpha is wire coverage (use with alpha-to-coverage). 0.5 m tile.
pub fn fence(size: u32) -> Tex {
    let n = 10.0; // diamonds per tile
    let wire = 0.004 / tile::FENCE * n; // wire width in diamond units
    Tex::from_fn(size, |u, v| {
        let (a, b) = ((u + v) * n, (u - v) * n);
        let da = (a - a.round()).abs();
        let db = (b - b.round()).abs();
        let d = da.min(db) / std::f32::consts::SQRT_2;
        let cov = 1.0 - smoothstep(wire * 0.5, wire, d);
        let c = Vec3::new(0.55, 0.56, 0.55);
        [c.x, c.y, c.z, cov]
    })
}

/// Window interiors, 4×4 cells: each a room seen through glass (back wall,
/// ceiling light, blinds or curtains). Used as base colour and, at night,
/// emissive. Cells in row 3 are dark (unlit) variants.
pub fn window_atlas(size: u32, seed: u32) -> Tex {
    let cell = size / 4;
    Tex::from_fn(size, |u, v| {
        let (cx, cy) = ((u * 4.0).floor() as u32, (v * 4.0).floor() as u32);
        let (fu, fv) = ((u * 4.0).fract(), (v * 4.0).fract());
        let id = hash3(cx as i32, cy as i32, seed);
        let lit = cy < 3;
        let warm = unit(id) > 0.4;
        let room = if warm {
            Vec3::new(1.0, 0.78, 0.5)
        } else {
            Vec3::new(0.75, 0.85, 1.0)
        };
        // Back wall brighter near a ceiling light, darker floor band.
        let light = (1.0 - (fu - 0.5).abs() * 1.2) * (1.0 - fv * 0.8);
        let mut c = if lit {
            room * (0.25 + 0.75 * light.max(0.0))
        } else {
            Vec3::new(0.05, 0.055, 0.06) * (0.6 + 0.4 * light.max(0.0))
        };
        // Blinds (horizontal slats) or a curtain on part of the window.
        let kind = hash(id) % 4;
        let slat = ((fv * 40.0).fract() < 0.5) as i32 as f32;
        match kind {
            0 => {
                let cover = 0.25 + 0.5 * unit(hash(id ^ 1));
                if fv < cover {
                    c = c * 0.5 + Vec3::splat(0.08 + 0.04 * slat) * if lit { 2.0 } else { 0.6 };
                }
            }
            1 => {
                if !(0.3..=0.8).contains(&fu) {
                    c *= 0.35;
                }
            }
            2
                // Furniture silhouette: a desk and a figure-ish block.
                if (fv > 0.7 || (fu > 0.6 && fu < 0.7 && fv > 0.45)) => {
                    c *= 0.2;
                }
            _ => {}
        }
        let _ = cell;
        // Thin dark frame so neighbouring cells never bleed.
        let frame = (fu.min(1.0 - fu)).min(fv.min(1.0 - fv));
        if frame < 0.02 {
            c = Vec3::splat(0.02);
        }
        [c.x, c.y, c.z, 1.0]
    })
}

/// Shop signs and fascia boards, 4×4 cells: coloured boards with blocks of
/// "lettering". Used as base colour and as emissive at night.
pub fn sign_atlas(size: u32, seed: u32) -> Tex {
    let palette = [
        srgb(180, 30, 40),
        srgb(20, 60, 120),
        srgb(30, 110, 70),
        srgb(230, 180, 40),
        srgb(20, 20, 24),
        srgb(240, 240, 235),
        srgb(200, 90, 30),
        srgb(110, 40, 120),
    ];
    Tex::from_fn(size, |u, v| {
        let (cx, cy) = ((u * 4.0).floor() as i32, (v * 4.0).floor() as i32);
        let (fu, fv) = ((u * 4.0).fract(), (v * 4.0).fract());
        let id = hash3(cx, cy, seed);
        let bg = palette[(id % 8) as usize];
        let fg = palette[((hash(id) % 7 + 1 + id % 8) % 8) as usize];
        let fg = if (fg - bg).length() < 0.2 {
            Vec3::ONE - bg
        } else {
            fg
        };
        // Lettering: a row of glyph blocks between 30% and 70% height.
        let letters = 4 + (hash(id ^ 9) % 7) as i32;
        let start = 0.5 - letters as f32 * 0.035;
        let gx = (fu - start) / 0.07;
        let in_text = gx >= 0.0 && gx < letters as f32 && fv > 0.32 && fv < 0.68;
        let glyph = hash3(gx as i32, cx * 31 + cy, seed ^ 4);
        let (gu, gv) = (gx.fract(), (fv - 0.32) / 0.36);
        let stroke = match glyph % 4 {
            0 => gu < 0.25 || !(0.2..=0.8).contains(&gv),
            1 => !(0.25..=0.65).contains(&gu) || (gv > 0.42 && gv < 0.58),
            2 => gu < 0.25 || gv > 0.8,
            _ => (!(0.2..=0.8).contains(&gv) || (gv > 0.42 && gv < 0.58)) && gu < 0.8,
        } && gu < 0.8;
        let mut c = if in_text && stroke { fg } else { bg };
        let frame = (fu.min(1.0 - fu)).min(fv.min(1.0 - fv));
        if frame < 0.03 {
            c = Vec3::splat(0.03);
        }
        [c.x, c.y, c.z, 1.0]
    })
}

/// Road decal cells in a 4×4 atlas (alpha = coverage). See [`DecalCell`].
pub fn decal_atlas(size: u32, seed: u32) -> PbrTex {
    let mut albedo = Tex::new(size, [0.0; 4]);
    let mut height = Tex::new(size, [0.5; 4]);
    let mut orm = Tex::new(size, [1.0, 0.85, 0.0, 1.0]);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            let (cx, cy) = ((u * 4.0).floor() as u32, (v * 4.0).floor() as u32);
            let (fu, fv) = ((u * 4.0).fract(), (v * 4.0).fract());
            let p = Vec2::new(fu - 0.5, fv - 0.5);
            let cell = DecalCell::from_index(cy * 4 + cx);
            let s = seed.wrapping_add(cy * 4 + cx);
            let grain = value(u * 512.0, v * 512.0, 512, s);
            let (col, a, h, r) = match cell {
                Some(DecalCell::Patch) => {
                    // Rectangular repair with a slightly ragged sealed edge.
                    let rag = 0.02 * fbm(fu * 20.0, fv * 20.0, 0, 3, s);
                    let d = (0.46 - p.x.abs()).min(0.46 - p.y.abs()) + rag;
                    let a = smoothstep(0.0, 0.01, d);
                    let seal = 1.0 - smoothstep(0.0, 0.03, d.abs());
                    let blotch = fbm(fu * 6.0, fv * 6.0, 0, 4, s ^ 3);
                    let c = Vec3::splat(0.022 + 0.02 * grain + 0.015 * blotch)
                        .lerp(Vec3::splat(0.012), seal);
                    (c, a.max(seal), 0.5 + 0.1 * grain, 0.8 - 0.25 * seal)
                }
                Some(DecalCell::Crack) | Some(DecalCell::CrackBranch) => {
                    // Meandering crack lines from a warped distance field.
                    let w = fbm(fu * 5.0, fv * 5.0, 0, 4, s) - 0.5;
                    let mut d = (p.y + w * 0.5).abs();
                    if cell == Some(DecalCell::CrackBranch) {
                        let w2 = fbm(fu * 5.0 + 3.0, fv * 5.0, 0, 4, s ^ 1) - 0.5;
                        d = d.min((p.x + w2 * 0.4).abs().max(-p.y - 0.1));
                    }
                    let fade = 1.0 - smoothstep(0.3, 0.5, p.x.abs().max(p.y.abs()));
                    let core = 1.0 - smoothstep(0.003, 0.009, d);
                    let seal = 1.0 - smoothstep(0.01, 0.03, d);
                    let a = (seal * 0.85).max(core) * fade;
                    (Vec3::splat(0.012), a, 0.5 - 0.4 * core, 0.6)
                }
                Some(DecalCell::Manhole) | Some(DecalCell::ManholeRect) => {
                    let round = cell == Some(DecalCell::Manhole);
                    let d = if round {
                        p.length()
                    } else {
                        p.x.abs().max(p.y.abs())
                    };
                    let a = 1.0 - smoothstep(0.46, 0.47, d);
                    let rim = smoothstep(0.40, 0.41, d);
                    // Cast pattern: concentric rings or a grid of raised studs.
                    let pattern = if round {
                        let ang = p.y.atan2(p.x);
                        (((d * 40.0).fract() < 0.5) as i32
                            ^ (((ang * 6.0 / std::f32::consts::PI).floor() as i32) & 1))
                            as f32
                    } else {
                        (((fu * 16.0).fract() < 0.35) || ((fv * 16.0).fract() < 0.35)) as i32 as f32
                    };
                    let rust = fbm(fu * 8.0, fv * 8.0, 0, 4, s);
                    let iron = Vec3::new(0.06, 0.055, 0.05) * (0.8 + 0.4 * rust)
                        + Vec3::new(0.03, 0.012, 0.0) * smoothstep(0.55, 0.8, rust);
                    let c = iron
                        * (0.8 + 0.4 * pattern)
                        * (1.0 - 0.4 * rim * (1.0 - smoothstep(0.45, 0.46, d)));
                    (
                        c,
                        a,
                        0.5 + 0.15 * pattern - 0.2 * rim,
                        0.45 + 0.3 * rust - 0.15 * pattern,
                    )
                }
                Some(DecalCell::Drain) => {
                    let a = 1.0 - smoothstep(0.44, 0.45, p.x.abs().max(p.y.abs() * 1.8));
                    let slot = ((fu * 12.0).fract() < 0.55) && p.y.abs() < 0.2;
                    let c = if slot {
                        Vec3::splat(0.004)
                    } else {
                        Vec3::new(0.07, 0.065, 0.06)
                    };
                    (c, a, if slot { 0.0 } else { 0.6 }, 0.5)
                }
                Some(DecalCell::OilStain) => {
                    let n = fbm(fu * 4.0, fv * 4.0, 0, 5, s);
                    let d = p.length() + (n - 0.5) * 0.3;
                    let a = (1.0 - smoothstep(0.15, 0.42, d)) * 0.75;
                    (Vec3::splat(0.008), a, 0.5, 0.35)
                }
                Some(DecalCell::Skid) => {
                    // Two parallel tyre marks along v, fading at both ends.
                    let lane = |c: f32| 1.0 - smoothstep(0.06, 0.1, (p.x - c).abs());
                    let streaks = 0.6 + 0.4 * value(fu * 60.0, fv * 3.0, 0, s);
                    let fade = (1.0 - smoothstep(0.3, 0.5, p.y.abs())) * (0.4 + 0.6 * (0.5 - p.y));
                    let a =
                        (lane(-0.22) + lane(0.22)).min(1.0) * streaks * fade.clamp(0.0, 1.0) * 0.8;
                    (Vec3::splat(0.01), a, 0.5, 0.55)
                }
                Some(DecalCell::Seam) => {
                    // Sealed longitudinal joint (tar band) along v.
                    let w = fbm(fu * 3.0, fv * 12.0, 0, 3, s) - 0.5;
                    let d = (p.x + w * 0.1).abs();
                    let a = 1.0 - smoothstep(0.03, 0.05, d);
                    (Vec3::splat(0.01), a, 0.5, 0.35)
                }
                Some(DecalCell::Arrow) => {
                    // White straight-ahead lane arrow (points to −v).
                    let stem = p.x.abs() < 0.05 && p.y > -0.1 && p.y < 0.45;
                    let head = p.y <= -0.1 && p.y > -0.45 && p.x.abs() < (p.y + 0.45) * 0.45;
                    let wear = fbm(fu * 10.0, fv * 10.0, 0, 4, s);
                    let a = if stem || head {
                        0.9 * smoothstep(0.25, 0.5, wear + 0.2)
                    } else {
                        0.0
                    };
                    (Vec3::splat(0.75), a, 0.55, 0.6)
                }
                None => (Vec3::ZERO, 0.0, 0.5, 0.9),
            };
            let i = (y * size + x) as usize;
            albedo.px[i] = rgb(col, a);
            height.px[i] = [h; 4];
            orm.px[i] = [1.0, r, 0.0, 1.0];
        }
    }
    PbrTex {
        normal: height.normal_from(0, 3.0),
        albedo,
        orm,
    }
}

/// Cells of [`decal_atlas`], row-major in a 4×4 grid.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DecalCell {
    Patch,
    Crack,
    CrackBranch,
    Manhole,
    ManholeRect,
    Drain,
    OilStain,
    Skid,
    Seam,
    Arrow,
}

impl DecalCell {
    pub const ALL: [DecalCell; 10] = [
        DecalCell::Patch,
        DecalCell::Crack,
        DecalCell::CrackBranch,
        DecalCell::Manhole,
        DecalCell::ManholeRect,
        DecalCell::Drain,
        DecalCell::OilStain,
        DecalCell::Skid,
        DecalCell::Seam,
        DecalCell::Arrow,
    ];

    pub fn from_index(i: u32) -> Option<Self> {
        Self::ALL.get(i as usize).copied()
    }

    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as u32
    }

    /// UV rectangle (min, max) of this cell in the atlas.
    pub fn uv_rect(self) -> (Vec2, Vec2) {
        let i = self.index();
        let min = Vec2::new((i % 4) as f32, (i / 4) as f32) * 0.25;
        (min, min + Vec2::splat(0.25))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn textures_are_deterministic_and_in_range() {
        let a = asphalt(64, 1);
        assert_eq!(a, asphalt(64, 1));
        assert_ne!(a.albedo, asphalt(64, 2).albedo);
        for t in [
            &a.albedo,
            &a.normal,
            &a.orm,
            &paving(64, 1).albedo,
            &decal_atlas(64, 1).albedo,
        ] {
            assert!(
                t.px.iter()
                    .flatten()
                    .all(|v| v.is_finite() && (-1e-4..=1.0 + 1e-4).contains(v))
            );
        }
    }

    #[test]
    fn asphalt_albedo_is_physically_plausible() {
        // Weathered asphalt albedo is about 0.05-0.15 (linear).
        let a = asphalt(128, 7).albedo;
        let mean = a.px.iter().map(|p| (p[0] + p[1] + p[2]) / 3.0).sum::<f32>() / a.px.len() as f32;
        assert!((0.04..0.15).contains(&mean), "{mean}");
    }

    #[test]
    fn mip_chain_reaches_one_texel() {
        let t = fence(64);
        let (data, mips) = t.mip_chain(Encoding::Srgb);
        assert_eq!(mips, 7);
        let expected: usize = (0..7).map(|l| (64usize >> l).pow(2) * 4).sum();
        assert_eq!(data.len(), expected);
    }

    #[test]
    fn tiles_wrap_seamlessly() {
        // Opposite edges of a tiling texture differ no more than neighbours inside.
        let t = asphalt(128, 3).albedo;
        let s = t.size as i32;
        let lum = |p: [f32; 4]| p[0] + p[1] + p[2];
        let edge: f32 = (0..s)
            .map(|y| (lum(t.get(0, y)) - lum(t.get(s - 1, y))).abs())
            .sum::<f32>()
            / s as f32;
        let inner: f32 = (0..s)
            .map(|y| (lum(t.get(60, y)) - lum(t.get(61, y))).abs())
            .sum::<f32>()
            / s as f32;
        assert!(edge < inner * 2.0 + 0.01, "edge {edge} inner {inner}");
    }
}

/// UV rectangle of cell `i` (row-major) in a 4×4 atlas, inset half a texel
/// at 512 px so mips do not bleed between cells.
pub fn sign_cell_rect(i: u32) -> (Vec2, Vec2) {
    let min = Vec2::new((i % 4) as f32, (i / 4) as f32) * 0.25;
    let inset = Vec2::splat(0.004);
    (min + inset, min + Vec2::splat(0.25) - inset)
}

/// Road signs in [`road_sign_atlas`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoadSignCell {
    SpeedLimit,
    NoParking,
    StreetName,
    OneWay,
    Stop,
    Pedestrian,
    BusStop,
    NoEntry,
}

impl RoadSignCell {
    pub const ALL: [RoadSignCell; 8] = [
        RoadSignCell::SpeedLimit,
        RoadSignCell::NoParking,
        RoadSignCell::StreetName,
        RoadSignCell::OneWay,
        RoadSignCell::Stop,
        RoadSignCell::Pedestrian,
        RoadSignCell::BusStop,
        RoadSignCell::NoEntry,
    ];

    fn index(self) -> u32 {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0) as u32
    }

    /// Plate size (width, height) in metres.
    pub fn size(self) -> (f32, f32) {
        match self {
            RoadSignCell::SpeedLimit | RoadSignCell::NoParking | RoadSignCell::NoEntry => {
                (0.6, 0.6)
            }
            RoadSignCell::StreetName => (1.2, 0.3),
            RoadSignCell::OneWay => (0.9, 0.3),
            RoadSignCell::Stop | RoadSignCell::Pedestrian => (0.75, 0.75),
            RoadSignCell::BusStop => (0.45, 0.6),
        }
    }

    pub fn round(self) -> bool {
        matches!(
            self,
            RoadSignCell::SpeedLimit | RoadSignCell::NoParking | RoadSignCell::NoEntry
        )
    }

    /// UV rectangle with the plate's aspect ratio, centred in the cell.
    pub fn uv_rect(self) -> (Vec2, Vec2) {
        let (a, b) = sign_cell_rect(self.index());
        let (w, h) = self.size();
        let c = (a + b) / 2.0;
        let half = (b - a) / 2.0;
        let half = if w >= h {
            Vec2::new(half.x, half.y * h / w)
        } else {
            Vec2::new(half.x * w / h, half.y)
        };
        (c - half, c + half)
    }
}

/// Road signs, 4×4 cells (see [`RoadSignCell`]); alpha cuts shaped plates.
pub fn road_sign_atlas(size: u32) -> Tex {
    let red = srgb(200, 20, 30);
    let white = srgb(240, 240, 236);
    let blue = srgb(20, 70, 160);
    let green = srgb(0, 110, 60);
    let black = Vec3::splat(0.015);
    Tex::from_fn(size, |u, v| {
        let i = (v * 4.0).floor() as u32 * 4 + (u * 4.0).floor() as u32;
        let Some(cell) = RoadSignCell::ALL.get(i as usize).copied() else {
            return [0.0; 4];
        };
        let (fu, fv) = ((u * 4.0).fract(), (v * 4.0).fract());
        let (w, h) = cell.size();
        // Plate coordinates in metres, origin at the centre, y up.
        let scale = w.max(h);
        let x = (fu - 0.5) * scale;
        let y = (0.5 - fv) * scale;
        let r = (x * x + y * y).sqrt();
        let glyphs = |x0: f32, y0: f32, gh: f32, n: i32, seed: u32| -> bool {
            let gx = (x - x0) / (gh * 0.7);
            let gy = (y - y0) / gh;
            if gx < 0.0 || gx >= n as f32 || !(0.0..1.0).contains(&gy) {
                return false;
            }
            let (cu, cv) = (gx.fract(), gy);
            let g = hash3(gx as i32, 0, seed) % 5;
            cu < 0.75
                && match g {
                    0 => cu < 0.18 || !(0.15..=0.85).contains(&cv),
                    1 => !(0.18..=0.57).contains(&cu) || (cv > 0.42 && cv < 0.58),
                    2 => cv > 0.85 || (cu - 0.37).abs() < 0.1,
                    3 => cu < 0.18 || cv < 0.15,
                    _ => !(0.15..=0.85).contains(&cv) || (cv > 0.42 && cv < 0.58) || cu > 0.57,
                }
        };
        let (c, a) = match cell {
            RoadSignCell::SpeedLimit => {
                let ring = r > 0.23 && r < 0.3;
                let digits = {
                    // "35" as seven-segment-ish strokes.
                    let seg = |x0: f32| {
                        let (gx, gy) = ((x - x0) / 0.09, (y + 0.1) / 0.2);
                        (0.0..1.0).contains(&gx)
                            && (0.0..1.0).contains(&gy)
                            && (!(0.15..=0.85).contains(&gy)
                                || (gy > 0.43 && gy < 0.57)
                                || if x0 < 0.0 {
                                    gx > 0.75
                                } else {
                                    (gx < 0.25 && gy > 0.5) || (gx > 0.75 && gy < 0.5)
                                })
                    };
                    seg(-0.11) || seg(0.02)
                };
                (
                    if ring {
                        red
                    } else if digits {
                        black
                    } else {
                        white
                    },
                    (r < 0.3) as i32 as f32,
                )
            }
            RoadSignCell::NoParking | RoadSignCell::NoEntry => {
                let ring = r > 0.23;
                let c = if cell == RoadSignCell::NoEntry {
                    if y.abs() < 0.05 && x.abs() < 0.2 {
                        white
                    } else {
                        red
                    }
                } else if ring || ((x - y).abs() < 0.05 && r < 0.25) {
                    red
                } else {
                    blue
                };
                (c, (r < 0.3) as i32 as f32)
            }
            RoadSignCell::StreetName => {
                let border = x.abs() > 0.57 || y.abs() > 0.12;
                let text = glyphs(-0.5, -0.07, 0.14, 10, 17);
                (
                    if border || text { white } else { green },
                    (y.abs() < 0.15) as i32 as f32,
                )
            }
            RoadSignCell::OneWay => {
                let arrow = (y.abs() < 0.04 && x > -0.4 && x < 0.25)
                    || ((0.25..0.4).contains(&x) && y.abs() < (0.4 - x) * 0.8);
                let text = glyphs(-0.3, -0.035, 0.07, 6, 3) && arrow;
                (
                    if text {
                        black
                    } else if arrow {
                        white
                    } else {
                        black
                    },
                    (y.abs() < 0.15) as i32 as f32,
                )
            }
            RoadSignCell::Stop => {
                let oct = x
                    .abs()
                    .max(y.abs())
                    .max((x.abs() + y.abs()) / std::f32::consts::SQRT_2);
                let text = glyphs(-0.25, -0.08, 0.16, 4, 1);
                let border = oct > 0.33;
                (
                    if text || border { white } else { red },
                    (oct < 0.37) as i32 as f32,
                )
            }
            RoadSignCell::Pedestrian => {
                let tri = y > -0.25 && y < 0.25 && x.abs() < (0.25 - y) * 0.6;
                let figure = (x.abs() < 0.025 && y > -0.15 && y < 0.05)
                    || ((x * x + (y - 0.1).powi(2)).sqrt() < 0.04);
                let c = if tri {
                    if figure { black } else { white }
                } else {
                    blue
                };
                (c, (x.abs() < 0.37 && y.abs() < 0.37) as i32 as f32)
            }
            RoadSignCell::BusStop => {
                let band = y > 0.12;
                let text = glyphs(-0.17, 0.15, 0.1, 3, 9);
                let bus = x.abs() < 0.15
                    && y > -0.22
                    && y < 0.02
                    && !(y > -0.12 && y < -0.02 && x.abs() < 0.12);
                let c = if band {
                    if text { white } else { blue }
                } else if bus {
                    blue
                } else {
                    white
                };
                (c, (x.abs() < 0.225 && y.abs() < 0.3) as i32 as f32)
            }
        };
        [c.x, c.y, c.z, a]
    })
}

/// Black and white chequer, 2×2 cells per tile.
pub fn chequer(size: u32) -> Tex {
    Tex::from_fn(size, |u, v| {
        let c = if ((u * 2.0) as i32 + (v * 2.0) as i32) % 2 == 0 {
            0.8
        } else {
            0.012
        };
        [c, c, c, 1.0]
    })
}

/// Distant-tower façade (HLOD dressing): a 32 × 32 m tile of 8 floors by 16
/// window columns. With `emissive` false: daytime base colour (glass with
/// slight per-window variation, spandrels); with `emissive` true: the lit
/// windows only (for the emissive texture).
pub fn skyline(size: u32, seed: u32, emissive: bool) -> Tex {
    Tex::from_fn(size, |u, v| {
        let (cx, cy) = ((u * 16.0).floor() as i32, (v * 8.0).floor() as i32);
        let (fu, fv) = ((u * 16.0).fract(), (v * 8.0).fract());
        let id = hash3(cx, cy, seed);
        let lit = unit(id) < 0.35;
        let glass = fu > 0.08 && fu < 0.92 && fv > 0.25 && fv < 0.95;
        let warm = unit(hash(id)) > 0.3;
        let c = if emissive {
            if glass && lit {
                if warm {
                    Vec3::new(1.0, 0.75, 0.45)
                } else {
                    Vec3::new(0.7, 0.82, 1.0)
                }
            } else {
                Vec3::ZERO
            }
        } else if !glass {
            Vec3::new(0.2, 0.2, 0.2)
        } else {
            Vec3::new(0.07, 0.09, 0.11) * (0.8 + 0.5 * unit(hash(id ^ 7)))
        };
        [c.x, c.y, c.z, 1.0]
    })
}
