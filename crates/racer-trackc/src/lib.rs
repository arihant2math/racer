//! Track compiler: reads `data/tracks/<id>/track.toml`, validates it, builds
//! the road, curb, apron and wall geometry, and generates the racing line and
//! per-car speed profiles stored in `data/tracks/<id>/generated/`.
//!
//! The game builds geometry from the data at load time (it is cheap and
//! deterministic) and reads the racing line and car models from the
//! generated files, which `racer-trackc compile` writes and a test keeps up
//! to date.

pub mod data;
pub mod geometry;
pub mod line;
pub mod profile;

use std::{
    fmt::Write as _,
    path::{Path, PathBuf},
};

use serde::Deserialize;

pub use data::TrackFile;
pub use geometry::{Geometry, Pose, Sample};
pub use line::RacingLine;
pub use profile::{CarModel, Limits};

/// Track ids shipped in `data/tracks`.
pub const TRACKS: [&str; 1] = ["downtown-loop"];

/// `data/tracks` in the repository.
pub fn tracks_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/tracks")
}

/// A loaded track: geometry, racing line and the car models with their
/// full-limit speed profiles along the line.
#[derive(Clone, Debug)]
pub struct Track {
    pub geometry: Geometry,
    pub line: RacingLine,
    pub cars: Vec<CarProfile>,
}

#[derive(Clone, Debug)]
pub struct CarProfile {
    pub model: CarModel,
    /// Speed at each line point at the model's limit.
    pub speeds: Vec<f32>,
    pub lap_time: f32,
}

impl Track {
    pub fn car(&self, id: &str) -> Option<&CarProfile> {
        self.cars.iter().find(|c| c.model.id == id)
    }
}

/// Reads and validates a track file, then builds its geometry.
pub fn build(dir: &Path) -> Result<Geometry, String> {
    let file = TrackFile::load(&dir.join("track.toml"))?;
    let errors = geometry::validate(&file);
    if !errors.is_empty() {
        return Err(format!("{}: {}", dir.display(), errors.join("; ")));
    }
    Ok(geometry::build(&file))
}

/// Computes the racing line and every car's profile (runs the testbed).
pub fn generate(geometry: &Geometry) -> Result<(RacingLine, Vec<CarProfile>), String> {
    let line = line::minimum_curvature(geometry, line::CAR_HALF_WIDTH);
    let cars = racer_dynamics::spec::CARS
        .iter()
        .map(|id| {
            let model = CarModel::from_spec(&racer_dynamics::load_car(id)?)?;
            Ok(profile_for(&line, model))
        })
        .collect::<Result<Vec<_>, String>>()?;
    Ok((line, cars))
}

fn profile_for(line: &RacingLine, model: CarModel) -> CarProfile {
    let speeds = profile::speed_profile(line, &model, Limits::FULL);
    let lap_time = profile::lap_time(line, &speeds);
    CarProfile {
        model,
        speeds,
        lap_time,
    }
}

/// Loads `data/tracks/<id>` with its generated line and car models.
pub fn load(id: &str) -> Result<Track, String> {
    load_dir(&tracks_dir().join(id))
}

pub fn load_dir(dir: &Path) -> Result<Track, String> {
    let geometry = build(dir)?;
    let gen_dir = dir.join("generated");
    let read = |name: &str| {
        let path = gen_dir.join(name);
        std::fs::read_to_string(&path).map_err(|e| {
            format!(
                "{}: {e} (run `racer-trackc compile {}`)",
                path.display(),
                dir.display()
            )
        })
    };
    let offsets = parse_column(&read("racing-line.csv")?, 1)?;
    if offsets.len() != geometry.samples.len() {
        return Err(format!(
            "{}: racing line has {} points for {} samples; run `racer-trackc compile`",
            dir.display(),
            offsets.len(),
            geometry.samples.len()
        ));
    }
    let line = line::from_offsets(&geometry, &offsets);
    let summary: Summary =
        toml::from_str(&read("summary.toml")?).map_err(|e| format!("summary.toml: {e}"))?;
    let cars = summary
        .car
        .into_iter()
        .map(|model| profile_for(&line, model))
        .collect();
    Ok(Track {
        geometry,
        line,
        cars,
    })
}

fn parse_column(csv: &str, column: usize) -> Result<Vec<f32>, String> {
    csv.lines()
        .skip(1)
        .map(|l| {
            l.split(',')
                .nth(column)
                .and_then(|v| v.trim().parse().ok())
                .ok_or_else(|| format!("bad CSV line {l:?}"))
        })
        .collect()
}

#[derive(Deserialize)]
struct Summary {
    car: Vec<CarModel>,
}

impl<'de> Deserialize<'de> for CarModel {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            id: String,
            mass: f32,
            lateral: f32,
            braking: f32,
            traction: f32,
            drag: f32,
            lift: f32,
            top_speed: f32,
            drive: Vec<f32>,
            #[allow(dead_code)]
            lap_time: f32,
        }
        let r = Raw::deserialize(d)?;
        Ok(CarModel {
            id: r.id,
            mass: r.mass,
            lateral: r.lateral,
            braking: r.braking,
            traction: r.traction,
            drag: r.drag,
            lift: r.lift,
            top_speed: r.top_speed,
            drive: r.drive,
        })
    }
}

/// Writes `generated/` for the track in `dir`; returns the files written.
pub fn compile(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let geometry = build(dir)?;
    let (line, cars) = generate(&geometry)?;
    let out = dir.join("generated");
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut written = Vec::new();
    let mut write = |name: &str, text: String| {
        let path = out.join(name);
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        written.push(path);
        Ok::<_, String>(())
    };
    let mut csv = String::from("s,offset,x,y,z,distance,curvature\n");
    for p in &line.points {
        writeln!(
            csv,
            "{:.3},{:.4},{:.3},{:.3},{:.3},{:.3},{:.6}",
            p.s, p.offset, p.pos.x, p.pos.y, p.pos.z, p.distance, p.curvature
        )
        .unwrap();
    }
    write("racing-line.csv", csv)?;
    let mut summary = format!(
        "# Generated by `racer-trackc compile`; do not edit.\n\
         # Car models: point-mass limits measured on the racer-dynamics testbed\n\
         # (accelerations in m/s^2, drag and lift per (m/s)^2, drive per m/s from 0).\n\
         id = \"{}\"\nlength_m = {:.2}\nline_length_m = {:.2}\nsamples = {}\ncheckpoints = {}\n",
        geometry.id,
        geometry.length,
        line.length,
        geometry.samples.len(),
        geometry.checkpoints.len()
    );
    for car in &cars {
        let m = &car.model;
        let drive: Vec<String> = m.drive.iter().map(|a| format!("{a:.4}")).collect();
        write!(
            summary,
            "\n[[car]]\nid = \"{}\"\nlap_time = {:.3}\nmass = {:.2}\nlateral = {:.4}\nbraking = {:.4}\n\
             traction = {:.4}\ndrag = {:.7}\nlift = {:.7}\ntop_speed = {:.2}\ndrive = [{}]\n",
            m.id,
            car.lap_time,
            m.mass,
            m.lateral,
            m.braking,
            m.traction,
            m.drag,
            m.lift,
            m.top_speed,
            drive.join(", ")
        )
        .unwrap();
        let mut csv = String::from("distance,speed\n");
        for (p, v) in line.points.iter().zip(&car.speeds) {
            writeln!(csv, "{:.3},{v:.3}", p.distance).unwrap();
        }
        write(&format!("speed-{}.csv", m.id), csv)?;
    }
    write("summary.toml", summary)?;
    Ok(written)
}
