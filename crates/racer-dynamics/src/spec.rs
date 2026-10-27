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
    /// Parses and [validates](Self::validate) a `car.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let spec: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        spec.validate()?;
        Ok(spec)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Field checks for [`CarSpec::validate`]; each error names the field.
fn positive(field: &str, v: f64) -> Result<(), String> {
    if v.is_finite() && v > 0.0 {
        Ok(())
    } else {
        Err(format!("{field} = {v}: must be positive and finite"))
    }
}

fn non_negative(field: &str, v: f64) -> Result<(), String> {
    if v.is_finite() && v >= 0.0 {
        Ok(())
    } else {
        Err(format!(
            "{field} = {v}: must be zero or positive and finite"
        ))
    }
}

fn finite(field: &str, v: f64) -> Result<(), String> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(format!("{field} = {v}: must be finite"))
    }
}

/// `lo < v < hi` (`lo < v <= hi` with `inclusive`).
fn between(field: &str, v: f64, lo: f64, hi: f64, inclusive: bool) -> Result<(), String> {
    if v > lo && (v < hi || inclusive && v == hi) {
        Ok(())
    } else {
        let close = if inclusive { "]" } else { ")" };
        Err(format!("{field} = {v}: must be in ({lo}, {hi}{close}"))
    }
}

/// `a < b`, both already checked finite.
fn ascending(fields: [&str; 2], a: f64, b: f64) -> Result<(), String> {
    if a < b {
        Ok(())
    } else {
        Err(format!(
            "{} = {a} must be below {} = {b}",
            fields[0], fields[1]
        ))
    }
}

impl CarSpec {
    /// Checks every number the model uses for range and finiteness, so a bad
    /// file fails at load time with the field's name instead of panicking or
    /// going NaN in [`Vehicle::step`](crate::Vehicle::step).
    pub fn validate(&self) -> Result<(), String> {
        let c = &self.chassis;
        let e = &self.estimated;
        finite("chassis.curb_mass_kg", c.curb_mass_kg)?;
        non_negative("estimated.chassis.payload_kg", e.chassis.payload_kg)?;
        positive(
            "chassis.curb_mass_kg + estimated.chassis.payload_kg",
            c.curb_mass_kg + e.chassis.payload_kg,
        )?;
        between(
            "chassis.front_weight_fraction",
            c.front_weight_fraction,
            0.0,
            1.0,
            false,
        )?;
        for (field, v) in [
            ("chassis.length_m", c.length_m),
            ("chassis.width_m", c.width_m),
            ("chassis.height_m", c.height_m),
            ("chassis.wheelbase_m", c.wheelbase_m),
            ("chassis.track_front_m", c.track_front_m),
            ("chassis.track_rear_m", c.track_rear_m),
            ("estimated.chassis.cg_height_m", e.chassis.cg_height_m),
            (
                "estimated.chassis.gyration_roll_m",
                e.chassis.gyration_roll_m,
            ),
            (
                "estimated.chassis.gyration_pitch_m",
                e.chassis.gyration_pitch_m,
            ),
            ("estimated.chassis.gyration_yaw_m", e.chassis.gyration_yaw_m),
        ] {
            positive(field, v)?;
        }

        // Engine: idle < launch < limiter, a torque curve sorted by rpm.
        let en = &e.engine;
        positive("estimated.engine.idle_rpm", en.idle_rpm)?;
        positive("estimated.engine.launch_rpm", en.launch_rpm)?;
        positive("engine.limiter_rpm", self.engine.limiter_rpm)?;
        ascending(
            ["estimated.engine.idle_rpm", "estimated.engine.launch_rpm"],
            en.idle_rpm,
            en.launch_rpm,
        )?;
        ascending(
            ["estimated.engine.launch_rpm", "engine.limiter_rpm"],
            en.launch_rpm,
            self.engine.limiter_rpm,
        )?;
        positive("estimated.engine.inertia_kgm2", en.inertia_kgm2)?;
        non_negative("estimated.engine.drag_nm[0]", en.drag_nm[0])?;
        non_negative("estimated.engine.drag_nm[1]", en.drag_nm[1])?;
        if en.torque_curve.is_empty() {
            return Err("estimated.engine.torque_curve is empty".into());
        }
        for (i, &[rpm, nm]) in en.torque_curve.iter().enumerate() {
            non_negative(&format!("estimated.engine.torque_curve[{i}] rpm"), rpm)?;
            non_negative(&format!("estimated.engine.torque_curve[{i}] torque"), nm)?;
        }
        if let Some(i) = en.torque_curve.windows(2).position(|w| w[1][0] <= w[0][0]) {
            return Err(format!(
                "estimated.engine.torque_curve: rpm must increase (point {} at {} rpm \
                 follows {} rpm)",
                i + 1,
                en.torque_curve[i + 1][0],
                en.torque_curve[i][0]
            ));
        }
        if self.engine.top_speed_governed {
            positive("engine.top_speed_kmh", self.engine.top_speed_kmh)?;
        }

        // Gearbox: 1..=127 forward gears, one final drive or one per gear.
        let gb = &self.gearbox;
        if gb.ratios.is_empty() || gb.ratios.len() > i8::MAX as usize {
            return Err(format!(
                "gearbox.ratios has {} gears: must have 1 to {}",
                gb.ratios.len(),
                i8::MAX
            ));
        }
        for (i, &r) in gb.ratios.iter().enumerate() {
            positive(&format!("gearbox.ratios[{i}]"), r)?;
        }
        positive("gearbox.reverse", gb.reverse)?;
        if gb.final_drive.len() != 1 && gb.final_drive.len() != gb.ratios.len() {
            return Err(format!(
                "gearbox.final_drive has {} entries: must have 1 or one per gear ({})",
                gb.final_drive.len(),
                gb.ratios.len()
            ));
        }
        for (i, &r) in gb.final_drive.iter().enumerate() {
            positive(&format!("gearbox.final_drive[{i}]"), r)?;
        }
        let gf = &e.gearbox;
        non_negative("estimated.gearbox.shift_time_s", gf.shift_time_s)?;
        positive("estimated.gearbox.upshift_rpm[0]", gf.upshift_rpm[0])?;
        positive("estimated.gearbox.upshift_rpm[1]", gf.upshift_rpm[1])?;
        positive("estimated.gearbox.downshift_rpm", gf.downshift_rpm)?;
        positive(
            "estimated.gearbox.clutch_max_torque_nm",
            gf.clutch_max_torque_nm,
        )?;
        between(
            "estimated.gearbox.efficiency",
            gf.efficiency,
            0.0,
            1.0,
            true,
        )?;

        // Differentials: an LSD needs some locking torque, a centre split is a share.
        let d = &e.differentials;
        for (name, diff) in [("front", d.front), ("rear", d.rear), ("center", d.center)] {
            let Some(diff) = diff else { continue };
            let field = |f: &str| format!("estimated.differentials.{name}.{f}");
            non_negative(&field("preload_nm"), diff.preload_nm)?;
            non_negative(&field("power_ramp"), diff.power_ramp)?;
            non_negative(&field("coast_ramp"), diff.coast_ramp)?;
            if diff.kind == DiffKind::Lsd
                && diff.preload_nm == 0.0
                && diff.power_ramp == 0.0
                && diff.coast_ramp == 0.0
            {
                return Err(format!(
                    "estimated.differentials.{name}: an lsd needs preload_nm or a ramp \
                     (use kind = \"open\" for an open differential)"
                ));
            }
            if name == "center" {
                between(&field("front_split"), diff.front_split, 0.0, 1.0, true)?;
            }
        }

        // Brakes, steering and aero.
        let b = &e.brakes;
        non_negative("estimated.brakes.front_torque_nm", b.front_torque_nm)?;
        non_negative("estimated.brakes.rear_torque_nm", b.rear_torque_nm)?;
        non_negative(
            "estimated.brakes.handbrake_torque_nm",
            b.handbrake_torque_nm,
        )?;
        positive(
            "steering.lock_to_lock_turns",
            self.steering.lock_to_lock_turns,
        )?;
        positive("steering.ratio", self.steering.ratio)?;
        // Road-wheel lock (half the lock-to-lock angle) must stay below 90 degrees.
        between(
            "steering.lock_to_lock_turns * 180 / steering.ratio (road-wheel lock, degrees)",
            self.steering.lock_to_lock_turns * 180.0 / self.steering.ratio,
            0.0,
            90.0,
            false,
        )?;
        finite("estimated.steering.ackermann", e.steering.ackermann)?;
        non_negative("aero.drag_coefficient", self.aero.drag_coefficient)?;
        non_negative("estimated.aero.frontal_area_m2", e.aero.frontal_area_m2)?;
        finite(
            "estimated.aero.lift_area_front_m2",
            e.aero.lift_area_front_m2,
        )?;
        finite("estimated.aero.lift_area_rear_m2", e.aero.lift_area_rear_m2)?;

        // Suspension and tires, per axle.
        for (axle, size, fit, tire) in [
            (
                "front",
                &self.tires.front,
                &e.suspension.front,
                &e.tires.front,
            ),
            ("rear", &self.tires.rear, &e.suspension.rear, &e.tires.rear),
        ] {
            positive(&format!("tires.{axle} radius"), tire_radius(size)?)?;
            let field = |f: &str| format!("estimated.suspension.{axle}.{f}");
            positive(&field("ride_frequency_hz"), fit.ride_frequency_hz)?;
            non_negative(&field("damping_ratio_bump"), fit.damping_ratio_bump)?;
            non_negative(&field("damping_ratio_rebound"), fit.damping_ratio_rebound)?;
            positive(&field("bump_travel_m"), fit.bump_travel_m)?;
            non_negative(&field("droop_travel_m"), fit.droop_travel_m)?;
            non_negative(&field("anti_roll_n_per_m"), fit.anti_roll_n_per_m)?;
            finite(&field("roll_center_height_m"), fit.roll_center_height_m)?;

            let field = |f: &str| format!("estimated.tires.{axle}.{f}");
            for (f, v) in [
                ("mu_long", tire.mu_long),
                ("mu_lat", tire.mu_lat),
                ("reference_load_n", tire.reference_load_n),
                ("peak_slip_ratio", tire.peak_slip_ratio),
                ("relaxation_long_m", tire.relaxation_long_m),
                ("relaxation_lat_m", tire.relaxation_lat_m),
                ("inertia_kgm2", tire.inertia_kgm2),
            ] {
                positive(&field(f), v)?;
            }
            finite(&field("load_sensitivity"), tire.load_sensitivity)?;
            non_negative(&field("rolling_resistance"), tire.rolling_resistance)?;
            non_negative(&field("pneumatic_trail_m"), tire.pneumatic_trail_m)?;
            between(
                &field("peak_slip_angle_deg"),
                tire.peak_slip_angle_deg,
                0.0,
                90.0,
                false,
            )?;
            // The Magic Formula only peaks for 1 < C (and stays positive past
            // the peak for C <= 2), and is monotonic up to the peak for E < 1.
            between(&field("shape_long"), tire.shape_long, 1.0, 2.0, true)?;
            between(&field("shape_lat"), tire.shape_lat, 1.0, 2.0, true)?;
            if !(tire.curvature.is_finite() && tire.curvature < 1.0) {
                return Err(format!(
                    "{} = {}: must be finite and below 1",
                    field("curvature"),
                    tire.curvature
                ));
            }
        }
        Ok(())
    }
}

/// Car ids shipped in `data/cars`.
pub const CARS: [&str; 3] = [
    "vw-golf-gti-mk8",
    "bmw-m3-competition-g80",
    "porsche-911-carrera-s-992",
];

/// `data/cars` in the repository.
pub fn cars_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/cars")
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

    #[test]
    fn shipped_cars_validate() {
        for id in CARS {
            load_car(id).unwrap().validate().unwrap();
        }
    }

    #[test]
    fn validation_names_the_bad_field() {
        let check = |edit: &dyn Fn(&mut CarSpec), field: &str| {
            let mut spec = load_car(CARS[0]).unwrap();
            edit(&mut spec);
            let err = spec.validate().expect_err(field);
            assert!(err.contains(field), "{field}: {err}");
        };
        check(
            &|s| s.estimated.engine.torque_curve.swap(1, 2),
            "torque_curve",
        );
        check(
            &|s| s.estimated.engine.torque_curve[0][1] = f64::NAN,
            "torque_curve[0] torque",
        );
        check(
            &|s| s.gearbox.final_drive.truncate(2),
            "gearbox.final_drive",
        );
        check(&|s| s.gearbox.ratios[3] = -1.0, "gearbox.ratios[3]");
        check(&|s| s.gearbox.ratios = vec![1.0; 200], "gearbox.ratios");
        check(&|s| s.tires.rear = "235/35 Rnan".into(), "tires.rear");
        check(
            &|s| s.estimated.engine.launch_rpm = 500.0,
            "estimated.engine.idle_rpm",
        );
        check(
            &|s| {
                let lsd = s.estimated.differentials.front.as_mut().unwrap();
                (lsd.preload_nm, lsd.power_ramp, lsd.coast_ramp) = (0.0, 0.0, 0.0);
            },
            "differentials.front",
        );
        check(
            &|s| s.estimated.tires.rear.curvature = f64::INFINITY,
            "estimated.tires.rear.curvature",
        );
    }
}
