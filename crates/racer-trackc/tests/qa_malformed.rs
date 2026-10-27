//! QA repros for malformed track data that validation accepts (see
//! `docs/qa/tooling-data-build.md`). Each test fails today and is ignored
//! with its bug id so the suite stays green.

use std::time::Duration;

use racer_trackc::{TrackFile, geometry};

fn shipped() -> String {
    std::fs::read_to_string(racer_trackc::tracks_dir().join("downtown-loop/track.toml")).unwrap()
}

/// The shipped track with the first occurrence of `from` replaced by `to`.
fn mutated(from: &str, to: &str) -> TrackFile {
    let text = shipped();
    assert!(text.contains(from), "fixture no longer contains {from:?}");
    TrackFile::parse(&text.replacen(from, to, 1)).unwrap()
}

fn assert_rejected(file: &TrackFile, what: &str) {
    let errors = geometry::validate(file);
    assert!(!errors.is_empty(), "validate accepted {what}");
}

#[test]
#[ignore = "QA-TOOL-1"]
fn rejects_bad_curb_width() {
    for value in ["nan", "-5.0", "0.0"] {
        let file = mutated("curb_width = 0.9", &format!("curb_width = {value}"));
        assert_rejected(&file, &format!("curb_width = {value}"));
    }
}

#[test]
#[ignore = "QA-TOOL-2"]
fn rejects_overlapping_or_reversed_grid_slots() {
    for value in ["0.0", "-7.0", "1.0"] {
        let file = mutated(
            "grid_row_spacing = 7.0",
            &format!("grid_row_spacing = {value}"),
        );
        assert_rejected(&file, &format!("grid_row_spacing = {value}"));
    }
}

#[test]
#[ignore = "QA-TOOL-3"]
fn rejects_non_finite_race_layout() {
    for (from, to) in [
        ("grid_row_spacing = 7.0", "grid_row_spacing = nan"),
        ("grid_column_offset = 2.6", "grid_column_offset = nan"),
        ("checkpoint_spacing = 90.0", "checkpoint_spacing = nan"),
        ("checkpoint_spacing = 90.0", "checkpoint_spacing = inf"),
        ("sprint_finish = [0.0, 42.0]", "sprint_finish = [nan, 0.0]"),
    ] {
        assert_rejected(&mutated(from, to), to);
    }
}

#[test]
#[ignore = "QA-TOOL-3"]
fn accepted_tracks_have_finite_grid_poses() {
    let file = mutated("grid_row_spacing = 7.0", "grid_row_spacing = nan");
    if geometry::validate(&file).is_empty() {
        let g = geometry::build(&file);
        assert!(
            g.grid.iter().all(|p| p.position.is_finite()),
            "validated track has non-finite grid poses"
        );
    }
}

#[test]
#[ignore = "QA-TOOL-4"]
fn compile_handles_ids_that_need_toml_escaping() {
    let dir = std::env::temp_dir().join(format!("racer-trackc-qa-id-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let text = shipped().replacen(
        "id = \"downtown-loop\"",
        "id = \"downtown \\\"loop\\\"\"",
        1,
    );
    std::fs::write(dir.join("track.toml"), text).unwrap();
    // Either validation rejects the id, or the compiled output loads back.
    let compiled = racer_trackc::compile(&dir);
    let loaded = compiled.is_ok().then(|| racer_trackc::load_dir(&dir));
    std::fs::remove_dir_all(&dir).ok();
    if let Some(loaded) = loaded {
        assert!(
            loaded.is_ok(),
            "compile wrote a summary.toml it cannot read back: {}",
            loaded.err().unwrap()
        );
    }
}

/// Every plan coordinate scaled by `k`.
fn scaled(k: f32) -> TrackFile {
    let mut file = TrackFile::parse(&shipped()).unwrap();
    let s = |p: &mut [f32; 2]| *p = [p[0] * k, p[1] * k];
    file.nodes.iter_mut().for_each(|n| s(&mut n.at));
    s(&mut file.race.start);
    s(&mut file.race.sprint_finish);
    file.race.sectors.iter_mut().for_each(s);
    file
}

#[test]
#[ignore = "QA-TOOL-5"]
fn validate_terminates_on_huge_tracks() {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(geometry::validate(&scaled(1e15)));
    });
    let errors = rx
        .recv_timeout(Duration::from_secs(20))
        .expect("validate did not finish within 20 s for a 2e18 m track");
    assert!(!errors.is_empty(), "validate accepted a 2e18 m track");
}
