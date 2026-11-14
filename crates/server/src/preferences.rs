//! `PUT /api/v1/preferences` and `DELETE /api/v1/preferences/{id}`: the
//! signed-in user's saved meeting preferences, one per normalized meeting
//! title. A preference lists the attendee renames and the manual people the
//! user wants in the participant editor of that meeting.

use std::collections::HashSet;

use axum::Json;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use schisaciaccole_core::api::{
    MeetingPreference, PersonKind, PreferencePerson, PutPreferenceRequest,
};
use schisaciaccole_core::plan::normalize_title;
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::AuthUser;
use crate::error::ApiFailure;
use crate::store;

/// Longest meeting title accepted, in characters, after trimming.
pub const MAX_TITLE_CHARS: usize = 500;
/// Most people a preference may have.
pub const MAX_PEOPLE: usize = 100;
/// Longest person label accepted, in characters, after trimming.
pub const MAX_LABEL_CHARS: usize = 200;
/// Longest attendee email accepted, in characters, after trimming.
pub const MAX_EMAIL_CHARS: usize = 320;

/// A preference request that passed validation, with text fields trimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidPreference {
    /// Normalized title, the key of the preference.
    pub meeting_key: String,
    /// Trimmed title as the user wrote it.
    pub meeting_title: String,
    /// People with trimmed labels and emails.
    pub people: Vec<PreferencePerson>,
}

/// `PUT /api/v1/preferences`: creates the preference of the meeting title,
/// or replaces the one already saved for the same normalized title (same
/// letters ignoring case and spacing). Answers 200 with the stored
/// [`MeetingPreference`]; the id stays the same when a preference is
/// replaced.
///
/// # Errors
///
/// - 401 `unauthorized` when [`AuthUser`] cannot be extracted;
/// - 400 `invalid_request` when the body is not valid JSON of the expected
///   shape or breaks a rule of [`validate_preference`];
/// - 500 `internal` on database failure.
pub async fn put_preference(
    State(state): State<AppState>,
    user: AuthUser,
    body: Result<Json<PutPreferenceRequest>, JsonRejection>,
) -> Result<Json<MeetingPreference>, ApiFailure> {
    let Json(request) =
        body.map_err(|rejection| ApiFailure::InvalidRequest(rejection.body_text()))?;
    let valid = validate_preference(&request)?;
    let id = store::upsert_preference(
        &state.pool,
        user.user_id,
        &valid.meeting_key,
        &valid.meeting_title,
        &valid.people,
    )
    .await?;
    Ok(Json(MeetingPreference {
        id: id.to_string(),
        meeting_title: valid.meeting_title,
        people: valid.people,
    }))
}

/// `DELETE /api/v1/preferences/{id}`: deletes one of the user's
/// preferences. Answers 204 without a body.
///
/// # Errors
///
/// - 401 `unauthorized` when [`AuthUser`] cannot be extracted;
/// - 404 `not_found` when `id` is not a UUID, no preference has it, or it
///   belongs to another user (the three cases are indistinguishable);
/// - 500 `internal` on database failure.
pub async fn delete_preference(
    State(state): State<AppState>,
    user: AuthUser,
    Path(id): Path<String>,
) -> Result<StatusCode, ApiFailure> {
    let id = Uuid::parse_str(&id).map_err(|_| ApiFailure::NotFound)?;
    if store::delete_preference(&state.pool, user.user_id, id).await? {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiFailure::NotFound)
    }
}

/// Checks a preference request and returns it with trimmed text.
///
/// Rules, checked in this order: the normalized title is not empty and the
/// trimmed title has at most [`MAX_TITLE_CHARS`] characters; there are 1 to
/// [`MAX_PEOPLE`] people; every label has 1 to [`MAX_LABEL_CHARS`]
/// characters after trimming; attendees have a non-empty email of at most
/// [`MAX_EMAIL_CHARS`] characters after trimming, manual people have none
/// (an empty string counts as an email); the same email (trimmed, ASCII
/// lowercase) appears at most once.
///
/// Lengths count characters, not bytes. Emails are not checked for syntax.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` naming the first rule broken.
pub fn validate_preference(request: &PutPreferenceRequest) -> Result<ValidPreference, ApiFailure> {
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
    if request.people.is_empty() || request.people.len() > MAX_PEOPLE {
        return Err(invalid(&format!(
            "people must have between 1 and {MAX_PEOPLE} items"
        )));
    }

    let mut people = Vec::with_capacity(request.people.len());
    for (index, person) in request.people.iter().enumerate() {
        people.push(validate_person(index, person)?);
    }

    let mut seen = HashSet::with_capacity(people.len());
    for (index, person) in people.iter().enumerate() {
        if let Some(email) = person.email.as_deref()
            && !seen.insert(email.to_ascii_lowercase())
        {
            return Err(invalid(&format!(
                "people[{index}].email is already in the list"
            )));
        }
    }

    Ok(ValidPreference {
        meeting_key,
        meeting_title: meeting_title.to_string(),
        people,
    })
}

/// Checks the person at `index` and returns it with trimmed label and email.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` naming the person and the rule
/// broken: label length, email presence or length.
fn validate_person(
    index: usize,
    person: &PreferencePerson,
) -> Result<PreferencePerson, ApiFailure> {
    let label = person.label.trim();
    if label.is_empty() || label.chars().count() > MAX_LABEL_CHARS {
        return Err(invalid(&format!(
            "people[{index}].label must have between 1 and {MAX_LABEL_CHARS} characters"
        )));
    }
    let email = match (person.kind, person.email.as_deref()) {
        (PersonKind::Attendee, Some(email)) => {
            let email = email.trim();
            if email.is_empty() || email.chars().count() > MAX_EMAIL_CHARS {
                return Err(invalid(&format!(
                    "people[{index}].email must have between 1 and {MAX_EMAIL_CHARS} characters"
                )));
            }
            Some(email.to_string())
        }
        (PersonKind::Attendee, None) => {
            return Err(invalid(&format!(
                "people[{index}].email is required for attendees"
            )));
        }
        (PersonKind::Manual, Some(_)) => {
            return Err(invalid(&format!(
                "people[{index}].email is only allowed for attendees"
            )));
        }
        (PersonKind::Manual, None) => None,
    };
    Ok(PreferencePerson {
        kind: person.kind,
        email,
        label: label.to_string(),
    })
}

fn invalid(message: &str) -> ApiFailure {
    ApiFailure::InvalidRequest(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attendee(email: &str) -> PreferencePerson {
        PreferencePerson {
            kind: PersonKind::Attendee,
            email: Some(email.to_string()),
            label: "Ada".to_string(),
        }
    }

    fn manual(label: &str) -> PreferencePerson {
        PreferencePerson {
            kind: PersonKind::Manual,
            email: None,
            label: label.to_string(),
        }
    }

    fn request(title: &str, people: Vec<PreferencePerson>) -> PutPreferenceRequest {
        PutPreferenceRequest {
            meeting_title: title.to_string(),
            people,
        }
    }

    fn message_of(result: Result<ValidPreference, ApiFailure>) -> String {
        match result {
            Err(ApiFailure::InvalidRequest(message)) => message,
            other => panic!("expected InvalidRequest, got {other:?}"),
        }
    }

    #[test]
    fn accepts_valid_request_and_trims_text() {
        let mut person = attendee("  ada@example.com ");
        person.label = "  Ada  ".to_string();

        let valid =
            validate_preference(&request("  Daily  Sync ", vec![person, manual(" Marco ")]))
                .unwrap();

        assert_eq!(valid.meeting_key, "daily sync");
        assert_eq!(valid.meeting_title, "Daily  Sync");
        assert_eq!(valid.people[0].email.as_deref(), Some("ada@example.com"));
        assert_eq!(valid.people[0].label, "Ada");
        assert_eq!(valid.people[1].label, "Marco");
    }

    #[test]
    fn rejects_blank_title() {
        let result = validate_preference(&request(" \t ", vec![attendee("a@x.com")]));

        assert!(message_of(result).contains("meeting_title"));
    }

    #[test]
    fn rejects_title_over_500_characters() {
        let title = "a".repeat(MAX_TITLE_CHARS + 1);

        let result = validate_preference(&request(&title, vec![attendee("a@x.com")]));

        assert!(message_of(result).contains("meeting_title"));
    }

    #[test]
    fn accepts_title_of_500_multibyte_characters() {
        let title = "è".repeat(MAX_TITLE_CHARS);

        let result = validate_preference(&request(&title, vec![attendee("a@x.com")]));

        assert!(result.is_ok());
    }

    #[test]
    fn rejects_no_people() {
        let result = validate_preference(&request("Daily", Vec::new()));

        assert!(message_of(result).contains("people"));
    }

    #[test]
    fn rejects_more_than_100_people() {
        let people = (0..=MAX_PEOPLE).map(|i| manual(&format!("P{i}"))).collect();

        let result = validate_preference(&request("Daily", people));

        assert!(message_of(result).contains("people"));
    }

    #[test]
    fn accepts_100_people() {
        let people = (0..MAX_PEOPLE).map(|i| manual(&format!("P{i}"))).collect();

        assert!(validate_preference(&request("Daily", people)).is_ok());
    }

    #[test]
    fn rejects_blank_label() {
        let result = validate_preference(&request("Daily", vec![manual("  ")]));

        assert!(message_of(result).contains("people[0].label"));
    }

    #[test]
    fn rejects_label_over_200_characters() {
        let label = "a".repeat(MAX_LABEL_CHARS + 1);

        let result = validate_preference(&request("Daily", vec![manual(&label)]));

        assert!(message_of(result).contains("people[0].label"));
    }

    #[test]
    fn accepts_label_of_200_multibyte_characters() {
        let label = "è".repeat(MAX_LABEL_CHARS);

        assert!(validate_preference(&request("Daily", vec![manual(&label)])).is_ok());
    }

    #[test]
    fn rejects_attendee_without_email() {
        let mut person = attendee("a@x.com");
        person.email = None;

        let result = validate_preference(&request("Daily", vec![person]));

        assert!(message_of(result).contains("people[0].email"));
    }

    #[test]
    fn rejects_attendee_with_blank_email() {
        let result = validate_preference(&request("Daily", vec![attendee("   ")]));

        assert!(message_of(result).contains("people[0].email"));
    }

    #[test]
    fn rejects_email_over_320_characters() {
        let email = "a".repeat(MAX_EMAIL_CHARS + 1);

        let result = validate_preference(&request("Daily", vec![attendee(&email)]));

        assert!(message_of(result).contains("people[0].email"));
    }

    #[test]
    fn rejects_manual_person_with_email() {
        let mut person = manual("Marco");
        person.email = Some("a@x.com".to_string());

        let result = validate_preference(&request("Daily", vec![person]));

        assert!(message_of(result).contains("people[0].email"));
    }

    #[test]
    fn rejects_manual_person_with_empty_email() {
        let mut person = manual("Marco");
        person.email = Some(String::new());

        let result = validate_preference(&request("Daily", vec![person]));

        assert!(message_of(result).contains("people[0].email"));
    }

    // Trim and ASCII case are ignored when comparing emails.
    #[test]
    fn rejects_duplicate_email_ignoring_case_and_spacing() {
        let people = vec![attendee("ada@example.com"), attendee("  ADA@Example.com ")];

        let result = validate_preference(&request("Daily", people));

        assert!(message_of(result).contains("people[1].email"));
    }

    #[test]
    fn accepts_distinct_emails_and_repeated_manual_labels() {
        let people = vec![
            attendee("a@x.com"),
            attendee("b@x.com"),
            manual("Marco"),
            manual("Marco"),
        ];

        assert!(validate_preference(&request("Daily", people)).is_ok());
    }

    // Title rules come before people rules.
    #[test]
    fn reports_the_title_before_the_people() {
        let result = validate_preference(&request("", Vec::new()));

        assert!(message_of(result).contains("meeting_title"));
    }
}
