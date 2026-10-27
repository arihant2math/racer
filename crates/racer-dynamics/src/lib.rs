//! Vehicle physics core. Pure Rust with no Bevy dependency so the same code
//! runs on client, server, AI and in headless tests.

/// Chassis rigid-body step rate (Hz). Every simulation, client or server, ticks at this rate.
pub const CHASSIS_HZ: u32 = 240;

/// Wheel spin and drivetrain substeps per chassis step, for stiff-system stability.
pub const DRIVETRAIN_SUBSTEPS: u32 = 4;

/// Length of one chassis step in seconds.
pub fn chassis_dt() -> f64 {
    1.0 / f64::from(CHASSIS_HZ)
}

/// Length of one drivetrain substep in seconds.
pub fn substep_dt() -> f64 {
    chassis_dt() / f64::from(DRIVETRAIN_SUBSTEPS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn substeps_run_at_960_hz() {
        assert_eq!(CHASSIS_HZ * DRIVETRAIN_SUBSTEPS, 960);
        assert!((substep_dt() * f64::from(DRIVETRAIN_SUBSTEPS) - chassis_dt()).abs() < 1e-15);
        assert!((1.0 / substep_dt() - 960.0).abs() < 1e-9);
    }
}
