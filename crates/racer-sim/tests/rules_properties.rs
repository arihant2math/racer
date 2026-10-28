//! Property-based tests (QA, adversarial edge cases) for the race rules:
//! arbitrary drives along the track (forwards, backwards, off the corridor
//! and back, resets) fed to `Progress`, `Timing` and `ResetMonitor`, and
//! arbitrary fields for `standings`. See `docs/qa/adversarial-edge-cases.md`.
//!
//! Small case counts by default; `QA_EDGE_CASES=<n>` for a deeper run.
//! The `qa_edge_*` tests are regressions with the shrunk counterexamples
//! the properties found hard-coded (fixed, see `docs/qa/fixes-race-rules.md`).

use std::sync::LazyLock;

use bevy::math::Vec3;
use proptest::prelude::*;
use racer_sim::rules::{
    Progress, ProgressEvent, ResetMonitor, Status, Timing, reset_pose, standings,
};
use racer_trackc::{Geometry, TrackFile, geometry, geometry::interpolate, tracks_dir};

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

/// The shipped track, and the same track with the tightest checkpoint
/// spacing `validate` allows (20 m), which puts a checkpoint 6.7 m from a
/// sector end.
static TRACKS: LazyLock<[Geometry; 2]> = LazyLock::new(|| {
    let text = std::fs::read_to_string(tracks_dir().join("downtown-loop/track.toml")).unwrap();
    let shipped = TrackFile::parse(&text).unwrap();
    let mut dense = shipped.clone();
    dense.race.checkpoint_spacing = 20.0;
    [shipped, dense].map(|f| {
        assert_eq!(geometry::validate(&f), Vec::<String>::new());
        geometry::build(&f)
    })
});

const DT: f32 = 1.0 / 240.0;

/// One leg of a drive.
#[derive(Clone, Copy, Debug)]
enum Leg {
    /// Drive `distance` metres along the road (negative: backwards) at
    /// `lateral` (fraction of the half width) and `speed` m/s.
    Drive {
        distance: f32,
        lateral: f32,
        speed: f32,
    },
    /// Leave the corridor sideways (over the wall), carry on for `distance`
    /// metres outside it, and come back onto the road.
    Excursion { distance: f32 },
    /// A reset from the last valid point (as `apply_resets` does).
    Reset,
}

fn leg() -> impl Strategy<Value = Leg> {
    prop_oneof![
        12 => (-40.0..250.0f32, -0.9..0.9f32, 5.0..90.0f32)
            .prop_map(|(distance, lateral, speed)| Leg::Drive { distance, lateral, speed }),
        // Up to 60 m: past ~26 m the car is off the track until it is
        // reset (QA-EDGE-2), and on the dense track it can pass two gates
        // (QA-EDGE-1).
        1 => (5.0..60.0f32).prop_map(|distance| Leg::Excursion { distance }),
        1 => Just(Leg::Reset),
    ]
}

/// What the gates credited at unwrapped `distance` should be: every gate at
/// or before it, counted over laps (the line at 0 does not count).
fn expected_credit(g: &Geometry, distance: f32) -> (u32, usize) {
    let gates = g.checkpoints.len() + 1;
    let mut count = 0usize;
    let mut lap = 0u32;
    'outer: loop {
        for k in 0..gates {
            let at = lap as f32 * g.length + g.checkpoints.get(k).copied().unwrap_or(g.length);
            if at > distance {
                break 'outer;
            }
            count += 1;
        }
        lap += 1;
    }
    ((count / gates) as u32, count % gates)
}

/// Drives `p` (and its timing) through `legs`; returns every problem found.
struct Run<'a> {
    g: &'a Geometry,
    p: Progress,
    monitor: ResetMonitor,
    timing: Timing,
    now: f32,
    /// Where the car really is (unwrapped distance) and its lateral offset (m).
    s: f32,
    lateral: f32,
    /// Lap start times still standing, and the sectors of each standing lap.
    starts: Vec<f32>,
    lap_sectors: Vec<Vec<f32>>,
    laps_counted: i64,
    problems: Vec<String>,
}

impl<'a> Run<'a> {
    fn new(g: &'a Geometry, slot: usize) -> Self {
        let p = Progress::new(g, g.grid[slot].position + Vec3::Y * 0.5);
        Run {
            g,
            monitor: ResetMonitor::new(&p),
            s: p.distance,
            lateral: p.lateral,
            p,
            timing: Timing::start(0.0),
            now: 0.0,
            starts: vec![0.0],
            lap_sectors: Vec::new(),
            laps_counted: 0,
            problems: Vec::new(),
        }
    }

    fn event(&mut self, e: ProgressEvent) {
        self.timing.on_event(e, self.now, self.g);
        match e {
            ProgressEvent::Lap => {
                self.laps_counted += 1;
                self.starts.push(self.now);
                self.lap_sectors.push(self.timing.last_sectors.clone());
            }
            ProgressEvent::LapUndone => {
                self.laps_counted -= 1;
                self.starts.pop();
                self.lap_sectors.pop();
            }
            _ => {}
        }
    }

    fn update(&mut self, position: Vec3, velocity: Vec3) {
        self.now += DT;
        let laps = self.p.laps;
        let forward = velocity.normalize_or(Vec3::NEG_Z);
        for e in self.p.update(self.g, position, velocity, forward, DT) {
            self.event(e);
        }
        let up = 1.0;
        if self
            .monitor
            .update(&self.p, up, velocity.length(), true, DT)
            .is_some()
        {
            self.reset();
        }
        if self.p.laps.abs_diff(laps) > 1 {
            self.problem(format!("laps jumped {laps} -> {}", self.p.laps));
        }
    }

    fn reset(&mut self) {
        let (pose, d, x) = reset_pose(self.g, self.monitor.last_valid, &[]);
        self.monitor.clear();
        for e in self.p.move_to(self.g, d, x) {
            self.event(e);
        }
        let p2 = Progress::new(self.g, pose.position);
        if !p2.on_track {
            self.problem(format!("reset pose off the track at d = {d}"));
        }
        self.s = d;
        self.lateral = x;
    }

    fn problem(&mut self, what: String) {
        if self.problems.len() < 5 {
            self.problems.push(format!(
                "t = {:.3}: {what} (true s {:.1}, progress {:.1}, laps {}, gate {})",
                self.now, self.s, self.p.distance, self.p.laps, self.p.next_gate
            ));
        }
    }

    /// Drives along the centreline in steps of at most `speed * DT`.
    fn drive(&mut self, distance: f32, lateral: f32, speed: f32, off: bool) {
        let step = speed * DT * distance.signum();
        let steps = (distance.abs() / (speed * DT)).ceil() as usize;
        for _ in 0..steps {
            self.s += step;
            let here = interpolate(&self.g.samples, self.g.length, self.s);
            let x = if off {
                here.wall(1) + 3.0
            } else {
                lateral * here.half_width
            };
            self.lateral = x;
            self.update(here.at(x, 0.5), here.tangent * speed * distance.signum());
        }
    }

    fn check(&mut self) {
        let p = self.p;
        if !p.on_track {
            return;
        }
        let expected = expected_credit(self.g, p.distance);
        if (p.laps, p.next_gate) != expected {
            self.problem(format!(
                "credit (laps, gate) = {:?}, expected {expected:?} for the distance",
                (p.laps, p.next_gate)
            ));
        }
        if (p.distance - self.s).abs() > 2.0 {
            self.problem("progress lost track of the car".into());
        }
        if i64::from(p.laps) != self.laps_counted {
            self.problem(format!(
                "laps {} vs Lap events {}",
                p.laps, self.laps_counted
            ));
        }
        let t = self.timing.clone();
        if t.laps.len() != p.laps as usize {
            self.problem(format!("timing has {} laps", t.laps.len()));
            return;
        }
        let expected_start = *self.starts.last().unwrap();
        if t.lap_start != expected_start {
            self.problem(format!(
                "lap_start {} but the standing lap started at {expected_start}",
                t.lap_start
            ));
        }
        if t.laps.iter().any(|&l| l <= 0.0) {
            self.problem(format!("non-positive lap time {:?}", t.laps));
        }
        let min = t.laps.iter().copied().reduce(f32::min);
        if t.best_lap != min {
            self.problem(format!("best lap {:?}, laps {:?}", t.best_lap, t.laps));
        }
        if let (Some(&last), false) = (t.laps.last(), t.last_sectors.is_empty()) {
            let sum: f32 = t.last_sectors.iter().sum();
            if (sum - last).abs() > 1e-3 * last.max(1.0) {
                self.problem(format!(
                    "last sectors {:?} sum to {sum}, lap {last}",
                    t.last_sectors
                ));
            }
        }
        // Best sectors: the minimum over the laps that still stand.
        let sectors = self.g.sector_ends.len() + 1;
        let best: Vec<f32> = (0..sectors)
            .map(|i| {
                self.lap_sectors
                    .iter()
                    .filter_map(|s| s.get(i).copied())
                    .fold(f32::MAX, f32::min)
            })
            .collect();
        if !self.lap_sectors.is_empty() && t.best_sectors != best {
            self.problem(format!(
                "best sectors {:?}, standing laps' best {best:?}",
                t.best_sectors
            ));
        }
    }

    fn run(&mut self, legs: &[Leg]) {
        for &leg in legs {
            match leg {
                Leg::Drive {
                    distance,
                    lateral,
                    speed,
                } => self.drive(distance, lateral, speed, false),
                Leg::Excursion { distance } => {
                    self.drive(distance, 0.0, 30.0, true);
                    self.drive(1.0, 0.0, 30.0, false);
                }
                Leg::Reset => self.reset(),
            }
            self.check();
        }
    }
}

proptest! {
    #![proptest_config(config(48))]

    /// Driving forwards and backwards on the road (no excursions, no
    /// resets): credit always matches the distance, laps count and uncount
    /// one at a time, and timing agrees with the laps that stand.
    #[test]
    fn progress_and_timing_follow_a_drive_on_the_road(
        track in 0usize..2,
        slot in 0usize..8,
        legs in prop::collection::vec(
            (-40.0..250.0f32, -0.9..0.9f32, 5.0..90.0f32)
                .prop_map(|(distance, lateral, speed)| Leg::Drive { distance, lateral, speed }),
            1..40,
        ),
    ) {
        let g = &TRACKS[track];
        let mut run = Run::new(g, slot);
        run.run(&legs);
        prop_assert!(run.problems.is_empty(), "{:#?}", run.problems);
    }

    /// The same with excursions off the corridor and resets mixed in, on
    /// both tracks (the dense one has gates closer than one step's window).
    #[test]
    fn progress_survives_excursions_and_resets(
        track in 0usize..2,
        slot in 0usize..8,
        legs in prop::collection::vec(leg(), 1..30),
    ) {
        let g = &TRACKS[track];
        let mut run = Run::new(g, slot);
        run.run(&legs);
        prop_assert!(run.problems.is_empty(), "{:#?}", run.problems);
    }

    /// Standings are a permutation in a consistent total order: finishers
    /// (laps desc, time asc), then racing cars, then DNFs (laps, gate,
    /// distance desc).
    #[test]
    fn standings_are_a_consistent_total_order(
        cars in prop::collection::vec(
            (0u8..3, 0.0..2000.0f32, 0u32..12, 0usize..25, -100.0..30000.0f32), 0..12),
    ) {
        let g = &TRACKS[0];
        let base = Progress::new(g, g.grid[0].position);
        let list: Vec<(Status, Progress)> = cars
            .iter()
            .map(|&(s, t, laps, gate, d)| {
                let status = match s {
                    0 => Status::Finished(t),
                    1 => Status::Racing,
                    _ => Status::Dnf,
                };
                let mut p = base;
                p.laps = laps;
                p.next_gate = gate;
                p.distance = d;
                (status, p)
            })
            .collect();
        let order = standings(&list);
        let mut seen = order.clone();
        seen.sort_unstable();
        prop_assert_eq!(seen, (0..list.len()).collect::<Vec<_>>());
        let rank = |s: &Status| match s { Status::Finished(_) => 0, Status::Racing => 1, Status::Dnf => 2 };
        for w in order.windows(2) {
            let ((sa, pa), (sb, pb)) = (&list[w[0]], &list[w[1]]);
            prop_assert!(rank(sa) <= rank(sb));
            if rank(sa) != rank(sb) {
                continue;
            }
            match (sa, sb) {
                (Status::Finished(ta), Status::Finished(tb)) => {
                    prop_assert!(pa.laps > pb.laps || (pa.laps == pb.laps && ta <= tb));
                    if pa.laps == pb.laps && ta == tb {
                        prop_assert!(pa.distance >= pb.distance);
                    }
                }
                _ => prop_assert!(pa.key().0 > pb.key().0
                    || (pa.key().0 == pb.key().0 && pa.key().1 > pb.key().1)
                    || (pa.key().0 == pb.key().0 && pa.key().1 == pb.key().1 && pa.key().2 >= pb.key().2)),
            }
        }
    }

    /// Reset placement from any last valid point: on the road, behind the
    /// point, facing along the track, clear of the other cars when it can be.
    #[test]
    fn reset_pose_is_on_the_road_behind_and_clear(
        track in 0usize..2,
        distance in -100.0..6000.0f32,
        lateral in -20.0..20.0f32,
        others in prop::collection::vec((-60.0..10.0f32, -6.0..6.0f32), 0..8),
    ) {
        let g = &TRACKS[track];
        let others: Vec<Vec3> = others
            .iter()
            .map(|&(back, x)| interpolate(&g.samples, g.length, distance + back).at(x, 0.5))
            .collect();
        let (pose, d, x) = reset_pose(g, (distance, lateral), &others);
        prop_assert!(pose.position.is_finite() && pose.rotation.is_finite());
        prop_assert!(d <= distance - 8.0 + 1e-3 && d >= distance - 38.0 - 1e-3);
        let here = interpolate(&g.samples, g.length, d);
        prop_assert!(x.abs() <= here.half_width - 1.6 + 1e-3);
        let p = Progress::new(g, pose.position + Vec3::Y * 0.5);
        prop_assert!(p.on_track);
        prop_assert!((pose.rotation * Vec3::NEG_Z).dot(here.tangent) > 0.99);
    }
}

/// QA-EDGE-1: a progress update that jumps over two gates credits only the
/// first; the second is then behind the car and can never be credited by
/// driving forwards, so the car never completes another lap (it would have to
/// back over the gate). Shrunk case: on the shipped track with the
/// validator's minimum `checkpoint_spacing` (20 m), a car leaves the corridor
/// 2 m before a sector-end checkpoint, travels outside it past the next
/// checkpoint 6.7 m later (the property found it with a 13.5 m excursion,
/// half a second at 30 m/s, well inside the 3 s off-track reset) and comes
/// back: progress credits the first gate only and the car is stuck on the
/// second for every later lap.
#[test]
fn qa_edge_1_one_update_over_two_gates_loses_the_lap_for_good() {
    let g = &TRACKS[1];
    // The first pair of gates closer together than the 26 m the window allows.
    let k = g
        .checkpoints
        .windows(2)
        .position(|w| w[1] - w[0] < 20.0)
        .expect("a close pair");
    let (a, b) = (g.checkpoints[k], g.checkpoints[k + 1]);
    let mut run = Run::new(g, 0);
    run.run(&[Leg::Drive {
        distance: a - 2.0 - run.s,
        lateral: 0.0,
        speed: 30.0,
    }]);
    assert_eq!(run.p.next_gate, k);
    // Out of the corridor for (b - a) + 4 m, back in just past `b`.
    run.run(&[Leg::Excursion {
        distance: b - a + 3.0,
    }]);
    // Then drive two more laps on the road.
    run.run(&[Leg::Drive {
        distance: 2.0 * g.length,
        lateral: 0.0,
        speed: 60.0,
    }]);
    assert!(
        run.p.laps >= 2,
        "after 2 more laps: laps {}, next gate {} (stuck at gate {} of {}), problems {:#?}",
        run.p.laps,
        run.p.next_gate,
        k + 1,
        g.checkpoints.len(),
        run.problems
    );
}

/// QA-EDGE-2: a car that rejoins the corridor more than ~26 m (the search
/// window, `(WINDOW + 2) * sample spacing`) after it left it has its progress
/// frozen for the rest of the lap: the update that would move it is
/// discarded as an impossible jump, but `sample` follows the car, so every
/// later update is the same "impossible" jump. The car drives a whole lap with
/// its progress, gates and standings stuck where it left, and its
/// `ResetMonitor::last_valid` (from that progress) points back there too.
/// Shrunk case: from the grid, 33.5 m outside the corridor (about a second
/// at 30 m/s, inside the 3 s off-track reset), then a lap on the road.
#[test]
fn qa_edge_2_rejoining_far_downstream_freezes_progress() {
    let g = &TRACKS[0];
    let mut run = Run::new(g, 0);
    run.run(&[Leg::Excursion { distance: 33.5 }]);
    let frozen = run.p.distance;
    run.run(&[Leg::Drive {
        distance: 500.0,
        lateral: 0.0,
        speed: 40.0,
    }]);
    assert!(
        (run.p.distance - run.s).abs() < 2.0,
        "car at s = {:.1} after 500 m on the road, progress still {:.1} (frozen at {frozen:.1}), \
         next gate {}, last valid {:?}",
        run.s,
        run.p.distance,
        run.p.next_gate,
        run.monitor.last_valid
    );
}

/// QA-EDGE-11: a reset after reversing back over a checkpoint puts the car
/// ahead of that checkpoint without crediting it again. `ResetMonitor`
/// keeps `last_valid` from the first second of reversing (before the
/// wrong-way flag), `reset_pose` goes 8 m behind that, and `move_to`
/// only ever takes credit back. The gate is then behind the car for good:
/// it can never complete the lap. Shrunk case (shipped track): drive 210.7 m
/// from pole, reverse 39.7 m at 5 m/s (back over the checkpoint at ~180 m),
/// reset (stuck, or the driver's reset key, or an AI giving up on recovery).
#[test]
fn qa_edge_11_reset_after_reversing_skips_an_uncredited_gate() {
    let g = &TRACKS[0];
    let mut run = Run::new(g, 0);
    run.run(&[
        Leg::Drive {
            distance: 210.65636,
            lateral: 0.0,
            speed: 5.0,
        },
        Leg::Drive {
            distance: -39.69333,
            lateral: 0.0,
            speed: 5.0,
        },
    ]);
    let before = (run.s, run.p.next_gate);
    run.reset();
    let after = (run.s, run.p.next_gate);
    run.run(&[Leg::Drive {
        distance: 2.0 * g.length,
        lateral: 0.0,
        speed: 60.0,
    }]);
    assert!(
        run.p.laps >= 2,
        "reset moved the car from {before:?} to {after:?} (s, next gate); \
         after 2 more laps: laps {}, next gate {}",
        run.p.laps,
        run.p.next_gate
    );
}

/// QA-EDGE-9: driving backwards over the line twice (two laps undone) puts
/// `Timing::lap_start` at the start of the lap undone first instead of the
/// race start: `previous_lap_start` holds one level of history only. The
/// next lap is then timed from the wrong point. Shrunk case: two laps
/// (Timing events alone), undo both, redo one.
#[test]
fn qa_edge_9_second_lap_undone_restores_the_wrong_lap_start() {
    let g = &TRACKS[0];
    let mut t = Timing::start(0.0);
    let n = g.checkpoints.len();
    let lap = |t: &mut Timing, start: f32| {
        for i in 0..n {
            t.on_event(ProgressEvent::Checkpoint(i), start + 1.0 + i as f32, g);
        }
        t.on_event(ProgressEvent::Lap, start + 100.0, g);
    };
    lap(&mut t, 0.0); // lap 1: 0 -> 100
    lap(&mut t, 100.0); // lap 2: 100 -> 200
    t.on_event(ProgressEvent::LapUndone, 201.0, g);
    for i in (0..n).rev() {
        t.on_event(ProgressEvent::CheckpointUndone(i), 202.0, g);
    }
    t.on_event(ProgressEvent::LapUndone, 203.0, g);
    assert_eq!(
        t.lap_start, 0.0,
        "lap 1 is undone too: back to the race start"
    );
}

/// QA-EDGE-10: undoing a lap does not undo what it did to the best sectors
/// (and the best lap's checkpoint times used for the live delta): a lap that
/// no longer counts keeps its sector times as "best" on the HUD. Shrunk case:
/// one fast lap, taken back over the line.
#[test]
fn qa_edge_10_undone_lap_keeps_its_best_sectors_and_delta_reference() {
    let g = &TRACKS[0];
    let mut t = Timing::start(0.0);
    let n = g.checkpoints.len();
    for i in 0..n {
        t.on_event(ProgressEvent::Checkpoint(i), 1.0 + i as f32, g);
    }
    t.on_event(ProgressEvent::Lap, 50.0, g);
    t.on_event(ProgressEvent::LapUndone, 50.5, g);
    assert_eq!(t.best_lap, None);
    assert!(
        t.best_sectors.is_empty(),
        "no lap stands, yet best sectors are {:?}",
        t.best_sectors
    );
    // Back over the last checkpoint and forwards again: no lap stands, so
    // there is nothing to measure a delta against.
    t.on_event(ProgressEvent::CheckpointUndone(n - 1), 51.0, g);
    t.on_event(ProgressEvent::Checkpoint(n - 1), 52.0, g);
    assert_eq!(t.delta, None, "delta against a lap that does not count");
    // The lap redone stands again, with its own sectors as the bests.
    t.on_event(ProgressEvent::Lap, 53.0, g);
    assert_eq!(t.best_lap, Some(53.0));
    assert_eq!(t.best_sectors, t.last_sectors);
    assert!((t.last_sectors.iter().sum::<f32>() - 53.0).abs() < 1e-3);
}
