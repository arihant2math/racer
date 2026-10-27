//! Gate 5, visual review pack: Blender beauty renders from fixed cameras,
//! side-by-sides with reference photos and blueprint views, and the rubric a
//! (multimodal) reviewer scores them with.

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use image::{RgbImage, imageops};

use crate::{
    refs::{RefsFile, Sheet},
    report::{Gate, Status},
};

/// Cameras `review_render.py` always renders (close-ups depend on parts).
pub const CAMERAS: [&str; 7] = [
    "front-three-quarter",
    "rear-three-quarter",
    "side",
    "front",
    "rear",
    "top",
    "front-low",
];

pub const RUBRIC: &str = "\
# Review rubric

Score each criterion 0–2 (0 = wrong or missing, 1 = recognisable but off,
2 = matches the references). The review passes with no 0 and a total of at
least 12 of 16. Cite the image that supports each score.

| # | Criterion | What to compare |
|---|---|---|
| 1 | Proportions | Side/front/rear/top renders vs blueprint side-by-sides: overhangs, roof and window line, wheel-to-body ratio, stance (ride height, wheel position in the arches), track vs body width. |
| 2 | Front signature | Round headlights with four-point DRL, bonnet shape and creases, front intakes. |
| 3 | Rear signature | Full-width light bar, engine-lid grille, retractable spoiler, exhaust outlets. |
| 4 | Wheels | Staggered 20/21-inch wheels filling the arches, spoke design, tyre sidewall profile, brake visible behind spokes. |
| 5 | Glass and openings | Windows read as glass (tinted, reflective), dark interior visible through them, door and panel gaps where the real car has them. |
| 6 | Surface quality | No faceting, dents, lumps, wavy reflections, noise or melted detail; crisp feature lines. |
| 7 | Materials | Paint (metallic/clearcoat), black trim, chrome, rubber, rims, emissive lights assigned to the right surfaces. |
| 8 | Overall likeness | Would someone recognise a 992 Carrera S at a glance from the three-quarter renders? |
";

pub fn find_blender(root: &Path) -> Option<PathBuf> {
    if let Ok(b) = std::env::var("BLENDER") {
        return Some(PathBuf::from(b));
    }
    let local = root.join(".tools/blender/blender");
    if local.exists() {
        return Some(local);
    }
    Command::new("blender")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|_| PathBuf::from("blender"))
}

/// Scales `img` to `height`.
fn to_height(img: &RgbImage, height: u32) -> RgbImage {
    let w = (img.width() as f32 * height as f32 / img.height() as f32).round() as u32;
    imageops::resize(img, w.max(1), height, imageops::FilterType::Triangle)
}

/// Two images next to each other at the same height, on white.
pub fn side_by_side(a: &RgbImage, b: &RgbImage, height: u32) -> RgbImage {
    let (a, b) = (to_height(a, height), to_height(b, height));
    let gap = 16;
    let mut out = RgbImage::from_pixel(
        a.width() + gap + b.width(),
        height,
        image::Rgb([255, 255, 255]),
    );
    imageops::replace(&mut out, &a, 0, 0);
    imageops::replace(&mut out, &b, (a.width() + gap) as i64, 0);
    out
}

fn blueprint_views(sheet: &Sheet, refs_dir: &Path) -> Vec<(String, RgbImage)> {
    let Ok(img) = image::open(refs_dir.join(&sheet.image)) else {
        return Vec::new();
    };
    let img = img.to_rgb8();
    sheet
        .views
        .iter()
        .map(|v| {
            let [x0, y0, x1, y1] = v.crop;
            let mut crop = imageops::crop_imm(
                &img,
                x0,
                y0,
                x1.min(img.width()) - x0,
                y1.min(img.height()) - y0,
            )
            .to_image();
            // Present every view with the front on the left, like the renders.
            if v.front.as_deref() == Some("right") {
                crop = imageops::flip_horizontal(&crop);
            }
            (v.view.clone(), crop)
        })
        .collect()
}

pub struct ReviewOptions<'a> {
    pub root: &'a Path,
    pub glb: &'a Path,
    pub refs_dir: &'a Path,
    pub refs: Option<&'a RefsFile>,
    pub out_dir: &'a Path,
    pub samples: u32,
    /// Reuse renders already in `out_dir/review` instead of running Blender.
    pub reuse: bool,
}

pub fn run(o: &ReviewOptions) -> Gate {
    let name = "Visual review pack";
    let review_dir = o.out_dir.join("review");
    if let Err(e) = std::fs::create_dir_all(&review_dir) {
        return Gate::skipped(5, name, format!("{}: {e}", review_dir.display()));
    }
    let mut g = Gate::new(5, name);
    if !o.reuse {
        let Some(blender) = find_blender(o.root) else {
            return Gate::skipped(
                5,
                name,
                "Blender not found (run tools/setup.sh --with-blender or set BLENDER)",
            );
        };
        let script = o.root.join("pipelines/blender/qa/review_render.py");
        let out = Command::new(&blender)
            .args(["-b", "--factory-startup", "--python"])
            .arg(&script)
            .arg("--")
            .arg(o.glb)
            .arg(&review_dir)
            .args(["--samples", &o.samples.to_string()])
            .output();
        let log = match &out {
            Ok(out) => {
                String::from_utf8_lossy(&out.stdout).to_string()
                    + &String::from_utf8_lossy(&out.stderr)
            }
            Err(e) => e.to_string(),
        };
        let _ = std::fs::write(review_dir.join("blender.log"), &log);
        if !out.as_ref().is_ok_and(|o| o.status.success()) {
            g.check(
                "renders",
                "Blender review renders",
                "Blender failed (see review/blender.log)",
                "all cameras",
                false,
            );
            return g;
        }
    }
    let mut renders: Vec<String> = std::fs::read_dir(&review_dir)
        .map(|d| {
            d.flatten()
                .map(|e| e.file_name().to_string_lossy().to_string())
                .filter(|n| n.ends_with(".png") && !n.starts_with("compare-"))
                .collect()
        })
        .unwrap_or_default();
    renders.sort();
    let missing: Vec<&str> = CAMERAS
        .iter()
        .copied()
        .filter(|c| !renders.contains(&format!("{c}.png")))
        .collect();
    g.check(
        "renders",
        "beauty renders from the fixed cameras",
        if missing.is_empty() {
            format!("{} renders", renders.len())
        } else {
            format!("missing {}", missing.join(", "))
        },
        CAMERAS.join(", "),
        missing.is_empty(),
    );
    if let Ok(stats) = std::fs::read_to_string(review_dir.join("review.json"))
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(&stats)
    {
        g.info(
            "blender_stats",
            "scene as Blender imports it",
            format!(
                "LOD0 {} triangles, per LOD {}, {} mesh objects, import {} s",
                v["lod0_triangles"], v["per_lod_triangles"], v["objects"], v["import_seconds"]
            ),
        );
    }
    for r in &renders {
        g.images.push(format!("review/{r}"));
    }

    // Side-by-sides.
    let load = |p: &Path| image::open(p).ok().map(|i| i.to_rgb8());
    if let Some(refs) = o.refs {
        for sheet in refs.sheets.iter().filter(|s| s.role != "crosscheck") {
            for (view, bp) in blueprint_views(sheet, o.refs_dir) {
                if let Some(render) = load(&review_dir.join(format!("{view}.png"))) {
                    let name = format!("review/compare-blueprint-{}-{view}.png", sheet.id);
                    if side_by_side(&render, &bp, 360)
                        .save(o.out_dir.join(&name))
                        .is_ok()
                    {
                        g.images.push(name);
                    }
                }
            }
        }
        for photo in &refs.photos {
            let (Some(render), Some(mut p)) = (
                load(&review_dir.join(format!("{}.png", photo.angle))),
                load(&o.refs_dir.join(&photo.image)),
            ) else {
                continue;
            };
            if photo.mirror {
                p = imageops::flip_horizontal(&p);
            }
            let stem = Path::new(&photo.image)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();
            let name = format!("review/compare-photo-{}-{stem}.png", photo.angle);
            if side_by_side(&render, &p, 480)
                .save(o.out_dir.join(&name))
                .is_ok()
            {
                g.images.push(name);
            }
        }
    }
    let _ = std::fs::write(review_dir.join("rubric.md"), RUBRIC);
    if g.status == Status::Pass {
        g.status = Status::Review;
    }
    g.note(
        "The renders and side-by-sides need a reviewer: score them with `review/rubric.md` \
         (left = render, right = reference in every side-by-side).",
    );
    g
}
