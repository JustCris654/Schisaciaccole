//! HTTP router and shared application state.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use axum::routing::{get, post};
use axum::{Json, Router};
use schisaciaccole_core::api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH,
    HEALTH_PATH, ME_PATH, MEETINGS_PATH,
};
use serde::Serialize;
use sqlx::PgPool;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_governor::key_extractor::SmartIpKeyExtractor;
use tower_http::trace::TraceLayer;
use tracing::info_span;

use crate::auth;
use crate::config::Config;
use crate::google::GoogleOidc;
use crate::google_tokens::GoogleTokenCache;
use crate::login_store::LoginStore;
use crate::meetings;

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
    /// Google access tokens of the users.
    pub google_tokens: Arc<GoogleTokenCache>,
}

impl AppState {
    /// Creates the state from an already validated configuration, a pool
    /// and a Google client, with an empty login store and token cache.
    pub fn new(config: Config, pool: PgPool, google: GoogleOidc) -> Self {
        Self {
            config: Arc::new(config),
            pool,
            google: Arc::new(google),
            logins: Arc::new(LoginStore::new()),
            google_tokens: Arc::new(GoogleTokenCache::new()),
        }
    }
}

/// Body of the health check response.
#[derive(Debug, Serialize)]
struct Health {
    status: &'static str,
}

/// Per-IP rate limit of the `/auth/*` routes: a client may send `burst`
/// requests at once, then one more every `replenish_every`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthRateLimit {
    /// Requests allowed in a burst.
    pub burst: u32,
    /// Time after which one request of the burst is available again.
    pub replenish_every: Duration,
}

impl Default for AuthRateLimit {
    /// Burst of 10 requests, one more every 2 seconds.
    fn default() -> Self {
        Self {
            burst: 10,
            replenish_every: Duration::from_secs(2),
        }
    }
}

/// How often the rate limiter forgets the IPs that are back to a full burst.
const RATE_LIMIT_CLEANUP_PERIOD: Duration = Duration::from_secs(60);

/// Builds the router with all routes, the tracing layer and the default
/// [`AuthRateLimit`] on the `/auth/*` routes.
///
/// Must be called inside a Tokio runtime. See [`router_with_limit`].
pub fn router(state: AppState) -> Router {
    router_with_limit(state, Some(AuthRateLimit::default()))
}

/// Builds the router like [`router`], with `auth_limit` as the per-IP rate
/// limit of the `/auth/*` routes, or no limit when it is `None`.
///
/// The client IP is read from `X-Forwarded-For`, `X-Real-IP` or `Forwarded`,
/// then from the peer address, so the server must run behind a proxy that
/// sets those headers itself; otherwise a client could pick its own IP. A
/// request with none of them and no peer address (the server not started
/// with connect info) is answered with a 500. Requests over the limit get a
/// 429 with a `Retry-After` header. `/healthz` and `/api/v1/*` are not
/// limited.
///
/// Request spans record the method and the path only: query strings carry
/// login states and codes, which must never be logged.
///
/// Must be called inside a Tokio runtime when `auth_limit` is `Some`: it
/// spawns the task that clears the limiter.
pub fn router_with_limit(state: AppState, auth_limit: Option<AuthRateLimit>) -> Router {
    let trace = TraceLayer::new_for_http().make_span_with(|request: &Request| {
        info_span!(
            "request",
            method = %request.method(),
            path = %request.uri().path(),
        )
    });
    let mut auth_routes = Router::new()
        .route(AUTH_LOGIN_PATH, get(auth::login))
        .route(AUTH_CALLBACK_PATH, get(auth::google_callback))
        .route(AUTH_TOKEN_PATH, post(auth::token))
        .route(AUTH_REFRESH_PATH, post(auth::refresh))
        .route(AUTH_LOGOUT_PATH, post(auth::logout));
    if let Some(limit) = auth_limit {
        auth_routes = with_rate_limit(auth_routes, limit);
    }
    Router::new()
        .route(HEALTH_PATH, get(health))
        .merge(auth_routes)
        .route(ME_PATH, get(auth::me))
        .route(MEETINGS_PATH, get(meetings::list_meetings))
        .layer(trace)
        .with_state(state)
}

/// Adds the per-IP rate limit to `routes` and starts the task that
/// periodically drops the limiter entries of idle IPs.
///
/// A `limit` with a zero burst or a zero period is replaced by the default
/// limit.
fn with_rate_limit(routes: Router<AppState>, limit: AuthRateLimit) -> Router<AppState> {
    let limit = if limit.burst == 0 || limit.replenish_every.is_zero() {
        AuthRateLimit::default()
    } else {
        limit
    };
    let config = GovernorConfigBuilder::default()
        .period(limit.replenish_every)
        .burst_size(limit.burst)
        .key_extractor(SmartIpKeyExtractor)
        .finish();
    let Some(config) = config else {
        return routes;
    };
    let limiter = config.limiter().clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(RATE_LIMIT_CLEANUP_PERIOD).await;
            limiter.retain_recent();
        }
    });
    routes.route_layer(GovernorLayer::new(config))
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

    async fn login_status_from(app: &Router, ip: &str) -> StatusCode {
        let request = Request::get(AUTH_LOGIN_PATH)
            .header("x-forwarded-for", ip)
            .body(Body::empty())
            .unwrap();
        app.clone().oneshot(request).await.unwrap().status()
    }

    // Without parameters /auth/login answers 400, so a 400 means the request
    // went through the limiter.
    #[tokio::test]
    async fn auth_routes_answer_429_after_the_burst() {
        let app = router(test_state());
        for _ in 0..10 {
            assert_eq!(
                login_status_from(&app, "203.0.113.1").await,
                StatusCode::BAD_REQUEST
            );
        }

        let status = login_status_from(&app, "203.0.113.1").await;

        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn rate_limit_is_per_ip() {
        let app = router(test_state());
        for _ in 0..11 {
            login_status_from(&app, "203.0.113.1").await;
        }

        let status = login_status_from(&app, "203.0.113.2").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn healthz_is_not_rate_limited() {
        let app = router(test_state());
        for _ in 0..11 {
            login_status_from(&app, "203.0.113.1").await;
        }

        let request = Request::get(HEALTH_PATH)
            .header("x-forwarded-for", "203.0.113.1")
            .body(Body::empty())
            .unwrap();
        let response = app.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_routes_are_not_limited_without_a_limit() {
        let app = router_with_limit(test_state(), None);
        for _ in 0..11 {
            login_status_from(&app, "203.0.113.1").await;
        }

        let status = login_status_from(&app, "203.0.113.1").await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
