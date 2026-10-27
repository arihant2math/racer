//! Vehicle physics core. Pure Rust with no Bevy dependency so the same code
//! runs on client, server, AI and in headless tests.
//!
//! [`Vehicle::step`] is a pure function of plain data: chassis state,
//! controls, wheel contacts and `dt` in; chassis force and torque plus the
//! new [`VehicleState`] out. The caller integrates the rigid body (Avian in
//! the game, [`testbed`] in tests) and does the wheel shape casts.

pub mod assists;
pub mod drivetrain;
pub mod spec;
pub mod testbed;
pub mod tire;
pub mod vehicle;

pub use assists::Assists;
pub use glam::{DQuat, DVec3};
pub use spec::{CarSpec, load_car};
pub use tire::Surface;
pub use vehicle::{
    ChassisState, Controls, StepOutput, Telemetry, Vehicle, VehicleState, WheelContact,
};

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
