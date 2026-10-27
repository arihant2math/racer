//! QA (adversarial edge cases): the results table's time formatting. See
//! `docs/qa/adversarial-edge-cases.md`. `QA_EDGE_CASES=<n>` for a deeper run.

use proptest::prelude::*;
use racer_game::flow::lap_time;

fn config(default_cases: u32) -> ProptestConfig {
    let cases = std::env::var("QA_EDGE_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default_cases);
    ProptestConfig {
        cases,
        failure_persistence: None,
        ..ProptestConfig::default()
    }
}

/// Parses "m:ss.000" back to seconds, checking the seconds field is < 60.
fn parse(s: &str) -> Result<f32, String> {
    let (m, sec) = s.split_once(':').ok_or("no colon")?;
    let m: f32 = m.parse().map_err(|_| "minutes")?;
    if sec.len() != 6 {
        return Err(format!("seconds field {sec:?}"));
    }
    let sec: f32 = sec.parse().map_err(|_| "seconds")?;
    if sec >= 60.0 {
        return Err(format!("seconds field {sec}"));
    }
    Ok(m * 60.0 + sec)
}

/// Times a hair under a whole minute: QA-EDGE-15, own regression test.
fn rounds_up_to_a_minute(t: f32) -> bool {
    t % 60.0 >= 59.9995
}

proptest! {
    #![proptest_config(config(2000))]

    /// Every time in a race's range formats as m:ss.000 with seconds below
    /// 60 and reads back within half a millisecond (plus f32 rounding).
    #[test]
    fn lap_time_round_trips(t in 0.0..7200.0f32) {
        prop_assume!(!rounds_up_to_a_minute(t));
        let s = lap_time(t);
        let back = parse(&s);
        prop_assert!(back.is_ok(), "{t} -> {s:?}: {back:?}");
        prop_assert!((back.unwrap() - t).abs() <= 0.0005 + t * 1e-6, "{t} -> {s:?}");
    }
}

/// QA-EDGE-15: `lap_time` formats the minutes and the seconds separately and
/// rounds only the seconds, so a time a hair under a full minute shows as
/// "0:60.000" (and 119.9996 s as "1:60.000") instead of "1:00.000" and
/// "2:00.000".
#[test]
#[ignore = "QA-EDGE-15"]
fn qa_edge_15_time_just_under_a_minute_shows_60_seconds() {
    for t in [59.9996f32, 119.9997] {
        let s = lap_time(t);
        assert!(parse(&s).is_ok(), "{t} s formats as {s:?}");
    }
}
