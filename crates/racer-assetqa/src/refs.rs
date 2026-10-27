//! Reference silhouettes from blueprint sheets (`data/cars/<id>/refs/refs.toml`)
//! and gate 4, silhouette accuracy.
//!
//! A sheet is one image holding several orthographic views drawn at one scale.
//! Each view is cropped, its ink (dark or saturated pixels) is closed by
//! filling every region not reachable from the crop border, and the largest
//! connected piece is kept. Alignment, all in pixels of the sheet:
//!
//! * scale: the side view's axle centres are `wheelbase` (car.toml) apart;
//!   every view on the sheet shares that scale;
//! * side: the axle midpoint is the origin (z = 0) and `ground_y` is y = 0;
//! * front / rear: `ground_y` is y = 0 and the centreline (`centre_x`, or the
//!   middle of the silhouette) is x = 0;
//! * top: the centreline (`centre_y`, or the middle) is x = 0 and the front-most
//!   pixel sits at the front-most z measured in the side view.

use std::path::{Path, PathBuf};

use glam::{Vec2, Vec3};
use image::RgbImage;
use serde::Deserialize;

use crate::{
    model::Model,
    raster::{self, Grid, Mask, View},
    report::Gate,
    spec::CarSpec,
    thresholds::Thresholds,
};

#[derive(Clone, Debug, Deserialize)]
pub struct RefsFile {
    #[serde(default, rename = "sheet")]
    pub sheets: Vec<Sheet>,
    #[serde(default, rename = "photo")]
    pub photos: Vec<Photo>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Sheet {
    pub id: String,
    pub image: String,
    pub source: String,
    #[serde(default)]
    pub note: String,
    /// "gate" (default) fails the gate; "crosscheck" is reported only.
    #[serde(default = "gate_role")]
    pub role: String,
    #[serde(default = "default_luma")]
    pub ink_max_luma: u8,
    #[serde(default = "default_saturation")]
    pub ink_min_saturation: u8,
    /// Radius (px) of the morphological closing that seals small gaps in
    /// outlines and watermark channels before filling.
    #[serde(default = "default_close")]
    pub close_px: u32,
    /// Axle centres in the side view (pixels).
    pub front_axle_x: f32,
    pub rear_axle_x: f32,
    #[serde(rename = "view")]
    pub views: Vec<ViewRef>,
}

fn gate_role() -> String {
    "gate".into()
}
fn default_luma() -> u8 {
    150
}
fn default_close() -> u32 {
    2
}
fn default_saturation() -> u8 {
    60
}

#[derive(Clone, Debug, Deserialize)]
pub struct ViewRef {
    /// `side`, `front`, `rear` or `top`.
    pub view: String,
    /// `[x0, y0, x1, y1]`, x1/y1 exclusive.
    pub crop: [u32; 4],
    /// Which way the car's front points in the image (side and top views).
    #[serde(default)]
    pub front: Option<String>,
    #[serde(default)]
    pub ground_y: Option<f32>,
    #[serde(default)]
    pub centre_x: Option<f32>,
    #[serde(default)]
    pub centre_y: Option<f32>,
}

/// A photo for the side-by-side review pack.
#[derive(Clone, Debug, Deserialize)]
pub struct Photo {
    pub image: String,
    /// Review camera it pairs with (see `pipelines/blender/qa/review_render.py`).
    pub angle: String,
    pub source: String,
    #[serde(default)]
    pub license: String,
    #[serde(default)]
    pub note: String,
    /// Flip horizontally so the car faces the same way as the render.
    #[serde(default)]
    pub mirror: bool,
}

/// One aligned reference view.
#[derive(Clone, Debug)]
pub struct Prepared {
    pub sheet: String,
    pub crosscheck: bool,
    pub view: View,
    pub mask: Mask,
    /// Maps view metres to pixels of `mask`.
    pub grid: Grid,
}

impl RefsFile {
    pub fn load(path: &Path) -> Result<RefsFile, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Square dilation (`grow`) or erosion of a mask by `r` pixels.
fn morph(m: &Mask, r: u32, grow: bool) -> Mask {
    let r = r as i64;
    let pass = |src: &Mask, horizontal: bool| {
        let mut out = Mask::new(src.width, src.height);
        for y in 0..src.height as i64 {
            for x in 0..src.width as i64 {
                let mut any = !grow;
                for d in -r..=r {
                    let (nx, ny) = if horizontal { (x + d, y) } else { (x, y + d) };
                    let inside =
                        nx >= 0 && ny >= 0 && nx < src.width as i64 && ny < src.height as i64;
                    // Outside the image counts as empty for growing and as
                    // full for shrinking, so borders don't erode.
                    let v = if inside { src.get(nx, ny) } else { !grow };
                    if grow && v {
                        any = true;
                        break;
                    }
                    if !grow && !v {
                        any = false;
                        break;
                    }
                }
                out.set(x as u32, y as u32, any);
            }
        }
        out
    };
    pass(&pass(m, true), false)
}

/// Ink → bottom closed under wheels → closing → filled silhouette → largest
/// piece.
pub fn extract_mask(
    img: &RgbImage,
    crop: [u32; 4],
    max_luma: u8,
    min_sat: u8,
    close_px: u32,
) -> Mask {
    let [x0, y0, x1, y1] = crop;
    let (w, h) = (x1.min(img.width()) - x0, y1.min(img.height()) - y0);
    let mut ink = Mask::new(w, h);
    for y in 0..h {
        for x in 0..w {
            let p = img.get_pixel(x0 + x, y0 + y).0;
            let luma = (p[0] as u32 + p[1] as u32 + p[2] as u32) / 3;
            let sat = p.iter().max().unwrap() - p.iter().min().unwrap();
            ink.set(x, y, luma < max_luma as u32 || sat > min_sat);
        }
    }
    // Wheels stand on the crop's bottom edge, where their outlines are cut:
    // close the bottom row under ink that reaches the last few rows.
    for x in 0..w {
        if (h.saturating_sub(4)..h).any(|y| ink.get(x as i64, y as i64)) {
            ink.set(x, h - 1, true);
        }
    }
    let ink = if close_px > 0 {
        morph(&ink, close_px, true)
    } else {
        ink
    };
    // Background = non-ink reachable from the border (4-connected).
    let mut outside = Mask::new(w, h);
    let mut stack: Vec<(u32, u32)> = Vec::new();
    for x in 0..w {
        stack.push((x, 0));
        stack.push((x, h - 1));
    }
    for y in 0..h {
        stack.push((0, y));
        stack.push((w - 1, y));
    }
    while let Some((x, y)) = stack.pop() {
        let i = (y * w + x) as usize;
        if outside.data[i] || ink.data[i] {
            continue;
        }
        outside.data[i] = true;
        if x > 0 {
            stack.push((x - 1, y));
        }
        if x + 1 < w {
            stack.push((x + 1, y));
        }
        if y > 0 {
            stack.push((x, y - 1));
        }
        if y + 1 < h {
            stack.push((x, y + 1));
        }
    }
    let mut filled = Mask::new(w, h);
    for i in 0..filled.data.len() {
        filled.data[i] = !outside.data[i];
    }
    if close_px > 0 {
        filled = morph(&filled, close_px, false);
    }
    largest_component(&filled)
}

fn largest_component(m: &Mask) -> Mask {
    let (w, h) = (m.width, m.height);
    let mut label = vec![0u32; m.data.len()];
    let mut best = (0usize, 0u32);
    let mut next = 0u32;
    for start in 0..m.data.len() {
        if !m.data[start] || label[start] != 0 {
            continue;
        }
        next += 1;
        let mut size = 0;
        let mut stack = vec![start];
        label[start] = next;
        while let Some(i) = stack.pop() {
            size += 1;
            let (x, y) = ((i as u32 % w) as i64, (i as u32 / w) as i64);
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let (nx, ny) = (x + dx, y + dy);
                    if m.get(nx, ny) {
                        let j = (ny as u32 * w + nx as u32) as usize;
                        if label[j] == 0 {
                            label[j] = next;
                            stack.push(j);
                        }
                    }
                }
            }
        }
        if size > best.0 {
            best = (size, next);
        }
    }
    let mut out = Mask::new(w, h);
    for i in 0..out.data.len() {
        out.data[i] = best.1 != 0 && label[i] == best.1;
    }
    out
}

/// Crops, masks and aligns every view of a sheet.
pub fn prepare_sheet(sheet: &Sheet, dir: &Path, spec: &CarSpec) -> Result<Vec<Prepared>, String> {
    let path: PathBuf = dir.join(&sheet.image);
    let img = image::open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .to_rgb8();
    let mpp = spec.wheelbase / (sheet.rear_axle_x - sheet.front_axle_x).abs();
    let flip = |v: &ViewRef| v.front.as_deref() == Some("right");
    let mut out = Vec::new();
    let mut side_front_z: Option<f32> = None;
    // Side first: the top view needs its front-most z.
    let mut views: Vec<&ViewRef> = sheet.views.iter().collect();
    views.sort_by_key(|v| v.view != "side");
    for v in views {
        let view = View::parse(&v.view).ok_or_else(|| format!("unknown view {:?}", v.view))?;
        let mut mask = extract_mask(
            &img,
            v.crop,
            sheet.ink_max_luma,
            sheet.ink_min_saturation,
            sheet.close_px,
        );
        if let Some(g) = v.ground_y {
            // Drop the drawn ground line and anything under it.
            let first = (g - v.crop[1] as f32).round().max(0.0) as u32;
            for y in first..mask.height {
                for x in 0..mask.width {
                    mask.set(x, y, false);
                }
            }
        }
        let (bx0, by0, bx1, by1) = mask
            .bbox()
            .ok_or_else(|| format!("{}: empty silhouette in view {}", sheet.id, v.view))?;
        let [x0, y0, ..] = v.crop.map(|c| c as f32);
        let ground = || {
            v.ground_y
                .map(|g| g - y0)
                .ok_or_else(|| format!("{}: view {} needs ground_y", sheet.id, v.view))
        };
        let grid = match view {
            View::Side => {
                let grid = Grid {
                    width: mask.width,
                    height: mask.height,
                    ox: (sheet.front_axle_x + sheet.rear_axle_x) / 2.0 - x0,
                    oy: ground()?,
                    mpp,
                    flip_u: flip(v),
                };
                let front_px = if flip(v) {
                    bx1 as f32 + 1.0
                } else {
                    bx0 as f32
                };
                side_front_z = Some(grid.to_metric(front_px, 0.0).x);
                grid
            }
            View::Front | View::Rear => Grid {
                width: mask.width,
                height: mask.height,
                ox: v
                    .centre_x
                    .map(|c| c - x0)
                    .unwrap_or((bx0 + bx1 + 1) as f32 / 2.0),
                oy: ground()?,
                mpp,
                flip_u: false,
            },
            View::Top => {
                let z_front = side_front_z
                    .ok_or_else(|| format!("{}: top view needs a side view", sheet.id))?;
                let ox = if flip(v) {
                    bx1 as f32 + 1.0 + z_front / mpp
                } else {
                    bx0 as f32 - z_front / mpp
                };
                Grid {
                    width: mask.width,
                    height: mask.height,
                    ox,
                    oy: v
                        .centre_y
                        .map(|c| c - y0)
                        .unwrap_or((by0 + by1 + 1) as f32 / 2.0),
                    mpp,
                    flip_u: flip(v),
                }
            }
            _ => return Err(format!("{}: view {} not supported", sheet.id, v.view)),
        };
        out.push(Prepared {
            sheet: sheet.id.clone(),
            crosscheck: sheet.role == "crosscheck",
            view,
            mask,
            grid,
        });
    }
    Ok(out)
}

/// Resamples `b` into `a`'s grid (nearest pixel) and compares.
pub fn compare_refs(a: &Prepared, b: &Prepared) -> (f32, raster::BoundaryDistance) {
    let mut resampled = Mask::new(a.mask.width, a.mask.height);
    for y in 0..a.mask.height {
        for x in 0..a.mask.width {
            let m = a.grid.to_metric(x as f32 + 0.5, y as f32 + 0.5);
            let p = b.grid.to_px(m.x, m.y);
            resampled.set(x, y, b.mask.get(p.x.floor() as i64, p.y.floor() as i64));
        }
    }
    let d = raster::boundary_distance(&a.mask, &resampled);
    (
        raster::iou(&a.mask, &resampled),
        raster::BoundaryDistance {
            mean: d.mean * a.grid.mpp,
            p95: d.p95 * a.grid.mpp,
            max: d.max * a.grid.mpp,
        },
    )
}

/// LOD0 render triangles of the model.
pub fn lod0_triangles(model: &Model) -> Vec<[Vec3; 3]> {
    let mut tris = Vec::new();
    model.for_each_triangle(model.render_nodes(0), |t| tris.push(t));
    tris
}

/// Gate 4. Writes one overlay PNG per view into `out_dir` (if given).
pub fn silhouettes(
    model: &Model,
    refs: &[Prepared],
    t: &Thresholds,
    out_dir: Option<&Path>,
) -> Gate {
    let mut g = Gate::new(4, "Silhouette accuracy");
    if refs.is_empty() {
        g.check("references", "reference views", "none", "≥ 1", false);
        return g;
    }
    let tris = lod0_triangles(model);
    for r in refs {
        let mask = raster::silhouette(&r.grid, r.view, tris.iter().copied());
        let iou = raster::iou(&mask, &r.mask);
        let d = raster::boundary_distance(&mask, &r.mask);
        let (mean, p95) = (d.mean * r.grid.mpp, d.p95 * r.grid.mpp);
        let id = format!("{}:{}", r.sheet, r.view.name());
        let value = format!(
            "IoU {iou:.3}, outline mean {:.1} mm, p95 {:.1} mm, max {:.0} mm",
            mean * 1000.0,
            p95 * 1000.0,
            d.max * r.grid.mpp * 1000.0
        );
        let pass = iou >= t.silhouette_iou
            && mean <= t.silhouette_mean_distance
            && p95 <= t.silhouette_p95_distance;
        let what = format!("{} view vs {}", r.view.name(), r.sheet);
        if r.crosscheck {
            g.info(
                &id,
                format!("{what} (cross-check)"),
                format!("{value} ({})", if pass { "within" } else { "outside" }),
            );
        } else {
            g.check(
                &id,
                what,
                value,
                format!(
                    "IoU ≥ {}, mean ≤ {:.0} mm, p95 ≤ {:.0} mm",
                    t.silhouette_iou,
                    t.silhouette_mean_distance * 1000.0,
                    t.silhouette_p95_distance * 1000.0
                ),
                pass,
            );
        }
        if let Some(dir) = out_dir {
            let name = format!("silhouette-{}-{}.png", r.sheet, r.view.name());
            if raster::overlay(&mask, &r.mask)
                .save(dir.join(&name))
                .is_ok()
            {
                g.images.push(name);
            }
        }
    }
    g.note(
        "Overlays: dark grey = both, red = model only, blue = reference only. \
         References are scaled by the wheelbase and aligned on the ground line \
         (see `data/cars/<id>/refs/refs.toml`).",
    );
    g
}

/// Silhouette of the analytic shape list, for synthetic references.
pub fn mask_from_fn(grid: &Grid, inside: impl Fn(Vec2) -> bool) -> Mask {
    let mut m = Mask::new(grid.width, grid.height);
    for y in 0..grid.height {
        for x in 0..grid.width {
            m.set(x, y, inside(grid.to_metric(x as f32 + 0.5, y as f32 + 0.5)));
        }
    }
    m
}
