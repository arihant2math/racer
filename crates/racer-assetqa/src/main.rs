//! racer-assetqa: quality gate for generated car assets.
//!
//! ```text
//! racer-assetqa check <car-id> [--glb PATH] [--out DIR] [--root DIR] [--samples N]
//!                              [--no-review] [--reuse-review] [--no-smoke] [--smoke-timeout SECS]
//! racer-assetqa refs <car-id> [--out DIR] [--root DIR]
//! racer-assetqa fixture <out-dir> [--car <car-id>] [--root DIR]
//! racer-assetqa smoke <glb> <out-dir> [--timeout SECS]
//! ```
//! `check` writes `report.md`, `report.json` and images into the output
//! directory (default `target/assetqa/<car-id>`) and exits 0 when no gate
//! fails (verdict PASS, or REVIEW while the review pack awaits its reviewer),
//! 1 when a gate fails and 2 on bad usage.

use std::{
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

use racer_assetqa::{
    fixture,
    model::Model,
    refs::{self, RefsFile},
    report::{Report, Status},
    review, smoke,
    spec::CarSpec,
    static_gates,
    thresholds::Thresholds,
};

const USAGE: &str = "usage:
  racer-assetqa check <car-id> [--glb PATH] [--out DIR] [--root DIR] [--samples N]
                               [--no-review] [--reuse-review] [--no-smoke] [--smoke-timeout SECS]
  racer-assetqa refs <car-id> [--out DIR] [--root DIR]
  racer-assetqa fixture <out-dir> [--car <car-id>] [--root DIR]
  racer-assetqa smoke <glb> <out-dir> [--timeout SECS]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("check") => check(&args[1..]),
        Some("refs") => refs_cmd(&args[1..]),
        Some("fixture") => fixture_cmd(&args[1..]),
        Some("smoke") => smoke_cmd(&args[1..]),
        _ => Err(USAGE.into()),
    };
    match result {
        Ok(code) => code,
        Err(e) => {
            eprintln!("racer-assetqa: {e}");
            ExitCode::from(2)
        }
    }
}

struct Args<'a> {
    positional: Vec<&'a str>,
    flags: Vec<(&'a str, Option<&'a str>)>,
}

impl Args<'_> {
    fn get(&self, name: &str) -> Option<&str> {
        self.flags.iter().find(|f| f.0 == name).and_then(|f| f.1)
    }
    fn has(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f.0 == name)
    }
    fn parse_or<T: std::str::FromStr>(&self, name: &str, default: T) -> Result<T, String> {
        match self.get(name) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("invalid --{name} {v}")),
        }
    }
}

/// `valued` flags take a value; `switches` don't.
fn parse<'a>(args: &'a [String], valued: &[&str], switches: &[&str]) -> Result<Args<'a>, String> {
    let mut out = Args {
        positional: Vec::new(),
        flags: Vec::new(),
    };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if let Some(f) = a.strip_prefix("--") {
            if valued.contains(&f) {
                let v = it.next().ok_or_else(|| format!("--{f} needs a value"))?;
                out.flags.push((f, Some(v)));
            } else if switches.contains(&f) {
                out.flags.push((f, None));
            } else {
                return Err(format!("unknown flag --{f}\n{USAGE}"));
            }
        } else {
            out.positional.push(a);
        }
    }
    Ok(out)
}

/// The repository root: `--root`, else the nearest ancestor of the current
/// directory holding `data/cars`, else this crate's workspace.
fn root(args: &Args) -> PathBuf {
    if let Some(r) = args.get("root") {
        return PathBuf::from(r);
    }
    if let Ok(cwd) = std::env::current_dir() {
        for dir in cwd.ancestors() {
            if dir.join("data/cars").is_dir() {
                return dir.to_path_buf();
            }
        }
    }
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn load_refs(
    dir: &Path,
    spec: &CarSpec,
) -> Result<(Option<RefsFile>, Vec<refs::Prepared>), String> {
    let path = dir.join("refs.toml");
    if !path.exists() {
        return Ok((None, Vec::new()));
    }
    let file = RefsFile::load(&path)?;
    let mut prepared = Vec::new();
    for sheet in &file.sheets {
        prepared.extend(refs::prepare_sheet(sheet, dir, spec)?);
    }
    Ok((Some(file), prepared))
}

fn check(args: &[String]) -> Result<ExitCode, String> {
    let a = parse(
        args,
        &["glb", "out", "root", "samples", "smoke-timeout"],
        &["no-review", "reuse-review", "no-smoke"],
    )?;
    let [car] = a.positional[..] else {
        return Err(USAGE.into());
    };
    let root = root(&a);
    let spec = CarSpec::load(&root.join(format!("data/cars/{car}/car.toml")))?;
    let glb = a
        .get("glb")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(format!("assets/cars/{car}/car.glb")));
    let out = a
        .get("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(format!("target/assetqa/{car}")));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let t = Thresholds::default();

    let model = Model::load(&glb)?;
    let refs_dir = root.join(format!("data/cars/{car}/refs"));
    let (refs_file, prepared) = load_refs(&refs_dir, &spec)?;
    eprintln!("racer-assetqa: gates 1-4");
    let mut gates = static_gates(&model, &spec, &prepared, &t, Some(&out));

    if a.has("no-review") {
        gates.push(racer_assetqa::report::Gate::skipped(
            5,
            "Visual review pack",
            "disabled (--no-review)",
        ));
    } else {
        eprintln!("racer-assetqa: gate 5 (Blender review renders)");
        gates.push(review::run(&review::ReviewOptions {
            root: &root,
            glb: &glb,
            refs_dir: &refs_dir,
            refs: refs_file.as_ref(),
            out_dir: &out,
            samples: a.parse_or("samples", 48)?,
            reuse: a.has("reuse-review"),
        }));
    }
    if a.has("no-smoke") {
        gates.push(racer_assetqa::report::Gate::skipped(
            6,
            "In-engine smoke test (Bevy, lavapipe)",
            "disabled (--no-smoke)",
        ));
    } else {
        eprintln!("racer-assetqa: gate 6 (Bevy smoke test)");
        let timeout = Duration::from_secs(a.parse_or("smoke-timeout", 900)?);
        gates.push(smoke::run(&glb, &out, &t, timeout));
    }

    let report = Report {
        car_id: spec.id.clone(),
        car_name: spec.name.clone(),
        glb: glb.display().to_string(),
        glb_bytes: std::fs::metadata(&glb).map(|m| m.len()).unwrap_or(0),
        generator: model.generator.clone(),
        verdict: Report::verdict_of(&gates),
        gates,
        load_warnings: model.warnings.clone(),
    };
    std::fs::write(out.join("report.md"), report.to_markdown()).map_err(|e| e.to_string())?;
    std::fs::write(out.join("report.json"), report.to_json()).map_err(|e| e.to_string())?;
    for g in &report.gates {
        let failed: Vec<&str> = g.failed().iter().map(|c| c.id.as_str()).collect();
        println!(
            "{:<6} {}. {} {}",
            g.status.label(),
            g.number,
            g.name,
            failed.join(" ")
        );
    }
    println!(
        "verdict: {} (report: {})",
        report.verdict.label(),
        out.join("report.md").display()
    );
    Ok(if report.verdict == Status::Fail {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

/// Prepares every reference view, writes its mask and prints how well the
/// sheets agree with each other (the floor for the silhouette thresholds).
fn refs_cmd(args: &[String]) -> Result<ExitCode, String> {
    let a = parse(args, &["out", "root"], &[])?;
    let [car] = a.positional[..] else {
        return Err(USAGE.into());
    };
    let root = root(&a);
    let spec = CarSpec::load(&root.join(format!("data/cars/{car}/car.toml")))?;
    let dir = root.join(format!("data/cars/{car}/refs"));
    let out = a
        .get("out")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join(format!("target/assetqa/{car}/refs")));
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    let (_, prepared) = load_refs(&dir, &spec)?;
    for p in &prepared {
        let (x0, y0, x1, y1) = p.mask.bbox().unwrap();
        let w = (x1 - x0 + 1) as f32 * p.grid.mpp;
        let h = (y1 - y0 + 1) as f32 * p.grid.mpp;
        let lo = p.grid.to_metric(x0 as f32, y1 as f32 + 1.0);
        let hi = p.grid.to_metric(x1 as f32 + 1.0, y0 as f32);
        println!(
            "{:<28} {:<6} {:.2} mm/px  extent {w:.3} x {h:.3} m  u {:.3}..{:.3}  v {:.3}..{:.3}",
            p.sheet,
            p.view.name(),
            p.grid.mpp * 1000.0,
            lo.x.min(hi.x),
            lo.x.max(hi.x),
            lo.y,
            hi.y
        );
        let name = format!("mask-{}-{}.png", p.sheet, p.view.name());
        p.mask
            .to_image()
            .save(out.join(name))
            .map_err(|e| e.to_string())?;
    }
    for (i, a) in prepared.iter().enumerate() {
        for b in &prepared[i + 1..] {
            if a.view == b.view {
                let (iou, d) = refs::compare_refs(a, b);
                println!(
                    "agreement {} vs {} {}: IoU {iou:.3}, outline mean {:.1} mm, p95 {:.1} mm",
                    a.sheet,
                    b.sheet,
                    a.view.name(),
                    d.mean * 1000.0,
                    d.p95 * 1000.0
                );
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn fixture_cmd(args: &[String]) -> Result<ExitCode, String> {
    let a = parse(args, &["car", "root"], &[])?;
    let [out] = a.positional[..] else {
        return Err(USAGE.into());
    };
    let root = root(&a);
    let car = a.get("car").unwrap_or("porsche-911-carrera-s-992");
    let spec = CarSpec::load(&root.join(format!("data/cars/{car}/car.toml")))?;
    let out = PathBuf::from(out);
    std::fs::create_dir_all(&out).map_err(|e| e.to_string())?;
    for f in fixture::Fault::ALL {
        let path = out.join(format!("box-car-{}.glb", f.name()));
        std::fs::write(&path, fixture::box_car(&spec, f)).map_err(|e| e.to_string())?;
        println!("{}", path.display());
    }
    Ok(ExitCode::SUCCESS)
}

fn smoke_cmd(args: &[String]) -> Result<ExitCode, String> {
    let a = parse(args, &["timeout"], &[])?;
    let [glb, out] = a.positional[..] else {
        return Err(USAGE.into());
    };
    let timeout = Duration::from_secs(a.parse_or("timeout", 900)?);
    smoke::app_main(Path::new(glb), Path::new(out), timeout)?;
    Ok(ExitCode::SUCCESS)
}
