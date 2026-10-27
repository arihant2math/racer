//! QA repro for `RACER_BACKEND_ADDR` handling (see
//! `docs/qa/tooling-data-build.md`). Fixed; see `docs/qa/fixes-tooling.md`.

use std::{
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn hostname_address_does_not_panic() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_racer-backend"))
        .env("RACER_BACKEND_ADDR", "localhost:0")
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            assert_ne!(
                status.code(),
                Some(101),
                "racer-backend panicked on RACER_BACKEND_ADDR=localhost:0"
            );
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    // Still running: it accepted the address and is listening.
    child.kill().ok();
    child.wait().ok();
}

#[test]
fn bad_address_is_a_clean_error() {
    let out = Command::new(env!("CARGO_BIN_EXE_racer-backend"))
        .env("RACER_BACKEND_ADDR", "not an address")
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(1), "{err}");
    assert!(
        err.contains("not an address") && !err.contains("panicked"),
        "{err}"
    );
}
