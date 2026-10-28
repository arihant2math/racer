//! Starts the real server binary and checks that it ticks and exits cleanly.

use std::process::Command;
use std::time::{Duration, Instant};

#[test]
fn server_starts_ticks_and_exits_cleanly() {
    let start = Instant::now();
    let out = Command::new(env!("CARGO_BIN_EXE_racer-server"))
        .args(["--max-ticks", "240"])
        .output()
        .expect("failed to spawn racer-server");
    let log =
        String::from_utf8_lossy(&out.stderr).into_owned() + &String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "server failed: {:?}\n{log}",
        out.status
    );
    assert!(
        log.contains("reached 240 ticks"),
        "missing shutdown log:\n{log}"
    );
    assert!(start.elapsed() < Duration::from_secs(60));
}

#[test]
fn in_process_app_counts_fixed_ticks() {
    let mut app = racer_server::build_app(&racer_server::ServerConfig {
        max_ticks: Some(10),
        ..Default::default()
    });
    app.finish();
    app.cleanup();
    let deadline = Instant::now() + Duration::from_secs(10);
    let exit = loop {
        app.update();
        if let Some(exit) = app.should_exit() {
            break exit;
        }
        assert!(Instant::now() < deadline, "server did not exit");
        std::thread::sleep(racer_sim::tick_duration());
    };
    assert!(exit.is_success());
    assert!(app.world().resource::<racer_server::TickCount>().0 >= 10);
}

#[test]
fn zero_max_ticks_exits_before_the_first_tick() {
    let mut app = racer_server::build_app(&racer_server::ServerConfig {
        max_ticks: Some(0),
        ..Default::default()
    });
    app.finish();
    app.cleanup();
    // Long enough for a fixed step to be due if the first frame ran one.
    std::thread::sleep(racer_sim::tick_duration() * 4);
    app.update();
    assert!(app.should_exit().is_some_and(|e| e.is_success()));
    assert_eq!(app.world().resource::<racer_server::TickCount>().0, 0);
}
