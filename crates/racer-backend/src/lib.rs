//! Backend HTTP service: player identity, invite-code lobbies, quick match
//! and server allocation.
//!
//! Players are identified by a device key: a random secret the client makes
//! once and keeps locally, sent in the `x-device-key` header with every
//! request (no accounts). Lobbies live in memory; with
//! `RACER_BACKEND_STATE` set, the players (key, id, name) are kept in that
//! JSON file across restarts.
//!
//! When a lobby starts, a [`Fleet`] starts a dedicated server for it and the
//! lobby hands its members the server's address and the race's netcode key.

pub mod fleet;
mod lobby;

use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, Mutex},
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use serde::{Deserialize, Serialize};

pub use fleet::{Allocation, Fleet, LocalFleet, RaceRequest};
pub use lobby::{Lobby, LobbyState, Member, ServerInfo, Settings};

/// Device keys are this many bytes, sent as hex.
pub const DEVICE_KEY_BYTES: usize = 32;

/// Seconds a quick match waits for more players before starting.
pub const QUICK_MATCH_WAIT: f64 = 15.0;

/// Car classes for quick match (`open` takes any car).
pub const CLASSES: [&str; 4] = ["open", "hot-hatch", "sport-sedan", "sports-gt"];

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Player {
    pub id: u64,
    pub name: String,
}

#[derive(Default, Serialize, Deserialize)]
struct Players {
    /// Device key (hex) to player.
    by_key: HashMap<String, Player>,
    next_id: u64,
}

/// Shared service state.
#[derive(Clone)]
pub struct Backend {
    inner: Arc<Mutex<Inner>>,
    fleet: Arc<dyn Fleet>,
    state_file: Option<PathBuf>,
}

struct Inner {
    players: Players,
    lobbies: HashMap<String, Lobby>,
}

impl Backend {
    /// A backend allocating servers with `fleet`, keeping players in
    /// `state_file` if given.
    pub fn new(fleet: Arc<dyn Fleet>, state_file: Option<PathBuf>) -> Self {
        let players = state_file
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Backend {
            inner: Arc::new(Mutex::new(Inner {
                players,
                lobbies: HashMap::new(),
            })),
            fleet,
            state_file,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn save(&self, players: &Players) {
        let Some(path) = &self.state_file else { return };
        match serde_json::to_string_pretty(players) {
            Ok(json) => {
                // Written aside and renamed, so a crash never leaves half a file.
                let tmp = path.with_extension("tmp");
                if let Err(e) = std::fs::write(&tmp, json).and_then(|_| std::fs::rename(&tmp, path))
                {
                    eprintln!("racer-backend: {}: {e}", path.display());
                }
            }
            Err(e) => eprintln!("racer-backend: {e}"),
        }
    }

    /// The player behind the request's device key, registered on first use.
    fn player(&self, headers: &HeaderMap) -> Result<Player, ApiError> {
        let key = headers
            .get("x-device-key")
            .and_then(|v| v.to_str().ok())
            .map(str::to_ascii_lowercase)
            .filter(|k| k.len() == 2 * DEVICE_KEY_BYTES && k.bytes().all(|b| b.is_ascii_hexdigit()))
            .ok_or(ApiError::unauthorized())?;
        let mut inner = self.lock();
        if let Some(p) = inner.players.by_key.get(&key) {
            return Ok(p.clone());
        }
        inner.players.next_id += 1;
        let id = inner.players.next_id;
        let player = Player {
            id,
            name: format!("Player {id}"),
        };
        inner.players.by_key.insert(key, player.clone());
        self.save(&inner.players);
        Ok(player)
    }

    /// Starts lobbies that are due: quick matches whose wait is over.
    pub async fn tick(&self, now: f64) {
        let due: Vec<String> = self
            .lock()
            .lobbies
            .values()
            .filter(|l| l.quick_match_due(now))
            .map(|l| l.code.clone())
            .collect();
        for code in due {
            if let Err(e) = self.start(&code).await {
                eprintln!("racer-backend: quick match {code}: {}", e.message);
            }
        }
        // Lobbies whose race ended long ago, or that everyone left.
        self.lock()
            .lobbies
            .retain(|_, l| !l.members.is_empty() && !l.expired(now));
    }

    /// Allocates a server for the lobby and hands it to its members.
    async fn start(&self, code: &str) -> Result<Lobby, ApiError> {
        let request = {
            let mut inner = self.lock();
            let lobby = inner.lobbies.get_mut(code).ok_or(ApiError::not_found())?;
            if lobby.state != LobbyState::Open {
                return Err(ApiError::conflict("the race has already started"));
            }
            lobby.state = LobbyState::Starting;
            RaceRequest {
                players: lobby.members.len(),
                settings: lobby.settings.clone(),
                key: random_bytes(),
            }
        };
        let fleet = self.fleet.clone();
        let key = request.key;
        let allocated = tokio::task::spawn_blocking(move || fleet.allocate(&request))
            .await
            .map_err(|e| ApiError::internal(e.to_string()))
            .and_then(|r| r.map_err(ApiError::internal));
        let mut inner = self.lock();
        let lobby = inner.lobbies.get_mut(code).ok_or(ApiError::not_found())?;
        match allocated {
            Ok(a) => {
                lobby.state = LobbyState::Racing;
                lobby.started_at = Some(now_secs());
                lobby.server = Some(ServerInfo {
                    addr: a.addr.to_string(),
                    key: hex(&key),
                });
                Ok(lobby.clone())
            }
            Err(e) => {
                lobby.state = LobbyState::Open;
                Err(e)
            }
        }
    }
}

/// Seconds since the Unix epoch.
fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0; N];
    getrandom::fill(&mut b).expect("the OS random source");
    b
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// An error answer: status and a message.
#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
        }
    }
    fn unauthorized() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "missing or malformed x-device-key (64 hex digits)",
        )
    }
    fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "no such lobby")
    }
    fn forbidden(m: &str) -> Self {
        Self::new(StatusCode::FORBIDDEN, m)
    }
    fn conflict(m: &str) -> Self {
        Self::new(StatusCode::CONFLICT, m)
    }
    fn bad_request(m: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, m)
    }
    fn internal(m: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, m)
    }
}

impl axum::response::IntoResponse for ApiError {
    fn into_response(self) -> axum::response::Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.message,
            }),
        )
            .into_response()
    }
}

#[derive(Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: String,
}

type ApiResult<T> = Result<Json<T>, ApiError>;

/// Builds the service router.
pub fn router(backend: Backend) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/me", get(me).post(rename))
        .route("/v1/lobbies", post(create_lobby))
        .route("/v1/lobbies/{code}", get(get_lobby))
        .route("/v1/lobbies/{code}/join", post(join_lobby))
        .route("/v1/lobbies/{code}/leave", post(leave_lobby))
        .route("/v1/lobbies/{code}/ready", post(set_ready))
        .route("/v1/lobbies/{code}/settings", post(set_settings))
        .route("/v1/lobbies/{code}/start", post(start_lobby))
        .route("/v1/quickmatch", post(quick_match))
        .with_state(backend)
}

async fn health() -> Json<&'static str> {
    Json("ok")
}

async fn me(State(b): State<Backend>, headers: HeaderMap) -> ApiResult<Player> {
    Ok(Json(b.player(&headers)?))
}

#[derive(Deserialize)]
struct Rename {
    name: String,
}

async fn rename(
    State(b): State<Backend>,
    headers: HeaderMap,
    Json(body): Json<Rename>,
) -> ApiResult<Player> {
    let player = b.player(&headers)?;
    let name = body.name.trim();
    if name.is_empty() || name.chars().count() > 24 || name.chars().any(char::is_control) {
        return Err(ApiError::bad_request(
            "a name is 1 to 24 printable characters",
        ));
    }
    let mut inner = b.lock();
    let mut renamed = player.clone();
    for p in inner
        .players
        .by_key
        .values_mut()
        .filter(|p| p.id == player.id)
    {
        p.name = name.to_string();
        renamed = p.clone();
    }
    for lobby in inner.lobbies.values_mut() {
        for m in lobby.members.iter_mut().filter(|m| m.id == player.id) {
            m.name = name.to_string();
        }
    }
    b.save(&inner.players);
    Ok(Json(renamed))
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct JoinBody {
    car: String,
}

async fn create_lobby(
    State(b): State<Backend>,
    headers: HeaderMap,
    body: Option<Json<Settings>>,
) -> ApiResult<Lobby> {
    let player = b.player(&headers)?;
    let settings = body.map(|Json(s)| s).unwrap_or_default();
    settings.validate().map_err(ApiError::bad_request)?;
    let mut inner = b.lock();
    leave_all(&mut inner, player.id);
    let code = loop {
        let code = lobby::invite_code(&random_bytes::<6>());
        if !inner.lobbies.contains_key(&code) {
            break code;
        }
    };
    let lobby = Lobby::new(code.clone(), &player, settings, None, now_secs());
    inner.lobbies.insert(code, lobby.clone());
    Ok(Json(lobby.view(player.id)))
}

fn leave_all(inner: &mut Inner, id: u64) {
    for lobby in inner.lobbies.values_mut() {
        lobby.leave(id);
    }
}

async fn get_lobby(
    State(b): State<Backend>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> ApiResult<Lobby> {
    let player = b.player(&headers)?;
    let inner = b.lock();
    let lobby = inner
        .lobbies
        .get(&code.to_ascii_uppercase())
        .ok_or(ApiError::not_found())?;
    if !lobby.is_member(player.id) {
        return Err(ApiError::forbidden("join the lobby first"));
    }
    Ok(Json(lobby.view(player.id)))
}

async fn join_lobby(
    State(b): State<Backend>,
    headers: HeaderMap,
    Path(code): Path<String>,
    body: Option<Json<JoinBody>>,
) -> ApiResult<Lobby> {
    let player = b.player(&headers)?;
    let car = body.map(|Json(j)| j.car).unwrap_or_default();
    let code = code.to_ascii_uppercase();
    let mut inner = b.lock();
    if !inner.lobbies.contains_key(&code) {
        return Err(ApiError::not_found());
    }
    if !inner.lobbies[&code].is_member(player.id) {
        leave_all(&mut inner, player.id);
    }
    let lobby = inner.lobbies.get_mut(&code).ok_or(ApiError::not_found())?;
    lobby.join(&player, car).map_err(ApiError::conflict)?;
    Ok(Json(lobby.view(player.id)))
}

async fn leave_lobby(
    State(b): State<Backend>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> Result<StatusCode, ApiError> {
    let player = b.player(&headers)?;
    let mut inner = b.lock();
    let lobby = inner
        .lobbies
        .get_mut(&code.to_ascii_uppercase())
        .ok_or(ApiError::not_found())?;
    lobby.leave(player.id);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct Ready {
    ready: bool,
    #[serde(default)]
    car: Option<String>,
}

async fn set_ready(
    State(b): State<Backend>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(body): Json<Ready>,
) -> ApiResult<Lobby> {
    let player = b.player(&headers)?;
    let mut inner = b.lock();
    let lobby = inner
        .lobbies
        .get_mut(&code.to_ascii_uppercase())
        .ok_or(ApiError::not_found())?;
    let member = lobby
        .members
        .iter_mut()
        .find(|m| m.id == player.id)
        .ok_or(ApiError::forbidden("join the lobby first"))?;
    member.ready = body.ready;
    if let Some(car) = body.car {
        member.car = car;
    }
    Ok(Json(lobby.view(player.id)))
}

async fn set_settings(
    State(b): State<Backend>,
    headers: HeaderMap,
    Path(code): Path<String>,
    Json(settings): Json<Settings>,
) -> ApiResult<Lobby> {
    let player = b.player(&headers)?;
    settings.validate().map_err(ApiError::bad_request)?;
    let mut inner = b.lock();
    let lobby = inner
        .lobbies
        .get_mut(&code.to_ascii_uppercase())
        .ok_or(ApiError::not_found())?;
    if lobby.host != player.id {
        return Err(ApiError::forbidden("only the host changes the settings"));
    }
    if lobby.state != LobbyState::Open {
        return Err(ApiError::conflict("the race has already started"));
    }
    lobby.settings = settings;
    // Everyone confirms again under the new settings.
    for m in lobby.members.iter_mut().filter(|m| m.id != player.id) {
        m.ready = false;
    }
    Ok(Json(lobby.view(player.id)))
}

async fn start_lobby(
    State(b): State<Backend>,
    headers: HeaderMap,
    Path(code): Path<String>,
) -> ApiResult<Lobby> {
    let player = b.player(&headers)?;
    let code = code.to_ascii_uppercase();
    {
        let inner = b.lock();
        let lobby = inner.lobbies.get(&code).ok_or(ApiError::not_found())?;
        if lobby.host != player.id {
            return Err(ApiError::forbidden("only the host starts the race"));
        }
        if !lobby.all_ready() {
            return Err(ApiError::conflict("not everyone is ready"));
        }
    }
    let lobby = b.start(&code).await?;
    Ok(Json(lobby.view(player.id)))
}

#[derive(Deserialize)]
struct QuickMatch {
    #[serde(default)]
    mode: lobby::Mode,
    #[serde(default = "open_class")]
    class: String,
    #[serde(default)]
    car: String,
}

fn open_class() -> String {
    "open".into()
}

/// Joins the open public lobby for the mode and class, or opens one.
async fn quick_match(
    State(b): State<Backend>,
    headers: HeaderMap,
    Json(q): Json<QuickMatch>,
) -> ApiResult<Lobby> {
    let player = b.player(&headers)?;
    if !CLASSES.contains(&q.class.as_str()) {
        return Err(ApiError::bad_request(format!(
            "unknown class {:?}; one of {CLASSES:?}",
            q.class
        )));
    }
    let mut inner = b.lock();
    leave_all(&mut inner, player.id);
    let open = inner
        .lobbies
        .values()
        .find(|l| {
            l.quick.as_deref() == Some(&q.class)
                && l.settings.mode == q.mode
                && l.state == LobbyState::Open
                && l.members.len() < lobby::MAX_PLAYERS
        })
        .map(|l| l.code.clone());
    let code = match open {
        Some(code) => code,
        None => {
            let code = loop {
                let code = lobby::invite_code(&random_bytes::<6>());
                if !inner.lobbies.contains_key(&code) {
                    break code;
                }
            };
            let settings = Settings {
                mode: q.mode,
                ..Settings::default()
            };
            let lobby = Lobby::new(code.clone(), &player, settings, Some(q.class), now_secs());
            inner.lobbies.insert(code.clone(), lobby);
            code
        }
    };
    let lobby = inner.lobbies.get_mut(&code).ok_or(ApiError::not_found())?;
    lobby.join(&player, q.car).map_err(ApiError::conflict)?;
    Ok(Json(lobby.view(player.id)))
}

/// Runs [`Backend::tick`] every second.
pub fn spawn_ticker(backend: Backend) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(1));
        loop {
            interval.tick().await;
            backend.tick(now_secs()).await;
        }
    });
}

#[cfg(test)]
mod tests;
