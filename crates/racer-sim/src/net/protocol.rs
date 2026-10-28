//! What client and server say to each other, and over which channels.
//!
//! Clients send inputs, never positions: every [`Inputs`] message repeats the
//! last [`INPUT_REDUNDANCY`] ticks of quantized [`NetControls`], so a lost
//! packet costs nothing. The server answers with a [`Snapshot`] of every car
//! at 60 Hz (the receiver's own car with its full vehicle state, for
//! rollback), each racer's race state at 10 Hz ([`Standings`]), and race
//! control messages on the reliable channel.

use avian3d::prelude::{AngularVelocity, LinearVelocity, Position, Rotation};
use bevy::prelude::*;
use lightyear::prelude::*;
use racer_dynamics::{Assists, Controls, VehicleState};
use serde::{Deserialize, Serialize};

use crate::race::{DriverKind, RaceConfig, RacePhase, Racer};

/// netcode.io protocol id: a client and server must agree on it.
pub const PROTOCOL_ID: u64 = 0x7261_6365_7203;

/// Key for direct connections (development): the client makes its own
/// connect token with it. Servers started by the backend use a random key and
/// hand out tokens instead.
pub const DEV_KEY: [u8; 32] = [0; 32];

/// Ticks of inputs repeated in every [`Inputs`] message (200 ms).
pub const INPUT_REDUNDANCY: usize = 48;

/// Server ticks between snapshots: 240 Hz / 4 = 60 Hz.
pub const SNAPSHOT_INTERVAL: u32 = 4;

/// Server ticks between two standings updates of one racer (10 Hz).
pub const STANDINGS_INTERVAL: u32 = 24;

/// Unreliable: inputs from clients, snapshots and standings from the server.
pub struct StateChannel;

/// Reliable and ordered: joining, lobby, race start and end, reset requests.
pub struct ControlChannel;

/// Driver inputs as sent over the wire. Both sides drive with the decoded
/// values, so the client predicts with exactly what the server applies.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetControls {
    pub throttle: u8,
    pub brake: u8,
    pub handbrake: u8,
    pub clutch: u8,
    pub steer: i16,
    pub flags: u8,
}

const SHIFT_UP: u8 = 1;
const SHIFT_DOWN: u8 = 2;
const AUTOMATIC: u8 = 4;
const ABS: u8 = 8;
const TCS: u8 = 16;
const ESC: u8 = 32;

impl NetControls {
    pub fn encode(c: &Controls) -> Self {
        let c = c.sanitized();
        let unit = |v: f64| (v * 255.0).round() as u8;
        let flag = |on: bool, bit: u8| if on { bit } else { 0 };
        NetControls {
            throttle: unit(c.throttle),
            brake: unit(c.brake),
            handbrake: unit(c.handbrake),
            clutch: unit(c.clutch),
            steer: (c.steer * 32767.0).round() as i16,
            flags: flag(c.shift_up, SHIFT_UP)
                | flag(c.shift_down, SHIFT_DOWN)
                | flag(c.automatic, AUTOMATIC)
                | flag(c.assists.abs, ABS)
                | flag(c.assists.tcs, TCS)
                | flag(c.assists.esc, ESC),
        }
    }

    pub fn decode(&self) -> Controls {
        let unit = |v: u8| f64::from(v) / 255.0;
        let on = |bit: u8| self.flags & bit != 0;
        Controls {
            throttle: unit(self.throttle),
            brake: unit(self.brake),
            handbrake: unit(self.handbrake),
            clutch: unit(self.clutch),
            steer: (f64::from(self.steer) / 32767.0).clamp(-1.0, 1.0),
            shift_up: on(SHIFT_UP),
            shift_down: on(SHIFT_DOWN),
            automatic: on(AUTOMATIC),
            assists: Assists {
                abs: on(ABS),
                tcs: on(TCS),
                esc: on(ESC),
            },
        }
    }

    /// The same inputs held for another tick: shift requests are edges and
    /// must not repeat.
    pub fn held(&self) -> Self {
        NetControls {
            flags: self.flags & !(SHIFT_UP | SHIFT_DOWN),
            ..*self
        }
    }
}

/// Client to server: the inputs for ticks `last - controls.len() + 1 ..= last`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Inputs {
    pub last: u32,
    pub controls: Vec<NetControls>,
}

/// Client to server, first message after connecting.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Join {
    pub name: String,
    /// Car id (`data/cars/<id>`).
    pub car: String,
    pub assists: Assists,
}

/// Client to server: put my car back on the track (reset key, AI).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RequestReset;

/// Server to client while waiting for players.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LobbyStatus {
    pub players: Vec<String>,
    pub expected: u32,
    /// Seconds until the race starts without the missing players.
    pub timeout: Option<f32>,
}

/// One car on the grid.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GridCar {
    pub slot: u8,
    pub car: String,
    pub name: String,
    pub driver: DriverKind,
}

/// Server to client: the grid is set, the countdown runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaceStart {
    pub config: RaceConfig,
    pub grid: Vec<GridCar>,
    /// The receiver's grid slot.
    pub you: u8,
    /// Server tick the race is created on, and the first running tick (the
    /// countdown holds every car on the brakes before it).
    pub created: u32,
    pub start: u32,
}

/// The rigid-body state of a car after a tick.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Body {
    pub position: Vec3,
    pub rotation: Quat,
    pub velocity: Vec3,
    pub angular_velocity: Vec3,
}

impl Body {
    pub fn read(p: &Position, r: &Rotation, v: &LinearVelocity, w: &AngularVelocity) -> Self {
        Body {
            position: p.0,
            rotation: r.0,
            velocity: v.0,
            angular_velocity: w.0,
        }
    }
}

/// One car in a snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct CarSnapshot {
    pub slot: u8,
    pub body: Body,
    /// The inputs the server applied this tick (remote cars are extrapolated
    /// with them).
    pub controls: NetControls,
    pub ghosted: bool,
}

/// Server to client, every [`SNAPSHOT_INTERVAL`] ticks: every car after
/// server tick `tick`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Snapshot {
    pub tick: u32,
    pub cars: Vec<CarSnapshot>,
    /// The receiver's car model state, for an exact rollback.
    pub own: Option<VehicleState>,
    /// Newest input tick the server has from the receiver.
    pub input_ack: u32,
    /// Ticks since the last snapshot the server drove the receiver's car
    /// without its input (it came late or not at all).
    pub late: u16,
}

/// Server to client: one racer's race state (progress, timing, status).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Standings {
    pub tick: u32,
    pub slot: u8,
    pub racer: Racer,
    pub phase: RacePhase,
    pub winner: Option<f32>,
}

/// Server to client: the race is over; every racer's final state.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RaceOver {
    pub racers: Vec<(u8, Racer)>,
}

/// Registers the channels and messages; add after lightyear's client or
/// server plugins, on both sides.
pub struct ProtocolPlugin;

impl Plugin for ProtocolPlugin {
    fn build(&self, app: &mut App) {
        app.add_channel::<StateChannel>(ChannelSettings {
            mode: ChannelMode::UnorderedUnreliable,
            ..default()
        })
        .add_direction(NetworkDirection::Bidirectional);
        app.add_channel::<ControlChannel>(ChannelSettings {
            mode: ChannelMode::OrderedReliable(ReliableSettings::default()),
            ..default()
        })
        .add_direction(NetworkDirection::Bidirectional);
        app.register_message::<Inputs>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<Join>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<RequestReset>()
            .add_direction(NetworkDirection::ClientToServer);
        app.register_message::<LobbyStatus>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<RaceStart>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<Snapshot>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<Standings>()
            .add_direction(NetworkDirection::ServerToClient);
        app.register_message::<RaceOver>()
            .add_direction(NetworkDirection::ServerToClient);
    }
}

/// Simulated network conditions on one side's inbound path (half of the
/// round trip each).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Conditions {
    pub rtt_ms: u32,
    pub jitter_ms: u32,
    /// Packet loss, 0..1.
    pub loss: f32,
}

impl Conditions {
    /// Parses `RTT_MS[,JITTER_MS[,LOSS_PERCENT]]`, e.g. `150,15,1`.
    pub fn parse(s: &str) -> Result<Self, String> {
        let mut parts = s.split(',');
        let mut next = |name: &str| -> Result<Option<f32>, String> {
            parts
                .next()
                .map(|p| {
                    p.trim()
                        .parse::<f32>()
                        .ok()
                        .filter(|v| v.is_finite() && *v >= 0.0)
                        .ok_or_else(|| format!("bad {name} in {s:?}"))
                })
                .transpose()
        };
        let rtt = next("RTT")?.ok_or("missing RTT")?;
        let jitter = next("jitter")?.unwrap_or(0.0);
        let loss = next("loss")?.unwrap_or(0.0);
        if loss > 100.0 || parts.next().is_some() {
            return Err(format!(
                "bad conditions {s:?}: RTT_MS[,JITTER_MS[,LOSS_PERCENT]]"
            ));
        }
        Ok(Conditions {
            rtt_ms: rtt as u32,
            jitter_ms: jitter as u32,
            loss: loss / 100.0,
        })
    }

    /// The conditioner for one direction.
    pub fn conditioner(&self) -> Option<RecvLinkConditioner> {
        (*self != Conditions::default()).then(|| {
            RecvLinkConditioner::new(
                LinkConditionerConfig::default()
                    .with_incoming_latency(std::time::Duration::from_millis(
                        u64::from(self.rtt_ms) / 2,
                    ))
                    .with_incoming_jitter(std::time::Duration::from_millis(
                        u64::from(self.jitter_ms) / 2,
                    ))
                    .with_fixed_loss(self.loss),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controls_survive_the_round_trip_and_decode_stably() {
        let c = Controls {
            throttle: 0.37,
            brake: 1.0,
            steer: -0.61,
            handbrake: 0.0,
            clutch: 0.2,
            shift_up: true,
            automatic: true,
            assists: Assists {
                abs: true,
                tcs: false,
                esc: true,
            },
            ..default()
        };
        let n = NetControls::encode(&c);
        let d = n.decode();
        assert!((d.throttle - c.throttle).abs() < 0.003);
        assert!((d.steer - c.steer).abs() < 1e-4);
        assert_eq!((d.shift_up, d.shift_down, d.automatic), (true, false, true));
        assert_eq!(d.assists, c.assists);
        // Decoded values encode to the same message: what the client drives
        // with is what the server drives with.
        assert_eq!(NetControls::encode(&d), n);
        assert!(!n.held().decode().shift_up);
        assert_eq!(
            NetControls::encode(&Controls {
                steer: f64::NAN,
                ..c
            })
            .steer,
            0
        );
    }

    #[test]
    fn conditions_parse() {
        let c = Conditions::parse("150,15,1").unwrap();
        assert_eq!((c.rtt_ms, c.jitter_ms), (150, 15));
        assert!((c.loss - 0.01).abs() < 1e-6);
        assert_eq!(Conditions::parse("0").unwrap(), Conditions::default());
        assert!(Conditions::parse("x").is_err());
        assert!(Conditions::parse("150,1,200").is_err());
        assert!(Conditions::parse("150,1,1,1").is_err());
    }
}
