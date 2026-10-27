//! racer-capture: render scenes to PNGs and check them.
//!
//! ```text
//! racer-capture scene <name> <out-dir> [--size WxH] [--timeout SECS]
//! racer-capture check <png>...
//! racer-capture diff <actual> <golden> [--pixel N] [--max-fraction F] [--max-mean E]
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
    scenes,
};

const USAGE: &str = "usage:
  racer-capture scene <name> <out-dir> [--size WxH] [--timeout SECS]
  racer-capture check <png>...
  racer-capture diff <actual> <golden> [--pixel N] [--max-fraction F] [--max-mean E]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("scene") => scene(&args[1..]),
        Some("check") => check_files(&args[1..]),
        Some("diff") => diff(&args[1..]),
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

fn scene(args: &[String]) -> Result<ExitCode, String> {
    let (pos, flags) = parse(args, &["size", "timeout"])?;
    let [name, out_dir] = pos[..] else {
        return Err(USAGE.into());
    };
    let size: String = flag(&flags, "size", "1280x720".to_string())?;
    let (w, h) = size
        .split_once('x')
        .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        .ok_or_else(|| format!("invalid --size {size}"))?;
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
    let poses = scenes::add_scene(&mut app, name).ok_or_else(|| {
        format!(
            "unknown scene {name:?}; known: {}",
            scenes::SCENES.join(", ")
        )
    })?;
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

fn diff(args: &[String]) -> Result<ExitCode, String> {
    let (pos, flags) = parse(args, &["pixel", "max-fraction", "max-mean"])?;
    let [actual, golden] = pos[..] else {
        return Err(USAGE.into());
    };
    let d = Tolerance::default();
    let tol = Tolerance {
        pixel: flag(&flags, "pixel", d.pixel)?,
        max_differing_fraction: flag(&flags, "max-fraction", d.max_differing_fraction)?,
        max_mean_error: flag(&flags, "max-mean", d.max_mean_error)?,
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
