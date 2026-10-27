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
