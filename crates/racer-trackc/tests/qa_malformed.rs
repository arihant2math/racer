//! QA repros for malformed track data that validation used to accept (see
//! `docs/qa/tooling-data-build.md` and `docs/qa/fixes-tooling.md`).

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
fn rejects_bad_curb_width() {
    for value in ["nan", "-5.0", "0.0"] {
        let file = mutated("curb_width = 0.9", &format!("curb_width = {value}"));
        assert_rejected(&file, &format!("curb_width = {value}"));
    }
}

#[test]
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

#[test]
fn rejects_laps_over_the_length_bound_quickly() {
    // ~33 km: coordinates within bounds, lap too long.
    let started = std::time::Instant::now();
    let errors = geometry::validate(&scaled(15.0));
    assert!(
        errors.iter().any(|e| e.contains("longer than")),
        "{errors:?}"
    );
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[test]
fn rejects_ids_unsafe_for_files_and_toml() {
    for id in ["", "a\\\\b", "../x", "Downtown", "a b"] {
        let file = mutated("id = \"downtown-loop\"", &format!("id = \"{id}\""));
        assert_rejected(&file, &format!("id {id:?}"));
    }
}

/// A scratch copy of the shipped track directory with its generated files.
fn scratch_copy(tag: &str) -> std::path::PathBuf {
    let src = racer_trackc::tracks_dir().join("downtown-loop");
    let dir = std::env::temp_dir().join(format!("racer-trackc-qa-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(dir.join("generated")).unwrap();
    std::fs::copy(src.join("track.toml"), dir.join("track.toml")).unwrap();
    for entry in std::fs::read_dir(src.join("generated")).unwrap() {
        let path = entry.unwrap().path();
        std::fs::copy(&path, dir.join("generated").join(path.file_name().unwrap())).unwrap();
    }
    dir
}

/// QA-TOOL-17: `compile` removes speed profiles of cars it no longer
/// compiles, and leaves unrelated files alone.
#[test]
fn compile_removes_stale_generated_files() {
    let dir = scratch_copy("stale");
    let generated = dir.join("generated");
    std::fs::write(generated.join("speed-old-car.csv"), "distance,speed\n").unwrap();
    std::fs::write(generated.join("notes.txt"), "kept").unwrap();
    let result = racer_trackc::compile(&dir);
    let stale = generated.join("speed-old-car.csv").exists();
    let notes = generated.join("notes.txt").exists();
    std::fs::remove_dir_all(&dir).ok();
    result.unwrap();
    assert!(!stale, "stale speed-old-car.csv left behind");
    assert!(notes, "unrelated file removed");
}

/// QA-EDGE-8: corrupt generated data is an error from `load_dir`, not a
/// panic or NaN geometry.
#[test]
fn load_rejects_corrupt_generated_data() {
    let dir = scratch_copy("corrupt");
    let line = dir.join("generated/racing-line.csv");
    let summary = dir.join("generated/summary.toml");
    let (line_text, summary_text) = (
        std::fs::read_to_string(&line).unwrap(),
        std::fs::read_to_string(&summary).unwrap(),
    );
    let mut failures = Vec::new();
    let nan_line = {
        let mut rows: Vec<String> = line_text.lines().map(String::from).collect();
        let mut cols: Vec<&str> = rows[1].split(',').collect();
        cols[1] = "NaN";
        rows[1] = cols.join(",");
        rows.join("\n") + "\n"
    };
    for (what, line_csv, summary_toml) in [
        ("NaN racing line offset", nan_line, summary_text.clone()),
        (
            "negative braking",
            line_text.clone(),
            summary_text.replacen("braking = ", "braking = -", 1),
        ),
        (
            "NaN top speed",
            line_text.clone(),
            summary_text.replacen("top_speed = ", "top_speed = nan #", 1),
        ),
    ] {
        std::fs::write(&line, line_csv).unwrap();
        std::fs::write(&summary, summary_toml).unwrap();
        let result = std::panic::catch_unwind(|| racer_trackc::load_dir(&dir).map(|_| ()));
        if !matches!(result, Ok(Err(_))) {
            failures.push(what);
        }
    }
    std::fs::remove_dir_all(&dir).ok();
    assert!(failures.is_empty(), "accepted or panicked: {failures:?}");
}
