//! HTTP router and shared application state.

use std::sync::Arc;

use axum::routing::get;
use axum::{Json, Router};
use schisaciaccole_core::api::HEALTH_PATH;
use serde::Serialize;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;

use crate::config::Config;

/// State shared by all request handlers. Cloning is cheap.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Server configuration.
    pub config: Arc<Config>,
    /// Connection pool of the application database.
    pub pool: PgPool,
}

impl AppState {
    /// Creates the state from an already validated configuration and a pool.
    pub fn new(config: Config, pool: PgPool) -> Self {
        Self {
            config: Arc::new(config),
            pool,
        }
    }
}

/// Body of the health check response.
#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

/// Builds the router with all routes and the tracing layer.
pub fn router(state: AppState) -> Router {
    Router::new()
        .route(HEALTH_PATH, get(health))
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Liveness probe: always answers 200 while the process serves requests.
async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    fn test_state() -> AppState {
        let config = Config::from_lookup(|name| match name {
            "DATABASE_URL" => Some("postgres://u:p@localhost/x".to_string()),
            "PUBLIC_BASE_URL" => Some("http://localhost:8080".to_string()),
            "GOOGLE_CLIENT_ID" => Some("id".to_string()),
            "GOOGLE_CLIENT_SECRET" => Some("secret".to_string()),
            "ALLOWED_HD" => Some("example.com".to_string()),
            "TOKEN_ENC_KEY" => Some("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=".to_string()),
            "JWT_SIGNING_KEY" => Some("0123456789abcdef0123456789abcdef".to_string()),
            _ => None,
        })
        .unwrap();
        let pool = PgPoolOptions::new()
            .connect_lazy("postgres://u:p@localhost/x")
            .unwrap();
        AppState::new(config, pool)
    }

    #[tokio::test]
    async fn healthz_returns_ok_json() {
        let request = Request::get(HEALTH_PATH).body(Body::empty()).unwrap();

        let response = router(test_state()).oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json, serde_json::json!({"status": "ok"}));
    }

    #[tokio::test]
    async fn unknown_path_returns_404() {
        let request = Request::get("/nope").body(Body::empty()).unwrap();

        let response = router(test_state()).oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
