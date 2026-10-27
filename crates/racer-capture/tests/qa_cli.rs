//! QA repros for racer-capture CLI argument handling (see
//! `docs/qa/tooling-data-build.md`). Fixed; see `docs/qa/fixes-tooling.md`.

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

/// QA-TOOL-15: bad scene arguments are usage errors, reported before the
/// output directory, window or renderer exist (no display needed).
#[test]
fn scene_rejects_bad_arguments_before_starting() {
    for (args, what) in [
        (&["nosuch"][..], "unknown scene"),
        (&["greybox", "--size", "0x0"], "invalid --size"),
        (&["greybox", "--size", "1280x0"], "invalid --size"),
        (&["greybox", "--size", "99999x720"], "invalid --size"),
    ] {
        let dir = out_dir("scene");
        let mut full = vec!["scene", args[0], &dir];
        full.extend(&args[1..]);
        let out = capture(&full);
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{args:?}: {err}");
        assert!(err.contains(what), "{args:?}: {err}");
        assert!(
            !std::path::Path::new(&dir).exists(),
            "{args:?}: created {dir}"
        );
    }
}

#[test]
fn diff_rejects_negative_and_oversized_tolerances() {
    let golden = concat!(env!("CARGO_MANIFEST_DIR"), "/goldens/greybox/side.png");
    for (flag, value) in [
        ("--max-fraction", "-1"),
        ("--max-fraction", "2"),
        ("--max-mean", "nan"),
        ("--max-mean", "-0.5"),
        ("--max-mean", "inf"),
    ] {
        let out = capture(&["diff", golden, golden, flag, value]);
        assert_eq!(out.status.code(), Some(2), "{flag} {value}");
    }
    let out = capture(&["diff", golden, golden, "--max-fraction", "0"]);
    assert!(out.status.success(), "identical images must pass");
}

#[test]
fn ai_races_rejects_first_beyond_the_seed_range() {
    let dir = out_dir("first-range");
    let out = capture(&["ai-races", &dir, "--races", "1", "--first", "4294967295"]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(out.status.code(), Some(2));
}
