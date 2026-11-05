//! End-to-end tests of the login flow against a mocked Google (wiremock)
//! and a throwaway PostgreSQL database (`#[sqlx::test]`).

use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chrono::Utc;
use http_body_util::BodyExt;
use openidconnect::core::{CoreJwsSigningAlgorithm, CoreRsaPrivateSigningKey};
use openidconnect::{
    Audience, EndUserEmail, EndUserName, IdTokenClaims, IssuerUrl, JsonWebKeyId, LocalizedClaim,
    Nonce, PkceCodeVerifier, PrivateSigningKey, StandardClaims, SubjectIdentifier,
};
use schisaciaccole_core::api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH,
    ApiError, ME_PATH, Me, TokenResponse,
};
use schisaciaccole_server::app::{AppState, router};
use schisaciaccole_server::config::Config;
use schisaciaccole_server::crypto;
use schisaciaccole_server::google::{GoogleClaims, GoogleIdToken, GoogleOidc};
use schisaciaccole_server::login_store::{PENDING_LOGIN_TTL, PendingLogin};
use schisaciaccole_server::store;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use url::Url;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const CLIENT_ID: &str = "client-id";
const ALLOWED_HD: &str = "example.com";
const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.owned.readonly";
const APP_PORT: u16 = 49152;
const APP_STATE: &str = "app-state-0123456789";
const APP_VERIFIER: &str = "app-verifier-0123456789-abcdefghijklmnopqrstuvwxyz";
const GOOGLE_CODE: &str = "google-code";
const GOOGLE_REFRESH_TOKEN: &str = "google-refresh-token";
const TOKEN_ENC_KEY_B64: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// Router, state and mocked Google of one test.
struct Harness {
    app: Router,
    state: AppState,
    google: MockServer,
    key: CoreRsaPrivateSigningKey,
}

/// Claims of the ID token returned by the mocked Google.
struct IdentityClaims {
    nonce: String,
    email_verified: bool,
    hd: Option<String>,
}

/// Google parameters of a started login, read from the authorize redirect.
struct StartedLogin {
    authorize_url: Url,
    state: String,
    nonce: String,
}

fn app_challenge() -> String {
    URL_SAFE_NO_PAD.encode(crypto::sha256(APP_VERIFIER.as_bytes()))
}

fn config(issuer: &str) -> Config {
    Config::from_lookup(|name| match name {
        "DATABASE_URL" => Some("postgres://unused".to_string()),
        "PUBLIC_BASE_URL" => Some("https://api.example.com".to_string()),
        "GOOGLE_CLIENT_ID" => Some(CLIENT_ID.to_string()),
        "GOOGLE_CLIENT_SECRET" => Some("client-secret".to_string()),
        "ALLOWED_HD" => Some(ALLOWED_HD.to_string()),
        "TOKEN_ENC_KEY" => Some(TOKEN_ENC_KEY_B64.to_string()),
        "JWT_SIGNING_KEY" => Some("0123456789abcdef0123456789abcdef".to_string()),
        "GOOGLE_ISSUER_URL" => Some(issuer.to_string()),
        "GOOGLE_CALENDAR_SCOPE" => Some(CALENDAR_SCOPE.to_string()),
        _ => None,
    })
    .unwrap()
}

async fn harness(pool: PgPool) -> Harness {
    let google = MockServer::start().await;
    let issuer = google.uri();
    let key = CoreRsaPrivateSigningKey::from_pem(
        include_str!("fixtures/google_test_key.pem"),
        Some(JsonWebKeyId::new("test-key".to_string())),
    )
    .unwrap();

    let discovery = json!({
        "issuer": issuer,
        "authorization_endpoint": format!("{issuer}/authorize"),
        "token_endpoint": format!("{issuer}/token"),
        "jwks_uri": format!("{issuer}/jwks"),
        "response_types_supported": ["code"],
        "subject_types_supported": ["public"],
        "id_token_signing_alg_values_supported": ["RS256"],
    });
    Mock::given(method("GET"))
        .and(path("/.well-known/openid-configuration"))
        .respond_with(ResponseTemplate::new(200).set_body_json(discovery))
        .mount(&google)
        .await;
    Mock::given(method("GET"))
        .and(path("/jwks"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "keys": [key.as_verification_key()] })),
        )
        .mount(&google)
        .await;

    let config = config(&issuer);
    let oidc = GoogleOidc::discover(&config, AUTH_CALLBACK_PATH)
        .await
        .unwrap();
    let state = AppState::new(config, pool, oidc);
    Harness {
        app: router(state.clone()),
        state,
        google,
        key,
    }
}

impl Harness {
    async fn send(&self, request: Request<Body>) -> Response {
        self.app.clone().oneshot(request).await.unwrap()
    }

    fn id_token(&self, claims: IdentityClaims) -> String {
        let now = Utc::now();
        let standard = StandardClaims::new(SubjectIdentifier::new("google-sub-1".to_string()))
            .set_email(Some(EndUserEmail::new("ada@example.com".to_string())))
            .set_email_verified(Some(claims.email_verified))
            .set_name(Some(LocalizedClaim::from(EndUserName::new(
                "Ada Lovelace".to_string(),
            ))));
        let id_claims = IdTokenClaims::new(
            IssuerUrl::new(self.google.uri()).unwrap(),
            vec![Audience::new(CLIENT_ID.to_string())],
            now + chrono::Duration::hours(1),
            now,
            standard,
            GoogleClaims { hd: claims.hd },
        )
        .set_nonce(Some(Nonce::new(claims.nonce)));
        GoogleIdToken::new(
            id_claims,
            &self.key,
            CoreJwsSigningAlgorithm::RsaSsaPkcs1V15Sha256,
            None,
            None,
        )
        .unwrap()
        .to_string()
    }

    /// Mounts the Google token endpoint answering with an ID token built
    /// from `claims`, plus a refresh token when `with_refresh` is true.
    async fn mock_token_endpoint(&self, claims: IdentityClaims, with_refresh: bool) {
        let mut body = json!({
            "access_token": "google-access-token",
            "token_type": "Bearer",
            "expires_in": 3600,
            "scope": format!("openid email profile {CALENDAR_SCOPE}"),
            "id_token": self.id_token(claims),
        });
        if with_refresh {
            body["refresh_token"] = json!(GOOGLE_REFRESH_TOKEN);
        }
        Mock::given(method("POST"))
            .and(path("/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.google)
            .await;
    }

    async fn start_login(&self) -> StartedLogin {
        let uri = format!(
            "{AUTH_LOGIN_PATH}?port={APP_PORT}&state={APP_STATE}&code_challenge={}&code_challenge_method=S256",
            app_challenge()
        );
        let response = self.send(get(&uri)).await;
        assert_eq!(response.status(), StatusCode::FOUND);
        let authorize_url = Url::parse(&location(&response)).unwrap();
        StartedLogin {
            state: query_param(&authorize_url, "state").unwrap(),
            nonce: query_param(&authorize_url, "nonce").unwrap(),
            authorize_url,
        }
    }

    async fn callback(&self, google_state: &str) -> Response {
        let uri = format!("{AUTH_CALLBACK_PATH}?code={GOOGLE_CODE}&state={google_state}");
        self.send(get(&uri)).await
    }

    /// Runs login and callback with a valid identity; returns the loopback
    /// redirect URL.
    async fn login_to_loopback(&self) -> Url {
        let login = self.start_login().await;
        self.mock_token_endpoint(valid_claims(&login.nonce), true)
            .await;
        let response = self.callback(&login.state).await;
        assert_eq!(response.status(), StatusCode::FOUND);
        Url::parse(&location(&response)).unwrap()
    }

    /// Runs the whole login and returns the one-time app code.
    async fn login_to_app_code(&self) -> String {
        let loopback = self.login_to_loopback().await;
        query_param(&loopback, "code").unwrap()
    }

    async fn exchange(&self, code: &str, verifier: &str) -> Response {
        let body = json!({ "code": code, "code_verifier": verifier });
        self.send(post_json(AUTH_TOKEN_PATH, &body)).await
    }

    async fn login_to_tokens(&self) -> TokenResponse {
        let code = self.login_to_app_code().await;
        let response = self.exchange(&code, APP_VERIFIER).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }

    async fn me(&self, access_token: &str) -> Response {
        let request = Request::get(ME_PATH)
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .body(Body::empty())
            .unwrap();
        self.send(request).await
    }

    async fn refresh(&self, refresh_token: &str) -> Response {
        let body = json!({ "refresh_token": refresh_token });
        self.send(post_json(AUTH_REFRESH_PATH, &body)).await
    }

    async fn logout(&self, refresh_token: &str) -> Response {
        let body = json!({ "refresh_token": refresh_token });
        self.send(post_json(AUTH_LOGOUT_PATH, &body)).await
    }
}

fn valid_claims(nonce: &str) -> IdentityClaims {
    IdentityClaims {
        nonce: nonce.to_string(),
        email_verified: true,
        hd: Some(ALLOWED_HD.to_string()),
    }
}

fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

fn post_json(uri: &str, body: &Value) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, "schisaciaccole-test")
        .body(Body::from(body.to_string()))
        .unwrap()
}

fn location(response: &Response) -> String {
    response
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

fn query_param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

async fn json_body<T: serde::de::DeserializeOwned>(response: Response) -> T {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

async fn text_body(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

async fn assert_api_error(response: Response, status: StatusCode, code: &str) {
    assert_eq!(response.status(), status);
    let body: ApiError = json_body(response).await;
    assert_eq!(body.code, code);
}

// The whole happy path, as the desktop app drives it.
#[sqlx::test(migrations = "./migrations")]
async fn full_login_flow(pool: PgPool) {
    let h = harness(pool).await;

    let login = h.start_login().await;
    assert!(login.authorize_url.as_str().starts_with(&h.google.uri()));
    h.mock_token_endpoint(valid_claims(&login.nonce), true)
        .await;

    let callback = h.callback(&login.state).await;
    assert_eq!(callback.status(), StatusCode::FOUND);
    let loopback = Url::parse(&location(&callback)).unwrap();
    assert_eq!(loopback.host_str(), Some("127.0.0.1"));
    assert_eq!(loopback.port(), Some(APP_PORT));
    assert_eq!(loopback.path(), "/callback");
    assert_eq!(query_param(&loopback, "state").as_deref(), Some(APP_STATE));
    let code = query_param(&loopback, "code").unwrap();

    let response = h.exchange(&code, APP_VERIFIER).await;
    assert_eq!(response.status(), StatusCode::OK);
    let tokens: TokenResponse = json_body(response).await;
    assert_eq!(tokens.expires_in, 900);

    let me = h.me(&tokens.access_token).await;
    assert_eq!(me.status(), StatusCode::OK);
    let me: Me = json_body(me).await;
    assert_eq!(me.email, "ada@example.com");
    assert_eq!(me.display_name.as_deref(), Some("Ada Lovelace"));

    let refreshed = h.refresh(&tokens.refresh_token).await;
    assert_eq!(refreshed.status(), StatusCode::OK);
    let refreshed: TokenResponse = json_body(refreshed).await;
    assert_ne!(refreshed.refresh_token, tokens.refresh_token);
    assert_api_error(
        h.refresh(&tokens.refresh_token).await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;

    let logout = h.logout(&refreshed.refresh_token).await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert_api_error(
        h.me(&refreshed.access_token).await,
        StatusCode::UNAUTHORIZED,
        "unauthorized",
    )
    .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn login_redirects_to_google_with_expected_params(pool: PgPool) {
    let h = harness(pool).await;

    let login = h.start_login().await;

    let url = &login.authorize_url;
    assert_eq!(url.path(), "/authorize");
    assert_eq!(query_param(url, "client_id").as_deref(), Some(CLIENT_ID));
    assert_eq!(
        query_param(url, "redirect_uri").as_deref(),
        Some("https://api.example.com/auth/google/callback")
    );
    assert_eq!(
        query_param(url, "scope"),
        Some(format!("openid email profile {CALENDAR_SCOPE}"))
    );
    assert_eq!(query_param(url, "access_type").as_deref(), Some("offline"));
    assert_eq!(query_param(url, "prompt").as_deref(), Some("consent"));
    assert_eq!(query_param(url, "hd").as_deref(), Some(ALLOWED_HD));
    assert_eq!(
        query_param(url, "code_challenge_method").as_deref(),
        Some("S256")
    );
    assert!(query_param(url, "code_challenge").is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn google_token_request_carries_the_pkce_verifier(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;
    let challenge = query_param(&login.authorize_url, "code_challenge").unwrap();
    h.mock_token_endpoint(valid_claims(&login.nonce), true)
        .await;

    h.callback(&login.state).await;

    let requests = h.google.received_requests().await.unwrap();
    let token_request = requests
        .iter()
        .find(|request| request.url.path() == "/token")
        .unwrap();
    let form: Vec<(String, String)> = url::form_urlencoded::parse(&token_request.body)
        .into_owned()
        .collect();
    let field = |name: &str| {
        form.iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    assert_eq!(field("code").as_deref(), Some(GOOGLE_CODE));
    assert!(crypto::pkce_s256_matches(
        &field("code_verifier").unwrap(),
        &challenge
    ));
}

#[sqlx::test(migrations = "./migrations")]
async fn callback_stores_encrypted_google_refresh_token(pool: PgPool) {
    let h = harness(pool.clone()).await;

    let tokens = h.login_to_tokens().await;

    let claims = schisaciaccole_server::tokens::verify_access_token(
        &h.state.config.jwt_signing_key,
        &tokens.access_token,
    )
    .unwrap();
    let creds = store::google_credentials(&pool, claims.sub)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(creds.refresh_token_enc, GOOGLE_REFRESH_TOKEN.as_bytes());
    let plain = crypto::decrypt(&h.state.config.token_enc_key, &creds.refresh_token_enc).unwrap();
    assert_eq!(plain, GOOGLE_REFRESH_TOKEN.as_bytes());
    assert_eq!(
        creds.scopes,
        format!("openid email profile {CALENDAR_SCOPE}")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn token_exchange_records_user_agent_as_device_label(pool: PgPool) {
    let h = harness(pool.clone()).await;

    h.login_to_tokens().await;

    let label: Option<String> = sqlx::query_scalar("SELECT device_label FROM app_sessions")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(label.as_deref(), Some("schisaciaccole-test"));
}

#[sqlx::test(migrations = "./migrations")]
async fn wrong_hd_redirects_with_forbidden_domain(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;
    let claims = IdentityClaims {
        hd: Some("other.com".to_string()),
        ..valid_claims(&login.nonce)
    };
    h.mock_token_endpoint(claims, true).await;

    let response = h.callback(&login.state).await;

    let loopback = Url::parse(&location(&response)).unwrap();
    assert_eq!(
        query_param(&loopback, "error").as_deref(),
        Some("forbidden_domain")
    );
    assert_eq!(query_param(&loopback, "state").as_deref(), Some(APP_STATE));
    assert_eq!(query_param(&loopback, "code"), None);
}

#[sqlx::test(migrations = "./migrations")]
async fn consumer_account_without_hd_redirects_with_forbidden_domain(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;
    let claims = IdentityClaims {
        hd: None,
        ..valid_claims(&login.nonce)
    };
    h.mock_token_endpoint(claims, true).await;

    let response = h.callback(&login.state).await;

    let loopback = Url::parse(&location(&response)).unwrap();
    assert_eq!(
        query_param(&loopback, "error").as_deref(),
        Some("forbidden_domain")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unverified_email_redirects_with_forbidden_domain(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;
    let claims = IdentityClaims {
        email_verified: false,
        ..valid_claims(&login.nonce)
    };
    h.mock_token_endpoint(claims, true).await;

    let response = h.callback(&login.state).await;

    let loopback = Url::parse(&location(&response)).unwrap();
    assert_eq!(
        query_param(&loopback, "error").as_deref(),
        Some("forbidden_domain")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn wrong_nonce_redirects_with_invalid_code(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;
    h.mock_token_endpoint(valid_claims("another-nonce"), true)
        .await;

    let response = h.callback(&login.state).await;

    let loopback = Url::parse(&location(&response)).unwrap();
    assert_eq!(
        query_param(&loopback, "error").as_deref(),
        Some("invalid_code")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn google_error_redirects_with_invalid_code(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;

    let uri = format!(
        "{AUTH_CALLBACK_PATH}?error=access_denied&state={}",
        login.state
    );
    let response = h.send(get(&uri)).await;

    let loopback = Url::parse(&location(&response)).unwrap();
    assert_eq!(
        query_param(&loopback, "error").as_deref(),
        Some("invalid_code")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn missing_refresh_token_without_stored_one_requires_reauth(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;
    h.mock_token_endpoint(valid_claims(&login.nonce), false)
        .await;

    let response = h.callback(&login.state).await;

    let loopback = Url::parse(&location(&response)).unwrap();
    assert_eq!(
        query_param(&loopback, "error").as_deref(),
        Some("google_reauth_required")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn unknown_state_shows_expired_page(pool: PgPool) {
    let h = harness(pool).await;

    let response = h.callback("never-issued").await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(text_body(response).await.contains("Login scaduto"));
}

#[sqlx::test(migrations = "./migrations")]
async fn replayed_callback_shows_expired_page(pool: PgPool) {
    let h = harness(pool).await;
    let login = h.start_login().await;
    h.mock_token_endpoint(valid_claims(&login.nonce), true)
        .await;
    h.callback(&login.state).await;

    let response = h.callback(&login.state).await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(response.headers().get(header::LOCATION).is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn expired_pending_login_shows_expired_page(pool: PgPool) {
    let h = harness(pool).await;
    let created = Instant::now()
        .checked_sub(PENDING_LOGIN_TTL + Duration::from_secs(1))
        .unwrap();
    let pending = PendingLogin {
        app_port: APP_PORT,
        app_state: APP_STATE.to_string(),
        app_challenge: app_challenge(),
        google_pkce_verifier: PkceCodeVerifier::new("verifier".to_string()),
        nonce: Nonce::new("nonce".to_string()),
    };
    h.state
        .logins
        .insert_pending("old-state".to_string(), pending, created);

    let response = h.callback("old-state").await;

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    assert!(text_body(response).await.contains("Login scaduto"));
}

#[sqlx::test(migrations = "./migrations")]
async fn reused_app_code_is_rejected(pool: PgPool) {
    let h = harness(pool).await;
    let code = h.login_to_app_code().await;
    let first = h.exchange(&code, APP_VERIFIER).await;
    assert_eq!(first.status(), StatusCode::OK);

    let second = h.exchange(&code, APP_VERIFIER).await;

    assert_api_error(second, StatusCode::BAD_REQUEST, "invalid_code").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn wrong_verifier_is_rejected(pool: PgPool) {
    let h = harness(pool).await;
    let code = h.login_to_app_code().await;

    let response = h.exchange(&code, "not-the-app-verifier").await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_code").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn token_exchange_rejects_malformed_body(pool: PgPool) {
    let h = harness(pool).await;

    let response = h
        .send(post_json(AUTH_TOKEN_PATH, &json!({ "code": "x" })))
        .await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn login_rejects_privileged_port(pool: PgPool) {
    let h = harness(pool).await;
    let uri = format!(
        "{AUTH_LOGIN_PATH}?port=80&state={APP_STATE}&code_challenge={}&code_challenge_method=S256",
        app_challenge()
    );

    let response = h.send(get(&uri)).await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn login_rejects_plain_challenge_method(pool: PgPool) {
    let h = harness(pool).await;
    let uri = format!(
        "{AUTH_LOGIN_PATH}?port={APP_PORT}&state={APP_STATE}&code_challenge={}&code_challenge_method=plain",
        app_challenge()
    );

    let response = h.send(get(&uri)).await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn login_rejects_missing_params(pool: PgPool) {
    let h = harness(pool).await;

    let response = h.send(get(AUTH_LOGIN_PATH)).await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn me_without_bearer_is_unauthorized(pool: PgPool) {
    let h = harness(pool).await;

    let response = h.send(get(ME_PATH)).await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "unauthorized").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn me_with_garbage_bearer_is_unauthorized(pool: PgPool) {
    let h = harness(pool).await;

    let response = h.me("garbage").await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "unauthorized").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_returns_tokens_that_open_me(pool: PgPool) {
    let h = harness(pool).await;
    let tokens = h.login_to_tokens().await;

    let refreshed: TokenResponse = json_body(h.refresh(&tokens.refresh_token).await).await;

    assert_eq!(h.me(&refreshed.access_token).await.status(), StatusCode::OK);
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_with_unknown_token_is_unauthorized(pool: PgPool) {
    let h = harness(pool).await;

    let response = h.refresh("never-issued").await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "unauthorized").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn refresh_after_logout_is_unauthorized(pool: PgPool) {
    let h = harness(pool).await;
    let tokens = h.login_to_tokens().await;
    h.logout(&tokens.refresh_token).await;

    let response = h.refresh(&tokens.refresh_token).await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "unauthorized").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn logout_with_unknown_token_is_no_content(pool: PgPool) {
    let h = harness(pool).await;

    let response = h.logout("never-issued").await;

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
}
