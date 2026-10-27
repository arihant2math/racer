//! Wavefront OBJ export of kit assets (one object per asset/LOD, one
//! material group per slot), used by the Blender review pipeline in
//! `pipelines/blender/` and for inspection in any DCC tool.

use std::{fmt::Write as _, path::Path};

use crate::kit::{Asset, Slot};

/// Approximate display colour of a slot (sRGB) for the MTL file.
pub fn slot_colour(slot: Slot) -> [f32; 3] {
    match slot {
        Slot::Road | Slot::RoadPlain | Slot::Decal => [0.1, 0.1, 0.11],
        Slot::Sidewalk | Slot::Kerb | Slot::Concrete | Slot::Stone => [0.6, 0.6, 0.58],
        Slot::CurbPaint | Slot::PaintRed | Slot::SignalRed => [0.75, 0.1, 0.1],
        Slot::Galvanised | Slot::RoofMetal | Slot::Fence => [0.7, 0.7, 0.72],
        Slot::Rubber | Slot::BoardDark | Slot::DarkMetal | Slot::SignalOff | Slot::CarGlass => {
            [0.05, 0.05, 0.05]
        }
        Slot::Chequer | Slot::BoardWhite | Slot::PaintWhite | Slot::LampLens => [0.9, 0.9, 0.9],
        Slot::Led | Slot::SignalAmber | Slot::PaintYellow => [0.95, 0.65, 0.1],
        Slot::SignalGreen | Slot::Foliage | Slot::Leaves => [0.1, 0.5, 0.2],
        Slot::SeatBlue => [0.1, 0.25, 0.7],
        Slot::Banner => [0.8, 0.2, 0.2],
        Slot::Crowd => [0.5, 0.4, 0.35],
        Slot::Wood | Slot::Bark => [0.4, 0.26, 0.15],
        Slot::RoadSign | Slot::ShopSign | Slot::Lightbox => [0.3, 0.5, 0.8],
        Slot::Glass | Slot::CurtainGlass | Slot::Window | Slot::ShopGlass | Slot::Skyline => {
            [0.25, 0.35, 0.4]
        }
        Slot::CarPaint => [0.6, 0.6, 0.62],
        Slot::Brick => [0.6, 0.3, 0.22],
        Slot::Plaster => [0.85, 0.82, 0.75],
        Slot::Mullion => [0.2, 0.2, 0.22],
        Slot::Roof => [0.25, 0.25, 0.25],
        Slot::Awning => [0.5, 0.1, 0.1],
    }
}

fn slot_name(slot: Slot) -> String {
    format!("{slot:?}").to_lowercase()
}

/// Writes `<dir>/<asset>_lod<k>.obj` for every LOD plus a shared `kit.mtl`.
pub fn write_obj(assets: &[Asset], dir: &Path) -> std::io::Result<Vec<std::path::PathBuf>> {
    std::fs::create_dir_all(dir)?;
    let mut mtl = String::new();
    let mut slots: Vec<Slot> = assets
        .iter()
        .flat_map(|a| a.lods.iter().flatten().map(|p| p.material))
        .collect();
    slots.sort();
    slots.dedup();
    for s in &slots {
        let [r, g, b] = slot_colour(*s);
        let _ = writeln!(mtl, "newmtl {}\nKd {r} {g} {b}\n", slot_name(*s));
    }
    std::fs::write(dir.join("kit.mtl"), mtl)?;
    let mut written = Vec::new();
    for a in assets {
        for (lod, parts) in a.lods.iter().enumerate() {
            let mut o = String::from("mtllib kit.mtl\n");
            let _ = writeln!(o, "o {}_lod{lod}", a.name);
            let mut base = 1;
            for part in parts {
                let m = &part.mesh;
                for p in &m.positions {
                    let _ = writeln!(o, "v {} {} {}", p.x, p.y, p.z);
                }
                for uv in &m.uvs {
                    let _ = writeln!(o, "vt {} {}", uv.x, 1.0 - uv.y);
                }
                for n in &m.normals {
                    let _ = writeln!(o, "vn {} {} {}", n.x, n.y, n.z);
                }
                let _ = writeln!(o, "usemtl {}", slot_name(part.material));
                for t in m.indices.chunks_exact(3) {
                    let [a, b, c] = [
                        t[0] as usize + base,
                        t[1] as usize + base,
                        t[2] as usize + base,
                    ];
                    let _ = writeln!(o, "f {a}/{a}/{a} {b}/{b}/{b} {c}/{c}/{c}");
                }
                base += m.positions.len();
            }
            let path = dir.join(format!("{}_lod{lod}.obj", a.name));
            std::fs::write(&path, o)?;
            written.push(path);
        }
    }
    Ok(written)
}
