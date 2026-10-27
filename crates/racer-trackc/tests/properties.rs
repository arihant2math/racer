//! Property-based tests (QA, adversarial edge cases) for the track compiler:
//! mutated `track.toml` data must be rejected with errors or build sound
//! geometry, never panic or hang. See `docs/qa/adversarial-edge-cases.md`.
//!
//! Small case counts by default; `QA_EDGE_CASES=<n>` for a deeper run.
//! Failing properties are `#[ignore = "QA-EDGE-N"]` regressions with the
//! shrunk counterexample hard-coded.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    path::PathBuf,
    process::{Command, Stdio},
    sync::LazyLock,
    time::{Duration, Instant},
};

use glam::Vec3;
use proptest::prelude::*;
use racer_trackc::{
    Geometry, Limits, TrackFile, geometry,
    line::{self, CAR_HALF_WIDTH},
    profile::{self, CarModel},
    tracks_dir,
};

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

static SHIPPED: LazyLock<String> = LazyLock::new(|| {
    std::fs::read_to_string(tracks_dir().join("downtown-loop/track.toml")).unwrap()
});

fn shipped() -> TrackFile {
    TrackFile::parse(&SHIPPED).unwrap()
}

/// One edit of the shipped track. Values stay in ranges that cannot hang the
/// validator (see QA-EDGE-6 for the ones that do).
#[derive(Clone, Debug)]
enum Edit {
    MoveNode(usize, f32, f32),
    Radius(usize, f32),
    Width(usize, f32),
    Elevation(usize, f32),
    Bank(usize, f32),
    Apron(usize, f32, f32),
    Curbs(usize, bool),
    CurbWidth(f32),
    WallHeight(f32),
    GridSlots(usize),
    RowSpacing(f32),
    ColumnOffset(f32),
    CheckpointSpacing(f32),
    MoveSector(usize, f32, f32),
    MoveStart(f32),
    RemoveNode(usize),
}

fn finite_or_odd(range: std::ops::Range<f32>) -> impl Strategy<Value = f32> {
    prop_oneof![
        8 => range,
        1 => Just(0.0f32),
        1 => Just(-1.0f32),
        1 => Just(f32::NAN),
        1 => Just(f32::INFINITY),
    ]
}

fn edit() -> impl Strategy<Value = Edit> {
    let node = 0usize..14;
    prop_oneof![
        (node.clone(), -15.0..15.0f32, -15.0..15.0f32)
            .prop_map(|(i, x, z)| Edit::MoveNode(i, x, z)),
        (node.clone(), 5.0..60.0f32).prop_map(|(i, r)| Edit::Radius(i, r)),
        (node.clone(), 5.0..26.0f32).prop_map(|(i, w)| Edit::Width(i, w)),
        // Below 0 is QA-EDGE-13 (own regression test).
        (node.clone(), 0.0..10.0f32).prop_map(|(i, e)| Edit::Elevation(i, e)),
        (node.clone(), -12.0..12.0f32).prop_map(|(i, b)| Edit::Bank(i, b)),
        (node.clone(), 0.0..8.0f32, 0.0..8.0f32).prop_map(|(i, l, r)| Edit::Apron(i, l, r)),
        (node.clone(), any::<bool>()).prop_map(|(i, c)| Edit::Curbs(i, c)),
        finite_or_odd(0.1..2.0).prop_map(Edit::CurbWidth),
        finite_or_odd(0.3..3.5).prop_map(Edit::WallHeight),
        (0usize..20).prop_map(Edit::GridSlots),
        finite_or_odd(-10.0..15.0).prop_map(Edit::RowSpacing),
        finite_or_odd(-4.0..6.0).prop_map(Edit::ColumnOffset),
        prop_oneof![
            10.0..400.0f32,
            Just(f32::NAN),
            Just(f32::INFINITY),
            Just(1.0e9f32)
        ]
        .prop_map(Edit::CheckpointSpacing),
        (0usize..2, -60.0..60.0f32, -60.0..60.0f32).prop_map(|(i, x, z)| Edit::MoveSector(i, x, z)),
        (-100.0..100.0f32).prop_map(Edit::MoveStart),
        node.prop_map(Edit::RemoveNode),
    ]
}

fn apply(file: &mut TrackFile, edit: &Edit) {
    let n = file.nodes.len();
    match *edit {
        Edit::MoveNode(i, x, z) => {
            let a = &mut file.nodes[i % n].at;
            a[0] += x;
            a[1] += z;
        }
        Edit::Radius(i, r) => file.nodes[i % n].radius = Some(r),
        Edit::Width(i, w) => file.nodes[i % n].width = Some(w),
        Edit::Elevation(i, e) => file.nodes[i % n].elevation = Some(e),
        Edit::Bank(i, b) => file.nodes[i % n].bank_deg = Some(b),
        Edit::Apron(i, l, r) => file.nodes[i % n].apron = Some([l, r]),
        Edit::Curbs(i, c) => file.nodes[i % n].curbs = c,
        Edit::CurbWidth(w) => file.defaults.curb_width = w,
        Edit::WallHeight(h) => file.defaults.wall_height = h,
        Edit::GridSlots(s) => file.race.grid_slots = s,
        Edit::RowSpacing(s) => file.race.grid_row_spacing = s,
        Edit::ColumnOffset(o) => file.race.grid_column_offset = o,
        Edit::CheckpointSpacing(s) => file.race.checkpoint_spacing = s,
        Edit::MoveSector(i, x, z) => {
            if let Some(p) = file.race.sectors.get_mut(i) {
                p[0] += x;
                p[1] += z;
            }
        }
        Edit::MoveStart(z) => file.race.start[1] += z,
        Edit::RemoveNode(i) => {
            file.nodes.remove(i % n);
        }
    }
}

/// Every soundness check a validated track must pass; returns the problems.
fn soundness(file: &TrackFile, g: &Geometry) -> Vec<String> {
    let mut out = Vec::new();
    let meshes = [
        ("road", &g.road),
        ("aprons", &g.aprons),
        ("walls", &g.walls),
        ("markings", &g.markings),
        ("curbs red", &g.curbs[0]),
        ("curbs white", &g.curbs[1]),
    ];
    for (name, mesh) in meshes {
        if let Some(v) = mesh.vertices.iter().find(|v| !v.is_finite()) {
            out.push(format!("{name}: non-finite vertex {v}"));
            continue;
        }
        for tri in &mesh.indices {
            let [a, b, c] = tri.map(|i| mesh.vertices[i as usize]);
            let area = (b - a).cross(c - a).length() / 2.0;
            if area < 1e-4 {
                out.push(format!("{name}: degenerate triangle {a} {b} {c}"));
                break;
            }
        }
    }
    // Road and aprons face up (counter-clockwise seen from above) and are no
    // steeper than banking plus grade allow (the road; the aprons only
    // checked for flipping here, their slope is QA-EDGE-12).
    for (name, mesh, min_y) in [("road", &g.road, 0.95), ("aprons", &g.aprons, 0.0)] {
        if let Some(tri) = mesh.indices.iter().find(|t| {
            let [a, b, c] = t.map(|i| mesh.vertices[i as usize]);
            (b - a).cross(c - a).normalize_or_zero().y < min_y
        }) {
            out.push(format!("{name}: triangle {tri:?} does not face up"));
        }
    }
    if !g.length.is_finite() || g.length <= 0.0 {
        out.push(format!("length {}", g.length));
    }
    if !g.checkpoints.windows(2).all(|w| w[0] < w[1])
        || g.checkpoints.iter().any(|&c| !(c > 0.0 && c < g.length))
    {
        out.push(format!(
            "checkpoints not strictly inside the lap: {:?}",
            g.checkpoints
        ));
    }
    if !g.sector_ends.windows(2).all(|w| w[0] < w[1]) {
        out.push(format!("sector ends out of order: {:?}", g.sector_ends));
    }
    if !(g.sprint_finish > 0.0 && g.sprint_finish < g.length) {
        out.push(format!("sprint finish {}", g.sprint_finish));
    }
    // Grid: on the road, behind the line, in order, cars clear of each other.
    let behind = |p: Vec3| {
        let (mut best, mut s) = (f32::MAX, 0.0);
        for x in &g.samples {
            let d = x.pos.distance(p);
            if d < best {
                (best, s) = (d, x.s);
            }
        }
        s
    };
    for (i, slot) in g.grid.iter().enumerate() {
        if !slot.position.is_finite() || !slot.rotation.is_finite() {
            out.push(format!("grid slot {i}: non-finite pose"));
            continue;
        }
        let s = behind(slot.position);
        if !(s > g.length - 250.0 && s < g.length - 1.0) {
            out.push(format!("grid slot {i} at s = {s} is not behind the line"));
        }
        for (j, other) in g.grid.iter().enumerate().skip(i + 1) {
            let d = slot.rotation.inverse() * (other.position - slot.position);
            if d.x.abs() < 2.0 && d.z.abs() < 4.8 {
                out.push(format!("grid slots {i} and {j} overlap ({d})"));
            }
        }
    }
    if g.grid.len() != file.race.grid_slots {
        out.push("grid slot count".into());
    }
    out
}

/// Data hitting QA-EDGE-5/6/7, which have their own regression tests; the
/// properties skip it so they can look for something new.
fn known_bad(file: &TrackFile) -> bool {
    let r = &file.race;
    !file.defaults.curb_width.is_finite()
        || !r.grid_row_spacing.is_finite()
        || !r.grid_column_offset.is_finite()
        || r.grid_row_spacing < 2.4
        || (r.grid_row_spacing < 4.8 && r.grid_column_offset.abs() < 1.0)
        || r.grid_slots > 16
        || r.checkpoint_spacing < 1.0
        || r.checkpoint_spacing.is_nan()
}

/// Validates with a panic guard; `Ok(None)` when the data is rejected.
fn validate_and_build(file: &TrackFile) -> Result<Option<Geometry>, String> {
    catch_unwind(AssertUnwindSafe(|| {
        let errors = geometry::validate(file);
        errors.is_empty().then(|| geometry::build(file))
    }))
    .map_err(|e| {
        e.downcast_ref::<String>()
            .cloned()
            .or_else(|| e.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    })
}

proptest! {
    #![proptest_config(config(24))]

    /// Mutated tracks are either rejected or build sound geometry, and
    /// building is deterministic.
    #[test]
    fn mutated_tracks_are_rejected_or_sound(edits in prop::collection::vec(edit(), 1..4)) {
        let mut file = shipped();
        for e in &edits {
            apply(&mut file, e);
        }
        prop_assume!(!known_bad(&file));
        match validate_and_build(&file) {
            Err(panic) => prop_assert!(false, "validate panicked: {panic}"),
            Ok(None) => {}
            Ok(Some(g)) => {
                let problems = soundness(&file, &g);
                prop_assert!(problems.is_empty(), "accepted but unsound: {problems:#?}");
                let again = geometry::build(&file);
                let bits = |g: &Geometry| g.road.vertices.iter().chain(&g.walls.vertices)
                    .flat_map(|v| v.to_array().map(f32::to_bits)).collect::<Vec<_>>();
                prop_assert_eq!(bits(&g), bits(&again));
                prop_assert_eq!(&g.checkpoints, &again.checkpoints);
            }
        }
    }

    /// Arbitrary text mutations of `track.toml` never panic the parser or
    /// the validator.
    #[test]
    fn mutated_text_never_panics(
        cuts in prop::collection::vec((0.0..1.0f64, 0usize..8, "[-0-9.\\[\\],=a-z \n]{0,4}"), 1..4)
    ) {
        let mut text = SHIPPED.clone();
        for (at, len, insert) in cuts {
            let mut i = (at * text.len() as f64) as usize;
            while !text.is_char_boundary(i) {
                i -= 1;
            }
            let mut j = (i + len).min(text.len());
            while !text.is_char_boundary(j) {
                j -= 1;
            }
            text.replace_range(i..j, &insert);
        }
        let Ok(file) = TrackFile::parse(&text) else { return Ok(()) };
        prop_assume!(!known_bad(&file));
        prop_assume!(file.nodes.iter().all(|n| n.at.iter().all(|c| c.abs() < 1.0e4)));
        if let Err(panic) = validate_and_build(&file) {
            prop_assert!(false, "validate panicked: {panic}");
        }
    }
}

proptest! {
    #![proptest_config(config(4))]

    /// The racing line on a mutated (valid) track stays inside its bounds,
    /// is finite and never bends more than the centreline's tightest corner.
    #[test]
    fn racing_line_converges_inside_the_road(edits in prop::collection::vec(edit(), 1..3)) {
        let mut file = shipped();
        for e in &edits {
            apply(&mut file, e);
        }
        prop_assume!(!known_bad(&file));
        let Ok(Some(g)) = validate_and_build(&file) else { return Ok(()) };
        let l = line::minimum_curvature(&g, CAR_HALF_WIDTH);
        prop_assert!(l.length.is_finite() && l.length > 0.0 && l.length <= g.length * 1.01);
        for (p, s) in l.points.iter().zip(&g.samples) {
            let (lo, hi) = line::bounds(s, CAR_HALF_WIDTH);
            prop_assert!(p.offset.is_finite() && p.offset >= lo - 1e-4 && p.offset <= hi + 1e-4,
                "offset {} outside {lo}..{hi} at s = {}", p.offset, s.s);
            prop_assert!(p.pos.is_finite() && p.curvature.is_finite());
        }
        let centre = g.samples.iter().map(|s| s.curvature.abs()).fold(0.0, f32::max);
        let worst = l.points.iter().map(|p| p.curvature.abs()).fold(0.0, f32::max);
        prop_assert!(worst <= centre * 1.05, "line curvature {worst} vs centreline {centre}");
    }
}

fn model(lateral: f32, braking: f32, top: f32, drive: Vec<f32>) -> CarModel {
    CarModel {
        id: "qa".into(),
        mass: 1500.0,
        lateral,
        braking,
        traction: 10.0,
        drag: 0.0004,
        lift: 0.0001,
        top_speed: top,
        drive,
    }
}

static DOWNTOWN: LazyLock<racer_trackc::Track> =
    LazyLock::new(|| racer_trackc::load("downtown-loop").unwrap());

proptest! {
    #![proptest_config(config(64))]

    /// Speed profiles for arbitrary point-mass models: finite, positive,
    /// under the top speed, inside the grip at every corner, and no
    /// deceleration between points beyond what the model can brake.
    #[test]
    fn speed_profile_respects_the_model(
        lateral in 4.0..15.0f32, braking in 4.0..15.0f32, top in 20.0..100.0f32,
        drive in prop::collection::vec(0.0..12.0f32, 2..130),
        limit in 0.5..=1.0f32,
    ) {
        let t = &*DOWNTOWN;
        let m = model(lateral, braking, top, drive);
        let limits = Limits { lateral: limit, braking: limit, drive: limit };
        let v = profile::speed_profile(&t.line, &m, limits);
        let pts = &t.line.points;
        let n = pts.len();
        for k in 0..n {
            prop_assert!(v[k].is_finite() && v[k] > 0.0 && v[k] <= top + 1e-3, "v[{k}] = {}", v[k]);
            let lat = v[k] * v[k] * pts[k].curvature.abs();
            prop_assert!(lat <= m.lateral_at(v[k], limits) * 1.001 + 1e-3, "lateral {lat} at {k}");
            let j = (k + 1) % n;
            let d = (pts[j].pos - pts[k].pos).length().max(1e-3);
            let grade = (pts[j].pos.y - pts[k].pos.y) / d;
            let decel = (v[k] * v[k] - v[j] * v[j]) / (2.0 * d);
            let most = limit * braking + m.drag * v[j] * v[j] + 9.81 * grade;
            prop_assert!(decel <= most.max(0.0) * 1.001 + 1e-3, "decel {decel} > {most} at {k}");
        }
        prop_assert!(profile::lap_time(&t.line, &v).is_finite());
    }
}

/// Shipped track with one textual replacement, written to a temp dir.
fn temp_track(tag: &str, from: &str, to: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("racer-qa-edge-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    assert!(SHIPPED.contains(from), "{from}");
    std::fs::write(dir.join("track.toml"), SHIPPED.replacen(from, to, 1)).unwrap();
    dir
}

/// Runs `racer-trackc validate` on `dir` with a memory cap and a timeout;
/// returns (exit code, seconds, stderr) or `None` on timeout (killed).
fn validate_cli(dir: &std::path::Path, timeout: Duration) -> Option<(Option<i32>, String)> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "ulimit -v 4000000; exec '{}' validate '{}'",
            env!("CARGO_BIN_EXE_racer-trackc"),
            dir.display()
        ))
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let mut err = String::new();
            use std::io::Read;
            child.stderr.take().unwrap().read_to_string(&mut err).ok();
            return Some((status.code(), err));
        }
        if start.elapsed() > timeout {
            child.kill().ok();
            child.wait().ok();
            return None;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// QA-EDGE-5: non-finite or negative race/default values pass validation and
/// build NaN geometry or broken grids. Shrunk counterexamples (one field of
/// the shipped track each).
#[test]
#[ignore = "QA-EDGE-5"]
fn qa_edge_5_validate_accepts_non_finite_and_negative_layout_values() {
    type Edit = fn(&mut TrackFile);
    let cases: [(&str, Edit); 5] = [
        ("defaults.curb_width = nan", |f| {
            f.defaults.curb_width = f32::NAN
        }),
        ("race.grid_row_spacing = nan", |f| {
            f.race.grid_row_spacing = f32::NAN
        }),
        ("race.grid_row_spacing = -1", |f| {
            f.race.grid_row_spacing = -1.0
        }),
        ("race.grid_row_spacing = 0", |f| {
            f.race.grid_row_spacing = 0.0
        }),
        ("race.grid_column_offset = nan", |f| {
            f.race.grid_column_offset = f32::NAN
        }),
    ];
    let mut failures = Vec::new();
    for (what, edit) in cases {
        let mut file = shipped();
        edit(&mut file);
        match validate_and_build(&file) {
            Ok(Some(g)) => {
                let p = soundness(&file, &g);
                if !p.is_empty() {
                    failures.push(format!("{what}: accepted, {}", p[0]));
                }
            }
            Ok(None) => {}
            Err(e) => failures.push(format!("{what}: panicked {e}")),
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// QA-EDGE-6: `validate` builds the geometry before it checks
/// `checkpoint_spacing`, and the checkpoint loop `s += spacing` never
/// terminates for 0 (or any spacing below half an f32 ulp of the lap length,
/// e.g. 1e-4 m on a 2 km lap): `racer-trackc validate` spins forever while
/// pushing checkpoints until it runs out of memory.
#[test]
#[ignore = "QA-EDGE-6"]
fn qa_edge_6_tiny_checkpoint_spacing_hangs_validate() {
    for spacing in ["0.0", "0.0001"] {
        let dir = temp_track(
            "spacing",
            "checkpoint_spacing = 90.0",
            &format!("checkpoint_spacing = {spacing}"),
        );
        let result = validate_cli(&dir, Duration::from_secs(5));
        std::fs::remove_dir_all(&dir).ok();
        match result {
            None => panic!("checkpoint_spacing = {spacing}: validate still running after 5 s"),
            Some((code, err)) => assert!(
                code == Some(1) && err.contains("checkpoint_spacing"),
                "checkpoint_spacing = {spacing}: exit {code:?}: {err}"
            ),
        }
    }
}

/// The shipped track with every plan coordinate scaled by `k`.
fn scaled_track(k: f32) -> String {
    let mut file = shipped();
    let mut out = SHIPPED.clone();
    for n in &mut file.nodes {
        let old = format!("at = [{:?}, {:?}]", n.at[0], n.at[1]);
        let new = format!("at = [{:?}, {:?}]", n.at[0] * k, n.at[1] * k);
        out = out.replacen(&old, &new, 1);
    }
    let r = &file.race;
    for p in r.sectors.iter().chain([&r.start, &r.sprint_finish]) {
        let old = format!("[{:?}, {:?}]", p[0], p[1]);
        out = out.replacen(&old, &format!("[{:?}, {:?}]", p[0] * k, p[1] * k), 1);
    }
    out
}

/// QA-EDGE-7: `validate` builds the geometry before it checks `grid_slots`,
/// and nothing bounds the track's size, so a huge grid panics with
/// "capacity overflow" and a huge (or just long) track runs for minutes to
/// hours (1 m samples, an O(n^2) overlap check, a 0.5 m brute-force
/// projection) or exhausts memory, instead of returning an error.
#[test]
#[ignore = "QA-EDGE-7"]
fn qa_edge_7_huge_values_crash_or_hang_validate() {
    let mut failures = Vec::new();
    let big = scaled_track(1.0e6);
    assert!(big.contains("at = [0.0, 60000000.0]"), "scaling failed");
    for (what, text) in [
        (
            "grid_slots = 9223372036854775807",
            SHIPPED.replacen("grid_slots = 8", "grid_slots = 9223372036854775807", 1),
        ),
        ("plan coordinates x 1e6", big),
    ] {
        let dir = std::env::temp_dir().join(format!("racer-qa-edge-7-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("track.toml"), text).unwrap();
        let result = validate_cli(&dir, Duration::from_secs(30));
        std::fs::remove_dir_all(&dir).ok();
        match result {
            None => failures.push(format!("{what}: still running after 30 s")),
            Some((code, err)) if code != Some(1) || err.contains("panicked") => {
                let first: Vec<&str> = err.lines().take(2).collect();
                failures.push(format!("{what}: exit {code:?}: {}", first.join(" ")))
            }
            Some(_) => {}
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

/// QA-EDGE-8: `load_dir` panics on a `summary.toml` car model with fewer
/// than two `drive` entries (`drive_at` indexes `len - 2`) instead of
/// returning an error.
#[test]
#[ignore = "QA-EDGE-8"]
fn qa_edge_8_short_drive_table_panics_load() {
    let dir = std::env::temp_dir().join(format!("racer-qa-edge-8-{}", std::process::id()));
    let src = tracks_dir().join("downtown-loop");
    std::fs::create_dir_all(dir.join("generated")).unwrap();
    std::fs::copy(src.join("track.toml"), dir.join("track.toml")).unwrap();
    std::fs::copy(
        src.join("generated/racing-line.csv"),
        dir.join("generated/racing-line.csv"),
    )
    .unwrap();
    let summary = std::fs::read_to_string(src.join("generated/summary.toml")).unwrap();
    let start = summary.find("drive = [").unwrap();
    let end = start + summary[start..].find(']').unwrap() + 1;
    let mut short = summary.clone();
    short.replace_range(start..end, "drive = [3.0]");
    std::fs::write(dir.join("generated/summary.toml"), short).unwrap();
    let result = catch_unwind(|| racer_trackc::load_dir(&dir).map(|_| ()));
    std::fs::remove_dir_all(&dir).ok();
    assert!(
        matches!(result, Ok(Err(_))),
        "expected an error, got {:?}",
        result.map_err(|_| "panic")
    );
}

/// Steepest apron triangle (degrees from horizontal).
fn steepest_apron(g: &Geometry) -> f32 {
    let m = &g.aprons;
    m.indices
        .iter()
        .map(|t| {
            let [a, b, c] = t.map(|i| m.vertices[i as usize]);
            (b - a)
                .cross(c - a)
                .normalize_or_zero()
                .y
                .acos()
                .to_degrees()
        })
        .fold(0.0, f32::max)
}

/// QA-EDGE-12: banking that changes across a tight corner (allowed: each
/// node within +-10 degrees, blended between nodes) tilts the inner apron of
/// the hairpin up to ~31 degrees, a ramp 1 m high a car can be launched off.
/// Shrunk case: node 6 (hairpin entry) banked -7.64 degrees instead of +3.
#[test]
#[ignore = "QA-EDGE-12"]
fn qa_edge_12_banking_change_in_the_hairpin_makes_a_steep_apron() {
    assert!(steepest_apron(&geometry::build(&shipped())) < 12.0);
    let mut file = shipped();
    file.nodes[6].bank_deg = Some(-7.6445813);
    assert_eq!(geometry::validate(&file), Vec::<String>::new());
    let slope = steepest_apron(&geometry::build(&file));
    assert!(
        slope < 15.0,
        "accepted, but an apron triangle slopes {slope:.1} degrees"
    );
}

/// QA-EDGE-13: nothing bounds elevation from below, so a node at -2.4 m
/// (the shrunk case: node 4) passes validation with the road under the city
/// ground (the grass box spans `GROUND_Y - 1 ..= GROUND_Y`, -1.05..-0.05 m,
/// and `city_track` makes it a collider): the road is buried, and the walls'
/// outer face, which runs from the wall top down to the fixed ground level,
/// folds into degenerate triangles.
#[test]
#[ignore = "QA-EDGE-13"]
fn qa_edge_13_road_below_the_ground_plane_is_accepted() {
    let mut file = shipped();
    file.nodes[4].elevation = Some(-2.3868399);
    let errors = geometry::validate(&file);
    let lowest = geometry::build(&file)
        .samples
        .iter()
        .map(|s| s.pos.y)
        .fold(f32::MAX, f32::min);
    assert!(
        !errors.is_empty(),
        "accepted with the road surface down to {lowest:.2} m, under the ground at {} m",
        geometry::GROUND_Y
    );
}

/// A sanity check that the soundness checks above pass on the shipped track
/// (so a failure in the mutated properties is about the mutation).
#[test]
fn shipped_track_is_sound() {
    let file = shipped();
    let g = validate_and_build(&file).unwrap().unwrap();
    assert_eq!(soundness(&file, &g), Vec::<String>::new());
}
