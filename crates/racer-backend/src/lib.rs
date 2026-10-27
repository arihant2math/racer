//! Backend HTTP service.

use axum::{Json, Router, routing::get};

/// Builds the service router.
pub fn router() -> Router {
    Router::new().route("/health", get(health))
}

async fn health() -> Json<&'static str> {
    Json("ok")
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_returns_ok() {
        let response = router()
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        assert_eq!(&body[..], br#""ok""#);
    }

    #[tokio::test]
    async fn unknown_route_is_404() {
        let response = router()
            .oneshot(Request::get("/nope").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
