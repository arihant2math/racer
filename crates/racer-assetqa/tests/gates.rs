//! Calibration: every gate passes the good box car and catches the fault of
//! each broken variant; the committed 911 references are scaled sensibly.

use std::{collections::HashMap, path::Path, sync::OnceLock};

use glam::Vec2;
use racer_assetqa::{
    fixture::{self, Fault},
    model::Model,
    raster::{Grid, View},
    refs::{self, Prepared, RefsFile},
    report::{Gate, Status},
    spec::CarSpec,
    static_gates,
    thresholds::Thresholds,
};

fn root() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn spec() -> CarSpec {
    CarSpec::load(&root().join("data/cars/porsche-911-carrera-s-992/car.toml")).unwrap()
}

/// Synthetic blueprints of the good box car, drawn analytically (not by the
/// rasteriser under test) at 6 mm/px with the side view's front on the right.
fn synthetic_refs(spec: &CarSpec) -> Vec<Prepared> {
    let parts = fixture::parts(spec);
    [View::Side, View::Front, View::Rear, View::Top]
        .into_iter()
        .map(|view| {
            let half = Vec2::new(2.6, 1.6);
            let mut grid = Grid::fit(Vec2::new(-half.x, -half.y), half, 0.006, 10);
            if view == View::Side {
                grid.flip_u = true;
            }
            let mask = refs::mask_from_fn(&grid, |p| fixture::analytic_inside(&parts, view, p));
            Prepared {
                sheet: "synthetic".into(),
                crosscheck: false,
                view,
                mask,
                grid,
            }
        })
        .collect()
}

/// Gates 1–4 for every fixture variant, computed once.
fn results() -> &'static HashMap<&'static str, Vec<Gate>> {
    static RESULTS: OnceLock<HashMap<&'static str, Vec<Gate>>> = OnceLock::new();
    RESULTS.get_or_init(|| {
        let spec = spec();
        let refs = synthetic_refs(&spec);
        let t = Thresholds::default();
        Fault::ALL
            .iter()
            .map(|&f| {
                let model = Model::from_glb(&fixture::box_car(&spec, f)).unwrap();
                (f.name(), static_gates(&model, &spec, &refs, &t, None))
            })
            .collect()
    })
}

fn describe(gates: &[Gate]) -> String {
    gates
        .iter()
        .flat_map(|g| &g.checks)
        .filter(|c| c.status == Status::Fail)
        .map(|c| format!("{}: {} [{}]", c.id, c.value, c.threshold))
        .collect::<Vec<_>>()
        .join("\n")
}

fn status(fault: Fault, gate: u8, check: &str) -> Status {
    let gates = &results()[fault.name()];
    gates[gate as usize - 1]
        .status_of(check)
        .unwrap_or_else(|| panic!("{}: no check {check} in gate {gate}", fault.name()))
}

fn assert_fails(fault: Fault, gate: u8, checks: &[&str]) {
    let gates = &results()[fault.name()];
    assert_eq!(
        gates[gate as usize - 1].status,
        Status::Fail,
        "{}: gate {gate} should fail",
        fault.name()
    );
    for c in checks {
        assert_eq!(
            status(fault, gate, c),
            Status::Fail,
            "{}: check {c} should fail; failures:\n{}",
            fault.name(),
            describe(gates)
        );
    }
}

#[test]
fn good_fixture_passes_every_gate() {
    let gates = &results()["good"];
    assert!(
        gates.iter().all(|g| g.status == Status::Pass),
        "failures:\n{}",
        describe(gates)
    );
    // The silhouette match is near perfect, so the thresholds have headroom.
    for c in &gates[3].checks {
        let iou: f32 = c.value[4..9].parse().unwrap();
        assert!(iou > 0.98, "{}: {}", c.id, c.value);
    }
}

#[test]
fn wrong_scale_is_caught() {
    assert_fails(Fault::WrongScale, 1, &["units", "wheel_positions"]);
    assert_fails(
        Fault::WrongScale,
        2,
        &["length", "width", "height", "wheelbase"],
    );
    assert_fails(Fault::WrongScale, 4, &["synthetic:side", "synthetic:front"]);
}

#[test]
fn missing_wheels_are_caught() {
    assert_fails(Fault::MissingWheels, 1, &["nodes", "sockets", "facing"]);
    assert_fails(Fault::MissingWheels, 2, &["wheelbase"]);
}

#[test]
fn flipped_normals_are_caught() {
    assert_fails(Fault::FlippedNormals, 3, &["backfaces"]);
    // Normals were flipped with the winding, so they still agree.
    assert_eq!(
        status(Fault::FlippedNormals, 3, "normals_vs_winding"),
        Status::Pass
    );
}

#[test]
fn non_manifold_body_is_caught() {
    assert_fails(Fault::NonManifoldBody, 3, &["non_manifold"]);
    assert_eq!(results()["non-manifold-body"][0].status, Status::Pass);
}

#[test]
fn wrong_orientation_is_caught() {
    assert_fails(Fault::WrongOrientation, 1, &["facing", "placement"]);
    assert_fails(
        Fault::WrongOrientation,
        4,
        &["synthetic:side", "synthetic:top"],
    );
    // Bounding-box dimensions alone cannot see it.
    for c in ["length", "width", "height"] {
        assert_eq!(status(Fault::WrongOrientation, 2, c), Status::Pass);
    }
}

#[test]
fn z_up_is_caught() {
    assert_fails(Fault::ZUp, 1, &["up_axis", "ground"]);
    assert_fails(Fault::ZUp, 2, &["height"]);
}

#[test]
fn missing_uvs_are_caught() {
    assert_fails(Fault::NoUvs, 3, &["uv_present"]);
}

#[test]
fn concave_collision_is_caught() {
    assert_fails(Fault::ConcaveCollision, 3, &["collision_convex"]);
}

#[test]
fn stretched_car_is_caught() {
    assert_fails(Fault::Stretched, 2, &["length", "wheelbase"]);
    assert_fails(Fault::Stretched, 4, &["synthetic:side", "synthetic:top"]);
    assert_eq!(status(Fault::Stretched, 4, "synthetic:front"), Status::Pass);
}

#[test]
fn every_fault_fails_some_gate() {
    for f in Fault::ALL.into_iter().filter(|f| *f != Fault::None) {
        assert!(
            results()[f.name()].iter().any(|g| g.status == Status::Fail),
            "{} passed every gate",
            f.name()
        );
    }
}

/// The image path: a synthetic blueprint sheet written as a PNG with a
/// refs.toml, read back through `prepare_sheet`.
#[test]
fn reference_sheet_images_align() {
    let spec = spec();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("assetqa-sheet");
    std::fs::create_dir_all(&dir).unwrap();
    // Side view, 8 mm/px, front on the left, origin at (400, 300) px.
    let grid = Grid {
        width: 800,
        height: 360,
        ox: 400.0,
        oy: 300.0,
        mpp: 0.008,
        flip_u: false,
    };
    let parts = fixture::parts(&spec);
    let mask = refs::mask_from_fn(&grid, |p| fixture::analytic_inside(&parts, View::Side, p));
    let mut img = image::RgbImage::from_pixel(800, 360, image::Rgb([255, 255, 255]));
    for y in 0..360 {
        for x in 0..800 {
            if mask.get(x, y) {
                img.put_pixel(x as u32, y as u32, image::Rgb([200, 30, 30]));
            }
        }
    }
    img.save(dir.join("sheet.png")).unwrap();
    let wb_px = spec.wheelbase / 0.008;
    std::fs::write(
        dir.join("refs.toml"),
        format!(
            "[[sheet]]\nid = \"png\"\nimage = \"sheet.png\"\nsource = \"test\"\n\
             front_axle_x = {}\nrear_axle_x = {}\n\
             [[sheet.view]]\nview = \"side\"\ncrop = [50, 20, 760, 340]\nfront = \"left\"\nground_y = 300.0\n",
            400.0 - wb_px / 2.0,
            400.0 + wb_px / 2.0
        ),
    )
    .unwrap();
    let file = RefsFile::load(&dir.join("refs.toml")).unwrap();
    let prepared = refs::prepare_sheet(&file.sheets[0], &dir, &spec).unwrap();
    assert!((prepared[0].grid.mpp - 0.008).abs() < 1e-6);
    let model = Model::from_glb(&fixture::box_car(&spec, Fault::None)).unwrap();
    let gate = refs::silhouettes(&model, &prepared, &Thresholds::default(), None);
    assert_eq!(gate.status, Status::Pass, "{:?}", gate.checks);
}

/// The committed 911 blueprints come out at the car's real size, and the two
/// independent sheets agree (this bounds the silhouette thresholds).
#[test]
fn committed_911_references_are_consistent() {
    let spec = spec();
    let dir = root().join("data/cars/porsche-911-carrera-s-992/refs");
    let file = RefsFile::load(&dir.join("refs.toml")).unwrap();
    let mut by_sheet = Vec::new();
    for sheet in &file.sheets {
        let prepared = refs::prepare_sheet(sheet, &dir, &spec).unwrap();
        for p in &prepared {
            let (x0, y0, x1, y1) = p.mask.bbox().unwrap();
            let w = (x1 - x0 + 1) as f32 * p.grid.mpp;
            let h = (y1 - y0 + 1) as f32 * p.grid.mpp;
            match p.view {
                View::Side => {
                    assert!(
                        (w - spec.length).abs() < 0.06,
                        "{} side length {w}",
                        p.sheet
                    );
                    assert!(
                        (h - spec.height).abs() < 0.04,
                        "{} side height {h}",
                        p.sheet
                    );
                }
                View::Front | View::Rear => {
                    // Over the mirrors: 2.024 m published.
                    assert!(
                        (w - 2.024).abs() < 0.05,
                        "{} {:?} width {w}",
                        p.sheet,
                        p.view
                    );
                    assert!(
                        (h - spec.height).abs() < 0.04,
                        "{} {:?} height {h}",
                        p.sheet,
                        p.view
                    );
                }
                View::Top => {
                    assert!((w - spec.length).abs() < 0.06, "{} top length {w}", p.sheet);
                }
                _ => unreachable!(),
            }
        }
        by_sheet.push(prepared);
    }
    for (a, b) in by_sheet[0].iter().zip(&by_sheet[1]) {
        assert_eq!(a.view, b.view);
        let (iou, d) = refs::compare_refs(a, b);
        assert!(iou > 0.9, "{:?}: sheets agree only to IoU {iou}", a.view);
        assert!(
            d.mean < 0.03,
            "{:?}: mean outline distance {}",
            a.view,
            d.mean
        );
    }
}

/// Separately exported LOD files are merged and tagged.
#[test]
fn sibling_lod_files_merge() {
    let spec = spec();
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join("assetqa-lods");
    std::fs::create_dir_all(&dir).unwrap();
    let glb = fixture::box_car(&spec, Fault::None);
    std::fs::write(dir.join("car.glb"), &glb).unwrap();
    std::fs::write(dir.join("car_lod2.glb"), &glb).unwrap();
    let single = Model::from_glb(&glb).unwrap();
    let (merged, files) = Model::load_with_lods(&dir.join("car.glb")).unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(merged.nodes.len(), 2 * single.nodes.len());
    // Everything from the LOD2 file is at least LOD2 and its collision is dropped.
    let tail = &merged.nodes[single.nodes.len()..];
    assert!(tail.iter().all(|n| n.lod >= 2));
    assert!(
        tail.iter()
            .filter(|n| n.collision)
            .all(|n| n.prims.is_empty())
    );
    assert_eq!(
        merged.triangles(merged.collision_nodes()),
        single.triangles(single.collision_nodes())
    );
    // LOD0 is untouched, so the LOD0 checks see the same car.
    assert_eq!(
        merged.triangles(merged.render_nodes(0)),
        single.triangles(single.render_nodes(0))
    );
}
