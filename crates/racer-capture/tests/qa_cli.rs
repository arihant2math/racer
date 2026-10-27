//! QA repros for racer-capture CLI argument handling (see
//! `docs/qa/tooling-data-build.md`). Each test fails today and is ignored
//! with its bug id so the suite stays green.

use std::process::{Command, Output};

fn capture(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_racer-capture"))
        .args(args)
        .output()
        .unwrap()
}

fn out_dir(name: &str) -> String {
    let dir = std::env::temp_dir().join(format!("racer-capture-qa-{name}-{}", std::process::id()));
    dir.to_str().unwrap().to_string()
}

#[test]
#[ignore = "QA-TOOL-8"]
fn ai_laps_rejects_an_unknown_car() {
    let dir = out_dir("laps");
    let out = capture(&["ai-laps", &dir, "--car", "no-such-car", "--laps", "1"]);
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        !out.status.success(),
        "ai-laps --car no-such-car passed without running a lap"
    );
}

#[test]
#[ignore = "QA-TOOL-8"]
fn ai_races_rejects_zero_races() {
    let dir = out_dir("races");
    let out = capture(&["ai-races", &dir, "--races", "0"]);
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        !out.status.success(),
        "ai-races --races 0 passed without running a race"
    );
}

#[test]
#[ignore = "QA-TOOL-9"]
fn ai_races_rejects_overflowing_first_without_panicking() {
    let dir = out_dir("first");
    let out = capture(&[
        "ai-races",
        &dir,
        "--races",
        "2",
        "--first",
        &usize::MAX.to_string(),
    ]);
    std::fs::remove_dir_all(&dir).ok();
    assert_ne!(
        out.status.code(),
        Some(101),
        "panicked: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
#[ignore = "QA-TOOL-10"]
fn diff_rejects_nan_tolerances() {
    let golden = concat!(env!("CARGO_MANIFEST_DIR"), "/goldens/greybox/side.png");
    let out = capture(&["diff", golden, golden, "--max-fraction", "nan"]);
    assert_eq!(
        out.status.code(),
        Some(2),
        "--max-fraction nan was not rejected as a usage error: {}",
        String::from_utf8_lossy(&out.stdout)
    );
}
