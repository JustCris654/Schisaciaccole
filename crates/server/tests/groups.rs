//! Shared expansion behavior through both authenticated Calendar routes.
mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use common::*;
use schisaciaccole_core::api::{Meeting, SlotPlan};
use schisaciaccole_server::{config::GOOGLE_GROUPS_SCOPE, crypto, store};
use serde_json::{Value, json};
use sqlx::PgPool;
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, ResponseTemplate};

fn event() -> Value {
    json!({
        "id":"daily", "summary":"Daily",
        "start":{"dateTime":"2026-11-05T09:00:00Z"},
        "end":{"dateTime":"2026-11-05T09:30:00Z"},
        "attendees":[
            {"email":"team@example.com","optional":true},
            {"email":"ADA@example.com","displayName":"Direct Ada","responseStatus":"declined","optional":false},
            {"email":"broken@example.com"}
        ]
    })
}

async fn setup(h: &Harness, scope: &str) -> String {
    let tokens = h.login_to_tokens().await;
    let user = h.user_id(&tokens.access_token);
    let encrypted = crypto::encrypt(
        &h.state.config.token_enc_key,
        GOOGLE_REFRESH_TOKEN.as_bytes(),
    )
    .unwrap();
    store::save_google_credentials(
        &h.state.pool,
        user,
        &encrypted,
        &format!("{CALENDAR_SCOPE} {scope}"),
    )
    .await
    .unwrap();
    h.state.google_tokens.insert(
        user,
        openidconnect::AccessToken::new("google-token".into()),
        std::time::Instant::now() + std::time::Duration::from_secs(3600),
        std::time::Instant::now(),
    );
    Mock::given(method("GET"))
        .and(path(CALENDAR_EVENTS_PATH))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"items":[event()]})))
        .mount(&h.google)
        .await;
    Mock::given(method("GET"))
        .and(path(format!("{CALENDAR_EVENTS_PATH}/daily")))
        .respond_with(ResponseTemplate::new(200).set_body_json(event()))
        .mount(&h.google)
        .await;
    tokens.access_token
}

async fn request(h: &Harness, token: &str, uri: &str) -> axum::response::Response {
    h.send(
        Request::get(uri)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await
}

async fn list(h: &Harness, token: &str) -> Vec<Meeting> {
    let response = request(
        h,
        token,
        "/api/v1/meetings?from=2026-11-05T00:00:00Z&to=2026-11-06T00:00:00Z",
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    json_body(response).await
}

async fn root(h: &Harness, email: &str, id: &str, hits: u64) {
    Mock::given(method("GET"))
        .and(path("/identity/v1/groups:lookup"))
        .and(query_param("groupKey.id", email))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"name":format!("groups/{id}")})),
        )
        .expect(hits)
        .mount(&h.google)
        .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn both_routes_expand_and_preserve_direct_metadata_and_atomic_failed_roots(pool: PgPool) {
    let h = harness(pool).await;
    let token = setup(&h, GOOGLE_GROUPS_SCOPE).await;
    root(&h, "team@example.com", "team", 2).await;
    root(&h, "broken@example.com", "broken", 2).await;
    Mock::given(method("GET"))
        .and(path("/identity/v1/groups/team/memberships"))
        .and(query_param("view", "FULL"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"memberships":[
                {"preferredMemberKey":{"id":"ada@example.com"},"type":"USER"},
                {"preferredMemberKey":{"id":"bob@example.com"},"type":"USER"},
                {"preferredMemberKey":{"id":"BOB@example.com"},"type":"USER"}
            ]})),
        )
        .expect(2)
        .mount(&h.google)
        .await;
    Mock::given(method("GET"))
        .and(path("/identity/v1/groups/broken/memberships"))
        .respond_with(ResponseTemplate::new(403))
        .expect(2)
        .mount(&h.google)
        .await;
    let meetings = list(&h, &token).await;
    let meeting = &meetings[0];
    assert_eq!(meeting.attendees.len(), 3);
    assert_eq!(meeting.attendees[0].name, "Direct Ada");
    assert_eq!(
        meeting.attendees[0].response,
        schisaciaccole_core::api::ResponseStatus::Declined
    );
    assert!(!meeting.attendees[0].optional);
    assert_eq!(meeting.attendees[1].email, "broken@example.com");
    assert_eq!(meeting.attendees[2].email, "bob@example.com");
    assert!(meeting.attendees[2].optional);
    assert!(meeting.participant_resolution.incomplete);
    assert!(!meeting.participant_resolution.reauthorization_required);
    let response = request(&h, &token, "/api/v1/meetings/daily/plan").await;
    assert_eq!(response.status(), StatusCode::OK);
    let plan: SlotPlan = json_body(response).await;
    assert_eq!(plan.attendees.as_ref(), Some(&meeting.attendees));
    assert_eq!(plan.participant_resolution, meeting.participant_resolution);
    assert!(!plan.attendees_omitted);
    assert_eq!(plan.entries.len(), 2);
    let requests = h.google.received_requests().await.unwrap();
    assert!(
        requests
            .iter()
            .filter(|request| request.url.path().contains("calendar"))
            .all(|request| request.method == "GET")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn old_scope_grant_keeps_calendar_working_and_requires_explicit_reconnect(pool: PgPool) {
    let h = harness(pool).await;
    let token = setup(&h, "").await;
    for _ in 0..2 {
        let meetings = list(&h, &token).await;
        assert_eq!(meetings[0].attendees.len(), 3);
        assert!(meetings[0].participant_resolution.incomplete);
        assert!(meetings[0].participant_resolution.reauthorization_required);
    }
    let response = request(&h, &token, "/api/v1/meetings/daily/plan").await;
    assert_eq!(response.status(), StatusCode::OK);
    let plan: SlotPlan = json_body(response).await;
    assert!(plan.participant_resolution.reauthorization_required);
    assert_eq!(plan.attendees.unwrap().len(), 3);
    let requests = h.google.received_requests().await.unwrap();
    assert!(
        !requests
            .iter()
            .any(|request| request.url.path().contains("identity"))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn following_request_refetches_membership_instead_of_using_cross_request_cache(pool: PgPool) {
    let h = harness(pool).await;
    let token = setup(&h, GOOGLE_GROUPS_SCOPE).await;
    root(&h, "team@example.com", "team", 2).await;
    let first = Mock::given(method("GET"))
        .and(path("/identity/v1/groups/team/memberships"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"memberships":[{"preferredMemberKey":{"id":"bob@example.com"},"type":"USER"}]}),
        ))
        .expect(1)
        .mount_as_scoped(&h.google)
        .await;
    let before = list(&h, &token).await;
    assert!(
        before[0]
            .attendees
            .iter()
            .any(|person| person.email == "bob@example.com")
    );
    drop(first);
    Mock::given(method("GET"))
        .and(path("/identity/v1/groups/team/memberships"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({})))
        .expect(1)
        .mount(&h.google)
        .await;
    let after = list(&h, &token).await;
    assert!(
        !after[0]
            .attendees
            .iter()
            .any(|person| person.email == "bob@example.com")
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn second_uncached_refresh_rechecks_reduced_group_grant(pool: PgPool) {
    use wiremock::matchers::body_string_contains;
    let h = harness(pool).await;
    let token = setup(&h, GOOGLE_GROUPS_SCOPE).await;
    let user = h.user_id(&token);
    h.state.google_tokens.remove(user);
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token":"first-token", "token_type":"Bearer",
            "scope":format!("{CALENDAR_SCOPE} {GOOGLE_GROUPS_SCOPE}")
        })))
        .up_to_n_times(1)
        .expect(1)
        .mount(&h.google)
        .await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token":"reduced-token", "token_type":"Bearer", "expires_in":3600,
            "scope":CALENDAR_SCOPE
        })))
        .expect(1)
        .mount(&h.google)
        .await;
    let meetings = list(&h, &token).await;
    assert!(meetings[0].participant_resolution.reauthorization_required);
    assert_eq!(meetings[0].attendees.len(), 3);
    assert!(
        !h.google
            .received_requests()
            .await
            .unwrap()
            .iter()
            .any(|request| request.url.path().contains("identity"))
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn reconnect_wins_over_an_in_flight_refresh(pool: PgPool) {
    use std::time::Duration;
    use wiremock::matchers::body_string_contains;
    let h = harness(pool).await;
    let token = setup(&h, GOOGLE_GROUPS_SCOPE).await;
    let user = h.user_id(&token);
    h.state.google_tokens.remove(user);
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains(format!(
            "refresh_token={GOOGLE_REFRESH_TOKEN}"
        )))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_millis(200))
                .set_body_json(json!({
                    "access_token":"old-access-token", "token_type":"Bearer", "expires_in":3600,
                    "scope":CALENDAR_SCOPE
                })),
        )
        .up_to_n_times(1)
        .expect(1)
        .mount(&h.google)
        .await;
    let login = h.start_login().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=authorization_code"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token":"new-access-token", "token_type":"Bearer", "expires_in":3600,
            "refresh_token":"new-refresh-token",
            "scope":format!("{CALENDAR_SCOPE} {GOOGLE_GROUPS_SCOPE}"),
            "id_token":h.id_token(valid_claims(&login.nonce))
        })))
        .with_priority(1)
        .expect(1)
        .mount(&h.google)
        .await;
    let list_future = list(&h, &token);
    let reconnect = async {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if h.google
                    .received_requests()
                    .await
                    .unwrap()
                    .iter()
                    .any(|request| {
                        request.url.path() == "/token"
                            && String::from_utf8_lossy(&request.body)
                                .contains("grant_type=refresh_token")
                    })
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap();
        let response = h.callback(&login.state).await;
        assert_eq!(response.status(), StatusCode::FOUND);
        assert!(
            common::query_param(&url::Url::parse(&location(&response)).unwrap(), "code").is_some()
        );
    };
    tokio::join!(list_future, reconnect);
    let credentials = store::google_credentials(&h.state.pool, user)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        crypto::decrypt(
            &h.state.config.token_enc_key,
            &credentials.refresh_token_enc
        )
        .unwrap(),
        b"new-refresh-token"
    );
    assert!(credentials.scopes.contains(GOOGLE_GROUPS_SCOPE));
    assert_ne!(
        h.state
            .google_tokens
            .get(user, std::time::Instant::now())
            .map(|token| token.secret().clone()),
        Some("old-access-token".to_string())
    );
}
