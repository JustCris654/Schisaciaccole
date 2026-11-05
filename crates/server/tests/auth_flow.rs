//! End-to-end tests of the login flow against a mocked Google (wiremock)
//! and a throwaway PostgreSQL database (`#[sqlx::test]`).

mod common;

use std::time::{Duration, Instant};

use axum::http::{StatusCode, header};
use openidconnect::{Nonce, PkceCodeVerifier};
use schisaciaccole_core::api::{
    AUTH_CALLBACK_PATH, AUTH_LOGIN_PATH, AUTH_TOKEN_PATH, ME_PATH, Me, TokenResponse,
};
use schisaciaccole_server::crypto;
use schisaciaccole_server::login_store::{PENDING_LOGIN_TTL, PendingLogin};
use schisaciaccole_server::store;
use serde_json::json;
use sqlx::PgPool;
use url::Url;

use common::*;

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
        .insert_pending("old-state".to_string(), pending, created)
        .unwrap();

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
