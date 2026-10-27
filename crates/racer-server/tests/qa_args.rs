//! QA repros for racer-server argument handling (see
//! `docs/qa/tooling-data-build.md`). Each test fails today and is ignored
//! with its bug id so the suite stays green.

use std::{
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};

/// Runs the server, killing it after `limit`; `None` if it was still running.
fn server(args: &[&str], limit: Duration) -> Option<Output> {
    let mut child = Command::new(env!("CARGO_BIN_EXE_racer-server"))
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + limit;
    while Instant::now() < deadline {
        if child.try_wait().unwrap().is_some() {
            return Some(child.wait_with_output().unwrap());
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    child.kill().ok();
    child.wait().ok();
    None
}

#[test]
#[ignore = "QA-TOOL-6"]
fn max_ticks_zero_runs_no_ticks() {
    let out = server(&["--max-ticks", "0"], Duration::from_secs(30)).expect("server kept running");
    let log = String::from_utf8_lossy(&out.stderr);
    assert!(log.contains("reached 0 ticks"), "{log}");
}

#[test]
#[ignore = "QA-TOOL-7"]
fn malformed_max_ticks_is_a_clean_usage_error() {
    for args in [
        &["--max-ticks"][..],
        &["--max-ticks=10"],
        &["--max-tick", "10"],
        &["--max-ticks", "abc"],
    ] {
        let out = server(args, Duration::from_secs(10))
            .unwrap_or_else(|| panic!("{args:?}: still running after 10 s"));
        assert!(!out.status.success(), "{args:?}: exited successfully");
        assert_ne!(out.status.code(), Some(101), "{args:?}: panicked");
    }
}
