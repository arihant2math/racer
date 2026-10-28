//! Lobbies: members, ready state, the host's race settings, and the server
//! once the race starts.

use serde::{Deserialize, Serialize};

use crate::{Player, QUICK_MATCH_WAIT};

pub const MAX_PLAYERS: usize = 8;

/// A lobby is dropped this long after its race started (s).
const RACE_LIFETIME: f64 = 3600.0;

/// Invite codes use letters and digits that can't be mistaken for each other.
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

pub fn invite_code(random: &[u8; 6]) -> String {
    random
        .iter()
        .map(|b| CODE_ALPHABET[usize::from(*b) % CODE_ALPHABET.len()] as char)
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Circuit,
    Sprint,
    TimeTrial,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Collisions {
    #[default]
    Full,
    GhostFirstLap,
    Off,
}

/// The race the host sets up.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub track: String,
    pub mode: Mode,
    pub laps: u32,
    pub collisions: Collisions,
    /// Fill the empty grid slots with AI cars.
    pub ai_fill: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            track: "downtown-loop".into(),
            mode: Mode::Circuit,
            laps: 3,
            collisions: Collisions::Full,
            ai_fill: true,
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.track != "downtown-loop" {
            return Err(format!("unknown track {:?}", self.track));
        }
        if !(1..=10).contains(&self.laps) {
            return Err("laps are 1 to 10".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: u64,
    pub name: String,
    pub ready: bool,
    /// Car id the player picked (empty: the server's default).
    pub car: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LobbyState {
    Open,
    /// A server is being started.
    Starting,
    Racing,
}

/// Where the race runs: the server's address and the race's netcode key
/// (hex), for the members only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub addr: String,
    pub key: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Lobby {
    pub code: String,
    pub host: u64,
    pub members: Vec<Member>,
    pub settings: Settings,
    pub state: LobbyState,
    /// Quick-match class, for public lobbies.
    pub quick: Option<String>,
    pub server: Option<ServerInfo>,
    /// The receiver's player id (in answers).
    #[serde(default)]
    pub you: u64,
    #[serde(skip)]
    pub(crate) created_at: f64,
    #[serde(skip)]
    pub(crate) started_at: Option<f64>,
}

impl Lobby {
    pub fn new(
        code: String,
        host: &Player,
        settings: Settings,
        quick: Option<String>,
        now: f64,
    ) -> Self {
        Lobby {
            code,
            host: host.id,
            members: vec![Member {
                id: host.id,
                name: host.name.clone(),
                ready: quick.is_some(),
                car: String::new(),
            }],
            settings,
            state: LobbyState::Open,
            quick,
            server: None,
            you: 0,
            created_at: now,
            started_at: None,
        }
    }

    pub fn is_member(&self, id: u64) -> bool {
        self.members.iter().any(|m| m.id == id)
    }

    pub fn join(&mut self, player: &Player, car: String) -> Result<(), &'static str> {
        if let Some(m) = self.members.iter_mut().find(|m| m.id == player.id) {
            if !car.is_empty() {
                m.car = car;
            }
            return Ok(());
        }
        if self.state != LobbyState::Open {
            return Err("the race has already started");
        }
        if self.members.len() >= MAX_PLAYERS {
            return Err("the lobby is full");
        }
        self.members.push(Member {
            id: player.id,
            name: player.name.clone(),
            // Quick matches start by themselves; nobody needs to confirm.
            ready: self.quick.is_some(),
            car,
        });
        Ok(())
    }

    /// Removes the player; the next member becomes host.
    pub fn leave(&mut self, id: u64) {
        self.members.retain(|m| m.id != id);
        if self.host == id
            && let Some(next) = self.members.first()
        {
            self.host = next.id;
        }
    }

    /// The host counts as ready.
    pub fn all_ready(&self) -> bool {
        self.members.iter().all(|m| m.ready || m.id == self.host)
    }

    pub(crate) fn quick_match_due(&self, now: f64) -> bool {
        self.quick.is_some()
            && self.state == LobbyState::Open
            && (self.members.len() >= MAX_PLAYERS || now - self.created_at >= QUICK_MATCH_WAIT)
    }

    pub(crate) fn expired(&self, now: f64) -> bool {
        self.started_at.is_some_and(|t| now - t > RACE_LIFETIME)
    }

    /// The lobby as `id` sees it: the server only for members.
    pub fn view(&self, id: u64) -> Lobby {
        let mut l = self.clone();
        l.you = id;
        if !self.is_member(id) {
            l.server = None;
        }
        l
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn player(id: u64) -> Player {
        Player {
            id,
            name: format!("p{id}"),
        }
    }

    #[test]
    fn codes_are_six_unambiguous_characters() {
        let code = invite_code(&[0, 31, 32, 255, 7, 100]);
        assert_eq!(code.len(), 6);
        assert!(code.chars().all(|c| !"01IO".contains(c)), "{code}");
    }

    #[test]
    fn host_passes_on_and_full_lobbies_refuse() {
        let mut l = Lobby::new("ABCDEF".into(), &player(1), Settings::default(), None, 0.0);
        for id in 2..=8 {
            l.join(&player(id), String::new()).unwrap();
        }
        assert_eq!(l.join(&player(9), String::new()), Err("the lobby is full"));
        assert!(!l.all_ready());
        l.leave(1);
        assert_eq!(l.host, 2);
        for m in &mut l.members {
            m.ready = m.id != 2;
        }
        assert!(l.all_ready(), "the host need not be ready");
    }

    #[test]
    fn quick_matches_start_when_full_or_after_the_wait() {
        let mut l = Lobby::new("Q".into(), &player(1), Settings::default(), Some("open".into()), 100.0);
        assert!(!l.quick_match_due(100.0 + QUICK_MATCH_WAIT - 1.0));
        assert!(l.quick_match_due(100.0 + QUICK_MATCH_WAIT));
        for id in 2..=8 {
            l.join(&player(id), String::new()).unwrap();
        }
        assert!(l.quick_match_due(100.0));
    }
}
