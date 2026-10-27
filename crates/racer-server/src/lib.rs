//! Headless dedicated server: the simulation without rendering, windowing or audio.

use bevy::{app::ScheduleRunnerPlugin, log::LogPlugin, prelude::*};
use racer_sim::SimPlugin;

/// Server configuration.
#[derive(Clone, Debug, Default)]
pub struct ServerConfig {
    /// Exit after this many simulation steps. Runs until interrupted when `None`.
    pub max_ticks: Option<u64>,
}

#[derive(Resource)]
struct MaxTicks(u64);

/// Fixed steps completed since startup.
#[derive(Resource, Default, Debug)]
pub struct TickCount(pub u64);

/// Builds the headless server app.
pub fn build_app(config: &ServerConfig) -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(racer_sim::tick_duration())),
        LogPlugin::default(),
        SimPlugin,
    ))
    .init_resource::<TickCount>()
    .add_systems(FixedUpdate, count_ticks);
    if let Some(max) = config.max_ticks {
        app.insert_resource(MaxTicks(max))
            .add_systems(FixedUpdate, exit_after_max_ticks.after(count_ticks));
    }
    app
}

fn count_ticks(mut ticks: ResMut<TickCount>) {
    ticks.0 += 1;
}

fn exit_after_max_ticks(
    ticks: Res<TickCount>,
    max: Res<MaxTicks>,
    mut exit: MessageWriter<AppExit>,
    mut done: Local<bool>,
) {
    // Checked every fixed step (one frame can run several) so the server
    // stops exactly at the limit; later steps in the same frame are ignored.
    if !*done && ticks.0 >= max.0 {
        *done = true;
        info!("reached {} ticks, shutting down", ticks.0);
        exit.write(AppExit::Success);
    }
}
