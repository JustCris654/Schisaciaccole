//! HTTP router and shared application state.

use std::sync::Arc;

use axum::extract::Request;
use axum::routing::{get, post};
use axum::{Json, Router};
use schisaciaccole_core::api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH,
    HEALTH_PATH, ME_PATH,
};
use serde::Serialize;
use sqlx::PgPool;
use tower_http::trace::TraceLayer;
use tracing::info_span;

use crate::auth;
use crate::config::Config;
use crate::google::GoogleOidc;
use crate::login_store::LoginStore;

/// State shared by all request handlers. Cloning is cheap.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Server configuration.
    pub config: Arc<Config>,
    /// Connection pool of the application database.
    pub pool: PgPool,
    /// Google OpenID Connect client.
    pub google: Arc<GoogleOidc>,
    /// Logins in progress and one-time codes.
    pub logins: Arc<LoginStore>,
}

impl AppState {
    /// Creates the state from an already validated configuration, a pool
    /// and a Google client, with an empty login store.
    pub fn new(config: Config, pool: PgPool, google: GoogleOidc) -> Self {
        Self {
            config: Arc::new(config),
            pool,
            google: Arc::new(google),
            logins: Arc::new(LoginStore::new()),
        }
    }
}

/// Body of the health check response.
#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

/// Builds the router with all routes and the tracing layer.
///
/// Request spans record the method and the path only: query strings carry
/// login states and codes, which must never be logged.
pub fn router(state: AppState) -> Router {
    let trace = TraceLayer::new_for_http().make_span_with(|request: &Request| {
        info_span!(
            "request",
            method = %request.method(),
            path = %request.uri().path(),
        )
    });
    Router::new()
        .route(HEALTH_PATH, get(health))
        .route(AUTH_LOGIN_PATH, get(auth::login))
        .route(AUTH_CALLBACK_PATH, get(auth::google_callback))
        .route(AUTH_TOKEN_PATH, post(auth::token))
        .route(AUTH_REFRESH_PATH, post(auth::refresh))
        .route(AUTH_LOGOUT_PATH, post(auth::logout))
        .route(ME_PATH, get(auth::me))
        .layer(trace)
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
        let metadata = serde_json::from_value(serde_json::json!({
            "issuer": "https://issuer.example.com",
            "authorization_endpoint": "https://issuer.example.com/authorize",
            "token_endpoint": "https://issuer.example.com/token",
            "jwks_uri": "https://issuer.example.com/jwks",
            "response_types_supported": ["code"],
            "subject_types_supported": ["public"],
            "id_token_signing_alg_values_supported": ["RS256"],
        }))
        .unwrap();
        let google = GoogleOidc::from_metadata(
            &config,
            AUTH_CALLBACK_PATH,
            metadata,
            openidconnect::reqwest::Client::new(),
        )
        .unwrap();
        AppState::new(config, pool, google)
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
