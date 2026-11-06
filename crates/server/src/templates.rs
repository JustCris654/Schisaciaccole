//! `PUT /api/v1/templates` and `DELETE /api/v1/templates/{id}`: the
//! signed-in user's saved slot preferences, one per normalized meeting title.

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use schisaciaccole_core::api::{EntryKind, PutTemplateRequest, Template, TemplateEntry};
use schisaciaccole_core::plan::normalize_title;
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::AuthUser;
use crate::error::ApiFailure;
use crate::store;

/// Longest meeting title accepted, in characters, after trimming.
pub const MAX_TITLE_CHARS: usize = 500;
/// Most entries a template may have.
pub const MAX_ENTRIES: usize = 100;
/// Longest entry label accepted, in characters, after trimming.
pub const MAX_LABEL_CHARS: usize = 200;
/// Longest attendee email accepted, in characters, after trimming.
pub const MAX_EMAIL_CHARS: usize = 320;
/// Largest weight of an entry, in seconds (one day).
pub const MAX_WEIGHT_SECS: u32 = 86_400;

/// A template request that passed validation, with text fields trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidTemplate {
    /// Normalized title, the key of the template.
    pub meeting_key: String,
    /// Trimmed title as the user wrote it.
    pub meeting_title: String,
    /// Entries with trimmed labels and emails.
    pub entries: Vec<TemplateEntry>,
}

/// `PUT /api/v1/templates`: creates the template of the meeting title, or
/// replaces the one already saved for the same normalized title (same
/// letters ignoring case and spacing). Answers 200 with the stored
/// [`Template`]; the id stays the same when a template is replaced.
///
/// # Errors
///
/// - 401 `unauthorized` when [`AuthUser`] cannot be extracted;
/// - 400 `invalid_request` when the body is not valid JSON of the expected
///   shape or breaks a rule of [`validate_template`];
/// - 500 `internal` on database failure.
pub async fn put_template(
    State(state): State<AppState>,
    user: AuthUser,
    body: Result<Json<PutTemplateRequest>, JsonRejection>,
) -> Result<Json<Template>, ApiFailure> {
    let Json(request) =
        body.map_err(|rejection| ApiFailure::InvalidRequest(rejection.body_text()))?;
    let valid = validate_template(&request)?;
    let id = store::upsert_template(
        &state.pool,
        user.user_id,
        &valid.meeting_key,
        &valid.meeting_title,
        &valid.entries,
    )
    .await?;
    Ok(Json(Template {
        id: id.to_string(),
        meeting_title: valid.meeting_title,
        entries: valid.entries,
    }))
}

/// `DELETE /api/v1/templates/{id}`: deletes one of the user's templates.
/// Answers 204 without a body.
///
/// # Errors
///
/// - 401 `unauthorized` when [`AuthUser`] cannot be extracted;
/// - 404 `not_found` when `id` is not a UUID, no template has it, or it
///   belongs to another user (the three cases are indistinguishable);
/// - 500 `internal` on database failure.
pub async fn delete_template(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiFailure> {
    let id = Uuid::parse_str(&id).map_err(|_| ApiFailure::NotFound)?;
    if store::delete_template(&state.pool, user.user_id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiFailure::NotFound)
    }
}

/// Checks a template request and returns it with trimmed text.
///
/// Rules, checked in this order: the normalized title is not empty and the
/// trimmed title has at most [`MAX_TITLE_CHARS`] characters; there are 1 to
/// [`MAX_ENTRIES`] entries; every label has 1 to [`MAX_LABEL_CHARS`]
/// characters after trimming; attendee entries have a non-empty email of at
/// most [`MAX_EMAIL_CHARS`] characters after trimming, other entries have
/// none; at most one entry is free time and it is the last; every weight is
/// at most [`MAX_WEIGHT_SECS`]; the weights add up to more than zero.
///
/// Lengths count characters, not bytes. Emails are not checked for
/// syntax.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` naming the first rule broken.
pub fn validate_template(request: &PutTemplateRequest) -> Result<ValidTemplate, ApiFailure> {
    let meeting_key = normalize_title(&request.meeting_title);
    if meeting_key.is_empty() {
        return Err(invalid("meeting_title must not be empty"));
    }
    let meeting_title = request.meeting_title.trim();
    if meeting_title.chars().count() > MAX_TITLE_CHARS {
        return Err(invalid(&format!(
            "meeting_title must have at most {MAX_TITLE_CHARS} characters"
        )));
    }
    if request.entries.is_empty() || request.entries.len() > MAX_ENTRIES {
        return Err(invalid(&format!(
            "entries must have between 1 and {MAX_ENTRIES} items"
        )));
    }

    let mut entries = Vec::with_capacity(request.entries.len());
    for (index, entry) in request.entries.iter().enumerate() {
        entries.push(validate_entry(index, entry)?);
    }

    let free_time_count = entries
        .iter()
        .filter(|entry| entry.kind == EntryKind::FreeTime)
        .count();
    let free_time_is_last = entries
        .last()
        .is_some_and(|entry| entry.kind == EntryKind::FreeTime);
    if free_time_count > 1 || (free_time_count == 1 && !free_time_is_last) {
        return Err(invalid(
            "at most one free_time entry is allowed and it must be the last",
        ));
    }

    let total_secs: u64 = entries
        .iter()
        .map(|entry| u64::from(entry.weight_secs))
        .sum();
    if total_secs == 0 {
        return Err(invalid("the weights must add up to more than zero"));
    }

    Ok(ValidTemplate {
        meeting_key,
        meeting_title: meeting_title.to_string(),
        entries,
    })
}

/// Checks the entry at `index` and returns it with trimmed label and email.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` naming the entry and the rule
/// broken: label length, email presence or length, weight over
/// [`MAX_WEIGHT_SECS`].
fn validate_entry(index: usize, entry: &TemplateEntry) -> Result<TemplateEntry, ApiFailure> {
    let label = entry.label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err(invalid(&format!(
            "entries[{index}].label must have between 1 and {MAX_LABEL_CHARS} characters"
        )));
    }
    let email = match (entry.kind, entry.email.as_deref()) {
        (EntryKind::Attendee, Some(email)) => {
            let email = email.trim();
            if email.is_empty() || email.chars().count() > MAX_EMAIL_CHARS {
                return Err(invalid(&format!(
                    "entries[{index}].email must have between 1 and {MAX_EMAIL_CHARS} characters"
                )));
            }
            Some(email.to_string())
        }
        (EntryKind::Attendee, None) => {
            return Err(invalid(&format!(
                "entries[{index}].email is required for attendee entries"
            )));
        }
        (_, Some(_)) => {
            return Err(invalid(&format!(
                "entries[{index}].email is only allowed for attendee entries"
            )));
        }
        (_, None) => None,
    };
    if entry.weight_secs > MAX_WEIGHT_SECS {
        return Err(invalid(&format!(
            "entries[{index}].weight_secs must be at most {MAX_WEIGHT_SECS}"
        )));
    }
    Ok(TemplateEntry {
        kind: entry.kind,
        email,
        label: label.to_string(),
        weight_secs: entry.weight_secs,
    })
}

fn invalid(message: &str) -> ApiFailure {
    ApiFailure::InvalidRequest(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attendee(email: &str, secs: u32) -> TemplateEntry {
        TemplateEntry {
            kind: EntryKind::Attendee,
            email: Some(email.to_string()),
            label: "Ada".to_string(),
            weight_secs: secs,
        }
    }

    fn other(kind: EntryKind, secs: u32) -> TemplateEntry {
        TemplateEntry {
            kind,
            email: None,
            label: "Other".to_string(),
            weight_secs: secs,
        }
    }

    fn request(title: &str, entries: Vec<TemplateEntry>) -> PutTemplateRequest {
        PutTemplateRequest {
            meeting_title: title.to_string(),
            entries,
        }
    }

    fn message_of(result: Result<ValidTemplate, ApiFailure>) -> String {
        match result {
            Err(ApiFailure::InvalidRequest(message)) => message,
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    }

    #[test]
    fn accepts_valid_request_and_trims_text() {
        let mut entry = attendee("  ada@example.com ", 60);
        entry.label = "  Ada  ".to_string();
        let free = other(EntryKind::FreeTime, 0);

        let valid = validate_template(&request("  Daily  Sync ", vec![entry, free])).unwrap();

        assert_eq!(valid.meeting_key, "daily sync");
        assert_eq!(valid.meeting_title, "Daily  Sync");
        assert_eq!(valid.entries[0].email.as_deref(), Some("ada@example.com"));
        assert_eq!(valid.entries[0].label, "Ada");
    }

    #[test]
    fn rejects_blank_title() {
        let result = validate_template(&request(" \t ", vec![attendee("a@x.com", 60)]));

        assert!(message_of(result).contains("meeting_title"));
    }

    #[test]
    fn rejects_title_over_500_characters() {
        let title = "a".repeat(MAX_TITLE_CHARS + 1);

        let result = validate_template(&request(&title, vec![attendee("a@x.com", 60)]));

        assert!(message_of(result).contains("meeting_title"));
    }

    #[test]
    fn accepts_title_of_500_multibyte_characters() {
        let title = "è".repeat(MAX_TITLE_CHARS);

        let result = validate_template(&request(&title, vec![attendee("a@x.com", 60)]));

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_no_entries() {
        let result = validate_template(&request("Daily", Vec::new()));

        assert!(message_of(result).contains("entries"));
    }

    #[test]
    fn rejects_more_than_100_entries() {
        let entries = (0..=MAX_ENTRIES).map(|_| attendee("a@x.com", 60)).collect();

        let result = validate_template(&request("Daily", entries));

        assert!(message_of(result).contains("entries"));
    }

    #[test]
    fn accepts_100_entries() {
        let entries = (0..MAX_ENTRIES).map(|_| attendee("a@x.com", 60)).collect();

        assert!(validate_template(&request("Daily", entries)).is_ok());
    }

    #[test]
    fn rejects_blank_label() {
        let mut entry = attendee("a@x.com", 60);
        entry.label = "  ".to_string();

        let result = validate_template(&request("Daily", vec![entry]));

        assert!(message_of(result).contains("entries[0].label"));
    }

    #[test]
    fn rejects_label_over_200_characters() {
        let mut entry = attendee("a@x.com", 60);
        entry.label = "a".repeat(MAX_LABEL_CHARS + 1);

        let result = validate_template(&request("Daily", vec![entry]));

        assert!(message_of(result).contains("entries[0].label"));
    }

    #[test]
    fn rejects_attendee_without_email() {
        let mut entry = attendee("a@x.com", 60);
        entry.email = None;

        let result = validate_template(&request("Daily", vec![entry]));

        assert!(message_of(result).contains("entries[0].email"));
    }

    #[test]
    fn rejects_attendee_with_blank_email() {
        let result = validate_template(&request("Daily", vec![attendee("   ", 60)]));

        assert!(message_of(result).contains("entries[0].email"));
    }

    #[test]
    fn rejects_email_over_320_characters() {
        let email = "a".repeat(MAX_EMAIL_CHARS + 1);

        let result = validate_template(&request("Daily", vec![attendee(&email, 60)]));

        assert!(message_of(result).contains("entries[0].email"));
    }

    #[test]
    fn rejects_manual_entry_with_email() {
        let mut entry = other(EntryKind::Manual, 60);
        entry.email = Some("a@x.com".to_string());

        let result = validate_template(&request("Daily", vec![entry]));

        assert!(message_of(result).contains("entries[0].email"));
    }

    #[test]
    fn rejects_free_time_entry_with_email() {
        let mut entry = other(EntryKind::FreeTime, 60);
        entry.email = Some("a@x.com".to_string());

        let result = validate_template(&request("Daily", vec![entry]));

        assert!(message_of(result).contains("entries[0].email"));
    }

    #[test]
    fn rejects_two_free_time_entries() {
        let entries = vec![
            attendee("a@x.com", 60),
            other(EntryKind::FreeTime, 60),
            other(EntryKind::FreeTime, 60),
        ];

        let result = validate_template(&request("Daily", entries));

        assert!(message_of(result).contains("free_time"));
    }

    #[test]
    fn rejects_free_time_that_is_not_last() {
        let entries = vec![other(EntryKind::FreeTime, 60), attendee("a@x.com", 60)];

        let result = validate_template(&request("Daily", entries));

        assert!(message_of(result).contains("free_time"));
    }

    #[test]
    fn rejects_weight_over_one_day() {
        let result = validate_template(&request(
            "Daily",
            vec![attendee("a@x.com", MAX_WEIGHT_SECS + 1)],
        ));

        assert!(message_of(result).contains("entries[0].weight_secs"));
    }

    #[test]
    fn accepts_weight_of_exactly_one_day() {
        let result = validate_template(&request(
            "Daily",
            vec![attendee("a@x.com", MAX_WEIGHT_SECS)],
        ));

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_weights_adding_up_to_zero() {
        let entries = vec![attendee("a@x.com", 0), other(EntryKind::Manual, 0)];

        let result = validate_template(&request("Daily", entries));

        assert!(message_of(result).contains("more than zero"));
    }

    #[test]
    fn accepts_zero_weight_next_to_positive_one() {
        let entries = vec![attendee("a@x.com", 0), attendee("b@x.com", 30)];

        assert!(validate_template(&request("Daily", entries)).is_ok());
    }
}
