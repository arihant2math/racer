//! `data/cars/<id>/car.toml`: published specs with sources, estimated
//! parameters (kept apart under `[estimated]`) and reference performance
//! figures used by the maneuver tests.

use std::path::{Path, PathBuf};

use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CarSpec {
    pub id: String,
    pub name: String,
    /// Which variant the numbers describe (market, power level, gearbox).
    pub variant: String,
    pub chassis: Chassis,
    pub engine: Engine,
    pub gearbox: Gearbox,
    pub driveline: Driveline,
    pub brakes: Brakes,
    pub steering: Steering,
    pub tires: Tires,
    pub aero: Aero,
    /// Values that are not published, estimated from class norms and fitted
    /// to the reference figures.
    pub estimated: Estimated,
    pub reference: Reference,
}

// ---- published ----

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Chassis {
    pub sources: Vec<String>,
    /// Curb mass as published; `mass_basis` says which convention.
    pub curb_mass_kg: f64,
    pub mass_basis: String,
    pub front_weight_fraction: f64,
    pub length_m: f64,
    pub width_m: f64,
    pub height_m: f64,
    pub wheelbase_m: f64,
    pub track_front_m: f64,
    pub track_rear_m: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Engine {
    pub sources: Vec<String>,
    pub description: String,
    pub peak_power_kw: f64,
    pub peak_power_rpm: [f64; 2],
    pub peak_torque_nm: f64,
    pub peak_torque_rpm: [f64; 2],
    /// Rev limiter (or published redline when the limiter is not published).
    pub limiter_rpm: f64,
    pub top_speed_kmh: f64,
    /// Electronically limited top speed.
    pub top_speed_governed: bool,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gearbox {
    pub sources: Vec<String>,
    pub description: String,
    /// Forward gear ratios, first gear first.
    pub ratios: Vec<f64>,
    pub reverse: f64,
    /// One final drive for every gear, or one per forward gear (dual-clutch
    /// boxes with two output shafts).
    pub final_drive: Vec<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Fwd,
    Rwd,
    Awd,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Driveline {
    pub sources: Vec<String>,
    pub layout: Layout,
    pub description: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Brakes {
    pub sources: Vec<String>,
    pub front_disc_mm: f64,
    pub rear_disc_mm: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Steering {
    pub sources: Vec<String>,
    /// Steering-wheel turns lock to lock.
    pub lock_to_lock_turns: f64,
    /// Overall steering ratio (on-centre for variable-ratio racks).
    pub ratio: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tires {
    pub sources: Vec<String>,
    /// ISO size, e.g. "235/35 R19".
    pub front: String,
    pub rear: String,
    pub model: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Aero {
    pub sources: Vec<String>,
    pub drag_coefficient: f64,
}

// ---- estimated ----

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Estimated {
    /// How the estimates were made.
    pub notes: String,
    pub chassis: ChassisFit,
    pub engine: EngineFit,
    pub gearbox: GearboxFit,
    pub differentials: DiffFit,
    pub brakes: BrakesFit,
    pub steering: SteeringFit,
    pub aero: AeroFit,
    pub suspension: Axles<AxleFit>,
    pub tires: Axles<TireFit>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Axles<T> {
    pub front: T,
    pub rear: T,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChassisFit {
    /// Added to the curb mass: driver and test equipment not in `mass_basis`.
    pub payload_kg: f64,
    pub cg_height_m: f64,
    /// Radii of gyration (m) about the roll, pitch and yaw axes.
    pub gyration_roll_m: f64,
    pub gyration_pitch_m: f64,
    pub gyration_yaw_m: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EngineFit {
    /// Full-throttle torque curve `[rpm, Nm]`, shaped to the published peaks.
    pub torque_curve: Vec<[f64; 2]>,
    pub idle_rpm: f64,
    /// Engine speed the launch control / auto clutch holds while slipping.
    pub launch_rpm: f64,
    /// Crank, flywheel and clutch inertia.
    pub inertia_kgm2: f64,
    /// Closed-throttle drag torque at idle and at the limiter.
    pub drag_nm: [f64; 2],
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GearboxFit {
    pub shift_time_s: f64,
    /// Automatic-mode upshift engine speed at full and at light throttle.
    pub upshift_rpm: [f64; 2],
    pub downshift_rpm: f64,
    pub clutch_max_torque_nm: f64,
    /// Driveline efficiency, engine to wheels.
    pub efficiency: f64,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffSpec {
    pub kind: DiffKind,
    /// Locking torque at zero input torque (LSD only).
    #[serde(default)]
    pub preload_nm: f64,
    /// Extra locking torque per Nm of input torque, on power and on coast.
    #[serde(default)]
    pub power_ramp: f64,
    #[serde(default)]
    pub coast_ramp: f64,
    /// Share of torque sent to the front axle (centre differential only).
    #[serde(default)]
    pub front_split: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiffKind {
    Open,
    Lsd,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiffFit {
    pub front: Option<DiffSpec>,
    pub rear: Option<DiffSpec>,
    pub center: Option<DiffSpec>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrakesFit {
    /// Maximum brake torque per wheel.
    pub front_torque_nm: f64,
    pub rear_torque_nm: f64,
    pub handbrake_torque_nm: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SteeringFit {
    /// 0 = parallel steer, 1 = full Ackermann.
    pub ackermann: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AeroFit {
    pub frontal_area_m2: f64,
    /// Lift coefficient times area per axle (negative = downforce).
    pub lift_area_front_m2: f64,
    pub lift_area_rear_m2: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AxleFit {
    /// Heave natural frequency of the sprung corner.
    pub ride_frequency_hz: f64,
    /// Damping ratios in bump and rebound.
    pub damping_ratio_bump: f64,
    pub damping_ratio_rebound: f64,
    /// Wheel travel from static ride height to the bump stop, and down to full droop.
    pub bump_travel_m: f64,
    pub droop_travel_m: f64,
    /// Anti-roll bar rate at the wheel (N per m of left/right travel difference).
    pub anti_roll_n_per_m: f64,
    pub roll_center_height_m: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TireFit {
    /// Peak friction coefficients at the reference load.
    pub mu_long: f64,
    pub mu_lat: f64,
    pub reference_load_n: f64,
    /// Fractional loss of peak mu per reference load of extra load.
    pub load_sensitivity: f64,
    pub peak_slip_ratio: f64,
    pub peak_slip_angle_deg: f64,
    /// Magic Formula shape (C) and curvature (E) factors.
    pub shape_long: f64,
    pub shape_lat: f64,
    pub curvature: f64,
    pub relaxation_long_m: f64,
    pub relaxation_lat_m: f64,
    pub rolling_resistance: f64,
    /// Pneumatic trail at zero slip (self-aligning torque).
    pub pneumatic_trail_m: f64,
    /// Wheel, tire, hub and brake rotor inertia.
    pub inertia_kgm2: f64,
}

// ---- reference performance ----

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Figure {
    pub value: f64,
    pub source: String,
    #[serde(default)]
    pub note: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub zero_to_100_s: Figure,
    pub braking_100_to_0_m: Figure,
    pub skidpad_g: Figure,
    pub top_speed_kmh: Figure,
}

impl CarSpec {
    pub fn parse(text: &str) -> Result<Self, String> {
        toml::from_str(text).map_err(|e| e.to_string())
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Car ids shipped in `data/cars`.
pub const CARS: [&str; 3] = [
    "vw-golf-gti-mk8",
    "bmw-m3-competition-g80",
    "porsche-911-carrera-s-992",
];

/// `data/cars` (see `racer_data::dir` for where `data` is found).
pub fn cars_dir() -> PathBuf {
    racer_data::dir().join("cars")
}

/// Loads `data/cars/<id>/car.toml`.
pub fn load_car(id: &str) -> Result<CarSpec, String> {
    CarSpec::load(&cars_dir().join(id).join("car.toml"))
}

/// Tire radius (m) from an ISO size such as "235/35 R19" or "305/30 ZR21".
pub fn tire_radius(size: &str) -> Result<f64, String> {
    let bad = || format!("bad tire size {size:?}");
    let (width, rest) = size.split_once('/').ok_or_else(bad)?;
    let (aspect, rim) = rest.split_once('R').ok_or_else(bad)?;
    let width: f64 = width.trim().parse().map_err(|_| bad())?;
    let aspect: f64 = aspect
        .trim()
        .trim_end_matches('Z')
        .trim()
        .parse()
        .map_err(|_| bad())?;
    let rim: f64 = rim.trim().parse().map_err(|_| bad())?;
    Ok((rim * 25.4 / 2.0 + width * aspect / 100.0) / 1000.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tire_radius_from_iso_size() {
        assert!((tire_radius("225/40 R18").unwrap() - 0.3186).abs() < 1e-4);
        assert!((tire_radius("305/30 ZR21").unwrap() - 0.3582).abs() < 1e-4);
        assert!(tire_radius("225-40-18").is_err());
    }
}
