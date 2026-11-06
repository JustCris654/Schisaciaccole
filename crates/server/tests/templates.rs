//! End-to-end tests of `PUT /api/v1/templates` and
//! `DELETE /api/v1/templates/{id}` against a throwaway PostgreSQL database
//! (`#[sqlx::test]`).

mod common;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::response::Response;
use schisaciaccole_core::api::{
    EntryKind, PutTemplateRequest, TEMPLATES_PATH, Template, TemplateEntry, error_codes,
    template_path,
};
use schisaciaccole_server::store;
use serde_json::{Value, json};
use sqlx::PgPool;
use uuid::Uuid;

use common::*;

fn attendee(email: &str, label: &str, secs: u32) -> TemplateEntry {
    TemplateEntry {
        kind: EntryKind::Attendee,
        email: Some(email.to_string()),
        label: label.to_string(),
        weight_secs: secs,
    }
}

fn free_time(secs: u32) -> TemplateEntry {
    TemplateEntry {
        kind: EntryKind::FreeTime,
        email: None,
        label: "Free time".to_string(),
        weight_secs: secs,
    }
}

fn put_request(title: &str, entries: Vec<TemplateEntry>) -> Value {
    let request = PutTemplateRequest {
        meeting_title: title.to_string(),
        entries,
    };
    serde_json::to_value(request).unwrap()
}

async fn put_template(h: &Harness, access_token: Option<&str>, body: &Value) -> Response {
    let mut request = Request::put(TEMPLATES_PATH).header(header::CONTENT_TYPE, "application/json");
    if let Some(token) = access_token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    h.send(request.body(Body::from(body.to_string())).unwrap())
        .await
}

async fn delete_template(h: &Harness, access_token: &str, id: &str) -> Response {
    let request = Request::delete(template_path(id))
        .header(header::AUTHORIZATION, format!("Bearer {access_token}"))
        .body(Body::empty())
        .unwrap();
    h.send(request).await
}

async fn login(h: &Harness) -> String {
    h.login_to_tokens().await.access_token
}

#[sqlx::test(migrations = "./migrations")]
async fn put_creates_template_and_returns_trimmed_entries(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let body = put_request(
        "  Retrospective ",
        vec![attendee(" ada@example.com ", " Ada ", 120), free_time(60)],
    );

    let response = put_template(&h, Some(&token), &body).await;

    assert_eq!(response.status(), StatusCode::OK);
    let template: Template = json_body(response).await;
    assert!(Uuid::parse_str(&template.id).is_ok());
    assert_eq!(template.meeting_title, "Retrospective");
    assert_eq!(
        template.entries,
        vec![attendee("ada@example.com", "Ada", 120), free_time(60)]
    );
    let stored = store::template_by_key(&pool, h.user_id(&token), "retrospective")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.id.to_string(), template.id);
    assert_eq!(stored.entries, template.entries);
}

// The same title written with other case and spacing is the same template.
#[sqlx::test(migrations = "./migrations")]
async fn put_with_same_normalized_title_replaces_template(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let first = put_request(
        "SmartBIP | Retro",
        vec![
            attendee("ada@example.com", "Ada", 120),
            attendee("bob@example.com", "Bob", 60),
        ],
    );
    let second = put_request(
        "  smartbip |   RETRO ",
        vec![attendee("cy@example.com", "Cy", 30)],
    );
    let created: Template = json_body(put_template(&h, Some(&token), &first).await).await;

    let replaced: Template = json_body(put_template(&h, Some(&token), &second).await).await;

    assert_eq!(replaced.id, created.id);
    assert_eq!(replaced.meeting_title, "smartbip |   RETRO");
    let stored = store::template_by_key(&pool, h.user_id(&token), "smartbip | retro")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.meeting_title, "smartbip |   RETRO");
    assert_eq!(stored.entries, vec![attendee("cy@example.com", "Cy", 30)]);
}

#[sqlx::test(migrations = "./migrations")]
async fn put_with_invalid_entries_is_400_and_stores_nothing(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let body = put_request("Daily", vec![attendee("ada@example.com", "Ada", 0)]);

    let response = put_template(&h, Some(&token), &body).await;

    assert_api_error(
        response,
        StatusCode::BAD_REQUEST,
        error_codes::INVALID_REQUEST,
    )
    .await;
    let keys = store::template_keys(&pool, h.user_id(&token))
        .await
        .unwrap();
    assert!(keys.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn put_with_malformed_json_is_400(pool: PgPool) {
    let h = harness(pool).await;
    let token = login(&h).await;

    let response = put_template(&h, Some(&token), &json!({ "meeting_title": "Daily" })).await;

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
    let body = put_request("Daily", vec![attendee("ada@example.com", "Ada", 60)]);

    let response = put_template(&h, None, &body).await;

    assert_api_error(
        response,
        StatusCode::UNAUTHORIZED,
        error_codes::UNAUTHORIZED,
    )
    .await;
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_removes_template_then_answers_404(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let body = put_request("Daily", vec![attendee("ada@example.com", "Ada", 60)]);
    let template: Template = json_body(put_template(&h, Some(&token), &body).await).await;

    let first = delete_template(&h, &token, &template.id).await;
    let second = delete_template(&h, &token, &template.id).await;

    assert_eq!(first.status(), StatusCode::NO_CONTENT);
    assert_api_error(second, StatusCode::NOT_FOUND, error_codes::NOT_FOUND).await;
    let keys = store::template_keys(&pool, h.user_id(&token))
        .await
        .unwrap();
    assert!(keys.is_empty());
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_of_another_users_template_is_404_and_keeps_it(pool: PgPool) {
    let h = harness(pool.clone()).await;
    let token = login(&h).await;
    let other = store::upsert_user(&pool, "other-sub", "other@example.com", None)
        .await
        .unwrap();
    let other_id = store::upsert_template(
        &pool,
        other.id,
        "daily",
        "Daily",
        &[attendee("zoe@example.com", "Zoe", 60)],
    )
    .await
    .unwrap();

    let response = delete_template(&h, &token, &other_id.to_string()).await;

    assert_api_error(response, StatusCode::NOT_FOUND, error_codes::NOT_FOUND).await;
    let kept = store::template_by_key(&pool, other.id, "daily")
        .await
        .unwrap();
    assert!(kept.is_some());
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_of_non_uuid_id_is_404(pool: PgPool) {
    let h = harness(pool).await;
    let token = login(&h).await;

    let response = delete_template(&h, &token, "not-a-uuid").await;

    assert_api_error(response, StatusCode::NOT_FOUND, error_codes::NOT_FOUND).await;
}

#[sqlx::test(migrations = "./migrations")]
async fn delete_without_token_is_401(pool: PgPool) {
    let h = harness(pool).await;
    let request = Request::delete(template_path(&Uuid::new_v4().to_string()))
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
