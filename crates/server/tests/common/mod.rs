//! Helpers shared by the integration tests: a router backed by a throwaway
//! PostgreSQL database (`#[sqlx::test]`) and a Google mocked with wiremock,
//! plus the requests of the login flow.
//!
//! Each test binary uses only part of the helpers.
#![allow(dead_code)]

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
    Nonce, PrivateSigningKey, StandardClaims, SubjectIdentifier,
};
use schisaciaccole_core::api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH,
    ApiError, ME_PATH, TokenResponse,
};
use schisaciaccole_server::app::{AppState, router_with_limit};
use schisaciaccole_server::config::Config;
use schisaciaccole_server::crypto;
use schisaciaccole_server::google::{GoogleClaims, GoogleIdToken, GoogleOidc};
use schisaciaccole_server::tokens::verify_access_token;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;
use url::Url;
use uuid::Uuid;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

pub const CLIENT_ID: &str = "client-id";
pub const ALLOWED_HD: &str = "example.com";
pub const CALENDAR_SCOPE: &str = "https://www.googleapis.com/auth/calendar.events.owned.readonly";
pub const APP_PORT: u16 = 49152;
pub const APP_STATE: &str = "app-state-0123456789";
pub const APP_VERIFIER: &str = "app-verifier-0123456789-abcdefghijklmnopqrstuvwxyz";
pub const GOOGLE_CODE: &str = "google-code";
pub const GOOGLE_REFRESH_TOKEN: &str = "google-refresh-token";
pub const TOKEN_ENC_KEY_B64: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// Path of the events list under the mocked Calendar base URL.
pub const CALENDAR_EVENTS_PATH: &str = "/calendar/v3/calendars/primary/events";

/// Router, state and mocked Google of one test.
pub struct Harness {
    pub app: Router,
    pub state: AppState,
    pub google: MockServer,
    pub key: CoreRsaPrivateSigningKey,
}

/// Claims of the ID token returned by the mocked Google.
pub struct IdentityClaims {
    pub nonce: String,
    pub email_verified: bool,
    pub hd: Option<String>,
}

/// Google parameters of a started login, read from the authorize redirect.
pub struct StartedLogin {
    pub authorize_url: Url,
    pub state: String,
    pub nonce: String,
}

pub fn app_challenge() -> String {
    URL_SAFE_NO_PAD.encode(crypto::sha256(APP_VERIFIER.as_bytes()))
}

pub fn config(issuer: &str) -> Config {
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
        "GOOGLE_CALENDAR_BASE_URL" => Some(format!("{issuer}/calendar/v3")),
        _ => None,
    })
    .unwrap()
}

pub async fn harness(pool: PgPool) -> Harness {
    let google = MockServer::start().await;
    let issuer = google.uri();
    let key = CoreRsaPrivateSigningKey::from_pem(
        include_str!("../fixtures/google_test_key.pem"),
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
        app: router_with_limit(state.clone(), None),
        state,
        google,
        key,
    }
}

impl Harness {
    pub async fn send(&self, request: Request<Body>) -> Response {
        self.app.clone().oneshot(request).await.unwrap()
    }

    pub fn id_token(&self, claims: IdentityClaims) -> String {
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

    /// Mounts the Google token endpoint for the code exchange, answering
    /// with an ID token built from `claims`, plus a refresh token when
    /// `with_refresh` is true.
    pub async fn mock_token_endpoint(&self, claims: IdentityClaims, with_refresh: bool) {
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
            .and(body_string_contains("grant_type=authorization_code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&self.google)
            .await;
    }

    pub async fn start_login(&self) -> StartedLogin {
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

    pub async fn callback(&self, google_state: &str) -> Response {
        let uri = format!("{AUTH_CALLBACK_PATH}?code={GOOGLE_CODE}&state={google_state}");
        self.send(get(&uri)).await
    }

    /// Runs login and callback with a valid identity; returns the loopback
    /// redirect URL.
    pub async fn login_to_loopback(&self) -> Url {
        let login = self.start_login().await;
        self.mock_token_endpoint(valid_claims(&login.nonce), true)
            .await;
        let response = self.callback(&login.state).await;
        assert_eq!(response.status(), StatusCode::FOUND);
        Url::parse(&location(&response)).unwrap()
    }

    /// Runs the whole login and returns the one-time app code.
    pub async fn login_to_app_code(&self) -> String {
        let loopback = self.login_to_loopback().await;
        query_param(&loopback, "code").unwrap()
    }

    pub async fn exchange(&self, code: &str, verifier: &str) -> Response {
        let body = json!({ "code": code, "code_verifier": verifier });
        self.send(post_json(AUTH_TOKEN_PATH, &body)).await
    }

    pub async fn login_to_tokens(&self) -> TokenResponse {
        let code = self.login_to_app_code().await;
        let response = self.exchange(&code, APP_VERIFIER).await;
        assert_eq!(response.status(), StatusCode::OK);
        json_body(response).await
    }

    pub async fn me(&self, access_token: &str) -> Response {
        let request = Request::get(ME_PATH)
            .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
            .body(Body::empty())
            .unwrap();
        self.send(request).await
    }

    pub async fn refresh(&self, refresh_token: &str) -> Response {
        let body = json!({ "refresh_token": refresh_token });
        self.send(post_json(AUTH_REFRESH_PATH, &body)).await
    }

    pub async fn logout(&self, refresh_token: &str) -> Response {
        let body = json!({ "refresh_token": refresh_token });
        self.send(post_json(AUTH_LOGOUT_PATH, &body)).await
    }

    /// Id of the user an access token was issued to.
    pub fn user_id(&self, access_token: &str) -> Uuid {
        verify_access_token(&self.state.config.jwt_signing_key, access_token)
            .unwrap()
            .sub
    }
}

pub fn valid_claims(nonce: &str) -> IdentityClaims {
    IdentityClaims {
        nonce: nonce.to_string(),
        email_verified: true,
        hd: Some(ALLOWED_HD.to_string()),
    }
}

pub fn get(uri: &str) -> Request<Body> {
    Request::get(uri).body(Body::empty()).unwrap()
}

pub fn post_json(uri: &str, body: &Value) -> Request<Body> {
    Request::post(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, "schisaciaccole-test")
        .body(Body::from(body.to_string()))
        .unwrap()
}

pub fn location(response: &Response) -> String {
    response
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap()
        .to_string()
}

pub fn query_param(url: &Url, name: &str) -> Option<String> {
    url.query_pairs()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}

pub async fn json_body<T: serde::de::DeserializeOwned>(response: Response) -> T {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    serde_json::from_slice(&bytes).unwrap()
}

pub async fn text_body(response: Response) -> String {
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    String::from_utf8(bytes.to_vec()).unwrap()
}

pub async fn assert_api_error(response: Response, status: StatusCode, code: &str) {
    assert_eq!(response.status(), status);
    let body: ApiError = json_body(response).await;
    assert_eq!(body.code, code);
}
