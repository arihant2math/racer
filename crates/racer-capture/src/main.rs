//! racer-capture: render scenes to PNGs and check them.
//!
//! ```text
//! racer-capture scene <name> <out-dir> [--size WxH] [--timeout SECS]
//! racer-capture check <png>...
//! racer-capture diff <actual> <golden> [--pixel N] [--max-fraction F] [--max-mean E]
//! racer-capture telemetry <out-dir> [car-id...]
//! racer-capture ai-laps <out-dir> [--laps N] [--car ID]
//! racer-capture ai-races <out-dir> [--races N] [--laps N] [--first I]
//! ```
//! `diff` takes two files or two directories; with directories every PNG in the
//! golden directory must have a matching file in the actual directory.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use bevy::{prelude::*, window::WindowResolution, winit::WinitSettings};
use racer_capture::{
    check::{self, Tolerance},
    harness::CapturePlugin,
    race, scenes, telemetry,
};

const USAGE: &str = "usage:
  racer-capture scene <name> <out-dir> [--size WxH] [--timeout SECS]
  racer-capture check <png>...
  racer-capture diff <actual> <golden> [--pixel N] [--max-fraction F] [--max-mean E]
  racer-capture telemetry <out-dir> [car-id...]
  racer-capture ai-laps <out-dir> [--laps N] [--car ID]
  racer-capture ai-races <out-dir> [--races N] [--laps N] [--first I]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("scene") => scene(&args[1..]),
        Some("check") => check_files(&args[1..]),
        Some("diff") => diff(&args[1..]),
        Some("telemetry") => telemetry(&args[1..]),
        Some("ai-laps") => ai_laps(&args[1..]),
        Some("ai-races") => ai_races(&args[1..]),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("racer-capture: {e}");
            ExitCode::from(2)
        }
    }
}

/// Positional arguments and `--flag value` pairs.
type Parsed<'a> = (Vec<&'a str>, Vec<(&'a str, &'a str)>);

/// Splits `args` into positional arguments and `--flag value` pairs, rejecting
/// flags not in `known`.
fn parse<'a>(args: &'a [String], known: &[&str]) -> Result<Parsed<'a>, String> {
    let (mut positional, mut flags) = (Vec::new(), Vec::new());
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if let Some(flag) = arg.strip_prefix("--") {
            if !known.contains(&flag) {
                return Err(format!("unknown flag --{flag}\n{USAGE}"));
            }
            let value = it.next().ok_or_else(|| format!("--{flag} needs a value"))?;
            flags.push((flag, value.as_str()));
        } else {
            positional.push(arg.as_str());
        }
    }
    Ok((positional, flags))
}

fn flag<T: std::str::FromStr>(flags: &[(&str, &str)], name: &str, default: T) -> Result<T, String> {
    match flags.iter().rev().find(|(f, _)| *f == name) {
        Some((_, v)) => v
            .parse()
            .map_err(|_| format!("invalid value for --{name}: {v}")),
        None => Ok(default),
    }
}

/// Largest capture width or height (wgpu's default 2D texture size limit).
const MAX_SIZE: u32 = 8192;

fn scene(args: &[String]) -> Result<ExitCode, String> {
    let (pos, flags) = parse(args, &["size", "timeout"])?;
    let [name, out_dir] = pos[..] else {
        return Err(USAGE.into());
    };
    // Everything is checked before the output directory, window and
    // renderer are created.
    if !scenes::SCENES.contains(&name) {
        return Err(format!(
            "unknown scene {name:?}; known: {}",
            scenes::SCENES.join(", ")
        ));
    }
    let size: String = flag(&flags, "size", "1280x720".to_string())?;
    let (w, h) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .filter(|&(w, h)| (1..=MAX_SIZE).contains(&w) && (1..=MAX_SIZE).contains(&h))
        .ok_or_else(|| format!("invalid --size {size} (WxH, each 1 to {MAX_SIZE})"))?;
    let timeout = Duration::from_secs(flag(&flags, "timeout", 600)?);
    let out_dir = PathBuf::from(out_dir);
    std::fs::create_dir_all(&out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;

    let mut app = App::new();
    app.insert_resource(WinitSettings::continuous())
        .add_plugins(DefaultPlugins.set(WindowPlugin {
            primary_window: Some(Window {
                title: format!("racer-capture: {name}"),
                resolution: WindowResolution::new(w, h).with_scale_factor_override(1.0),
                ..default()
            }),
            ..default()
        }));
    let poses = scenes::add_scene(&mut app, name).expect("scene name checked above");
    app.add_plugins(CapturePlugin {
        poses,
        out_dir,
        timeout,
    });
    Ok(match app.run() {
        AppExit::Success => ExitCode::SUCCESS,
        AppExit::Error(_) => ExitCode::FAILURE,
    })
}

fn load(path: &Path) -> Result<image::RgbaImage, String> {
    Ok(image::open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?
        .to_rgba8())
}

fn check_files(args: &[String]) -> Result<ExitCode, String> {
    if args.is_empty() {
        return Err(USAGE.into());
    }
    let mut ok = true;
    for path in args {
        match load(Path::new(path)).and_then(|img| check::check(&img)) {
            Ok(s) => println!(
                "ok   {path}: {}x{}, mean luma {:.1}, stddev {:.1}, dominant colour {:.1}%",
                s.width,
                s.height,
                s.mean_luma,
                s.luma_stddev,
                s.dominant_fraction * 100.0
            ),
            Err(reason) => {
                ok = false;
                println!("FAIL {path}: {reason}");
            }
        }
    }
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// A tolerance flag: a number in `0..=max` (NaN and negatives would fail
/// or pass every image).
fn tolerance(flags: &[(&str, &str)], name: &str, default: f64, max: f64) -> Result<f64, String> {
    let v: f64 = flag(flags, name, default)?;
    if (0.0..=max).contains(&v) {
        Ok(v)
    } else {
        Err(format!("--{name} must be between 0 and {max}, got {v}"))
    }
}

fn diff(args: &[String]) -> Result<ExitCode, String> {
    let (pos, flags) = parse(args, &["pixel", "max-fraction", "max-mean"])?;
    let [actual, golden] = pos[..] else {
        return Err(USAGE.into());
    };
    let d = Tolerance::default();
    let tol = Tolerance {
        pixel: flag(&flags, "pixel", d.pixel)?,
        max_differing_fraction: tolerance(&flags, "max-fraction", d.max_differing_fraction, 1.0)?,
        max_mean_error: tolerance(&flags, "max-mean", d.max_mean_error, 255.0)?,
    };
    let (actual, golden) = (Path::new(actual), Path::new(golden));
    let pairs = if golden.is_dir() {
        let mut names: Vec<_> = std::fs::read_dir(golden)
            .map_err(|e| format!("{}: {e}", golden.display()))?
            .filter_map(|e| e.ok().map(|e| e.file_name()))
            .filter(|n| Path::new(n).extension().is_some_and(|x| x == "png"))
            .collect();
        names.sort();
        if names.is_empty() {
            return Err(format!("no PNGs in {}", golden.display()));
        }
        names
            .into_iter()
            .map(|n| (actual.join(&n), golden.join(&n)))
            .collect()
    } else {
        vec![(actual.to_path_buf(), golden.to_path_buf())]
    };
    let mut ok = true;
    for (a, g) in pairs {
        match load(&a).and_then(|a| check::diff(&a, &load(&g)?, tol)) {
            Ok(r) => {
                ok &= r.passed;
                println!(
                    "{} {}: mean error {:.3}, differing pixels {:.3}%",
                    if r.passed { "ok  " } else { "FAIL" },
                    a.display(),
                    r.mean_error,
                    r.differing_fraction * 100.0
                );
            }
            Err(e) => {
                ok = false;
                println!("FAIL {}: {e}", a.display());
            }
        }
    }
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// Headless scripted maneuvers on the Avian test track, one CSV per car and maneuver.
fn telemetry(args: &[String]) -> Result<ExitCode, String> {
    let (pos, _) = parse(args, &[])?;
    let Some((out_dir, cars)) = pos.split_first() else {
        return Err(USAGE.into());
    };
    let cars: Vec<&str> = if cars.is_empty() {
        racer_dynamics::spec::CARS.to_vec()
    } else {
        cars.to_vec()
    };
    for car in cars {
        match telemetry::run(car, Path::new(out_dir)) {
            Ok(paths) => paths.iter().for_each(|p| println!("wrote {}", p.display())),
            Err(e) => {
                eprintln!("racer-capture: {e}");
                return Ok(ExitCode::FAILURE);
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Solo AI laps on the city track for every car (or one) and difficulty.
fn ai_laps(args: &[String]) -> Result<ExitCode, String> {
    let (pos, flags) = parse(args, &["laps", "car"])?;
    let [out_dir] = pos[..] else {
        return Err(USAGE.into());
    };
    let laps: u32 = flag(&flags, "laps", 100)?;
    let only: String = flag(&flags, "car", String::new())?;
    if laps == 0 {
        return Err("--laps must be at least 1".into());
    }
    if !only.is_empty() && !racer_dynamics::spec::CARS.contains(&only.as_str()) {
        return Err(format!(
            "unknown car {only:?}; known: {}",
            racer_dynamics::spec::CARS.join(", ")
        ));
    }
    let out_dir = Path::new(out_dir);
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let track = std::sync::Arc::new(racer_trackc::load("downtown-loop")?);
    let cars: Vec<&str> = racer_dynamics::spec::CARS
        .into_iter()
        .filter(|c| only.is_empty() || *c == only)
        .collect();
    let started = std::time::Instant::now();
    let jobs: Vec<(&str, racer_sim::ai::Difficulty)> = cars
        .iter()
        .flat_map(|&c| racer_sim::ai::Difficulty::ALL.map(|d| (c, d)))
        .collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let runs = race::parallel(&jobs, threads, |&(car, d)| {
        race::solo(track.clone(), car, d, laps)
    })
    .into_iter()
    .collect::<Result<Vec<_>, String>>()?;
    let mut report = String::new();
    let ok = race::report_solo(&track, &runs, laps as usize, &mut report);
    println!("{report}");
    println!("{} s wall time", started.elapsed().as_secs());
    for run in &runs {
        let mut csv = String::from(
            "distance,speed,target,trim,lateral_error,front_slip,rear_slip,throttle,brake\n",
        );
        for r in &run.trace {
            let row: Vec<String> = r.iter().map(|v| format!("{v:.3}")).collect();
            csv.push_str(&row.join(","));
            csv.push('\n');
        }
        let path = out_dir.join(format!(
            "trace-{}-{}.csv",
            run.car,
            run.difficulty.name().to_lowercase()
        ));
        std::fs::write(&path, csv).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    let path = out_dir.join("solo.md");
    std::fs::write(&path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

/// 8-car mixed AI races with full contact on the city track.
fn ai_races(args: &[String]) -> Result<ExitCode, String> {
    let (pos, flags) = parse(args, &["races", "laps", "first"])?;
    let [out_dir] = pos[..] else {
        return Err(USAGE.into());
    };
    let races: usize = flag(&flags, "races", 12)?;
    let laps: u32 = flag(&flags, "laps", 10)?;
    let first: usize = flag(&flags, "first", 0)?;
    if races == 0 || laps == 0 {
        return Err("--races and --laps must be at least 1".into());
    }
    // Race indices seed the grid shuffle, which multiplies them as u64.
    let end = first
        .checked_add(races)
        .filter(|&end| end <= u32::MAX as usize)
        .ok_or(format!("--first plus --races must be at most {}", u32::MAX))?;
    let out_dir = Path::new(out_dir);
    std::fs::create_dir_all(out_dir).map_err(|e| format!("{}: {e}", out_dir.display()))?;
    let track = std::sync::Arc::new(racer_trackc::load("downtown-loop")?);
    let started = std::time::Instant::now();
    let jobs: Vec<usize> = (first..end).collect();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());
    let results = race::parallel(&jobs, threads, |&i| race::mixed(track.clone(), i, laps))
        .into_iter()
        .collect::<Result<Vec<_>, String>>()?;
    let mut report = String::new();
    // Each difficulty fills about 8/5 of the grid; ask for 100 laps over a full run.
    let min_laps = if races.saturating_mul(laps as usize) >= 100 {
        100
    } else {
        0
    };
    let ok = race::report_mixed(&results, min_laps, &mut report);
    println!("{report}");
    println!(
        "{} races, {} s wall time",
        races,
        started.elapsed().as_secs()
    );
    let path = out_dir.join("mixed.md");
    std::fs::write(&path, &report).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
