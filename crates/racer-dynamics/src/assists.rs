//! Driver assists as controllers inside the crate, so they behave the same
//! for players, AI and the server.

/// Which assists are switched on. Per player, sent with the inputs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Assists {
    /// Anti-lock brakes: keeps each braked wheel near its peak slip.
    pub abs: bool,
    /// Traction control: trims engine torque when driven wheels spin.
    pub tcs: bool,
    /// Stability control: brakes single wheels to hold the intended yaw rate.
    pub esc: bool,
}

impl Default for Assists {
    fn default() -> Self {
        Assists {
            abs: true,
            tcs: true,
            esc: true,
        }
    }
}

impl Assists {
    pub const OFF: Assists = Assists {
        abs: false,
        tcs: false,
        esc: false,
    };
}

/// Time constant of brake-pressure and torque-reduction changes (s).
const ACTUATOR_LAG: f64 = 0.01;

/// Moves an actuator factor towards `target` with first-order lag.
fn actuate(factor: f64, target: f64, dt: f64) -> f64 {
    factor + (target - factor) * (dt / ACTUATOR_LAG).min(1.0)
}

/// Proportional factor across a slip band: 1 below `0.8 * peak`, 0 above
/// `1.3 * peak` (slips and peak as positive magnitudes).
fn band(slip: f64, peak: f64) -> f64 {
    ((1.3 * peak - slip) / (0.5 * peak)).clamp(0.0, 1.0)
}

/// ABS brake-pressure factor for one wheel, given its slip ratio (negative
/// when braking) and the tire's peak slip ratio.
pub fn abs_update(factor: f64, slip: f64, peak: f64, dt: f64) -> f64 {
    actuate(factor, band(-slip, peak).max(0.05), dt)
}

/// TCS engine-torque factor from the largest driven-wheel slip; aims a
/// little above the peak, where launches are quickest.
pub fn tcs_update(factor: f64, slip: f64, peak: f64, dt: f64) -> f64 {
    actuate(factor, band(slip, 1.2 * peak), dt)
}

/// ESC yaw-rate controller. Returns per-wheel brake torques (FL, FR, RL, RR).
///
/// `yaw_rate` and `reference` are rad/s about the up axis (positive = left
/// turn). Oversteer brakes the outside front wheel, understeer the inside rear.
pub fn esc_torques(yaw_rate: f64, reference: f64, max_torque: f64) -> [f64; 4] {
    const DEADBAND: f64 = 0.04;
    const GAIN: f64 = 8000.0; // Nm per rad/s of yaw-rate error
    let mut out = [0.0; 4];
    let left = if reference.abs() > 0.02 {
        reference > 0.0
    } else {
        yaw_rate > 0.0
    };
    let excess = yaw_rate.abs() - reference.abs();
    let same_direction = reference.abs() <= 0.02 || yaw_rate.signum() == reference.signum();
    if same_direction && excess > DEADBAND {
        out[if left { 1 } else { 0 }] = (GAIN * (excess - DEADBAND)).min(max_torque);
    } else if !same_direction && yaw_rate.abs() > DEADBAND {
        // Spinning against the steering: brake the front wheel on the side the tail swings out.
        out[if yaw_rate > 0.0 { 1 } else { 0 }] =
            (GAIN * (yaw_rate.abs() - DEADBAND)).min(max_torque);
    } else if -excess > DEADBAND {
        out[if left { 2 } else { 3 }] = (0.5 * GAIN * (-excess - DEADBAND)).min(0.5 * max_torque);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn abs_releases_a_locking_wheel_and_reapplies() {
        let mut f = 1.0;
        for _ in 0..50 {
            f = abs_update(f, -0.5, 0.1, 1.0 / 960.0);
        }
        assert!(f < 0.1);
        for _ in 0..200 {
            f = abs_update(f, -0.02, 0.1, 1.0 / 960.0);
        }
        assert!(f > 0.99);
    }

    #[test]
    fn esc_brakes_the_outside_front_on_oversteer() {
        let t = esc_torques(0.8, 0.5, 2000.0);
        assert!(t[1] > 0.0 && t[0] == 0.0 && t[2] == 0.0 && t[3] == 0.0);
        let t = esc_torques(0.2, 0.5, 2000.0);
        assert!(
            t[2] > 0.0,
            "understeer in a left turn brakes the inside rear"
        );
        assert_eq!(esc_torques(0.5, 0.5, 2000.0), [0.0; 4]);
    }
}
