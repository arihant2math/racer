//! Automated quality gate for generated car assets (PLAN.md sections 7 and 8).
//!
//! `racer-assetqa check <car-id>` loads `assets/cars/<id>/car.glb` and runs six
//! gates against `data/cars/<id>/car.toml` and the reference blueprints in
//! `data/cars/<id>/refs/`: structure, dimensions, mesh hygiene and silhouette
//! accuracy (pure Rust, deterministic), a Blender review pack, and a Bevy
//! smoke test. See `docs/assets/qa/README.md`.

pub mod fixture;
pub mod hygiene;
pub mod model;
pub mod raster;
pub mod refs;
pub mod report;
pub mod review;
pub mod smoke;
pub mod spec;
pub mod structure;
pub mod thresholds;

use std::path::Path;

use model::Model;
use refs::Prepared;
use report::Gate;
use spec::CarSpec;
use thresholds::Thresholds;

/// Gates 1–4 (no external tools).
pub fn static_gates(
    model: &Model,
    spec: &CarSpec,
    refs: &[Prepared],
    t: &Thresholds,
    out_dir: Option<&Path>,
) -> Vec<Gate> {
    vec![
        structure::structure(model, spec, t),
        structure::dimensions(model, spec, t),
        hygiene::hygiene(model, t),
        refs::silhouettes(model, refs, t, out_dir),
    ]
}
