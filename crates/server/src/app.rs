//! HTTP router and shared application state.

use std::sync::Arc;
use std::time::Duration;

use axum::extract::Request;
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use schisaciaccole_core::api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH,
    HEALTH_PATH, ME_PATH, MEETING_PLAN_ROUTE, MEETINGS_PATH, TEMPLATE_ROUTE, TEMPLATES_PATH,
};
use serde::Serialize;
use sqlx::PgPool;
use tower_governor::GovernorLayer;
use tower_governor::governor::GovernorConfigBuilder;
use tower_http::trace::TraceLayer;
use tracing::info_span;

use crate::auth;
use crate::client_ip::ClientIpKeyExtractor;
use crate::config::Config;
use crate::google::GoogleOidc;
use crate::google_tokens::GoogleTokenCache;
use crate::login_store::LoginStore;
use crate::meeting_plan;
use crate::meetings;
use crate::templates;

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
/// The client IP is resolved by
/// [`resolve_client_ip`](crate::client_ip::resolve_client_ip) with the
/// `trusted_proxy_hops` of `state.config`: with 0 hops it is the peer
/// address and forwarding headers are ignored; with n hops it is the n-th
/// `X-Forwarded-For` entry from the right, or the peer address when that
/// entry is missing or invalid. A request with no resolvable IP (no usable
/// entry and the server not started with connect info) is answered with a
/// 500. Requests over the limit get a 429 with a `Retry-After` header.
/// `/healthz` and `/api/v1/*` are not limited.
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
        auth_routes = with_rate_limit(auth_routes, limit, state.config.trusted_proxy_hops);
    }
    Router::new()
        .route(HEALTH_PATH, get(health))
        .merge(auth_routes)
        .route(ME_PATH, get(auth::me))
        .route(MEETINGS_PATH, get(meetings::list_meetings))
        .route(MEETING_PLAN_ROUTE, get(meeting_plan::get_meeting_plan))
        .route(TEMPLATES_PATH, put(templates::put_template))
        .route(TEMPLATE_ROUTE, delete(templates::delete_template))
        .layer(trace)
        .with_state(state)
}

/// Adds the per-IP rate limit to `routes`, keyed by [`ClientIpKeyExtractor`]
/// with `trusted_hops`, and starts the task that periodically drops the
/// limiter entries of idle IPs.
///
/// A `limit` with a zero burst or a zero period is replaced by the default
/// limit.
fn with_rate_limit(
    routes: Router<AppState>,
    limit: AuthRateLimit,
    trusted_hops: u8,
) -> Router<AppState> {
    let limit = if limit.burst == 0 || limit.replenish_every.is_zero() {
        AuthRateLimit::default()
    } else {
        limit
    };
    let config = GovernorConfigBuilder::default()
        .period(limit.replenish_every)
        .burst_size(limit.burst)
        .key_extractor(ClientIpKeyExtractor { trusted_hops })
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
    use std::net::SocketAddr;

    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use sqlx::postgres::PgPoolOptions;
    use tower::ServiceExt;

    use super::*;

    fn test_state() -> AppState {
        test_state_with_hops("0")
    }

    fn test_state_with_hops(hops: &'static str) -> AppState {
        let config = Config::from_lookup(|name| match name {
            "DATABASE_URL" => Some("postgres://u:p@localhost/x".to_string()),
            "PUBLIC_BASE_URL" => Some("http://localhost:8080".to_string()),
            "GOOGLE_CLIENT_ID" => Some("id".to_string()),
            "GOOGLE_CLIENT_SECRET" => Some("secret".to_string()),
            "ALLOWED_HD" => Some("example.com".to_string()),
            "TOKEN_ENC_KEY" => Some("AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=".to_string()),
            "JWT_SIGNING_KEY" => Some("0123456789abcdef0123456789abcdef".to_string()),
            "TRUSTED_PROXY_HOPS" => Some(hops.to_string()),
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

    /// Sends `GET /auth/login` from the peer `peer` (no connect info when
    /// `None`) with `X-Forwarded-For: xff` (no header when `None`).
    async fn login_status(app: &Router, peer: Option<&str>, xff: Option<&str>) -> StatusCode {
        let mut builder = Request::get(AUTH_LOGIN_PATH);
        if let Some(xff) = xff {
            builder = builder.header("x-forwarded-for", xff);
        }
        let mut request = builder.body(Body::empty()).unwrap();
        if let Some(peer) = peer {
            let addr: SocketAddr = format!("{peer}:40000").parse().unwrap();
            request.extensions_mut().insert(ConnectInfo(addr));
        }
        app.clone().oneshot(request).await.unwrap().status()
    }

    async fn login_status_from(app: &Router, peer: &str) -> StatusCode {
        login_status(app, Some(peer), None).await
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

    // With no trusted proxy, rotating X-Forwarded-For does not open new
    // buckets.
    #[tokio::test]
    async fn zero_hops_ignores_rotated_x_forwarded_for() {
        let app = router(test_state());
        for i in 0..10 {
            let xff = format!("198.51.100.{i}");
            login_status(&app, Some("203.0.113.1"), Some(&xff)).await;
        }

        let status = login_status(&app, Some("203.0.113.1"), Some("198.51.100.99")).await;

        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    // Behind one proxy the rightmost entry is the client: a spoofed leftmost
    // entry does not open new buckets.
    #[tokio::test]
    async fn one_hop_keys_on_the_rightmost_entry() {
        let app = router(test_state_with_hops("1"));
        for i in 0..10 {
            let xff = format!("6.6.6.{i}, 198.51.100.7");
            login_status(&app, Some("10.0.0.1"), Some(&xff)).await;
        }

        let status = login_status(&app, Some("10.0.0.1"), Some("6.6.6.99, 198.51.100.7")).await;

        assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn one_hop_gives_each_rightmost_entry_its_own_bucket() {
        let app = router(test_state_with_hops("1"));
        for _ in 0..11 {
            login_status(&app, Some("10.0.0.1"), Some("198.51.100.7")).await;
        }

        let status = login_status(&app, Some("10.0.0.1"), Some("198.51.100.8")).await;

        assert_eq!(status, StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn auth_route_without_any_client_ip_is_500() {
        let app = router(test_state());

        let status = login_status(&app, None, Some("198.51.100.7")).await;

        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn healthz_is_not_rate_limited() {
        let app = router(test_state());
        for _ in 0..11 {
            login_status_from(&app, "203.0.113.1").await;
        }

        let mut request = Request::get(HEALTH_PATH).body(Body::empty()).unwrap();
        let addr: SocketAddr = "203.0.113.1:40000".parse().unwrap();
        request.extensions_mut().insert(ConnectInfo(addr));
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
