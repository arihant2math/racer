//! Online play in the game: quick match, invite-code lobbies (through the
//! backend) and direct connections to a server, the lobby screen, and the
//! online race, which runs through the same race HUD and results as offline.
//!
//! The backend is `RACER_BACKEND` (default `http://127.0.0.1:8080`). The
//! player's device key lives in the config directory (see [`config_dir`]);
//! `RACER_NAME` sets the name the backend shows.

use std::{
    net::{SocketAddr, ToSocketAddrs},
    path::PathBuf,
    sync::{
        Mutex,
        mpsc::{Receiver, TryRecvError, channel},
    },
    time::Duration,
};

use bevy::{
    input::keyboard::{Key, KeyboardInput},
    prelude::*,
};
use racer_dynamics::{Assists, spec::CARS};
use racer_sim::{
    net::{
        client::{
            ClientAuth, NetCar, NetClientConfig, NetClientPlugin, NetSession, RemoteRace,
            SessionState,
        },
        protocol::{Conditions, Join},
    },
    race::{Collisions, RaceMode},
};
use serde::{Deserialize, Serialize};

use crate::{
    flow::{AppState, RaceSetup},
    input::{DriverSettings, Player},
    race_scene::DriverAids,
    visuals::{CarPaint, PAINT},
};

pub struct OnlinePlugin;

impl Plugin for OnlinePlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(NetClientPlugin)
            .init_resource::<Online>()
            .init_resource::<TextEntry>()
            .add_systems(OnEnter(AppState::MainMenu), leave)
            .add_systems(
                OnEnter(AppState::Race),
                spawn_ping.run_if(resource_exists::<RemoteRace>),
            )
            .add_systems(
                Update,
                (
                    type_text.run_if(in_state(AppState::Entry)),
                    (poll_backend, connect_to_server, enter_race)
                        .chain()
                        .run_if(in_state(AppState::Lobby)),
                    dress_cars,
                    update_ping.run_if(in_state(AppState::Race)),
                ),
            );
    }
}

/// What the text-entry screen asks for.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EntryPurpose {
    #[default]
    JoinCode,
    Address,
}

#[derive(Resource, Default, Debug)]
pub struct TextEntry {
    pub purpose: EntryPurpose,
    pub text: String,
}

/// How this online session was started.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum OnlineMode {
    #[default]
    Off,
    /// A backend lobby (invite code or quick match).
    Lobby,
    /// A server address typed in (development).
    Direct(SocketAddr),
}

/// The online session as the menus see it.
#[derive(Resource, Default)]
pub struct Online {
    pub mode: OnlineMode,
    pub lobby: Option<LobbyView>,
    /// Last error or progress message.
    pub status: String,
    backend: Option<BackendClient>,
    pending: Option<Mutex<Receiver<Answer>>>,
    since_poll: f32,
}

impl Online {
    pub fn is_host(&self) -> bool {
        self.lobby.as_ref().is_some_and(|l| l.host == l.you)
    }

    pub fn me(&self) -> Option<&MemberView> {
        let l = self.lobby.as_ref()?;
        l.members.iter().find(|m| m.id == l.you)
    }
}

/// What a backend request came back with: the lobby, if the answer has one.
type Answer = Result<Option<LobbyView>, String>;

/// A backend lobby, as the backend sends it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct LobbyView {
    pub code: String,
    pub host: u64,
    pub members: Vec<MemberView>,
    pub settings: SettingsView,
    pub state: String,
    pub quick: Option<String>,
    pub server: Option<ServerView>,
    pub you: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct MemberView {
    pub id: u64,
    pub name: String,
    pub ready: bool,
    pub car: String,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SettingsView {
    pub track: String,
    pub mode: String,
    pub laps: u32,
    pub collisions: String,
    pub ai_fill: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ServerView {
    pub addr: String,
    pub key: String,
}

impl SettingsView {
    pub fn from_setup(s: &RaceSetup) -> Self {
        SettingsView {
            track: "downtown-loop".into(),
            mode: match s.mode {
                RaceMode::Circuit => "circuit",
                RaceMode::Sprint => "sprint",
                RaceMode::TimeTrial => "time-trial",
            }
            .into(),
            laps: s.laps,
            collisions: match s.collisions {
                Collisions::Full => "full",
                Collisions::GhostFirstLap => "ghost-first-lap",
                Collisions::Off => "off",
            }
            .into(),
            ai_fill: s.ai_fill,
        }
    }

    pub fn apply_to(&self, s: &mut RaceSetup) {
        s.mode = match self.mode.as_str() {
            "sprint" => RaceMode::Sprint,
            "time-trial" => RaceMode::TimeTrial,
            _ => RaceMode::Circuit,
        };
        s.laps = self.laps.clamp(1, 10);
        s.collisions = match self.collisions.as_str() {
            "ghost-first-lap" => Collisions::GhostFirstLap,
            "off" => Collisions::Off,
            _ => Collisions::Full,
        };
        s.ai_fill = self.ai_fill;
    }
}

/// Where the game keeps its files: `RACER_CONFIG_DIR`, else the platform's
/// per-user config directory.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RACER_CONFIG_DIR").filter(|d| !d.is_empty()) {
        return dir.into();
    }
    let home = |var: &str| std::env::var_os(var).map(PathBuf::from);
    let base = if cfg!(windows) {
        home("APPDATA")
    } else if cfg!(target_os = "macos") {
        home("HOME").map(|h| h.join("Library/Application Support"))
    } else {
        home("XDG_CONFIG_HOME")
            .filter(|p| p.is_absolute())
            .or_else(|| home("HOME").map(|h| h.join(".config")))
    };
    base.unwrap_or_else(std::env::temp_dir).join("racer")
}

/// This device's key: made once, then read back.
pub fn device_key() -> Result<String, String> {
    let path = config_dir().join("device-key");
    if let Ok(key) = std::fs::read_to_string(&path) {
        let key = key.trim().to_string();
        if key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Ok(key);
        }
    }
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let key: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(config_dir()).map_err(|e| format!("{}: {e}", path.display()))?;
    std::fs::write(&path, &key).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(key)
}

/// Talks to the backend from a worker thread.
#[derive(Clone)]
struct BackendClient {
    base: String,
    key: String,
    name: Option<String>,
}

impl BackendClient {
    fn from_env() -> Result<Self, String> {
        Ok(BackendClient {
            base: std::env::var("RACER_BACKEND")
                .unwrap_or_else(|_| "http://127.0.0.1:8080".into())
                .trim_end_matches('/')
                .to_string(),
            key: device_key()?,
            name: std::env::var("RACER_NAME")
                .ok()
                .filter(|n| !n.trim().is_empty()),
        })
    }

    /// `method path` with a JSON body; the lobby in the answer, if any.
    fn call(&self, method: &str, path: &str, body: Option<serde_json::Value>) -> Answer {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(10)))
            .http_status_as_error(false)
            .build()
            .into();
        let url = format!("{}{path}", self.base);
        if let Some(name) = &self.name {
            // Best effort: the name shows in lobbies.
            let _ = agent
                .post(format!("{}/v1/me", self.base))
                .header("x-device-key", &self.key)
                .send_json(serde_json::json!({ "name": name }));
        }
        let request = |m: &str| match m {
            "GET" => agent.get(&url).header("x-device-key", &self.key).call(),
            _ => agent
                .post(&url)
                .header("x-device-key", &self.key)
                .send_json(body.clone().unwrap_or(serde_json::Value::Null)),
        };
        let mut response = request(method).map_err(|e| format!("backend {}: {e}", self.base))?;
        let status = response.status();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| e.to_string())?;
        if !status.is_success() {
            let message = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error"].as_str().map(str::to_string))
                .unwrap_or_else(|| status.to_string());
            return Err(message);
        }
        if text.trim().is_empty() {
            return Ok(None);
        }
        serde_json::from_str(&text)
            .map(Some)
            .map_err(|e| format!("bad answer from the backend: {e}"))
    }
}

impl Online {
    /// Sends a request; the answer comes back through [`poll_backend`].
    pub fn request(&mut self, method: &'static str, path: String, body: Option<serde_json::Value>) {
        let client = match &self.backend {
            Some(c) => c.clone(),
            None => match BackendClient::from_env() {
                Ok(c) => {
                    self.backend = Some(c.clone());
                    c
                }
                Err(e) => {
                    self.status = e;
                    return;
                }
            },
        };
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            tx.send(client.call(method, &path, body)).ok();
        });
        self.pending = Some(Mutex::new(rx));
        self.since_poll = 0.0;
    }

    /// Shows `lobby` without a backend (screenshots).
    pub fn show(&mut self, lobby: LobbyView) {
        self.reset(OnlineMode::Lobby, "");
        self.lobby = Some(lobby);
    }

    pub fn start_quick_match(&mut self, setup: &RaceSetup) {
        self.reset(OnlineMode::Lobby, "Finding a race...");
        let mode = SettingsView::from_setup(setup).mode;
        self.request(
            "POST",
            "/v1/quickmatch".into(),
            Some(serde_json::json!({ "mode": mode, "class": "open", "car": CARS[setup.car] })),
        );
    }

    pub fn create_lobby(&mut self, setup: &RaceSetup) {
        self.reset(OnlineMode::Lobby, "Creating a lobby...");
        let settings = serde_json::to_value(SettingsView::from_setup(setup)).ok();
        self.request("POST", "/v1/lobbies".into(), settings);
    }

    pub fn join_lobby(&mut self, code: &str, setup: &RaceSetup) {
        self.reset(OnlineMode::Lobby, "Joining...");
        self.request(
            "POST",
            format!("/v1/lobbies/{}/join", code.trim().to_ascii_uppercase()),
            Some(serde_json::json!({ "car": CARS[setup.car] })),
        );
    }

    pub fn connect_direct(&mut self, address: &str) -> Result<(), String> {
        let addr = address
            .trim()
            .to_socket_addrs()
            .map_err(|e| format!("{address}: {e}"))?
            .next()
            .ok_or_else(|| format!("{address}: no address"))?;
        self.reset(OnlineMode::Direct(addr), "Connecting...");
        Ok(())
    }

    /// Ready (non-host) with the setup's car.
    pub fn set_ready(&mut self, ready: bool, setup: &RaceSetup) {
        if let Some(code) = self.lobby.as_ref().map(|l| l.code.clone()) {
            self.request(
                "POST",
                format!("/v1/lobbies/{code}/ready"),
                Some(serde_json::json!({ "ready": ready, "car": CARS[setup.car] })),
            );
        }
    }

    /// The host's settings, sent to the backend.
    pub fn push_settings(&mut self, setup: &RaceSetup) {
        if let Some(code) = self.lobby.as_ref().map(|l| l.code.clone()) {
            let settings = serde_json::to_value(SettingsView::from_setup(setup)).ok();
            self.request("POST", format!("/v1/lobbies/{code}/settings"), settings);
        }
    }

    pub fn start(&mut self) {
        if let Some(code) = self.lobby.as_ref().map(|l| l.code.clone()) {
            self.status = "Starting a server...".into();
            self.request("POST", format!("/v1/lobbies/{code}/start"), None);
        }
    }

    fn reset(&mut self, mode: OnlineMode, status: &str) {
        self.mode = mode;
        self.lobby = None;
        self.pending = None;
        self.status = status.into();
    }
}

/// Typing on the text-entry screen (Enter and Esc go through the menus).
fn type_text(mut entry: ResMut<TextEntry>, mut keys: MessageReader<KeyboardInput>) {
    for key in keys.read().filter(|k| k.state.is_pressed()) {
        match &key.logical_key {
            Key::Backspace => {
                entry.text.pop();
            }
            Key::Character(c) => {
                let purpose = entry.purpose;
                let allowed = |ch: char| match purpose {
                    EntryPurpose::JoinCode => ch.is_ascii_alphanumeric(),
                    EntryPurpose::Address => ch.is_ascii_alphanumeric() || ".:-[]".contains(ch),
                };
                for ch in c.chars().filter(|&ch| allowed(ch)) {
                    if entry.text.len() < 64 {
                        entry.text.push(if purpose == EntryPurpose::JoinCode {
                            ch.to_ascii_uppercase()
                        } else {
                            ch
                        });
                    }
                }
            }
            _ => {}
        }
    }
}

/// Answers from the backend, and a poll of the lobby every half second.
fn poll_backend(
    time: Res<Time<Real>>,
    mut online: ResMut<Online>,
    mut setup: ResMut<RaceSetup>,
    mut next: ResMut<NextState<AppState>>,
) {
    if let Some(rx) = &online.pending {
        let answer = rx
            .lock()
            .map_or(Err(TryRecvError::Disconnected), |rx| rx.try_recv());
        match answer {
            Ok(Ok(lobby)) => {
                online.pending = None;
                if let Some(lobby) = lobby {
                    if lobby.host != lobby.you {
                        lobby.settings.apply_to(&mut setup);
                    }
                    online.status = match lobby.state.as_str() {
                        "racing" => "Connecting to the race...".into(),
                        "starting" => "Starting a server...".into(),
                        _ if lobby.quick.is_some() => "Waiting for players...".into(),
                        _ => String::new(),
                    };
                    online.lobby = Some(lobby);
                }
            }
            Ok(Err(e)) => {
                online.pending = None;
                online.status = e;
                if online.lobby.is_none() {
                    // Nothing to show: back to the menu with the message.
                    next.set(AppState::MainMenu);
                }
            }
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => online.pending = None,
        }
    }
    online.since_poll += time.delta_secs();
    let poll = online.since_poll > 0.5 && online.pending.is_none() && online.backend.is_some();
    if poll && let Some(code) = online.lobby.as_ref().map(|l| l.code.clone()) {
        online.request("GET", format!("/v1/lobbies/{code}"), None);
    }
}

/// Connects to the race's server once there is one.
fn connect_to_server(
    mut commands: Commands,
    mut online: ResMut<Online>,
    setup: Res<RaceSetup>,
    config: Option<Res<NetClientConfig>>,
    session: Res<NetSession>,
) {
    if config.is_some() {
        if let SessionState::Failed(e) = &session.state {
            online.status = e.clone();
        }
        return;
    }
    let join = Join {
        name: online
            .me()
            .map(|m| m.name.clone())
            .or_else(|| std::env::var("RACER_NAME").ok())
            .unwrap_or_else(|| "Player".into()),
        car: CARS[setup.car].into(),
        assists: Assists {
            abs: setup.abs,
            tcs: setup.tcs,
            esc: setup.esc,
        },
    };
    let config = match &online.mode {
        OnlineMode::Direct(addr) => {
            NetClientConfig::direct(*addr, u64::from_le_bytes(random_id()), join)
        }
        OnlineMode::Lobby => {
            let Some(lobby) = &online.lobby else { return };
            let Some(server) = &lobby.server else { return };
            let (Ok(addr), Some(key)) = (server.addr.parse(), parse_key(&server.key)) else {
                online.status = format!("bad server address {}", server.addr);
                return;
            };
            NetClientConfig {
                server: addr,
                auth: ClientAuth {
                    key,
                    client_id: lobby.you,
                },
                join,
                conditions: Conditions::default(),
                autopilot: None,
            }
        }
        OnlineMode::Off => return,
    };
    let autopilot = setup.autopilot.then_some(racer_sim::ai::Difficulty::Alien);
    commands.insert_resource(NetClientConfig {
        autopilot,
        ..config
    });
}

fn random_id() -> [u8; 8] {
    let mut b = [0; 8];
    getrandom::fill(&mut b).ok();
    b
}

fn parse_key(hex: &str) -> Option<[u8; 32]> {
    if hex.len() != 64 || !hex.is_ascii() {
        return None;
    }
    let mut key = [0; 32];
    for (i, b) in key.iter_mut().enumerate() {
        *b = u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()?;
    }
    Some(key)
}

/// Into the race once the server has set the grid.
fn enter_race(session: Res<NetSession>, mut next: ResMut<NextState<AppState>>) {
    if session.state == SessionState::Racing {
        next.set(AppState::Race);
    }
}

/// Online cars get their paint; the local one is the player's.
fn dress_cars(
    mut commands: Commands,
    cars: Query<(Entity, &NetCar), Added<NetCar>>,
    setup: Res<RaceSetup>,
    mut settings: ResMut<DriverSettings>,
) {
    for (entity, car) in &cars {
        let mut e = commands.entity(entity);
        e.insert(CarPaint(PAINT[usize::from(car.slot) % PAINT.len()]));
        if car.local {
            e.insert((
                Player,
                DriverAids {
                    steering_assist: setup.steering_assist,
                    racing_line: false,
                    automatic: setup.automatic,
                },
            ));
            *settings = DriverSettings {
                assists: Assists {
                    abs: setup.abs,
                    tcs: setup.tcs,
                    esc: setup.esc,
                },
                steering_assist: setup.steering_assist,
                automatic: setup.automatic,
            };
        }
    }
}

/// Leaving the online session (back at the main menu).
fn leave(
    mut commands: Commands,
    mut online: ResMut<Online>,
    session: Res<NetSession>,
    config: Option<Res<NetClientConfig>>,
) {
    if let Some(code) = online.lobby.as_ref().map(|l| l.code.clone()) {
        online.request("POST", format!("/v1/lobbies/{code}/leave"), None);
    }
    online.lobby = None;
    online.mode = OnlineMode::Off;
    online.pending = None;
    if config.is_some() || session.state != SessionState::Idle {
        commands.run_system_cached(racer_sim::net::client::leave);
    }
}

#[derive(Component)]
struct PingText;

/// Round-trip time, top centre, with a warning above 250 ms.
fn spawn_ping(mut commands: Commands) {
    commands
        .spawn((
            DespawnOnExit(AppState::Race),
            Node {
                position_type: PositionType::Absolute,
                top: px(12),
                width: percent(100),
                justify_content: JustifyContent::Center,
                ..default()
            },
        ))
        .with_child((
            PingText,
            Text::new(""),
            TextFont::from_font_size(16.0),
            TextColor(Color::WHITE),
            Node {
                padding: UiRect::axes(px(8), px(3)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
        ));
}

/// Above this round-trip time the HUD warns.
pub const HIGH_PING_MS: u32 = 250;

pub fn ping_label(rtt: Duration) -> (String, bool) {
    let ms = rtt.as_millis() as u32;
    if ms > HIGH_PING_MS {
        (format!("Ping {ms} ms - high latency"), true)
    } else {
        (format!("Ping {ms} ms"), false)
    }
}

fn update_ping(
    session: Res<NetSession>,
    mut texts: Query<(&mut Text, &mut TextColor), With<PingText>>,
) {
    let (label, high) = ping_label(session.rtt);
    for (mut text, mut color) in &mut texts {
        if text.0 != label {
            text.0 = label.clone();
        }
        color.0 = if high {
            Color::srgb(1.0, 0.3, 0.2)
        } else {
            Color::WHITE
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_through_the_setup() {
        let mut setup = RaceSetup {
            mode: RaceMode::Sprint,
            laps: 4,
            collisions: Collisions::GhostFirstLap,
            ai_fill: false,
            ..default()
        };
        let view = SettingsView::from_setup(&setup);
        assert_eq!(view.collisions, "ghost-first-lap");
        let mut other = RaceSetup::default();
        view.apply_to(&mut other);
        assert_eq!(
            (other.mode, other.laps, other.collisions, other.ai_fill),
            (setup.mode, setup.laps, setup.collisions, setup.ai_fill)
        );
        setup.laps = 99;
        SettingsView::from_setup(&setup).apply_to(&mut other);
        assert_eq!(other.laps, 10);
    }

    #[test]
    fn ping_warns_above_250_ms() {
        assert_eq!(
            ping_label(Duration::from_millis(80)),
            ("Ping 80 ms".into(), false)
        );
        assert!(ping_label(Duration::from_millis(251)).1);
    }

    #[test]
    fn device_key_is_made_once() {
        let dir = std::env::temp_dir().join(format!("racer-config-{}", std::process::id()));
        // SAFETY: tests in this binary don't read RACER_CONFIG_DIR concurrently.
        unsafe { std::env::set_var("RACER_CONFIG_DIR", &dir) };
        let a = device_key().unwrap();
        let b = device_key().unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 64);
        std::fs::remove_dir_all(dir).ok();
    }
}
