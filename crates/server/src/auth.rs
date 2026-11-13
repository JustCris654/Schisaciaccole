//! Login through Google and app sessions: the `/auth/*` handlers and the
//! [`AuthUser`] extractor that protects the API.
//!
//! The desktop app opens `GET /auth/login` in the browser with its loopback
//! port, a state and a PKCE challenge. After the Google consent screen the
//! browser lands on `GET /auth/google/callback`, which redirects to the
//! app's loopback listener with a one-time code. The app exchanges that code
//! (plus its PKCE verifier) at `POST /auth/token` for an access token and a
//! refresh token.

use std::net::SocketAddr;
use std::time::Instant;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{ConnectInfo, FromRequestParts, Query, State};
use axum::http::request::Parts;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use axum::{Extension, Json};
use chrono::{DateTime, Days, Utc};
use openidconnect::{Nonce, PkceCodeVerifier};
use schisaciaccole_core::api::{Me, RefreshRequest, TokenRequest, TokenResponse, error_codes};
use serde::Deserialize;
use thiserror::Error;
use tracing::warn;
use url::form_urlencoded;
use uuid::Uuid;

use crate::app::AppState;
use crate::client_ip::resolve_client_ip;
use crate::config::Config;
use crate::crypto::{self, CryptoError};
use crate::error::ApiFailure;
use crate::google::GoogleError;
use crate::login_store::{AppCode, PendingLogin, PendingLoginRefused, StoreFull};
use crate::store;
use crate::tokens::{self, ACCESS_TOKEN_TTL_SECS};

/// Lifetime of an app session (and of each refresh token), in days.
pub const SESSION_TTL_DAYS: u64 = 90;

const MIN_APP_PORT: u16 = 1024;
const MIN_STATE_LEN: usize = 16;
const MAX_STATE_LEN: usize = 128;
const CHALLENGE_LEN: usize = 43;
const MAX_DEVICE_LABEL_CHARS: usize = 128;

const LOGIN_EXPIRED_PAGE: &str = "<!doctype html>
<html lang=\"it\">
<head><meta charset=\"utf-8\"><title>Login scaduto</title></head>
<body><p>Login scaduto, riprova dall'app.</p></body>
</html>
";

/// Query parameters of `GET /auth/login`, as received.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct LoginQuery {
    /// Port of the app's loopback listener.
    pub port: Option<String>,
    /// Opaque state chosen by the app.
    pub state: Option<String>,
    /// PKCE challenge chosen by the app.
    pub code_challenge: Option<String>,
    /// PKCE method; only `S256` is accepted.
    pub code_challenge_method: Option<String>,
}

/// Validated parameters of `GET /auth/login`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginParams {
    /// Loopback port, in `1024..=65535`.
    pub port: u16,
    /// App state: 16 to 128 characters of `[A-Za-z0-9_-]`.
    pub state: String,
    /// PKCE `S256` challenge: 43 base64url characters.
    pub code_challenge: String,
}

/// Query parameters of `GET /auth/google/callback`, as sent by Google.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct CallbackQuery {
    /// Authorization code, on success.
    pub code: Option<String>,
    /// State of the authorization request.
    pub state: Option<String>,
    /// OAuth error code, when the user denied access or Google failed.
    pub error: Option<String>,
}

/// User authenticated by a valid access token of an active session.
///
/// Extracting it reads `Authorization: Bearer <jwt>`, verifies the JWT and
/// checks in the database that the session is neither revoked nor expired,
/// so a logout takes effect immediately. Any failure is a 401
/// `unauthorized`; a database error is a 500.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthUser {
    /// Id of the user.
    pub user_id: Uuid,
    /// Id of the session the access token belongs to.
    pub session_id: Uuid,
}

impl FromRequestParts<AppState> for AuthUser {
    type Rejection = ApiFailure;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let token = bearer_token(&parts.headers).ok_or(ApiFailure::Unauthorized)?;
        let claims = tokens::verify_access_token(&state.config.jwt_signing_key, token)
            .map_err(|_| ApiFailure::Unauthorized)?;
        let session = store::find_active_session_by_id(&state.pool, claims.sid, Utc::now())
            .await?
            .ok_or(ApiFailure::Unauthorized)?;
        if session.user_id != claims.sub {
            return Err(ApiFailure::Unauthorized);
        }
        Ok(Self {
            user_id: session.user_id,
            session_id: session.id,
        })
    }
}

/// `GET /auth/login`: validates the app's parameters, remembers them and
/// redirects (302) to Google's authorization endpoint.
///
/// The login is counted against the client IP resolved by
/// [`resolve_client_ip`] with the configured trusted proxy hops; without
/// connect info and a usable `X-Forwarded-For` entry the client is unknown,
/// and all unknown clients share one quota.
///
/// # Errors
///
/// - 400 `invalid_request` when a parameter is missing or invalid (see
///   [`validate_login_params`]);
/// - 429 `too_many_requests` when the client already has the maximum number
///   of logins in progress;
/// - 503 `internal` when too many logins are in progress overall.
pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    connect_info: Option<Extension<ConnectInfo<SocketAddr>>>,
    query: Result<Query<LoginQuery>, QueryRejection>,
) -> Result<Response, ApiFailure> {
    let Query(query) =
        query.map_err(|rejection| ApiFailure::InvalidRequest(rejection.body_text()))?;
    let params = validate_login_params(&query)?;

    let config = &state.config;
    let peer = connect_info.map(|Extension(ConnectInfo(addr))| addr.ip());
    let client = resolve_client_ip(&headers, peer, config.trusted_proxy_hops);
    let request = state
        .google
        .authorize_request(&google_scopes(config), &config.allowed_hd);
    let pending = PendingLogin {
        app_port: params.port,
        app_state: params.state,
        app_challenge: params.code_challenge,
        google_pkce_verifier: request.pkce_verifier,
        nonce: request.nonce,
        client,
    };
    let inserted =
        state
            .logins
            .insert_pending(request.state.secret().clone(), pending, Instant::now());
    match inserted {
        Ok(()) => {}
        Err(PendingLoginRefused::ClientLimit) => {
            return Err(ApiFailure::TooManyRequests(
                "too many logins in progress from this client, retry later".to_string(),
            ));
        }
        Err(PendingLoginRefused::StoreFull(_)) => {
            return Err(ApiFailure::Unavailable(
                "too many logins in progress, retry later".to_string(),
            ));
        }
    }
    Ok(found(request.url.as_str()))
}

/// `GET /auth/google/callback`: completes the Google login and redirects
/// (302) to the app's loopback listener.
///
/// When the state is missing, unknown, already used or older than 10
/// minutes, the app's port is unknown: the answer is a 400 HTML page asking
/// to retry from the app. Otherwise the redirect goes to
/// `http://127.0.0.1:<port>/callback` with `code` and `state` on success, or
/// with `error` and `state` on failure:
/// - `invalid_code`: Google returned an error, no code, rejected the code,
///   or the ID token is missing or invalid (signature, issuer, audience,
///   expiry, nonce);
/// - `forbidden_domain`: the email is missing or not verified, or the `hd`
///   claim is not the allowed domain;
/// - `google_reauth_required`: Google sent no refresh token and none is
///   stored for the user;
/// - `internal`: network, database or crypto failure, or the one-time code
///   store is full.
pub async fn google_callback(
    State(state): State<AppState>,
    query: Result<Query<CallbackQuery>, QueryRejection>,
) -> Response {
    let Ok(Query(query)) = query else {
        return login_expired_page();
    };
    let Some(google_state) = query.state.as_deref() else {
        return login_expired_page();
    };
    let Some(pending) = state.logins.take_pending(google_state, Instant::now()) else {
        return login_expired_page();
    };

    let PendingLogin {
        app_port,
        app_state,
        app_challenge,
        google_pkce_verifier,
        nonce,
        client: _,
    } = pending;
    let outcome = finish_login(
        &state,
        query.code,
        query.error,
        app_challenge,
        google_pkce_verifier,
        &nonce,
    )
    .await;

    let mut params = form_urlencoded::Serializer::new(String::new());
    match outcome {
        Ok(code) => {
            params.append_pair("code", &code);
        }
        Err(err) => {
            warn!(error = %err, "Google login failed");
            params.append_pair("error", err.code());
        }
    }
    params.append_pair("state", &app_state);
    found(&format!(
        "http://127.0.0.1:{app_port}/callback?{}",
        params.finish()
    ))
}

/// `POST /auth/token`: exchanges a one-time code and its PKCE verifier for
/// a new session. Answers 200 with a [`TokenResponse`].
///
/// The code is consumed even when the verifier is wrong. The session's
/// device label is the `User-Agent` header, cut to 128 characters.
///
/// # Errors
///
/// - 400 `invalid_request` when the body is not a valid [`TokenRequest`];
/// - 400 `invalid_code` when the code is unknown, used, older than 60
///   seconds, or the verifier does not match the app's challenge;
/// - 500 `internal` on database, random generator or signing failure.
pub async fn token(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<TokenRequest>, JsonRejection>,
) -> Result<Response, ApiFailure> {
    let Json(request) =
        body.map_err(|rejection| ApiFailure::InvalidRequest(rejection.body_text()))?;
    let code = state
        .logins
        .take_code(&request.code, Instant::now())
        .ok_or(ApiFailure::InvalidCode)?;
    if !crypto::pkce_s256_matches(&request.code_verifier, &code.app_challenge) {
        return Err(ApiFailure::InvalidCode);
    }

    let now = Utc::now();
    let refresh_token = crypto::random_token()?;
    let session_id = store::create_session(
        &state.pool,
        code.user_id,
        &crypto::sha256(refresh_token.as_bytes()),
        device_label(&headers).as_deref(),
        session_expiry(now)?,
    )
    .await?;
    token_response(&state.config, code.user_id, session_id, refresh_token, now)
}

/// `POST /auth/refresh`: rotates a refresh token. Answers 200 with a new
/// [`TokenResponse`]; the old refresh token stops working and the session
/// lasts 90 more days.
///
/// # Errors
///
/// - 400 `invalid_request` when the body is not a valid [`RefreshRequest`];
/// - 401 `unauthorized` when the token is unknown, already rotated, revoked
///   or expired, or a concurrent refresh with the same token won;
/// - 500 `internal` on database, random generator or signing failure.
pub async fn refresh(
    State(state): State<AppState>,
    body: Result<Json<RefreshRequest>, JsonRejection>,
) -> Result<Response, ApiFailure> {
    let Json(request) =
        body.map_err(|rejection| ApiFailure::InvalidRequest(rejection.body_text()))?;
    let now = Utc::now();
    let old_hash = crypto::sha256(request.refresh_token.as_bytes());
    let session = store::find_active_session_by_hash(&state.pool, &old_hash, now)
        .await?
        .ok_or(ApiFailure::Unauthorized)?;

    let refresh_token = crypto::random_token()?;
    let rotated = store::rotate_session(
        &state.pool,
        session.id,
        &old_hash,
        &crypto::sha256(refresh_token.as_bytes()),
        session_expiry(now)?,
    )
    .await?;
    if !rotated {
        return Err(ApiFailure::Unauthorized);
    }
    token_response(
        &state.config,
        session.user_id,
        session.id,
        refresh_token,
        now,
    )
}

/// `POST /auth/logout`: revokes the session of a refresh token. Answers 204
/// whether or not the token was known, so it reveals nothing.
///
/// # Errors
///
/// - 400 `invalid_request` when the body is not a valid [`RefreshRequest`];
/// - 500 `internal` on database failure.
pub async fn logout(
    State(state): State<AppState>,
    body: Result<Json<RefreshRequest>, JsonRejection>,
) -> Result<StatusCode, ApiFailure> {
    let Json(request) =
        body.map_err(|rejection| ApiFailure::InvalidRequest(rejection.body_text()))?;
    let hash = crypto::sha256(request.refresh_token.as_bytes());
    store::revoke_session_by_hash(&state.pool, &hash, Utc::now()).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// `GET /api/v1/me`: returns the signed-in user.
///
/// # Errors
///
/// - 401 `unauthorized` when [`AuthUser`] cannot be extracted or the user
///   no longer exists;
/// - 500 `internal` on database failure.
pub async fn me(State(state): State<AppState>, user: AuthUser) -> Result<Json<Me>, ApiFailure> {
    let user = store::find_user(&state.pool, user.user_id)
        .await?
        .ok_or(ApiFailure::Unauthorized)?;
    Ok(Json(Me {
        email: user.email,
        display_name: user.display_name,
    }))
}

/// Checks the parameters of `GET /auth/login`.
///
/// Rules: `port` is a decimal number in `1024..=65535`; `state` has 16 to
/// 128 characters of `[A-Za-z0-9_-]`; `code_challenge` has exactly 43
/// characters of the same alphabet (base64url of a SHA-256, without
/// padding); `code_challenge_method` is exactly `S256`. A leading `+` on the
/// port is accepted, as `u16::from_str` does.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` naming the first parameter
/// that is missing or breaks its rule.
pub fn validate_login_params(query: &LoginQuery) -> Result<LoginParams, ApiFailure> {
    let port = query
        .port
        .as_deref()
        .and_then(|port| port.parse::<u16>().ok())
        .filter(|port| *port >= MIN_APP_PORT)
        .ok_or_else(|| invalid_request("port must be a number between 1024 and 65535"))?;

    let state = query
        .state
        .as_deref()
        .filter(|state| (MIN_STATE_LEN..=MAX_STATE_LEN).contains(&state.len()))
        .filter(|state| is_base64url(state))
        .ok_or_else(|| invalid_request("state must be 16 to 128 characters of [A-Za-z0-9_-]"))?;

    let code_challenge = query
        .code_challenge
        .as_deref()
        .filter(|challenge| challenge.len() == CHALLENGE_LEN && is_base64url(challenge))
        .ok_or_else(|| invalid_request("code_challenge must be 43 base64url characters"))?;

    if query.code_challenge_method.as_deref() != Some("S256") {
        return Err(invalid_request("code_challenge_method must be S256"));
    }

    Ok(LoginParams {
        port,
        state: state.to_string(),
        code_challenge: code_challenge.to_string(),
    })
}

/// Failure of the Google callback, reported to the app as an error code.
#[derive(Debug, Error)]
enum CallbackError {
    #[error("Google returned the error {0:?}")]
    GoogleDenied(String),

    #[error("callback without code")]
    MissingCode,

    #[error(transparent)]
    Google(#[from] GoogleError),

    #[error("email not verified or domain not allowed")]
    ForbiddenDomain,

    #[error("no Google refresh token received nor stored")]
    ReauthRequired,

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Crypto(#[from] CryptoError),

    #[error("too many one-time codes in the store")]
    StoreFull(#[from] StoreFull),
}

impl CallbackError {
    /// Error code sent to the app.
    fn code(&self) -> &'static str {
        match self {
            Self::GoogleDenied(_)
            | Self::MissingCode
            | Self::Google(
                GoogleError::Rejected(_)
                | GoogleError::MissingIdToken
                | GoogleError::InvalidIdToken(_),
            ) => error_codes::INVALID_CODE,
            Self::ForbiddenDomain => error_codes::FORBIDDEN_DOMAIN,
            Self::ReauthRequired => error_codes::GOOGLE_REAUTH_REQUIRED,
            Self::Google(_) | Self::Database(_) | Self::Crypto(_) | Self::StoreFull(_) => {
                error_codes::INTERNAL
            }
        }
    }
}

/// Exchanges the Google code, checks the account, stores the user and the
/// Google refresh token, and returns a new one-time app code.
///
/// # Errors
///
/// Returns `Err` in the cases listed on [`google_callback`].
async fn finish_login(
    state: &AppState,
    code: Option<String>,
    google_error: Option<String>,
    app_challenge: String,
    pkce_verifier: PkceCodeVerifier,
    nonce: &Nonce,
) -> Result<String, CallbackError> {
    if let Some(error) = google_error {
        return Err(CallbackError::GoogleDenied(error));
    }
    let code = code.ok_or(CallbackError::MissingCode)?;
    let identity = state
        .google
        .exchange_code(code, pkce_verifier, nonce)
        .await?;

    let config = &state.config;
    let domain_allowed = identity
        .hd
        .as_deref()
        .is_some_and(|hd| hd.eq_ignore_ascii_case(&config.allowed_hd));
    let email = match identity.email {
        Some(email) if identity.email_verified && domain_allowed => email,
        _ => return Err(CallbackError::ForbiddenDomain),
    };
    let user =
        store::upsert_user(&state.pool, &identity.sub, &email, identity.name.as_deref()).await?;

    match identity.refresh_token {
        Some(refresh_token) => {
            let encrypted =
                crypto::encrypt(&config.token_enc_key, refresh_token.secret().as_bytes())?;
            let scopes = match identity.scopes {
                Some(scopes) => scopes.join(" "),
                None => format!("openid {}", google_scopes(config).join(" ")),
            };
            store::save_google_credentials(&state.pool, user.id, &encrypted, &scopes).await?;
        }
        None => {
            if store::google_credentials(&state.pool, user.id)
                .await?
                .is_none()
            {
                return Err(CallbackError::ReauthRequired);
            }
        }
    }

    let app_code = crypto::random_token()?;
    let value = AppCode {
        user_id: user.id,
        app_challenge,
    };
    state
        .logins
        .insert_code(app_code.clone(), value, Instant::now())?;
    Ok(app_code)
}

/// Scopes requested to Google besides `openid`.
fn google_scopes(config: &Config) -> [&str; 3] {
    ["email", "profile", config.google_calendar_scope.as_str()]
}

/// Builds the token response: a new access token for the session plus the
/// given refresh token, marked as not cacheable.
///
/// # Errors
///
/// Returns `Err` when signing the access token fails.
fn token_response(
    config: &Config,
    user_id: Uuid,
    session_id: Uuid,
    refresh_token: String,
    now: DateTime<Utc>,
) -> Result<Response, ApiFailure> {
    let access_token =
        tokens::issue_access_token(&config.jwt_signing_key, user_id, session_id, now)?;
    let body = TokenResponse {
        access_token,
        refresh_token,
        expires_in: ACCESS_TOKEN_TTL_SECS,
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(body)).into_response())
}

/// Expiry of a session created or rotated at `now`.
///
/// # Errors
///
/// Returns `Err(ApiFailure::Internal)` when the date is out of range, which
/// cannot happen with a sane clock.
fn session_expiry(now: DateTime<Utc>) -> Result<DateTime<Utc>, ApiFailure> {
    now.checked_add_days(Days::new(SESSION_TTL_DAYS))
        .ok_or_else(|| ApiFailure::Internal("session expiry out of range".to_string()))
}

/// Extracts the token of an `Authorization: Bearer <token>` header.
///
/// The scheme is case insensitive and surrounding spaces are ignored.
/// Returns `None` when the header is missing, not ASCII, uses another
/// scheme, or has an empty token.
fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    let (scheme, token) = value.trim().split_once(' ')?;
    let token = token.trim();
    if !scheme.eq_ignore_ascii_case("bearer") || token.is_empty() {
        return None;
    }
    Some(token)
}

/// Device label of a new session: the `User-Agent` header, trimmed and cut
/// to 128 characters. `None` when the header is missing, empty or not
/// visible ASCII.
fn device_label(headers: &HeaderMap) -> Option<String> {
    let agent = headers.get(header::USER_AGENT)?.to_str().ok()?.trim();
    if agent.is_empty() {
        return None;
    }
    Some(agent.chars().take(MAX_DEVICE_LABEL_CHARS).collect())
}

/// True when `value` is made only of base64url characters `[A-Za-z0-9_-]`.
fn is_base64url(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn invalid_request(message: &str) -> ApiFailure {
    ApiFailure::InvalidRequest(message.to_string())
}

/// 302 redirect to `location`.
fn found(location: &str) -> Response {
    (
        StatusCode::FOUND,
        [(header::LOCATION, location.to_string())],
    )
        .into_response()
}

/// 400 HTML page shown when the callback cannot be matched to a login.
fn login_expired_page() -> Response {
    (StatusCode::BAD_REQUEST, Html(LOGIN_EXPIRED_PAGE)).into_response()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    const CHALLENGE: &str = "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM";

    fn valid_query() -> LoginQuery {
        LoginQuery {
            port: Some("49152".to_string()),
            state: Some("abcdefghijklmnop".to_string()),
            code_challenge: Some(CHALLENGE.to_string()),
            code_challenge_method: Some("S256".to_string()),
        }
    }

    fn rejects(query: &LoginQuery) -> bool {
        matches!(
            validate_login_params(query),
            Err(ApiFailure::InvalidRequest(_))
        )
    }

    #[test]
    fn accepts_valid_login_params() {
        let params = validate_login_params(&valid_query()).unwrap();

        assert_eq!(params.port, 49152);
        assert_eq!(params.state, "abcdefghijklmnop");
        assert_eq!(params.code_challenge, CHALLENGE);
    }

    #[test]
    fn accepts_port_bounds() {
        for port in ["1024", "65535"] {
            let query = LoginQuery {
                port: Some(port.to_string()),
                ..valid_query()
            };
            assert!(validate_login_params(&query).is_ok(), "port {port}");
        }
    }

    #[test]
    fn rejects_missing_port() {
        let query = LoginQuery {
            port: None,
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_privileged_port() {
        let query = LoginQuery {
            port: Some("1023".to_string()),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_port_above_u16() {
        let query = LoginQuery {
            port: Some("65536".to_string()),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_non_numeric_port() {
        let query = LoginQuery {
            port: Some("80a".to_string()),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_state_shorter_than_16() {
        let query = LoginQuery {
            state: Some("a".repeat(15)),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn accepts_state_of_128_chars() {
        let query = LoginQuery {
            state: Some("a".repeat(128)),
            ..valid_query()
        };
        assert!(validate_login_params(&query).is_ok());
    }

    #[test]
    fn rejects_state_longer_than_128() {
        let query = LoginQuery {
            state: Some("a".repeat(129)),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_state_with_forbidden_char() {
        let query = LoginQuery {
            state: Some("abcdefghijklmno=".to_string()),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_missing_state() {
        let query = LoginQuery {
            state: None,
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_challenge_of_42_chars() {
        let query = LoginQuery {
            code_challenge: Some(CHALLENGE[..42].to_string()),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_challenge_with_padding() {
        let query = LoginQuery {
            code_challenge: Some(format!("{}=", &CHALLENGE[..42])),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_plain_method() {
        let query = LoginQuery {
            code_challenge_method: Some("plain".to_string()),
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn rejects_missing_method() {
        let query = LoginQuery {
            code_challenge_method: None,
            ..valid_query()
        };
        assert!(rejects(&query));
    }

    #[test]
    fn bearer_token_is_extracted() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("Bearer abc"),
        );

        assert_eq!(bearer_token(&headers), Some("abc"));
    }

    #[test]
    fn bearer_scheme_is_case_insensitive() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::AUTHORIZATION,
            HeaderValue::from_static("bearer abc"),
        );

        assert_eq!(bearer_token(&headers), Some("abc"));
    }

    #[test]
    fn other_scheme_is_not_a_bearer_token() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Basic abc"));

        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn empty_bearer_token_is_none() {
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, HeaderValue::from_static("Bearer   "));

        assert_eq!(bearer_token(&headers), None);
    }

    #[test]
    fn missing_authorization_is_none() {
        assert_eq!(bearer_token(&HeaderMap::new()), None);
    }

    #[test]
    fn device_label_is_cut_to_128_chars() {
        let mut headers = HeaderMap::new();
        let agent = "x".repeat(200);
        headers.insert(header::USER_AGENT, HeaderValue::from_str(&agent).unwrap());

        assert_eq!(device_label(&headers).map(|l| l.len()), Some(128));
    }

    #[test]
    fn device_label_is_none_without_user_agent() {
        assert_eq!(device_label(&HeaderMap::new()), None);
    }

    #[test]
    fn device_label_is_none_for_blank_user_agent() {
        let mut headers = HeaderMap::new();
        headers.insert(header::USER_AGENT, HeaderValue::from_static("  "));

        assert_eq!(device_label(&headers), None);
    }

    #[test]
    fn callback_error_codes() {
        assert_eq!(
            CallbackError::GoogleDenied("access_denied".to_string()).code(),
            "invalid_code"
        );
        assert_eq!(CallbackError::MissingCode.code(), "invalid_code");
        assert_eq!(
            CallbackError::Google(GoogleError::MissingIdToken).code(),
            "invalid_code"
        );
        assert_eq!(
            CallbackError::Google(GoogleError::TokenRequest("down".to_string())).code(),
            "internal"
        );
        assert_eq!(CallbackError::ForbiddenDomain.code(), "forbidden_domain");
        assert_eq!(
            CallbackError::ReauthRequired.code(),
            "google_reauth_required"
        );
    }
}
