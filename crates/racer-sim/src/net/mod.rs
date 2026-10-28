//! Online races: the protocol shared by client and server, the client's
//! prediction and the remote-car extrapolation. The server is `racer-server`.
//!
//! lightyear carries the bytes: UDP, netcode.io connections, reliable and
//! unreliable channels, ping and the clock sync that keeps each client's
//! tick ahead of the server by its latency. Prediction, rollback and
//! extrapolation are built here on top (see `docs/milestones/M3.md`).

pub mod client;
pub mod extrapolate;
pub mod protocol;
