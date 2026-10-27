//! Headless races on the city track for verification and tuning: solo AI
//! lap runs per car and difficulty, and 8-car mixed races with full contact.
//! Counts laps, resets and stalls, and checks the lap-time spread between
//! difficulties against the generated optimal-line time.

use std::{fmt::Write as _, sync::Arc};

use avian3d::prelude::{CollidingEntities, LinearVelocity, Position};
use bevy::{ecs::schedule::SingleThreadedExecutor, prelude::*, time::TimeUpdateStrategy};
use racer_dynamics::{load_car, spec::CARS};
use racer_sim::{
    SimPlugin,
    ai::{AiDriver, Difficulty},
    race::{
        DriverKind, Race, RaceConfig, RacePhase, RacePlugin, RaceTrack, Racer, Reset, spawn_racer,
    },
    rules::Status,
    tick_duration,
    track::{city_track, spawn_track},
    vehicle::{CarBody, VehiclePlugin},
};
use racer_trackc::Track;

/// No progress (5 m along the track) for this long counts as a stall.
pub const STALL_AFTER: f32 = 10.0;
/// A reset this soon after car-to-car contact counts as a contact reset.
pub const CONTACT_WINDOW: f32 = 5.0;
/// No progress for this long counts as permanently stuck.
pub const STUCK_FOR_GOOD: f32 = 60.0;

/// A headless race, advanced one 240 Hz tick at a time.
pub struct RaceSim {
    pub app: App,
    pub cars: Vec<Entity>,
    /// Each car's body collider.
    pub bodies: Vec<Entity>,
    pub track: Arc<Track>,
}

impl RaceSim {
    pub fn new(
        track: Arc<Track>,
        config: RaceConfig,
        entries: &[(&str, DriverKind)],
    ) -> Result<Self, String> {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins.set(TaskPoolPlugin {
                task_pool_options: TaskPoolOptions::with_num_threads(1),
            }),
            TransformPlugin,
            SimPlugin,
            VehiclePlugin,
            RacePlugin,
        ))
        .insert_resource(TimeUpdateStrategy::ManualDuration(tick_duration()))
        .insert_resource(Race::new(&config))
        .insert_resource(config)
        .insert_resource(RaceTrack(track.clone()));
        app.finish();
        app.cleanup();
        // Runs are parallelised across threads by the caller; inside one run
        // the systems are too small to gain from Bevy's multi-threaded executor.
        for (_, schedule) in app.world_mut().resource_mut::<Schedules>().iter_mut() {
            schedule.set_executor(SingleThreadedExecutor::new());
        }
        let world = app.world_mut();
        let mut commands = world.commands();
        spawn_track(&mut commands, &city_track(&track.geometry));
        let mut cars = Vec::new();
        for (slot, (car, driver)) in entries.iter().enumerate() {
            let spec = load_car(car)?;
            let name = match driver {
                DriverKind::Human => "Player".to_string(),
                DriverKind::Ai(d) => format!("{} {}", d.name(), slot + 1),
            };
            cars.push(spawn_racer(
                &mut commands,
                &track,
                &spec,
                slot,
                name,
                *driver,
                (),
            )?);
        }
        world.flush();
        let bodies = cars
            .iter()
            .map(|&car| {
                let children = world.get::<Children>(car).expect("car body");
                children
                    .iter()
                    .find(|c| world.get::<CarBody>(*c).is_some())
                    .expect("car body")
            })
            .collect();
        Ok(RaceSim { app, cars, bodies, track })
    }

    pub fn step(&mut self) {
        self.app.update();
    }

    pub fn race(&self) -> &Race {
        self.app.world().resource::<Race>()
    }

    pub fn racer(&self, car: Entity) -> &Racer {
        self.app.world().get::<Racer>(car).unwrap()
    }

    pub fn speed(&self, car: Entity) -> f32 {
        self.app
            .world()
            .get::<LinearVelocity>(car)
            .unwrap()
            .0
            .length()
    }

    pub fn position(&self, car: Entity) -> Vec3 {
        self.app.world().get::<Position>(car).unwrap().0
    }

    /// Whether car `i`'s body touches another car's body.
    pub fn in_contact(&self, i: usize) -> bool {
        self.app
            .world()
            .get::<CollidingEntities>(self.bodies[i])
            .is_some_and(|c| c.iter().any(|e| self.bodies.contains(e)))
    }
}

/// Per-car outcome of a run.
#[derive(Clone, Debug)]
pub struct CarRun {
    pub car: String,
    pub difficulty: Difficulty,
    /// Every completed lap's time; the first is from the standing start.
    pub laps: Vec<f32>,
    pub resets: Vec<Reset>,
    /// Resets within [`CONTACT_WINDOW`] of touching another car.
    pub contact_resets: usize,
    /// Episodes of car-to-car contact.
    pub contacts: u32,
    /// Episodes of no progress for [`STALL_AFTER`].
    pub stalls: u32,
    /// Longest time without progress.
    pub longest_stall: f32,
    /// Lap time the driver's own speed profile predicts.
    pub planned: f32,
    /// Fastest flying lap at 60 Hz: line distance, speed, target speed,
    /// grip trim, lateral error, front and rear slip angle, throttle, brake.
    pub trace: Vec<[f32; 9]>,
}

impl CarRun {
    /// Laps after the first (flying laps).
    pub fn flying(&self) -> &[f32] {
        self.laps.get(1..).unwrap_or(&[])
    }

    pub fn mean_flying(&self) -> f32 {
        let f = self.flying();
        f.iter().sum::<f32>() / f.len().max(1) as f32
    }
}

/// Watches progress and records traces for each car of a sim.
struct Watch {
    last: Vec<(f32, f32)>,
    /// Last time each car touched another, and whether it touches now.
    contact: Vec<(f32, bool)>,
    stalled: Vec<bool>,
    runs: Vec<CarRun>,
    lap_trace: Vec<Vec<[f32; 9]>>,
    best: Vec<f32>,
}

impl Watch {
    fn new(sim: &RaceSim, entries: &[(&str, DriverKind)]) -> Self {
        let n = sim.cars.len();
        let runs = sim
            .cars
            .iter()
            .zip(entries)
            .map(|(&e, (car, driver))| CarRun {
                car: car.to_string(),
                difficulty: match driver {
                    DriverKind::Ai(d) => *d,
                    DriverKind::Human => Difficulty::Alien,
                },
                laps: Vec::new(),
                resets: Vec::new(),
                contact_resets: 0,
                contacts: 0,
                stalls: 0,
                longest_stall: 0.0,
                planned: sim
                    .app
                    .world()
                    .get::<AiDriver>(e)
                    .map_or(0.0, |ai| ai.planned_lap_time(&sim.track.line)),
                trace: Vec::new(),
            })
            .collect();
        Watch {
            last: vec![(f32::MIN, 0.0); n],
            contact: vec![(f32::MIN, false); n],
            stalled: vec![false; n],
            runs,
            lap_trace: vec![Vec::new(); n],
            best: vec![f32::MAX; n],
        }
    }

    fn observe(&mut self, sim: &RaceSim, tick: u64) {
        let now = sim.race().time;
        if sim.race().phase == RacePhase::Countdown {
            return;
        }
        let line = &sim.track.line;
        for (i, &car) in sim.cars.iter().enumerate() {
            let r = sim.racer(car);
            let run = &mut self.runs[i];
            if r.timing.laps.len() > run.laps.len() {
                let t = *r.timing.laps.last().unwrap();
                run.laps.push(t);
                if run.laps.len() > 1 && t < self.best[i] {
                    self.best[i] = t;
                    run.trace = std::mem::take(&mut self.lap_trace[i]);
                }
                self.lap_trace[i].clear();
            }
            if r.status != Status::Racing {
                continue;
            }
            let d = r.progress.distance;
            if d > self.last[i].0 + 5.0 || self.last[i].0 == f32::MIN {
                self.last[i] = (d, now);
                self.stalled[i] = false;
            }
            let idle = now - self.last[i].1;
            run.longest_stall = run.longest_stall.max(idle);
            if idle > STALL_AFTER && !self.stalled[i] {
                self.stalled[i] = true;
                run.stalls += 1;
            }
            if tick.is_multiple_of(4) {
                let k = r.progress.sample;
                let dbg = sim
                    .app
                    .world()
                    .get::<AiDriver>(car)
                    .map(|a| a.debug)
                    .unwrap_or_default();
                self.lap_trace[i].push([
                    line.points[k].distance,
                    sim.speed(car),
                    dbg.target_speed,
                    dbg.grip_trim,
                    dbg.lateral_error,
                    dbg.front_slip,
                    dbg.rear_slip,
                    dbg.throttle,
                    dbg.brake,
                ]);
            }
            let touching = sim.in_contact(i);
            if touching {
                if !self.contact[i].1 {
                    run.contacts += 1;
                }
                self.contact[i].0 = now;
            }
            self.contact[i].1 = touching;
            for reset in &r.resets[run.resets.len()..] {
                if reset.time - self.contact[i].0 < CONTACT_WINDOW {
                    run.contact_resets += 1;
                }
            }
            run.resets = r.resets.clone();
        }
    }
}

/// Runs a race until every car has `laps` laps (or the race ends, or
/// `max_time` passes); returns per-car outcomes.
pub fn run(
    track: Arc<Track>,
    config: RaceConfig,
    entries: &[(&str, DriverKind)],
    laps: u32,
    max_time: f32,
) -> Result<Vec<CarRun>, String> {
    let mut sim = RaceSim::new(track, config, entries)?;
    let mut watch = Watch::new(&sim, entries);
    let mut tick = 0u64;
    loop {
        sim.step();
        tick += 1;
        watch.observe(&sim, tick);
        let race = sim.race();
        let done = watch.runs.iter().all(|r| r.laps.len() >= laps as usize)
            || race.phase == RacePhase::Finished
            || race.time > max_time
            || watch.runs.iter().any(|r| r.longest_stall > STUCK_FOR_GOOD);
        if done {
            break;
        }
    }
    Ok(watch.runs)
}

/// A solo run: one AI driver alone on the track for `laps` laps.
pub fn solo(
    track: Arc<Track>,
    car: &str,
    difficulty: Difficulty,
    laps: u32,
) -> Result<CarRun, String> {
    let config = RaceConfig {
        laps: laps + 1,
        ..default()
    };
    // A slow Novice lap is ~25 % over the optimum; allow for resets on top.
    let limit = track.car(car).map_or(120.0, |c| c.lap_time) * 1.6 * (laps + 1) as f32 + 60.0;
    let mut runs = run(
        track,
        config,
        &[(car, DriverKind::Ai(difficulty))],
        laps,
        limit,
    )?;
    Ok(runs.remove(0))
}

/// One 8-car race with full contact: every difficulty at least once (three
/// repeated, rotating with `index`), the three car models in turn, and the
/// grid order shuffled by `index`.
pub fn mixed(track: Arc<Track>, index: usize, laps: u32) -> Result<Vec<CarRun>, String> {
    let mut entries: Vec<(&str, DriverKind)> = (0..8)
        .map(|i| {
            let d = Difficulty::ALL[(i + index) % Difficulty::ALL.len()];
            (CARS[(i + index / 2) % CARS.len()], DriverKind::Ai(d))
        })
        .collect();
    let mut seed = index as u64 * 0x9e37_79b9 + 1;
    for i in (1..entries.len()).rev() {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        entries.swap(i, (seed % (i as u64 + 1)) as usize);
    }
    let config = RaceConfig {
        laps,
        ..default()
    };
    let slowest = CARS.iter().filter_map(|c| track.car(c)).map(|c| c.lap_time).fold(0.0, f32::max);
    let limit = slowest * 1.6 * laps as f32 + 120.0;
    run(track, config, &entries, laps, limit)
}

/// Text report of mixed races, and whether every check passed: no car
/// permanently stuck, every difficulty at `min_laps` or more, and resets
/// without contact rare (at most one per 100 laps per difficulty).
pub fn report_mixed(races: &[Vec<CarRun>], min_laps: usize, out: &mut String) -> bool {
    let mut ok = true;
    let _ = writeln!(
        out,
        "| Difficulty | Cars | Laps | Contacts | Resets after contact | Other resets | Stalls (>{STALL_AFTER:.0} s) | Longest stall (s) | Permanently stuck | |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|");
    for d in Difficulty::ALL {
        let runs: Vec<&CarRun> = races.iter().flatten().filter(|r| r.difficulty == d).collect();
        let laps: usize = runs.iter().map(|r| r.laps.len()).sum();
        let contacts: u32 = runs.iter().map(|r| r.contacts).sum();
        let contact_resets: usize = runs.iter().map(|r| r.contact_resets).sum();
        let other: usize = runs.iter().map(|r| r.resets.len() - r.contact_resets).sum();
        let stalls: u32 = runs.iter().map(|r| r.stalls).sum();
        let longest = runs.iter().map(|r| r.longest_stall).fold(0.0, f32::max);
        let stuck = runs.iter().filter(|r| r.longest_stall >= STUCK_FOR_GOOD).count();
        let pass = stuck == 0 && laps >= min_laps && other * 100 <= laps.max(1);
        ok &= pass;
        let _ = writeln!(
            out,
            "| {} | {} | {laps} | {contacts} | {contact_resets} | {other} | {stalls} | {longest:.1} | {stuck} | {} |",
            d.name(),
            runs.len(),
            if pass { "ok" } else { "**FAIL**" }
        );
    }
    for run in races.iter().flatten() {
        for r in &run.resets {
            let _ = writeln!(
                out,
                "\n- reset: {} {} at {:.0} m, {:.0} s: {:?}",
                run.car,
                run.difficulty.name(),
                r.at,
                r.time,
                r.reason
            );
        }
    }
    ok
}

/// Runs `jobs` on `threads` worker threads, results in job order.
pub fn parallel<J: Sync, R: Send>(
    jobs: &[J],
    threads: usize,
    work: impl Fn(&J) -> R + Sync,
) -> Vec<R> {
    let next = std::sync::atomic::AtomicUsize::new(0);
    let results: Vec<std::sync::Mutex<Option<R>>> =
        jobs.iter().map(|_| std::sync::Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..threads.max(1) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    let Some(job) = jobs.get(i) else { break };
                    *results[i].lock().unwrap() = Some(work(job));
                }
            });
        }
    });
    results
        .into_iter()
        .map(|r| r.into_inner().unwrap().expect("job ran"))
        .collect()
}

/// Lap-time targets relative to the optimal-line time, per difficulty:
/// (lowest, highest) mean flying lap.
pub fn target(d: Difficulty) -> (f32, f32) {
    match d {
        Difficulty::Alien => (0.98, 1.025),
        Difficulty::Expert => (1.02, 1.06),
        Difficulty::Pro => (1.06, 1.11),
        Difficulty::Amateur => (1.10, 1.17),
        Difficulty::Novice => (1.15, 1.25),
    }
}

/// Text report of solo runs, and whether every check passed.
pub fn report_solo(track: &Track, runs: &[CarRun], out: &mut String) -> bool {
    let mut ok = true;
    let _ = writeln!(
        out,
        "| Car | Difficulty | Laps | Resets | Stalls | Longest stall (s) | Mean flying lap (s) | Best (s) | Spread | Own plan (s) | vs optimal | Target |"
    );
    let _ = writeln!(out, "|---|---|---|---|---|---|---|---|---|---|---|---|");
    for car in CARS {
        let Some(profile) = track.car(car) else {
            continue;
        };
        let opt = profile.lap_time;
        let mut means = Vec::new();
        for run in runs.iter().filter(|r| r.car == car) {
            let f = run.flying();
            let mean = run.mean_flying();
            let best = f.iter().copied().fold(f32::MAX, f32::min);
            let worst = f.iter().copied().fold(0.0, f32::max);
            let ratio = mean / opt;
            let (lo, hi) = target(run.difficulty);
            let pass = (lo..=hi).contains(&ratio)
                && run.longest_stall < STUCK_FOR_GOOD
                && run.resets.is_empty();
            ok &= pass;
            means.push((run.difficulty, mean));
            for r in &run.resets {
                eprintln!(
                    "reset: {car} {} at {:.0} m, t {:.0} s: {:?}",
                    run.difficulty.name(),
                    r.at,
                    r.time,
                    r.reason
                );
            }
            let _ = writeln!(
                out,
                "| {car} | {} | {} | {} | {} | {:.1} | {mean:.2} | {best:.2} | {:.2} | {:.2} | {:+.1} % | {:+.0}..{:+.0} % {} |",
                run.difficulty.name(),
                run.laps.len(),
                run.resets.len(),
                run.stalls,
                run.longest_stall,
                worst - best,
                run.planned,
                (ratio - 1.0) * 100.0,
                (lo - 1.0) * 100.0,
                (hi - 1.0) * 100.0,
                if pass { "ok" } else { "**FAIL**" }
            );
        }
        means.sort_by_key(|m| m.0);
        let monotonic = means.windows(2).all(|w| w[0].1 > w[1].1);
        ok &= monotonic;
        let _ = writeln!(
            out,
            "| {car} | optimal line | | | | | {opt:.2} | | | | | {} |",
            if monotonic {
                "ordered ok"
            } else {
                "**NOT ORDERED**"
            }
        );
    }
    ok
}
