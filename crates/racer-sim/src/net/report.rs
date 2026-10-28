//! What a server writes at the end of a race (`racer-server --report`), for
//! the net check to compare with what the clients saw.

use bevy::math::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ServerReport {
    /// Server ticks from the race start to its end.
    pub ticks: u32,
    pub start_tick: u32,
    pub contacts: Vec<Contact>,
    pub results: Vec<ResultRecord>,
    pub players: Vec<PlayerRecord>,
    /// Wall time of a server tick (ms).
    pub tick_ms_mean: f32,
    pub tick_ms_max: f32,
}

/// Two cars starting to touch.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct Contact {
    pub tick: u32,
    pub a: u8,
    pub b: u8,
    pub position: Vec3,
    /// Closing speed (m/s).
    pub speed: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ResultRecord {
    pub slot: u8,
    pub name: String,
    pub position: usize,
    pub laps: u32,
    /// Finish time, `None` for a DNF.
    pub finish: Option<f32>,
    pub resets: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerRecord {
    pub slot: u8,
    pub name: String,
    /// Ticks driven with a repeated input because the real one was late or lost.
    pub missing_inputs: u32,
    pub ticks: u32,
    pub disconnected: bool,
}
