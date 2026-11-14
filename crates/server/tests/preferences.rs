//! End-to-end tests of `PUT /api/v1/preferences` and
//! `DELETE /api/v1/preferences/{id}` against a throwaway PostgreSQL database
//! (`#[sqlx::test]`).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use schisaciaccole_core::api::{
    MeetingPreference, PREFERENCES_PATH, PersonKind, PreferencePerson, PutPreferenceRequest,
    error_codes, preference_path,
};
use schisaciaccole_server::store;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use common::*;

fn attendee(email: &str, label: &str) -> PreferencePerson {
    PreferencePerson {
        kind: PersonKind::Attendee,
        email: Some(email.to_string()),
        label: label.to_string(),
    }
}

fn manual(label: &str) -> PreferencePerson {
    PreferencePerson {
        kind: PersonKind::Manual,
        email: None,
        label: label.to_string(),
    }
}

fn put_request(title: &str, people: Vec<PreferencePerson>) -> Value {
    let request = PutPreferenceRequest {
        meeting_title: title.to_string(),
        people,
    };
    serde_json::to_value(request).unwrap()
}

async fn put_preference(h: &Harness, access_token: Option<&str>, body: &Value) -> Response {
    let mut request =
        Request::put(PREFERENCES_PATH).header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = access_token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    h.send(request.body(Body::from(body.to_string())).unwrap())
        .await
}

async fn delete_preference(h: &Harness, access_token: &str, id: &str) -> Response {
    let request = Request::delete(preference_path(id))
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    h.send(request).await
}

async fn login(h: &Harness) -> String {
    h.login_to_tokens().await.access_token
}

async fn stored(pool: &PgPool, user_id: Uuid, key: &str) -> Option<MeetingPreference> {
    store::preferences_by_keys(pool, user_id, &[key.to_string()])
        .await
        .unwrap()
        .remove(key)
}

async fn assert_invalid_and_nothing_stored(pool: &PgPool, h: &Harness, token: &str, body: &Value) {
    let response = put_preference(h, Some(token), body).await;

    assert_api_error(
        response,
        StatusCode::BAD_REQUEST,
        error_codes::INVALID_REQUEST,
    )
    .await;
    assert!(stored(pool, h.user_id(token), "daily").await.is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn put_creates_preference_and_returns_trimmed_people(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let body = put_request(
        "  Retrospective ",
        vec![
            attendee(" ada@example.com ", " Ada "),
            manual(" Marco (consulente) "),
        ],
    );

    let response = put_preference(&h, Some(&token), &body).await;

    assert_eq!(response.status(), StatusCode::OK);
    let preference: MeetingPreference = json_body(response).await;
    assert!(Uuid::parse_str(&preference.id).is_ok());
    assert_eq!(preference.meeting_title, "Retrospective");
    let expected = vec![
        attendee("ada@example.com", "Ada"),
        manual("Marco (consulente)"),
    ];
    assert_eq!(preference.people, expected);
    let saved = stored(&pool, h.user_id(&token), "retrospective")
        .await
        .unwrap();
    assert_eq!(saved, preference);
}

// The same title written with other case and spacing is the same preference.
#[sqlx::test(migrations = "./migrations")]
async fn put_with_same_normalized_title_replaces_people_and_keeps_id(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let first = put_request(
        "SmartBIP | Retro",
        vec![
            attendee("ada@example.com", "Ada"),
            attendee("bob@example.com", "Bob"),
            manual("Marco"),
        ],
    );
    let second = put_request(
        "  smartbip |   RETRO ",
        vec![manual("Zoe"), attendee("cy@example.com", "Cy")],
    );
    let created: MeetingPreference =
        json_body(put_preference(&h, Some(&token), &first).await).await;

    let replaced: MeetingPreference =
        json_body(put_preference(&h, Some(&token), &second).await).await;

    assert_eq!(replaced.id, created.id);
    assert_eq!(replaced.meeting_title, "smartbip |   RETRO");
    let saved = stored(&pool, h.user_id(&token), "smartbip | retro")
        .await
        .unwrap();
    assert_eq!(saved.id, created.id);
    assert_eq!(saved.meeting_title, "smartbip |   RETRO");
    assert_eq!(
        saved.people,
        vec![manual("Zoe"), attendee("cy@example.com", "Cy")]
    );
}

#[sqlx::test(migrations = "./migrations")]
async fn put_with_no_people_is_400_and_stores_nothing(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;

    assert_invalid_and_nothing_stored(&pool, &h, &token, &put_request("Daily", Vec::new())).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn put_with_duplicate_email_is_400_and_stores_nothing(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let people = vec![
        attendee("ada@example.com", "Ada"),
        attendee(" ADA@example.com", "Ada 2"),
    ];

    assert_invalid_and_nothing_stored(&pool, &h, &token, &put_request("Daily", people)).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn put_with_manual_person_with_email_is_400_and_stores_nothing(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let mut person = manual("Marco");
    person.email = Some("marco@example.com".to_string());

    assert_invalid_and_nothing_stored(&pool, &h, &token, &put_request("Daily", vec![person])).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn put_with_too_long_label_is_400_and_stores_nothing(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let people = vec![manual(&"a".repeat(201))];

    assert_invalid_and_nothing_stored(&pool, &h, &token, &put_request("Daily", people)).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn put_with_malformed_json_is_400(pool: PgPool) {
    let h = harness(pool).await;
    let token = login(&h).await;

    let response = put_preference(&h, Some(&token), &json!({ "meeting_title": "Daily" })).await;

    assert_api_error(
        response,
        StatusCode::BAD_REQUEST,
        error_codes::INVALID_REQUEST,
    )
    .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn put_without_token_is_401(pool: PgPool) {
    let h = harness(pool).await;
    let body = put_request("Daily", vec![manual("Marco")]);

    let response = put_preference(&h, None, &body).await;

    assert_api_error(
        response,
        StatusCode::UNAUTHORIZED,
        error_codes::UNAUTHORIZED,
    )
    .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_removes_preference_then_answers_404(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let body = put_request("Daily", vec![manual("Marco")]);
    let preference: MeetingPreference =
        json_body(put_preference(&h, Some(&token), &body).await).await;

    let first = delete_preference(&h, &token, &preference.id).await;
    let second = delete_preference(&h, &token, &preference.id).await;

    assert_eq!(first.status(), StatusCode::NO_CONTENT);
    assert_api_error(second, StatusCode::NOT_FOUND, error_codes::NOT_FOUND).await;
    assert!(stored(&pool, h.user_id(&token), "daily").await.is_none());
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_of_another_users_preference_is_404_and_keeps_it(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let other = store::upsert_user(&pool, "other-sub", "other@example.com", None)
        .await
        .unwrap();
    let other_id = store::upsert_preference(&pool, other.id, "daily", "Daily", &[manual("Zoe")])
        .await
        .unwrap();

    let response = delete_preference(&h, &token, &other_id.to_string()).await;

    assert_api_error(response, StatusCode::NOT_FOUND, error_codes::NOT_FOUND).await;
    assert!(stored(&pool, other.id, "daily").await.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_of_missing_id_is_404(pool: PgPool) {
    let h = harness(pool).await;
    let token = login(&h).await;

    let response = delete_preference(&h, &token, &Uuid::new_v4().to_string()).await;

    assert_api_error(response, StatusCode::NOT_FOUND, error_codes::NOT_FOUND).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_of_non_uuid_id_is_404(pool: PgPool) {
    let h = harness(pool).await;
    let token = login(&h).await;

    let response = delete_preference(&h, &token, "not-a-uuid").await;

    assert_api_error(response, StatusCode::NOT_FOUND, error_codes::NOT_FOUND).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_without_token_is_401(pool: PgPool) {
    let h = harness(pool).await;
    let request = Request::delete(preference_path(&Uuid::new_v4().to_string()))
        .body(Body::empty())
        .unwrap();

    let response = h.send(request).await;

    assert_api_error(
        response,
        StatusCode::UNAUTHORIZED,
        error_codes::UNAUTHORIZED,
    )
    .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn preferences_by_keys_filters_by_user_and_key(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let user = h.user_id(&token);
    let other = store::upsert_user(&pool, "other-sub", "other@example.com", None)
        .await
        .unwrap();
    store::upsert_preference(&pool, user, "daily", "Daily", &[manual("Ada")])
        .await
        .unwrap();
    store::upsert_preference(&pool, user, "retro", "Retro", &[manual("Bob")])
        .await
        .unwrap();
    store::upsert_preference(&pool, other.id, "daily", "Daily", &[manual("Zoe")])
        .await
        .unwrap();

    let found = store::preferences_by_keys(&pool, user, &["daily".to_string(), "x".to_string()])
        .await
        .unwrap();
    let none = store::preferences_by_keys(&pool, user, &[]).await.unwrap();

    assert_eq!(found.len(), 1);
    assert_eq!(found["daily"].people, vec![manual("Ada")]);
    assert!(none.is_empty());
}
