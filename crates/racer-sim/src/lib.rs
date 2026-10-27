//! Gameplay shared by the client and the dedicated server.

use std::time::Duration;

use bevy::prelude::*;

/// Length of one simulation tick, shared by client and server.
pub fn tick_duration() -> Duration {
    Duration::from_secs_f64(racer_dynamics::chassis_dt())
}

/// Configures the fixed simulation step shared by client and server.
pub struct SimPlugin;

impl Plugin for SimPlugin {
    fn build(&self, app: &mut App) {
        app.insert_resource(Time::<Fixed>::from_duration(tick_duration()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_step_matches_chassis_rate() {
        let mut app = App::new();
        app.add_plugins((MinimalPlugins, SimPlugin));
        app.update();
        let fixed = app.world().resource::<Time<Fixed>>();
        assert_eq!(fixed.timestep(), tick_duration());
        assert_eq!(fixed.timestep().as_micros(), 4166);
    }
}
