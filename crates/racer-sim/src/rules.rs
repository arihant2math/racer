//! Race rules as plain data, independent of the ECS so they are cheap to
//! unit test: track progress with ordered checkpoints, lap and sector
//! timing, wrong-way detection, reset monitoring and placement, standings
//! and finishing/DNF.
//!
//! Progress is a car's distance along the centreline from the start line,
//! unwrapped over laps (negative on the grid). It only advances while the
//! car is inside the walled corridor and is found by searching a short
//! window around the previous value, so it moves continuously: a car that
//! leaves the corridor (over a wall, through a gap) freezes its progress and
//! is reset after [`RESET_AFTER`] instead of gaining ground. Checkpoint gates
//! must be crossed in order and inside the corridor; crossing one backwards
//! takes its credit back, so a lap counts only when the car has driven all
//! of it forwards.

use bevy::math::Vec3;
use racer_trackc::{Geometry, Pose, geometry::interpolate};

/// Seconds a car may stay flipped, stuck or off the track before it is reset.
pub const RESET_AFTER: f32 = 3.0;
/// Seconds a reset car stays ghosted (no contact with other cars).
pub const GHOST_AFTER_RESET: f32 = 3.0;
/// Below this speed (m/s) a racing car counts as stuck.
const STUCK_SPEED: f32 = 1.0;
/// Samples searched either side of the previous position.
const WINDOW: usize = 24;
/// Distance beyond the wall face that still counts as on the track (m).
const CORRIDOR_SLACK: f32 = 0.8;
/// Seconds of driving against the track direction before the warning.
const WRONG_WAY_AFTER: f32 = 1.0;

/// What happened while updating progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProgressEvent {
    /// Checkpoint `i` (index into the track's checkpoints) credited.
    Checkpoint(usize),
    /// The start/finish line crossed with every checkpoint credited.
    Lap,
    /// The line crossed backwards: the last lap is taken back.
    LapUndone,
    /// Checkpoint `i` crossed backwards: its credit is taken back.
    CheckpointUndone(usize),
}

/// A car's position along the track.
#[derive(Clone, Copy, Debug)]
pub struct Progress {
    /// Unwrapped centreline distance from the start line (m).
    pub distance: f32,
    /// Nearest centreline sample.
    pub sample: usize,
    /// Offset from the centreline (m, positive right).
    pub lateral: f32,
    /// Laps completed.
    pub laps: u32,
    /// Next gate: an index into the checkpoints, or their count for the line.
    pub next_gate: usize,
    pub on_track: bool,
    pub wrong_way: bool,
    wrong_way_time: f32,
}

impl Progress {
    /// Progress of a car standing at `position` (on the grid, behind the line).
    pub fn new(track: &Geometry, position: Vec3) -> Self {
        let sample = nearest(track, position, 0, track.samples.len() / 2);
        let s = track.samples[sample].s;
        let distance = if s > track.length / 2.0 {
            s - track.length
        } else {
            s
        };
        let mut p = Progress {
            distance,
            sample,
            lateral: 0.0,
            laps: 0,
            next_gate: 0,
            on_track: true,
            wrong_way: false,
            wrong_way_time: 0.0,
        };
        p.update(track, position, Vec3::ZERO, Vec3::NEG_Z, 0.0);
        p
    }

    /// Distance of gate `g` in the current lap.
    fn gate(&self, track: &Geometry, g: usize) -> f32 {
        let s = track.checkpoints.get(g).copied().unwrap_or(track.length);
        self.laps as f32 * track.length + s
    }

    /// Distance of the gate credited last (the line of the previous lap at the start).
    fn previous_gate(&self, track: &Geometry) -> f32 {
        if self.next_gate == 0 {
            self.laps as f32 * track.length
        } else {
            self.gate(track, self.next_gate - 1)
        }
    }

    /// Updates from the car's position, velocity and forward direction.
    pub fn update(
        &mut self,
        track: &Geometry,
        position: Vec3,
        velocity: Vec3,
        forward: Vec3,
        dt: f32,
    ) -> Option<ProgressEvent> {
        let n = track.samples.len();
        let sample = nearest(track, position, self.sample, WINDOW);
        // Refine between the nearest sample and its neighbours.
        let a = &track.samples[sample];
        let along = (position - a.pos).dot(a.tangent);
        let s_local = a.s + along;
        let here = interpolate(&track.samples, track.length, s_local);
        let rel = position - here.pos;
        self.lateral = rel.dot(here.right);
        let height = rel.dot(here.up);
        let side = usize::from(self.lateral > 0.0);
        self.on_track =
            self.lateral.abs() < here.wall(side) + CORRIDOR_SLACK && (-1.5..4.0).contains(&height);
        // Wrong way: moving or pointing against the track direction.
        let backwards = velocity.dot(here.tangent) < -1.0
            || (forward.dot(here.tangent) < -0.5 && velocity.length() > 1.0);
        self.wrong_way_time = if self.on_track && backwards {
            self.wrong_way_time + dt
        } else {
            (self.wrong_way_time - 2.0 * dt).max(0.0)
        };
        self.wrong_way = self.wrong_way_time > WRONG_WAY_AFTER;
        if !self.on_track {
            return None;
        }
        self.sample = sample;
        let l = track.length;
        let unwrapped = s_local + l * ((self.distance - s_local) / l).round();
        let (old, new) = (self.distance, unwrapped);
        if (new - old).abs() > (WINDOW as f32 + 2.0) * l / n as f32 {
            // A jump the window cannot produce; ignore it.
            return None;
        }
        self.distance = new;
        let gates = track.checkpoints.len();
        let next = self.gate(track, self.next_gate);
        if old < next && new >= next {
            if self.next_gate == gates {
                self.laps += 1;
                self.next_gate = 0;
                return Some(ProgressEvent::Lap);
            }
            self.next_gate += 1;
            return Some(ProgressEvent::Checkpoint(self.next_gate - 1));
        }
        let prev = self.previous_gate(track);
        let credited = self.laps > 0 || self.next_gate > 0;
        if credited && old >= prev && new < prev {
            if self.next_gate == 0 {
                self.laps -= 1;
                self.next_gate = gates;
                return Some(ProgressEvent::LapUndone);
            }
            self.next_gate -= 1;
            return Some(ProgressEvent::CheckpointUndone(self.next_gate));
        }
        None
    }

    /// Moves the car back to `distance` (a reset), taking back the credit
    /// of every gate it is now behind. Returns those undo events.
    pub fn move_back_to(
        &mut self,
        track: &Geometry,
        distance: f32,
        lateral: f32,
    ) -> Vec<ProgressEvent> {
        let mut events = Vec::new();
        while (self.laps > 0 || self.next_gate > 0) && distance < self.previous_gate(track) {
            if self.next_gate == 0 {
                self.laps -= 1;
                self.next_gate = track.checkpoints.len();
                events.push(ProgressEvent::LapUndone);
            } else {
                self.next_gate -= 1;
                events.push(ProgressEvent::CheckpointUndone(self.next_gate));
            }
        }
        let n = track.samples.len();
        self.distance = distance;
        self.sample =
            ((distance.rem_euclid(track.length) / track.length * n as f32) as usize).min(n - 1);
        self.lateral = lateral;
        self.on_track = true;
        self.wrong_way = false;
        self.wrong_way_time = 0.0;
        events
    }

    /// Ordering key for standings: laps, then gates, then distance.
    pub fn key(&self) -> (u32, usize, f32) {
        (self.laps, self.next_gate, self.distance)
    }
}

/// Nearest sample to `position` within `window` samples of `hint`.
fn nearest(track: &Geometry, position: Vec3, hint: usize, window: usize) -> usize {
    let n = track.samples.len();
    (0..=2 * window.min(n / 2))
        .map(|i| (hint + n + i - window.min(n / 2)) % n)
        .min_by(|&a, &b| {
            let da = track.samples[a].pos.distance_squared(position);
            let db = track.samples[b].pos.distance_squared(position);
            da.total_cmp(&db)
        })
        .unwrap_or(hint)
}

/// Why a car is reset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetReason {
    Flipped,
    Stuck,
    OffTrack,
    /// Asked for by the driver (reset key, or an AI giving up on recovery).
    Requested,
}

/// Watches for a car that needs resetting and remembers where it last was
/// on the track in a valid state.
#[derive(Clone, Copy, Debug)]
pub struct ResetMonitor {
    flipped: f32,
    stuck: f32,
    off_track: f32,
    /// Last valid progress distance and lateral offset.
    pub last_valid: (f32, f32),
}

impl ResetMonitor {
    pub fn new(progress: &Progress) -> Self {
        ResetMonitor {
            flipped: 0.0,
            stuck: 0.0,
            off_track: 0.0,
            last_valid: (progress.distance, progress.lateral),
        }
    }

    /// `up_y` is the car's up vector's vertical component, `speed` its speed.
    /// `racing` is false before the start and after the finish, when standing
    /// still is fine.
    pub fn update(
        &mut self,
        progress: &Progress,
        up_y: f32,
        speed: f32,
        racing: bool,
        dt: f32,
    ) -> Option<ResetReason> {
        let tick = |t: &mut f32, on: bool| *t = if on { *t + dt } else { 0.0 };
        tick(&mut self.flipped, up_y < 0.3);
        tick(&mut self.stuck, racing && speed < STUCK_SPEED);
        tick(&mut self.off_track, !progress.on_track);
        if progress.on_track && up_y > 0.8 && !progress.wrong_way && speed > STUCK_SPEED {
            self.last_valid = (progress.distance, progress.lateral);
        }
        let reason = if self.flipped > RESET_AFTER {
            Some(ResetReason::Flipped)
        } else if self.off_track > RESET_AFTER {
            Some(ResetReason::OffTrack)
        } else if self.stuck > RESET_AFTER {
            Some(ResetReason::Stuck)
        } else {
            None
        };
        if reason.is_some() {
            self.clear();
        }
        reason
    }

    pub fn clear(&mut self) {
        self.flipped = 0.0;
        self.stuck = 0.0;
        self.off_track = 0.0;
    }
}

/// Where to put a car reset from `last_valid` (distance, lateral): on the
/// road a little behind that point, facing along the track, away from the
/// other cars' positions.
/// Returns the pose with its progress distance and lateral offset.
pub fn reset_pose(track: &Geometry, last_valid: (f32, f32), others: &[Vec3]) -> (Pose, f32, f32) {
    let (distance, lateral) = last_valid;
    let mut fallback = None;
    for back in [8.0, 18.0, 28.0, 38.0] {
        let d = distance - back;
        let sample = interpolate(&track.samples, track.length, d);
        let edge = sample.half_width - 1.6;
        for x in [lateral.clamp(-edge, edge), 0.0, -edge / 2.0, edge / 2.0] {
            let position = sample.at(x, 0.0);
            let pose = Pose {
                position,
                rotation: sample.rotation(),
            };
            fallback.get_or_insert((pose, d, x));
            if others.iter().all(|o| o.distance(position) > 7.0) {
                return (pose, d, x);
            }
        }
    }
    fallback.expect("at least one candidate")
}

/// A car's lap and sector times.
#[derive(Clone, Debug, Default)]
pub struct Timing {
    /// Race time the current lap started.
    pub lap_start: f32,
    previous_lap_start: f32,
    pub laps: Vec<f32>,
    pub best_lap: Option<f32>,
    /// Sector times of the current lap so far, and of the last full lap.
    pub sectors: Vec<f32>,
    pub last_sectors: Vec<f32>,
    pub best_sectors: Vec<f32>,
    /// Lap time at each checkpoint, current lap and best lap.
    checkpoint_times: Vec<f32>,
    best_checkpoint_times: Vec<f32>,
    /// Current lap against the best lap at the last checkpoint (s, negative is faster).
    pub delta: Option<f32>,
}

impl Timing {
    pub fn start(now: f32) -> Self {
        Timing {
            lap_start: now,
            previous_lap_start: now,
            ..Default::default()
        }
    }

    fn sector_start(&self) -> f32 {
        self.sectors.iter().sum::<f32>()
    }

    pub fn current_lap(&self, now: f32) -> f32 {
        now - self.lap_start
    }

    pub fn on_event(&mut self, event: ProgressEvent, now: f32, track: &Geometry) {
        let elapsed = now - self.lap_start;
        match event {
            ProgressEvent::Checkpoint(i) => {
                self.checkpoint_times.truncate(i);
                self.checkpoint_times.push(elapsed);
                self.delta = self.best_checkpoint_times.get(i).map(|best| elapsed - best);
                if track.sector_ends.contains(&i) {
                    let t = elapsed - self.sector_start();
                    self.sectors.push(t);
                }
            }
            ProgressEvent::CheckpointUndone(i) => {
                self.checkpoint_times.truncate(i);
                if track.sector_ends.contains(&i) {
                    self.sectors.pop();
                }
            }
            ProgressEvent::Lap => {
                let t = elapsed - self.sector_start();
                self.sectors.push(t);
                self.laps.push(elapsed);
                if self.best_lap.is_none_or(|b| elapsed < b) {
                    self.best_lap = Some(elapsed);
                    self.best_checkpoint_times = std::mem::take(&mut self.checkpoint_times);
                }
                let sectors = std::mem::take(&mut self.sectors);
                if self.best_sectors.len() < sectors.len() {
                    self.best_sectors.resize(sectors.len(), f32::MAX);
                }
                for (best, t) in self.best_sectors.iter_mut().zip(&sectors) {
                    *best = best.min(*t);
                }
                self.last_sectors = sectors;
                self.checkpoint_times.clear();
                self.previous_lap_start = self.lap_start;
                self.lap_start = now;
            }
            ProgressEvent::LapUndone => {
                // Back over the line: the lap did not happen.
                if let Some(t) = self.laps.pop() {
                    self.lap_start = self.previous_lap_start;
                    if self.best_lap == Some(t) {
                        self.best_lap = self.laps.iter().copied().reduce(f32::min);
                    }
                }
                self.sectors = std::mem::take(&mut self.last_sectors);
                self.sectors.pop();
            }
        }
    }
}

/// How a car's race stands.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    Racing,
    /// Finished at this race time.
    Finished(f32),
    /// Did not finish within the timeout after the winner.
    Dnf,
}

/// The end condition of a race.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Goal {
    Laps(u32),
    /// Point to point: finish at this distance from the start line.
    Distance(f32),
}

impl Goal {
    /// Whether `progress` has reached the goal (every gate before it credited).
    pub fn reached(self, progress: &Progress, track: &Geometry) -> bool {
        match self {
            Goal::Laps(n) => progress.laps >= n,
            Goal::Distance(d) => {
                let gates = track.checkpoints.iter().filter(|&&c| c < d).count();
                progress.laps == 0 && progress.next_gate >= gates && progress.distance >= d
            }
        }
    }
}

/// Seconds after the winner's finish before unfinished cars are out.
pub fn dnf_due(winner_time: Option<f32>, now: f32, timeout: f32) -> bool {
    winner_time.is_some_and(|w| now > w + timeout)
}

/// Standings order (indices into `cars`): finishers by finish time, then
/// racing cars by progress, then DNFs by progress.
pub fn standings(cars: &[(Status, Progress)]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..cars.len()).collect();
    let rank = |s: &Status| match s {
        Status::Finished(_) => 0,
        Status::Racing => 1,
        Status::Dnf => 2,
    };
    order.sort_by(|&a, &b| {
        let ((sa, pa), (sb, pb)) = (&cars[a], &cars[b]);
        rank(sa).cmp(&rank(sb)).then_with(|| match (sa, sb) {
            (Status::Finished(ta), Status::Finished(tb)) => ta.total_cmp(tb),
            _ => {
                let (ka, kb) = (pa.key(), pb.key());
                kb.0.cmp(&ka.0)
                    .then(kb.1.cmp(&ka.1))
                    .then(kb.2.total_cmp(&ka.2))
            }
        })
    });
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn heading(rotation: bevy::math::Quat) -> Vec3 {
        rotation * Vec3::NEG_Z
    }

    fn track() -> Geometry {
        racer_trackc::build(&racer_trackc::tracks_dir().join("downtown-loop")).unwrap()
    }

    /// Drives `progress` along the centreline from where it is to `to`
    /// (either direction) at lateral offset `x`, collecting the events.
    fn drive(track: &Geometry, p: &mut Progress, to: f32, x: f32) -> Vec<ProgressEvent> {
        let from = p.distance;
        let step = 0.25 * (to - from).signum();
        let mut events = Vec::new();
        let mut d = from;
        while (to - d) * step.signum() > 0.0 {
            d += step;
            let s = interpolate(&track.samples, track.length, d);
            let v = s.tangent * 60.0 * step.signum();
            events.extend(p.update(track, s.at(x, 0.5), v, v.normalize(), 1.0 / 240.0));
        }
        events
    }

    #[test]
    fn grid_is_behind_the_line_and_checkpoints_come_in_order() {
        let t = track();
        let mut p = Progress::new(&t, t.grid[7].position);
        assert!(p.distance < -40.0 && p.distance > -70.0, "{}", p.distance);
        assert!(p.on_track && p.laps == 0 && p.next_gate == 0);
        let events = drive(&t, &mut p, t.length + 10.0, 2.0);
        let n = t.checkpoints.len();
        let mut expected: Vec<ProgressEvent> = (0..n).map(ProgressEvent::Checkpoint).collect();
        expected.push(ProgressEvent::Lap);
        assert_eq!(events, expected);
        assert_eq!((p.laps, p.next_gate), (1, 0));
    }

    #[test]
    fn lap_counts_only_forwards_across_the_line() {
        let t = track();
        let mut p = Progress::new(&t, t.grid[0].position);
        // Across the line forwards at the start: no lap, no gate.
        assert!(drive(&t, &mut p, 20.0, 0.0).is_empty());
        // Back over the line from the first lap: nothing to take back.
        assert!(drive(&t, &mut p, -10.0, 0.0).is_empty());
        // A full lap counts once.
        let events = drive(&t, &mut p, t.length + 5.0, 0.0);
        assert_eq!(events.last(), Some(&ProgressEvent::Lap));
        assert_eq!(p.laps, 1);
        // Reversing over the line takes the lap back, driving over it again restores it.
        assert_eq!(
            drive(&t, &mut p, t.length - 5.0, 0.0),
            [ProgressEvent::LapUndone]
        );
        assert_eq!((p.laps, p.next_gate), (0, t.checkpoints.len()));
        assert_eq!(drive(&t, &mut p, t.length + 5.0, 0.0), [ProgressEvent::Lap]);
        assert_eq!(p.laps, 1);
    }

    #[test]
    fn reversing_through_a_checkpoint_takes_its_credit_back() {
        let t = track();
        let mut p = Progress::new(&t, t.grid[0].position);
        let c0 = t.checkpoints[0];
        assert_eq!(
            drive(&t, &mut p, c0 + 5.0, 0.0),
            [ProgressEvent::Checkpoint(0)]
        );
        assert_eq!(
            drive(&t, &mut p, c0 - 5.0, 0.0),
            [ProgressEvent::CheckpointUndone(0)]
        );
        assert_eq!(p.next_gate, 0);
    }

    #[test]
    fn shortcut_across_the_hairpin_is_rejected() {
        let t = track();
        let mut p = Progress::new(&t, t.grid[0].position);
        // Find the hairpin entry: the tightest curvature on the track.
        let apex = t
            .samples
            .iter()
            .max_by(|a, b| a.curvature.abs().total_cmp(&b.curvature.abs()))
            .unwrap()
            .s;
        drive(&t, &mut p, apex - 60.0, 0.0);
        let before = p;
        // Jump straight across the walls between the hairpin's two legs.
        let entry = interpolate(&t.samples, t.length, apex - 60.0);
        let exit = interpolate(&t.samples, t.length, apex + 60.0);
        let mut events = Vec::new();
        for k in 0..=200 {
            let pos = entry.pos.lerp(exit.pos, k as f32 / 200.0) + Vec3::Y * 0.5;
            events.extend(p.update(&t, pos, entry.tangent * 30.0, entry.tangent, 1.0 / 240.0));
        }
        assert!(events.is_empty(), "{events:?}");
        assert!(
            !p.on_track,
            "the far leg is not reachable through the window"
        );
        assert!(
            p.distance <= before.distance + 25.0,
            "{} -> {}",
            before.distance,
            p.distance
        );
        assert_eq!(p.next_gate, before.next_gate);
        // Off the track for 3 s: reset to where it left.
        let mut monitor = ResetMonitor::new(&before);
        let mut reason = None;
        for _ in 0..(240.0 * (RESET_AFTER + 0.1)) as usize {
            reason = reason.or(monitor.update(&p, 1.0, 30.0, true, 1.0 / 240.0));
        }
        assert_eq!(reason, Some(ResetReason::OffTrack));
        assert!((monitor.last_valid.0 - before.distance).abs() < 1.0);
    }

    #[test]
    fn wrong_way_is_flagged_after_a_second() {
        let t = track();
        let mut p = Progress::new(&t, t.grid[0].position);
        let s = interpolate(&t.samples, t.length, 100.0);
        p.distance = 100.0;
        p.sample = t.samples.iter().position(|x| x.s >= 100.0).unwrap();
        for k in 0..230 {
            p.update(
                &t,
                s.pos - s.tangent * (k as f32 * 0.04),
                -s.tangent * 10.0,
                -s.tangent,
                1.0 / 240.0,
            );
        }
        assert!(!p.wrong_way);
        for k in 230..300 {
            p.update(
                &t,
                s.pos - s.tangent * (k as f32 * 0.04),
                -s.tangent * 10.0,
                -s.tangent,
                1.0 / 240.0,
            );
        }
        assert!(p.wrong_way);
        // Turned around: the warning clears.
        for k in 0..480 {
            p.update(
                &t,
                s.pos + s.tangent * (k as f32 * 0.04),
                s.tangent * 10.0,
                s.tangent,
                1.0 / 240.0,
            );
        }
        assert!(!p.wrong_way);
    }

    #[test]
    fn standings_order_by_lap_then_gate_then_distance() {
        let t = track();
        let base = Progress::new(&t, t.grid[0].position);
        let at = |laps, gate, distance| Progress {
            laps,
            next_gate: gate,
            distance,
            ..base
        };
        let cars = [
            (Status::Racing, at(1, 3, 2500.0)),
            (Status::Racing, at(2, 0, 4300.0)),
            (Status::Dnf, at(2, 5, 4800.0)),
            (Status::Racing, at(1, 4, 2400.0)),
            (Status::Finished(201.0), at(3, 0, 6500.0)),
            (Status::Racing, at(1, 4, 2450.0)),
            (Status::Finished(199.5), at(3, 0, 6450.0)),
        ];
        assert_eq!(standings(&cars), [6, 4, 1, 5, 3, 0, 2]);
    }

    #[test]
    fn reset_lands_on_the_road_behind_the_last_valid_point_clear_of_cars() {
        let t = track();
        let last = (500.0, 3.0);
        let (pose, d, x) = reset_pose(&t, last, &[]);
        assert_eq!((d, x), (492.0, 3.0));
        let p = Progress::new(&t, pose.position);
        assert!(p.on_track);
        assert!((p.distance - 492.0).abs() < 0.6, "{}", p.distance);
        assert!((p.lateral - 3.0).abs() < 0.1);
        let s = interpolate(&t.samples, t.length, p.distance);
        assert!(heading(pose.rotation).dot(s.tangent) > 0.99);
        // A car parked there: the next candidate is clear of it.
        let other = pose.position;
        let (pose2, ..) = reset_pose(&t, last, &[other]);
        assert!(pose2.position.distance(other) > 7.0);
        assert!(Progress::new(&t, pose2.position).on_track);
        // The road edge is respected even if the car left from the apron.
        let (pose3, ..) = reset_pose(&t, (500.0, 9.0), &[]);
        let p3 = Progress::new(&t, pose3.position);
        assert!(p3.lateral.abs() < s.half_width - 1.5);
        // Placed behind a checkpoint it had passed: the credit is taken back.
        let mut q = Progress::new(&t, t.grid[0].position);
        drive(&t, &mut q, t.checkpoints[0] + 3.0, 0.0);
        assert_eq!(q.next_gate, 1);
        let undone = q.move_back_to(&t, t.checkpoints[0] - 5.0, 0.0);
        assert_eq!(undone, [ProgressEvent::CheckpointUndone(0)]);
        assert_eq!(
            drive(&t, &mut q, t.checkpoints[0] + 1.0, 0.0),
            [ProgressEvent::Checkpoint(0)]
        );
    }

    #[test]
    fn monitor_resets_flipped_and_stuck_cars_after_three_seconds() {
        let t = track();
        let p = Progress::new(&t, t.grid[0].position);
        let dt = 1.0 / 240.0;
        let run = |up, speed, racing, seconds: f32| {
            let mut m = ResetMonitor::new(&p);
            (0..(seconds * 240.0) as usize).find_map(|k| {
                m.update(&p, up, speed, racing, dt)
                    .map(|r| (r, k as f32 * dt))
            })
        };
        let (r, at) = run(-0.9, 0.0, false, 4.0).unwrap();
        assert_eq!(r, ResetReason::Flipped);
        assert!((at - RESET_AFTER).abs() < 0.01);
        assert_eq!(
            run(1.0, 0.2, true, 4.0).map(|x| x.0),
            Some(ResetReason::Stuck)
        );
        // Standing on the grid before the start is not stuck.
        assert_eq!(run(1.0, 0.0, false, 10.0), None);
        // Driving slowly but steadily is fine.
        assert_eq!(run(1.0, 3.0, true, 10.0), None);
    }

    #[test]
    fn unfinished_cars_are_out_after_the_timeout() {
        assert!(!dnf_due(None, 500.0, 30.0));
        assert!(!dnf_due(Some(200.0), 229.9, 30.0));
        assert!(dnf_due(Some(200.0), 230.1, 30.0));
    }

    #[test]
    fn timing_records_laps_sectors_and_delta() {
        let t = track();
        let mut timing = Timing::start(0.0);
        let n = t.checkpoints.len();
        let lap = |timing: &mut Timing, start: f32, pace: f32| {
            for i in 0..n {
                timing.on_event(
                    ProgressEvent::Checkpoint(i),
                    start + t.checkpoints[i] / pace,
                    &t,
                );
            }
            timing.on_event(ProgressEvent::Lap, start + t.length / pace, &t);
        };
        lap(&mut timing, 0.0, 25.0);
        let first = t.length / 25.0;
        assert!((timing.best_lap.unwrap() - first).abs() < 1e-3);
        assert_eq!(timing.last_sectors.len(), t.sector_ends.len() + 1);
        assert!((timing.last_sectors.iter().sum::<f32>() - first).abs() < 1e-3);
        // A faster second lap: negative delta at every checkpoint, new best.
        let start = timing.lap_start;
        timing.on_event(
            ProgressEvent::Checkpoint(0),
            start + t.checkpoints[0] / 26.0,
            &t,
        );
        assert!(timing.delta.unwrap() < 0.0);
        lap(&mut timing, start, 26.0);
        assert!(timing.best_lap.unwrap() < first);
        assert_eq!(timing.laps.len(), 2);
        assert!(
            timing
                .best_sectors
                .iter()
                .zip(&timing.last_sectors)
                .all(|(b, l)| b <= l)
        );
    }

    #[test]
    fn sprint_finishes_at_the_finish_distance_with_every_gate() {
        let t = track();
        let goal = Goal::Distance(t.sprint_finish);
        let mut p = Progress::new(&t, t.grid[0].position);
        drive(&t, &mut p, t.sprint_finish - 1.0, 0.0);
        assert!(!goal.reached(&p, &t));
        drive(&t, &mut p, t.sprint_finish + 1.0, 0.0);
        assert!(goal.reached(&p, &t));
        assert!(Goal::Laps(0).reached(&p, &t) && !Goal::Laps(1).reached(&p, &t));
    }
}
