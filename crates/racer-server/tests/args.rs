//! QA (adversarial edge cases): malformed command lines. See
//! `docs/qa/adversarial-edge-cases.md`.

use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Runs the server with `args`, killing it after `timeout`; returns the exit
/// code (`None` when killed or signalled) and stderr.
fn server(args: &[&str], timeout: Duration) -> (Option<i32>, bool, String) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_racer-server"))
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let timed_out = loop {
        if child.try_wait().unwrap().is_some() {
            break false;
        }
        if start.elapsed() > timeout {
            child.kill().ok();
            break true;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let out = child.wait_with_output().unwrap();
    (
        out.status.code(),
        timed_out,
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// QA-EDGE-14: a malformed `--max-ticks` panics (exit 101 with a panic
/// message and backtrace hint) instead of printing usage and exiting 2; a
/// missing value is silently ignored and the server runs forever.
#[test]
#[ignore = "QA-EDGE-14"]
fn qa_edge_14_bad_max_ticks_is_a_usage_error() {
    let mut failures = Vec::new();
    for args in [
        &["--max-ticks", "abc"][..],
        &["--max-ticks", "-1"],
        &["--max-ticks", "1e3"],
        &["--max-ticks"],
    ] {
        let (code, timed_out, err) = server(args, Duration::from_secs(10));
        if timed_out || code != Some(2) || err.contains("panicked") {
            let first = err.lines().find(|l| !l.trim().is_empty()).unwrap_or("");
            failures.push(format!(
                "{args:?}: exit {code:?}, still running after 10 s: {timed_out}, stderr: {first}"
            ));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
