//! `GET /api/v1/meetings/{event_id}/plan`: the slot plan of one meeting,
//! built from the user's saved template for the meeting title or, without
//! one, with equal slots for everybody.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use schisaciaccole_core::api::{PlanSource, SlotPlan};
use schisaciaccole_core::plan::{build_plan, normalize_title};
use schisaciaccole_core::slots::participants;

use crate::app::AppState;
use crate::auth::AuthUser;
use crate::calendar::{self, CalendarError};
use crate::error::ApiFailure;
use crate::events::map_event;
use crate::meetings::{calendar_failure, with_google_token};
use crate::store;

/// Longest event id accepted, in characters.
pub const MAX_EVENT_ID_CHARS: usize = 1024;

/// `GET /api/v1/meetings/{event_id}/plan`: reads the event from the user's
/// primary Google calendar and answers 200 with its [`SlotPlan`], marked as
/// not cacheable.
///
/// When the user saved a template for the normalized title of the meeting,
/// the plan comes from it (`source` `template`, with the template id);
/// otherwise every participant gets an equal slot (`source` `equal`, no
/// template id). See `build_plan` and `participants` in the core crate for
/// how participants and template entries are matched.
///
/// The Google access token is cached per user. When Google Calendar
/// answers 401, the token is refreshed and the request retried once.
///
/// # Errors
///
/// - 401 `unauthorized` when [`AuthUser`] cannot be extracted;
/// - 400 `invalid_request` when the event id is not valid (see
///   [`validate_event_id`]);
/// - 401 `google_reauth_required` when no Google credentials are stored,
///   they lack the Calendar scope, Google answers `invalid_grant` (the
///   credentials are then deleted), or Calendar answers 401 even with a
///   fresh token;
/// - 404 `not_found` when Google answers 404 or 410 for the event, or the
///   event is not a meeting of the user: cancelled, all-day, declined by
///   the user or not parsable;
/// - 502 `internal` when Google fails in any other way or is unreachable;
/// - 500 `internal` on database or decryption failure.
pub async fn get_meeting_plan(
    State(state): State<AppState>,
    user: AuthUser,
    Path(event_id): Path<String>,
) -> Result<Response, ApiFailure> {
    validate_event_id(&event_id)?;
    let result = with_google_token(&state, user.user_id, |token| {
        let event_id = event_id.as_str();
        let state = &state;
        async move {
            calendar::fetch_event(
                state.google.http(),
                &state.config.google_calendar_base_url,
                &token,
                event_id,
            )
            .await
        }
    })
    .await?;
    let item = result.map_err(|err| match err {
        CalendarError::NotFound => ApiFailure::NotFound,
        other => calendar_failure(other),
    })?;
    let mut meeting = map_event(&item).ok_or(ApiFailure::NotFound)?;
    crate::groups::resolve_meetings(&state, user.user_id, std::slice::from_mut(&mut meeting))
        .await?;

    let template =
        store::template_by_key(&state.pool, user.user_id, &normalize_title(&meeting.title)).await?;
    let people = participants(&meeting);
    let plan = match template {
        Some(template) => SlotPlan {
            source: PlanSource::Template,
            template_id: Some(template.id.to_string()),
            attendees: Some(meeting.attendees.clone()),
            attendees_omitted: meeting.attendees_omitted,
            participant_resolution: meeting.participant_resolution,
            entries: build_plan(&people, Some(&template.entries)),
        },
        None => SlotPlan {
            source: PlanSource::Equal,
            template_id: None,
            attendees: Some(meeting.attendees.clone()),
            attendees_omitted: meeting.attendees_omitted,
            participant_resolution: meeting.participant_resolution,
            entries: build_plan(&people, None),
        },
    };
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(plan)).into_response())
}

/// Checks the event id of the path.
///
/// Google event ids are made of letters, digits and a few symbols; only
/// ASCII letters, digits, `_` and `-` are accepted, 1 to
/// [`MAX_EVENT_ID_CHARS`] characters. Ids of recurring instances
/// (`base_20261105T083000Z`) pass; ids with other characters are refused
/// even when Google would know them.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` when the id is empty, too long
/// or has a character outside the accepted set.
pub fn validate_event_id(event_id: &str) -> Result<(), ApiFailure> {
    let valid_chars = event_id
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-');
    if event_id.is_empty() || event_id.len() > MAX_EVENT_ID_CHARS || !valid_chars {
        return Err(ApiFailure::InvalidRequest(
            "event_id must be 1 to 1024 letters, digits, '_' or '-'".to_string(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_rejected(event_id: &str) -> bool {
        matches!(
            validate_event_id(event_id),
            Err(ApiFailure::InvalidRequest(_))
        )
    }

    #[test]
    fn accepts_letters_digits_underscore_and_dash() {
        assert!(validate_event_id("abc_DEF-123").is_ok());
    }

    #[test]
    fn accepts_recurring_instance_id() {
        assert!(validate_event_id("daily_20261105T083000Z").is_ok());
    }

    #[test]
    fn accepts_the_longest_id() {
        assert!(validate_event_id(&"a".repeat(MAX_EVENT_ID_CHARS)).is_ok());
    }

    #[test]
    fn rejects_empty_id() {
        assert!(is_rejected(""));
    }

    #[test]
    fn rejects_id_longer_than_the_limit() {
        assert!(is_rejected(&"a".repeat(MAX_EVENT_ID_CHARS + 1)));
    }

    #[test]
    fn rejects_dot() {
        assert!(is_rejected("a.b"));
    }

    #[test]
    fn rejects_slash() {
        assert!(is_rejected("a/b"));
    }

    #[test]
    fn rejects_non_ascii_letter() {
        assert!(is_rejected("caffè"));
    }

    #[test]
    fn rejects_space() {
        assert!(is_rejected("a b"));
    }
}
