//! End-to-end tests of `GET /api/v1/meetings` against a mocked Google token
//! endpoint and Calendar API (wiremock) and a throwaway PostgreSQL database
//! (`#[sqlx::test]`).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use chrono::DateTime;
use schisaciaccole_core::api::{
    MEETINGS_PATH, Meeting, MeetingPreference, PersonKind, PreferencePerson, ResponseStatus,
};
use schisaciaccole_server::store;
use serde_json::{Value, json};
use sqlx::PgPool;
use url::form_urlencoded;
use wiremock::matchers::{
    bearer_token, body_string_contains, method, path, query_param, query_param_is_missing,
};
use wiremock::{Mock, ResponseTemplate};

use common::*;

const FROM: &str = "2026-11-05T00:00:00+01:00";
const TO: &str = "2026-11-06T00:00:00+01:00";
const GOOGLE_ACCESS_TOKEN: &str = "calendar-access-token";

/// Query string of a meetings request.
fn range_query(from: &str, to: &str) -> String {
    form_urlencoded::Serializer::new(String::new())
        .append_pair("from", from)
        .append_pair("to", to)
        .finish()
}

async fn meetings(h: &Harness, access_token: &str, query: &str) -> Response {
    let request = Request::get(format!("{MEETINGS_PATH}?{query}"))
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    h.send(request).await
}

/// Logs in and returns the app access token.
async fn login(h: &Harness) -> String {
    h.login_to_tokens().await.access_token
}

fn refresh_response(access_token: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "access_token": access_token,
        "token_type": "Bearer",
        "expires_in": 3599,
        "scope": CALENDAR_SCOPE,
    }))
}

/// Mounts the Google token endpoint for refresh token grants, answering
/// `access_token` and expecting `hits` requests.
async fn mock_refresh(h: &Harness, access_token: &str, hits: u64) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains(format!(
            "refresh_token={GOOGLE_REFRESH_TOKEN}"
        )))
        .respond_with(refresh_response(access_token))
        .expect(hits)
        .mount(&h.google)
        .await;
}

fn events_page(items: Value, next_page_token: Option<&str>) -> ResponseTemplate {
    let mut body = json!({ "kind": "calendar#events", "items": items });
    if let Some(token) = next_page_token {
        body["nextPageToken"] = json!(token);
    }
    ResponseTemplate::new(200).set_body_json(body)
}

/// Mounts the Calendar events endpoint for `access_token`, answering
/// `response` and expecting `hits` requests.
async fn mock_calendar(h: &Harness, access_token: &str, response: ResponseTemplate, hits: u64) {
    Mock::given(method("GET"))
        .and(path(CALENDAR_EVENTS_PATH))
        .and(bearer_token(access_token))
        .respond_with(response)
        .expect(hits)
        .mount(&h.google)
        .await;
}

fn timed_event(id: &str, summary: &str) -> Value {
    json!({
        "id": id,
        "status": "confirmed",
        "summary": summary,
        "start": { "dateTime": "2026-11-05T09:30:00+01:00", "timeZone": "Europe/Rome" },
        "end": { "dateTime": "2026-11-05T10:00:00+01:00", "timeZone": "Europe/Rome" },
    })
}

fn ids(meetings: &[Meeting]) -> Vec<&str> {
    meetings
        .iter()
        .map(|meeting| meeting.event_id.as_str())
        .collect()
}

// Login, app token, then meetings: Google is called with the expected
// parameters and its events are filtered and mapped.
#[sqlx::test(migrations = "./migrations")]
async fn full_flow_returns_mapped_meetings(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 1).await;
    let mut daily = timed_event("daily_1", "Daily");
    daily["recurringEventId"] = json!("daily");
    daily["attendees"] = json!([
        { "email": "ada@example.com", "self": true, "responseStatus": "accepted" },
        { "email": "room-1@resource.calendar.google.com", "resource": true, "responseStatus": "accepted" },
        { "email": "bob.rossi@example.com", "optional": true },
        { "email": "carla@example.com", "displayName": "Carla V.", "responseStatus": "tentative" },
    ]);
    let mut cancelled = timed_event("cancelled", "Cancelled");
    cancelled["status"] = json!("cancelled");
    let all_day = json!({
        "id": "holiday",
        "summary": "Holiday",
        "start": { "date": "2026-11-05" },
        "end": { "date": "2026-11-06" },
    });
    let mut declined = timed_event("declined", "Declined");
    declined["attendees"] = json!([
        { "email": "ada@example.com", "self": true, "responseStatus": "declined" },
    ]);
    let mut untitled = timed_event("untitled", "x");
    untitled.as_object_mut().unwrap().remove("summary");
    Mock::given(method("GET"))
        .and(path(CALENDAR_EVENTS_PATH))
        .and(bearer_token(GOOGLE_ACCESS_TOKEN))
        .and(query_param("timeMin", FROM))
        .and(query_param("timeMax", TO))
        .and(query_param("singleEvents", "true"))
        .and(query_param("orderBy", "startTime"))
        .and(query_param("eventTypes", "default"))
        .and(query_param("maxResults", "250"))
        .and(query_param_is_missing("pageToken"))
        .respond_with(events_page(
            json!([daily, cancelled, all_day, declined, untitled]),
            None,
        ))
        .expect(1)
        .mount(&h.google)
        .await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    let meetings: Vec<Meeting> = json_body(response).await;
    assert_eq!(ids(&meetings), ["daily_1", "untitled"]);
    let daily = &meetings[0];
    assert_eq!(daily.title, "Daily");
    assert_eq!(
        daily.start,
        DateTime::parse_from_rfc3339("2026-11-05T09:30:00+01:00").unwrap()
    );
    assert_eq!(
        daily.end,
        DateTime::parse_from_rfc3339("2026-11-05T10:00:00+01:00").unwrap()
    );
    assert_eq!(daily.recurring_event_id.as_deref(), Some("daily"));
    assert!(!daily.attendees_omitted);
    let emails: Vec<&str> = daily
        .attendees
        .iter()
        .map(|attendee| attendee.email.as_str())
        .collect();
    assert_eq!(
        emails,
        [
            "ada@example.com",
            "bob.rossi@example.com",
            "carla@example.com"
        ]
    );
    let ada = &daily.attendees[0];
    assert!(ada.is_self);
    assert_eq!(ada.response, ResponseStatus::Accepted);
    let bob = &daily.attendees[1];
    assert_eq!(bob.name, "Bob Rossi");
    assert!(bob.optional);
    assert_eq!(bob.response, ResponseStatus::NeedsAction);
    let carla = &daily.attendees[2];
    assert_eq!(carla.name, "Carla V.");
    assert_eq!(carla.response, ResponseStatus::Tentative);
    assert_eq!(meetings[1].title, "(senza titolo)");
}

// The preference key ignores case and spacing; other titles stay without one.
#[sqlx::test(migrations = "./migrations")]
async fn meeting_with_matching_preference_carries_it(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let access_token = login(&h).await;
    let people = [
        PreferencePerson {
            kind: PersonKind::Attendee,
            email: Some("ada@example.com".to_string()),
            label: "Ada R.".to_string(),
        },
        PreferencePerson {
            kind: PersonKind::Manual,
            email: None,
            label: "Demo".to_string(),
        },
    ];
    let id = store::upsert_preference(
        &pool,
        h.user_id(&access_token),
        "daily sync",
        "Daily sync",
        &people,
    )
    .await
    .unwrap();
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 1).await;
    let items = json!([
        timed_event("e1", "  DAILY   Sync "),
        timed_event("e2", "Retro")
    ]);
    mock_calendar(&h, GOOGLE_ACCESS_TOKEN, events_page(items, None), 1).await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let meetings: Vec<Meeting> = json_body(response).await;
    assert_eq!(
        meetings[0].preference,
        Some(MeetingPreference {
            id: id.to_string(),
            meeting_title: "Daily sync".to_string(),
            people: people.to_vec(),
        })
    );
    assert_eq!(meetings[1].preference, None);
}

#[sqlx::test(migrations = "./migrations")]
async fn second_call_reuses_cached_google_access_token(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 1).await;
    let page = events_page(json!([timed_event("e1", "One")]), None);
    mock_calendar(&h, GOOGLE_ACCESS_TOKEN, page, 2).await;

    let first = meetings(&h, &access_token, &range_query(FROM, TO)).await;
    let second = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_eq!(first.status(), StatusCode::OK);
    assert_eq!(second.status(), StatusCode::OK);
    h.google.verify().await;
}

#[sqlx::test(migrations = "./migrations")]
async fn follows_next_page_token(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 1).await;
    Mock::given(method("GET"))
        .and(path(CALENDAR_EVENTS_PATH))
        .and(query_param_is_missing("pageToken"))
        .respond_with(events_page(
            json!([timed_event("e1", "One")]),
            Some("page-2"),
        ))
        .expect(1)
        .mount(&h.google)
        .await;
    Mock::given(method("GET"))
        .and(path(CALENDAR_EVENTS_PATH))
        .and(query_param("pageToken", "page-2"))
        .and(query_param("timeMin", FROM))
        .and(bearer_token(GOOGLE_ACCESS_TOKEN))
        .respond_with(events_page(json!([timed_event("e2", "Two")]), None))
        .expect(1)
        .mount(&h.google)
        .await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let meetings: Vec<Meeting> = json_body(response).await;
    assert_eq!(ids(&meetings), ["e1", "e2"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn stops_after_five_pages(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 1).await;
    let page = events_page(json!([timed_event("e1", "One")]), Some("again"));
    mock_calendar(&h, GOOGLE_ACCESS_TOKEN, page, 5).await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let meetings: Vec<Meeting> = json_body(response).await;
    assert_eq!(meetings.len(), 5);
}

#[sqlx::test(migrations = "./migrations")]
async fn invalid_grant_requires_reauth_and_deletes_credentials(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let access_token = login(&h).await;
    let user_id = h.user_id(&access_token);
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant",
            "error_description": "Token has been expired or revoked.",
        })))
        .expect(1)
        .mount(&h.google)
        .await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "google_reauth_required").await;
    assert!(
        store::google_credentials(&pool, user_id)
            .await
            .unwrap()
            .is_none()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn other_token_endpoint_error_is_bad_gateway(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let access_token = login(&h).await;
    let user_id = h.user_id(&access_token);
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&h.google)
        .await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_api_error(response, StatusCode::BAD_GATEWAY, "internal").await;
    assert!(
        store::google_credentials(&pool, user_id)
            .await
            .unwrap()
            .is_some()
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn missing_calendar_scope_requires_reauth(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let access_token = login(&h).await;
    sqlx::query("UPDATE google_credentials SET scopes = 'openid email profile'")
        .execute(&pool)
        .await
        .unwrap();
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 0).await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "google_reauth_required").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn missing_credentials_require_reauth(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let access_token = login(&h).await;
    store::delete_google_credentials(&pool, h.user_id(&access_token))
        .await
        .unwrap();

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "google_reauth_required").await;
}

// The first Google access token is rejected by Calendar: a new one is
// obtained and the request retried with it.
#[sqlx::test(migrations = "./migrations")]
async fn calendar_401_refreshes_token_and_retries(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .respond_with(refresh_response("stale-token"))
        .up_to_n_times(1)
        .expect(1)
        .mount(&h.google)
        .await;
    mock_refresh(&h, "fresh-token", 1).await;
    mock_calendar(&h, "stale-token", ResponseTemplate::new(401), 1).await;
    let page = events_page(json!([timed_event("e1", "One")]), None);
    mock_calendar(&h, "fresh-token", page, 1).await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_eq!(response.status(), StatusCode::OK);
    let meetings: Vec<Meeting> = json_body(response).await;
    assert_eq!(ids(&meetings), ["e1"]);
}

#[sqlx::test(migrations = "./migrations")]
async fn calendar_401_after_refresh_requires_reauth(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 2).await;
    mock_calendar(&h, GOOGLE_ACCESS_TOKEN, ResponseTemplate::new(401), 2).await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "google_reauth_required").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn calendar_500_is_bad_gateway(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;
    mock_refresh(&h, GOOGLE_ACCESS_TOKEN, 1).await;
    mock_calendar(&h, GOOGLE_ACCESS_TOKEN, ResponseTemplate::new(500), 1).await;

    let response = meetings(&h, &access_token, &range_query(FROM, TO)).await;

    assert_api_error(response, StatusCode::BAD_GATEWAY, "internal").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn missing_params_are_rejected(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;

    let response = meetings(&h, &access_token, "").await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn non_rfc3339_param_is_rejected(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;

    let response = meetings(&h, &access_token, &range_query("2026-11-05", TO)).await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn to_not_after_from_is_rejected(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;

    let response = meetings(&h, &access_token, &range_query(FROM, FROM)).await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn range_over_48_hours_is_rejected(pool: PgPool) {
    let h = harness(pool).await;
    let access_token = login(&h).await;

    let response = meetings(
        &h,
        &access_token,
        &range_query(FROM, "2026-11-07T00:00:01+01:00"),
    )
    .await;

    assert_api_error(response, StatusCode::BAD_REQUEST, "invalid_request").await;
}

#[sqlx::test(migrations = "./migrations")]
async fn meetings_without_bearer_is_unauthorized(pool: PgPool) {
    let h = harness(pool).await;

    let response = h
        .send(get(&format!("{MEETINGS_PATH}?{}", range_query(FROM, TO))))
        .await;

    assert_api_error(response, StatusCode::UNAUTHORIZED, "unauthorized").await;
}
