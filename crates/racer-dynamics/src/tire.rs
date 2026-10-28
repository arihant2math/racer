//! Tire model: Pacejka Magic Formula with combined slip by the similarity
//! method (slips normalised by their peak values share one curve), load
//! sensitivity, first-order relaxation and a pneumatic-trail aligning torque.

use serde::Deserialize;

use crate::spec::TireFit;

/// Ground surfaces and their grip relative to dry asphalt.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    #[default]
    DryAsphalt,
    WetAsphalt,
    PaintedLine,
    Cobbles,
    MetalPlate,
    Curb,
    Grass,
    Dirt,
}

impl Surface {
    pub fn grip(self) -> f64 {
        match self {
            Surface::DryAsphalt => 1.0,
            Surface::WetAsphalt => 0.7,
            Surface::PaintedLine => 0.85,
            Surface::Cobbles => 0.8,
            Surface::MetalPlate => 0.6,
            Surface::Curb => 0.9,
            Surface::Grass => 0.45,
            Surface::Dirt => 0.6,
        }
    }

    /// Rolling resistance multiplier (soft ground drags).
    pub fn drag(self) -> f64 {
        match self {
            Surface::Grass => 6.0,
            Surface::Dirt => 4.0,
            _ => 1.0,
        }
    }
}

/// Tire parameters with the Magic Formula stiffness factors precomputed.
#[derive(Clone, Debug)]
pub struct Tire {
    pub radius: f64,
    pub inertia: f64,
    mu_long: f64,
    mu_lat: f64,
    reference_load: f64,
    load_sensitivity: f64,
    pub peak_slip_ratio: f64,
    /// tan of the peak slip angle.
    pub peak_slip_tan: f64,
    curve_long: Curve,
    curve_lat: Curve,
    pub relaxation_long: f64,
    pub relaxation_lat: f64,
    pub rolling_resistance: f64,
    trail: f64,
}

/// `sin(C atan(B s - E (B s - atan(B s))))` with B chosen so the peak is at s = 1.
#[derive(Clone, Copy, Debug)]
struct Curve {
    b: f64,
    c: f64,
    e: f64,
}

impl Curve {
    fn new(c: f64, e: f64) -> Self {
        // Peak where C atan(x - E (x - atan x)) = pi/2; the inner function is
        // monotonic and unbounded for E < 1 (checked by `CarSpec::validate`,
        // with 1 < C), so bracket and bisect for x and set B = x.
        let target = (std::f64::consts::FRAC_PI_2 / c).tan();
        let inner = |x: f64| x - e * (x - x.atan());
        let (mut lo, mut hi) = (0.0f64, 100.0f64);
        while inner(hi) < target {
            (lo, hi) = (hi, 2.0 * hi);
        }
        for _ in 0..80 {
            let x = 0.5 * (lo + hi);
            if inner(x) < target {
                lo = x;
            } else {
                hi = x;
            }
        }
        Curve {
            b: 0.5 * (lo + hi),
            c,
            e,
        }
    }

    fn eval(&self, s: f64) -> f64 {
        let bs = self.b * s;
        (self.c * (bs - self.e * (bs - bs.atan())).atan()).sin()
    }

    /// Slope at s = 0.
    fn slope(&self) -> f64 {
        self.b * self.c
    }
}

/// Forces in the wheel frame: `fx` forward, `fy` to the wheel's right,
/// `mz` self-aligning torque about the up axis (positive turns the wheel left,
/// so a tire sliding right yields negative `mz`, steering back towards straight).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TireForce {
    pub fx: f64,
    pub fy: f64,
    pub mz: f64,
}

impl Tire {
    pub fn new(fit: &TireFit, radius: f64) -> Self {
        Tire {
            radius,
            inertia: fit.inertia_kgm2,
            mu_long: fit.mu_long,
            mu_lat: fit.mu_lat,
            reference_load: fit.reference_load_n,
            load_sensitivity: fit.load_sensitivity,
            peak_slip_ratio: fit.peak_slip_ratio,
            peak_slip_tan: fit.peak_slip_angle_deg.to_radians().tan(),
            curve_long: Curve::new(fit.shape_long, fit.curvature),
            curve_lat: Curve::new(fit.shape_lat, fit.curvature),
            relaxation_long: fit.relaxation_long_m,
            relaxation_lat: fit.relaxation_lat_m,
            rolling_resistance: fit.rolling_resistance,
            trail: fit.pneumatic_trail_m,
        }
    }

    /// Peak friction coefficient at `load`, falling with load.
    pub fn mu(&self, load: f64) -> (f64, f64) {
        let scale =
            (1.0 - self.load_sensitivity * (load / self.reference_load - 1.0)).clamp(0.5, 1.3);
        (self.mu_long * scale, self.mu_lat * scale)
    }

    /// Longitudinal stiffness dFx/dkappa at `load`.
    pub fn long_stiffness(&self, load: f64) -> f64 {
        self.mu(load).0 * load * self.curve_long.slope() / self.peak_slip_ratio
    }

    /// Cornering stiffness dFy/dtan(alpha) at `load`.
    pub fn cornering_stiffness(&self, load: f64) -> f64 {
        self.mu(load).1 * load * self.curve_lat.slope() / self.peak_slip_tan
    }

    /// Steady-state forces for slip ratio `kappa` and lateral slip `tan_alpha`
    /// (positive when the contact patch slides to the right), at normal load
    /// `load` on a surface with relative grip `grip`.
    pub fn force(&self, kappa: f64, tan_alpha: f64, load: f64, grip: f64) -> TireForce {
        if load <= 0.0 {
            return TireForce::default();
        }
        let sx = kappa / self.peak_slip_ratio;
        let sy = tan_alpha / self.peak_slip_tan;
        let s = sx.hypot(sy);
        if s < 1e-9 {
            return TireForce::default();
        }
        let (mu_x, mu_y) = self.mu(load);
        let fx = mu_x * grip * load * self.curve_long.eval(s) * sx / s;
        let fy = -mu_y * grip * load * self.curve_lat.eval(s) * sy / s;
        // The pneumatic trail shrinks to zero as the patch saturates, which
        // is what makes the steering go light at the limit.
        let trail = self.trail * (1.0 - sy.abs() / 1.5).max(0.0) * (1.0 - sx.abs().min(1.0));
        TireForce {
            fx,
            fy,
            mz: fy * trail,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tire() -> Tire {
        Tire::new(
            &TireFit {
                mu_long: 1.2,
                mu_lat: 1.1,
                reference_load_n: 4000.0,
                load_sensitivity: 0.15,
                peak_slip_ratio: 0.1,
                peak_slip_angle_deg: 7.0,
                shape_long: 1.5,
                shape_lat: 1.3,
                curvature: 0.2,
                relaxation_long_m: 0.1,
                relaxation_lat_m: 0.4,
                rolling_resistance: 0.012,
                pneumatic_trail_m: 0.03,
                inertia_kgm2: 1.0,
            },
            0.32,
        )
    }

    #[test]
    fn pure_slip_peaks_at_the_configured_slip() {
        let t = tire();
        let fx = |k| t.force(k, 0.0, 4000.0, 1.0).fx;
        assert!((fx(0.1) - 1.2 * 4000.0).abs() < 1e-6);
        assert!(fx(0.09) < fx(0.1) && fx(0.11) < fx(0.1));
        assert!(
            fx(1.0) > 0.6 * fx(0.1),
            "locked wheel keeps most of its grip"
        );
        let tan7 = 7f64.to_radians().tan();
        assert!((t.force(0.0, tan7, 4000.0, 1.0).fy + 1.1 * 4000.0).abs() < 1e-6);
    }

    #[test]
    fn curve_peaks_at_one_for_shape_factors_close_to_one() {
        for c in [1.001, 1.01, 1.3, 2.0] {
            let curve = Curve::new(c, 0.2);
            assert!((curve.eval(1.0) - 1.0).abs() < 1e-9, "C {c}: {curve:?}");
        }
    }

    #[test]
    fn combined_slip_stays_inside_the_friction_ellipse() {
        let t = tire();
        for k in [-1.0, -0.2, -0.05, 0.0, 0.05, 0.2, 1.0] {
            for a in [-0.5, -0.1, -0.02, 0.02, 0.1, 0.5] {
                let f = t.force(k, a, 4000.0, 1.0);
                let e = (f.fx / (1.2 * 4000.0)).powi(2) + (f.fy / (1.1 * 4000.0)).powi(2);
                assert!(e <= 1.0 + 1e-9, "k {k} a {a}: {f:?}");
            }
        }
    }

    #[test]
    fn grip_coefficient_falls_with_load() {
        let t = tire();
        assert!(t.mu(8000.0).1 < t.mu(4000.0).1);
        assert!(
            t.force(0.0, 0.12, 8000.0, 1.0).fy.abs() > t.force(0.0, 0.12, 4000.0, 1.0).fy.abs()
        );
    }

    #[test]
    fn aligning_torque_fades_past_the_peak() {
        let t = tire();
        let mz = |a: f64| t.force(0.0, a.to_radians().tan(), 4000.0, 1.0).mz;
        assert!(mz(2.0) < 0.0, "a right slide turns the wheel right");
        assert!(mz(12.0).abs() < mz(3.0).abs());
    }
}
