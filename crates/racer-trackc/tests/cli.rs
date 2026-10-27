use std::process::Command;

fn trackc(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_racer-trackc"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn prints_version() {
    let out = trackc(&["--version"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).starts_with("racer-trackc "));
}

#[test]
fn rejects_bad_usage() {
    let out = trackc(&[]);
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&out.stderr).contains("usage:"));
}

#[test]
fn validates_the_shipped_tracks() {
    for id in racer_trackc::TRACKS {
        let dir = racer_trackc::tracks_dir().join(id);
        let out = trackc(&["validate", dir.to_str().unwrap()]);
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(String::from_utf8_lossy(&out.stdout).starts_with("ok "));
    }
}

#[test]
fn reports_invalid_data() {
    let dir = std::env::temp_dir().join(format!("racer-trackc-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let good = std::fs::read_to_string(racer_trackc::tracks_dir().join("downtown-loop/track.toml"))
        .unwrap();
    // A 40 m radius cannot fit between the first two nodes of the hairpin.
    let bad = good.replacen("radius = 11.0", "radius = 40.0", 1);
    std::fs::write(dir.join("track.toml"), bad).unwrap();
    let out = trackc(&["validate", dir.to_str().unwrap()]);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("corner radii"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
