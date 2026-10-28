use std::sync::atomic::{AtomicUsize, Ordering};

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::{Value, json};
use tower::ServiceExt;

use super::*;

/// A fleet that "starts" servers at a fixed address.
#[derive(Default)]
struct FakeFleet(AtomicUsize);

impl Fleet for FakeFleet {
    fn allocate(&self, race: &RaceRequest) -> Result<Allocation, String> {
        self.0.fetch_add(race.players, Ordering::SeqCst);
        Ok(Allocation {
            addr: "10.0.0.1:7777".parse().unwrap(),
        })
    }
}

fn key(n: u8) -> String {
    hex(&[n; DEVICE_KEY_BYTES])
}

async fn call(
    app: &Router,
    method: &str,
    uri: &str,
    who: Option<u8>,
    body: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder().method(method).uri(uri);
    if let Some(n) = who {
        req = req.header("x-device-key", key(n));
    }
    let req = if body.is_null() {
        req.body(Body::empty())
    } else {
        req.header("content-type", "application/json")
            .body(Body::from(body.to_string()))
    }
    .unwrap();
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

fn app() -> (Router, Backend, Arc<FakeFleet>) {
    let fleet = Arc::new(FakeFleet::default());
    let backend = Backend::new(fleet.clone(), None);
    (router(backend.clone()), backend, fleet)
}

#[tokio::test]
async fn health_returns_ok() {
    let (app, ..) = app();
    let response = app
        .oneshot(Request::get("/health").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    assert_eq!(&body[..], br#""ok""#);
}

#[tokio::test]
async fn unknown_route_is_404() {
    let (app, ..) = app();
    let (status, _) = call(&app, "GET", "/nope", None, Value::Null).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn device_keys_identify_players() {
    let (app, ..) = app();
    let (status, _) = call(&app, "GET", "/v1/me", None, Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let (_, me) = call(&app, "GET", "/v1/me", Some(1), Value::Null).await;
    let (_, again) = call(&app, "POST", "/v1/me", Some(1), json!({"name": "Ana"})).await;
    assert_eq!(me["id"], again["id"]);
    assert_eq!(again["name"], "Ana");
    let (_, other) = call(&app, "GET", "/v1/me", Some(2), Value::Null).await;
    assert_ne!(me["id"], other["id"]);
    let (status, _) = call(&app, "POST", "/v1/me", Some(1), json!({"name": ""})).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn invite_lobby_from_create_to_server() {
    let (app, _, fleet) = app();
    let (status, lobby) = call(&app, "POST", "/v1/lobbies", Some(1), json!({"laps": 2})).await;
    assert_eq!(status, StatusCode::OK);
    let code = lobby["code"].as_str().unwrap().to_string();
    assert_eq!(lobby["settings"]["laps"], 2);
    let base = format!("/v1/lobbies/{code}");

    // Strangers see nothing; joining by code (any case) works.
    let (status, _) = call(&app, "GET", &base, Some(2), Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, lobby) = call(
        &app,
        "POST",
        &format!("/v1/lobbies/{}/join", code.to_lowercase()),
        Some(2),
        json!({"car": "bmw-m3-competition-g80"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lobby["members"].as_array().unwrap().len(), 2);

    // Only the host changes settings or starts, and only once all are ready.
    let (status, _) = call(
        &app,
        "POST",
        &format!("{base}/settings"),
        Some(2),
        json!({"laps": 5}),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, lobby) = call(
        &app,
        "POST",
        &format!("{base}/settings"),
        Some(1),
        json!({"laps": 5, "collisions": "ghost-first-lap", "ai_fill": false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(lobby["settings"]["collisions"], "ghost-first-lap");
    let (status, _) = call(&app, "POST", &format!("{base}/start"), Some(1), Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
    call(
        &app,
        "POST",
        &format!("{base}/ready"),
        Some(2),
        json!({"ready": true}),
    )
    .await;
    let (status, _) = call(&app, "POST", &format!("{base}/start"), Some(2), Value::Null).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    let (status, lobby) = call(&app, "POST", &format!("{base}/start"), Some(1), Value::Null).await;
    assert_eq!(status, StatusCode::OK, "{lobby}");
    assert_eq!(lobby["state"], "racing");
    assert_eq!(lobby["server"]["addr"], "10.0.0.1:7777");
    assert_eq!(lobby["server"]["key"].as_str().unwrap().len(), 64);
    assert_eq!(fleet.0.load(Ordering::SeqCst), 2);

    // Members see the server; nobody joins a running race.
    let (_, seen) = call(&app, "GET", &base, Some(2), Value::Null).await;
    assert_eq!(seen["server"], lobby["server"]);
    let (status, _) = call(&app, "POST", &format!("{base}/join"), Some(3), Value::Null).await;
    assert_eq!(status, StatusCode::CONFLICT);
}

#[tokio::test]
async fn leaving_passes_the_host_on() {
    let (app, ..) = app();
    let (_, lobby) = call(&app, "POST", "/v1/lobbies", Some(1), Value::Null).await;
    let code = lobby["code"].as_str().unwrap().to_string();
    call(
        &app,
        "POST",
        &format!("/v1/lobbies/{code}/join"),
        Some(2),
        Value::Null,
    )
    .await;
    let (status, _) = call(
        &app,
        "POST",
        &format!("/v1/lobbies/{code}/leave"),
        Some(1),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, lobby) = call(
        &app,
        "GET",
        &format!("/v1/lobbies/{code}"),
        Some(2),
        Value::Null,
    )
    .await;
    assert_eq!(lobby["host"], lobby["you"]);
}

#[tokio::test]
async fn quick_match_groups_by_mode_and_class_and_starts_by_itself() {
    let (app, backend, fleet) = app();
    let (_, a) = call(
        &app,
        "POST",
        "/v1/quickmatch",
        Some(1),
        json!({"class": "hot-hatch"}),
    )
    .await;
    let (_, b) = call(
        &app,
        "POST",
        "/v1/quickmatch",
        Some(2),
        json!({"class": "hot-hatch"}),
    )
    .await;
    let (_, c) = call(
        &app,
        "POST",
        "/v1/quickmatch",
        Some(3),
        json!({"class": "open", "mode": "sprint"}),
    )
    .await;
    assert_eq!(a["code"], b["code"]);
    assert_ne!(a["code"], c["code"]);
    let (status, _) = call(
        &app,
        "POST",
        "/v1/quickmatch",
        Some(4),
        json!({"class": "trucks"}),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // Not yet due; then due after the wait.
    backend.tick(now_secs()).await;
    assert_eq!(fleet.0.load(Ordering::SeqCst), 0);
    backend.tick(now_secs() + QUICK_MATCH_WAIT + 1.0).await;
    assert_eq!(fleet.0.load(Ordering::SeqCst), 3);
    let code = a["code"].as_str().unwrap();
    let (_, lobby) = call(
        &app,
        "GET",
        &format!("/v1/lobbies/{code}"),
        Some(1),
        Value::Null,
    )
    .await;
    assert_eq!(lobby["state"], "racing");
}

#[tokio::test]
async fn players_survive_a_restart_with_a_state_file() {
    let dir = std::env::temp_dir().join(format!("racer-backend-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("state.json");
    let fleet: Arc<dyn Fleet> = Arc::new(FakeFleet::default());
    let first = router(Backend::new(fleet.clone(), Some(file.clone())));
    let (_, me) = call(&first, "POST", "/v1/me", Some(7), json!({"name": "Kim"})).await;
    let second = router(Backend::new(fleet, Some(file)));
    let (_, again) = call(&second, "GET", "/v1/me", Some(7), Value::Null).await;
    assert_eq!(me, again);
    std::fs::remove_dir_all(dir).ok();
}
