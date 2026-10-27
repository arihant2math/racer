//! racer-trackc: validate track data and generate its racing line and
//! speed profiles.
//!
//! ```text
//! racer-trackc validate <track-dir>...
//! racer-trackc compile <track-dir>...
//! ```

use std::{path::Path, process::ExitCode};

use racer_trackc::{TrackFile, geometry};

const USAGE: &str = "usage: racer-trackc <validate|compile> <track-dir>...";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.split_first() {
        Some((flag, [])) if flag == "--version" => {
            println!("racer-trackc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some((cmd, dirs)) if (cmd == "validate" || cmd == "compile") && !dirs.is_empty() => {
            let mut ok = true;
            for dir in dirs {
                let result = if cmd == "validate" {
                    validate(Path::new(dir))
                } else {
                    compile(Path::new(dir))
                };
                if let Err(e) = result {
                    eprintln!("racer-trackc: {e}");
                    ok = false;
                }
            }
            if ok {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn validate(dir: &Path) -> Result<(), String> {
    let file = TrackFile::load(&dir.join("track.toml"))?;
    let errors = geometry::validate(&file);
    if !errors.is_empty() {
        let list: Vec<String> = errors.iter().map(|e| format!("  {e}")).collect();
        return Err(format!(
            "{}: {} problem(s)\n{}",
            dir.display(),
            errors.len(),
            list.join("\n")
        ));
    }
    let g = geometry::build(&file);
    let triangles = g.road.triangle_count()
        + g.aprons.triangle_count()
        + g.walls.triangle_count()
        + g.curbs.iter().map(|c| c.triangle_count()).sum::<usize>();
    println!(
        "ok   {}: {} ({:.0} m, {} nodes, {} checkpoints, {} sectors, {} grid slots, {} triangles, {} buildings)",
        dir.display(),
        g.name,
        g.length,
        file.nodes.len(),
        g.checkpoints.len(),
        g.sector_ends.len() + 1,
        g.grid.len(),
        triangles,
        g.buildings.len()
    );
    Ok(())
}

fn compile(dir: &Path) -> Result<(), String> {
    validate(dir)?;
    for path in racer_trackc::compile(dir)? {
        println!("wrote {}", path.display());
    }
    let track = racer_trackc::load_dir(dir)?;
    println!("racing line {:.0} m", track.line.length);
    for car in &track.cars {
        let m = &car.model;
        println!(
            "  {:<28} optimal lap {:6.2} s  (lateral {:.2} g, braking {:.2} g, top {:.0} km/h)",
            m.id,
            car.lap_time,
            m.lateral / 9.81,
            m.braking / 9.81,
            m.top_speed * 3.6
        );
    }
    Ok(())
}
