//! racer-citykit: validate, export and measure the track-side asset kit.
//!
//! ```text
//! racer-citykit validate            # budgets, naming, UVs, watertightness, LODs
//! racer-citykit textures <dir>      # write every procedural texture as PNG
//! racer-citykit obj <dir>           # export every asset/LOD as OBJ (+ kit.mtl)
//! racer-citykit stats               # showcase street triangle/draw counts
//! ```

use std::{path::Path, process::ExitCode};

use bevy::prelude::*;
use racer_citykit::{
    catalogue::{self, catalogue},
    district::District,
    export,
    materials::{KitMaterials, TimeOfDay},
    showcase::build_showcase,
    texture::{self, Encoding},
};
use racer_render::surface::SurfaceMaterial;

const USAGE: &str = "usage:
  racer-citykit validate
  racer-citykit textures <dir>
  racer-citykit obj <dir>
  racer-citykit stats";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let r = match (args.first().map(String::as_str), args.get(1)) {
        (Some("validate"), None) => validate(),
        (Some("textures"), Some(dir)) => textures(Path::new(dir)),
        (Some("obj"), Some(dir)) => obj(Path::new(dir)),
        (Some("stats"), None) => stats(),
        _ => Err(USAGE.to_string()),
    };
    match r {
        Ok(code) => code,
        Err(e) => {
            eprintln!("racer-citykit: {e}");
            ExitCode::from(2)
        }
    }
}

fn validate() -> Result<ExitCode, String> {
    let mut failed = 0;
    println!("{:<34} {:>8} {:>8} {:>7} {:>22}  result", "asset", "LOD0", "LOD1", "sockets", "size (m)");
    for a in catalogue() {
        let f = catalogue::check(&a);
        let size = catalogue::bounds(&a).map_or(Vec3::ZERO, |(lo, hi)| hi - lo);
        println!(
            "{:<34} {:>8} {:>8} {:>7} {:>22}  {}",
            a.name,
            a.triangles(0),
            if a.lods.len() > 1 { a.triangles(1).to_string() } else { "-".into() },
            a.sockets.len(),
            format!("{:.1} x {:.1} x {:.1}", size.x, size.y, size.z),
            if f.errors.is_empty() { "ok".to_string() } else { format!("FAIL ({})", f.errors.len()) }
        );
        for e in &f.errors {
            println!("    {e}");
        }
        failed += usize::from(!f.errors.is_empty());
    }
    Ok(if failed == 0 { ExitCode::SUCCESS } else { ExitCode::FAILURE })
}

fn textures(dir: &Path) -> Result<ExitCode, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut out: Vec<(&str, texture::Tex, Encoding)> = Vec::new();
    for (name, t) in [
        ("asphalt", texture::asphalt(1024, 11)),
        ("paving", texture::paving(1024, 12)),
        ("concrete", texture::concrete(512, 13, 0.34)),
        ("brick", texture::brick(512, 15)),
        ("plaster", texture::plaster(512, 16)),
        ("roof", texture::roof(512, 17)),
        ("curb", texture::curb(512, 18)),
        ("decals", texture::decal_atlas(1024, 19)),
    ] {
        let leak: &'static str = Box::leak(format!("{name}_albedo").into_boxed_str());
        out.push((leak, t.albedo, Encoding::Srgb));
        let leak: &'static str = Box::leak(format!("{name}_normal").into_boxed_str());
        out.push((leak, t.normal, Encoding::Normal));
        let leak: &'static str = Box::leak(format!("{name}_orm").into_boxed_str());
        out.push((leak, t.orm, Encoding::Linear));
    }
    out.push(("fence", texture::fence(256), Encoding::Srgb));
    out.push(("windows", texture::window_atlas(512, 20), Encoding::Srgb));
    out.push(("signs", texture::sign_atlas(512, 21), Encoding::Srgb));
    out.push(("road_signs", texture::road_sign_atlas(512), Encoding::Srgb));
    out.push(("skyline", texture::skyline(512, 22, false), Encoding::Srgb));
    out.push(("skyline_lit", texture::skyline(512, 22, true), Encoding::Srgb));
    for (name, t, enc) in out {
        let path = dir.join(format!("{name}.png"));
        t.save_png(&path, enc).map_err(|e| format!("{}: {e}", path.display()))?;
        println!("wrote {}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn obj(dir: &Path) -> Result<ExitCode, String> {
    let files = export::write_obj(&catalogue(), dir).map_err(|e| e.to_string())?;
    println!("wrote {} OBJ files and kit.mtl to {}", files.len(), dir.display());
    Ok(ExitCode::SUCCESS)
}

fn stats() -> Result<ExitCode, String> {
    let (mut images, mut std_mats, mut surf) = (Assets::<Image>::default(), Assets::<StandardMaterial>::default(), Assets::<SurfaceMaterial>::default());
    let kit = KitMaterials::new(TimeOfDay::Day, 0.0, &mut images, &mut std_mats, &mut surf);
    let (batcher, lights, buildings) = build_showcase(&District::downtown(), &kit, &mut surf);
    let s = batcher.stats();
    println!("showcase street: {} mesh entities (draws before culling), {} triangles within 70 m (LOD0 + always-on), {} beyond (LOD1 + always-on), {} buildings, {} light sockets", s.entities, s.triangles_lod0, s.triangles_far, buildings, lights.len());
    Ok(ExitCode::SUCCESS)
}
