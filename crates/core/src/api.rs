//! Request and response types of the backend JSON API, shared by server and client.

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};

/// Path of the endpoint that starts the Google login flow.
pub const AUTH_LOGIN_PATH: &str = "/auth/login";
/// Path of the OAuth callback invoked by Google.
pub const AUTH_CALLBACK_PATH: &str = "/auth/google/callback";
/// Path of the endpoint that exchanges an authorization code for tokens.
pub const AUTH_TOKEN_PATH: &str = "/auth/token";
/// Path of the endpoint that renews an access token.
pub const AUTH_REFRESH_PATH: &str = "/auth/refresh";
/// Path of the endpoint that revokes a refresh token.
pub const AUTH_LOGOUT_PATH: &str = "/auth/logout";
/// Path of the endpoint that returns the signed-in user.
pub const ME_PATH: &str = "/api/v1/me";
/// Path of the endpoint that lists meetings; takes `from` and `to` (RFC 3339) query parameters.
pub const MEETINGS_PATH: &str = "/api/v1/meetings";
/// Path of the endpoint that creates or replaces a slot template (`PUT`).
pub const TEMPLATES_PATH: &str = "/api/v1/templates";
/// Axum route pattern of the slot plan of a meeting; see [`meeting_plan_path`].
pub const MEETING_PLAN_ROUTE: &str = "/api/v1/meetings/{event_id}/plan";
/// Axum route pattern of a single slot template; see [`template_path`].
pub const TEMPLATE_ROUTE: &str = "/api/v1/templates/{id}";
/// Path of the endpoint that creates or replaces a meeting preference (`PUT`).
pub const PREFERENCES_PATH: &str = "/api/v1/preferences";
/// Axum route pattern of a single meeting preference; see [`preference_path`].
pub const PREFERENCE_ROUTE: &str = "/api/v1/preferences/{id}";
/// Path of the health check endpoint.
pub const HEALTH_PATH: &str = "/healthz";

/// Builds the path of `GET /api/v1/meetings/{event_id}/plan`.
///
/// The id is inserted as is, without escaping: callers must pass an id made only of URL-safe
/// characters. Edge case: an empty id gives a path that matches no route.
pub fn meeting_plan_path(event_id: &str) -> String {
    format!("/api/v1/meetings/{event_id}/plan")
}

/// Builds the path of `/api/v1/templates/{id}`.
///
/// The id is inserted as is, without escaping: callers must pass an id made only of URL-safe
/// characters. Edge case: an empty id gives a path that matches no route.
pub fn template_path(id: &str) -> String {
    format!("/api/v1/templates/{id}")
}

/// Builds the path of `/api/v1/preferences/{id}`.
///
/// The id is inserted as is, without escaping: callers must pass an id made only of URL-safe
/// characters. Edge case: an empty id gives a path that matches no route.
pub fn preference_path(id: &str) -> String {
    format!("/api/v1/preferences/{id}")
}

/// Known values of [`ApiError::code`].
pub mod error_codes {
    /// The access token is missing, invalid or expired.
    pub const UNAUTHORIZED: &str = "unauthorized";
    /// Google rejected the stored credentials; the user must sign in again.
    pub const GOOGLE_REAUTH_REQUIRED: &str = "google_reauth_required";
    /// The request is malformed or has invalid parameters.
    pub const INVALID_REQUEST: &str = "invalid_request";
    /// The authorization code or its verifier is invalid.
    pub const INVALID_CODE: &str = "invalid_code";
    /// The user's email domain is not allowed.
    pub const FORBIDDEN_DOMAIN: &str = "forbidden_domain";
    /// The resource does not exist or belongs to another user.
    pub const NOT_FOUND: &str = "not_found";
    /// The client sent too many requests; retry later.
    pub const TOO_MANY_REQUESTS: &str = "too_many_requests";
    /// Unexpected server failure.
    pub const INTERNAL: &str = "internal";
}

/// Body of `POST /auth/token`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenRequest {
    /// Authorization code received from the login flow.
    pub code: String,
    /// PKCE code verifier matching the code challenge of the login.
    pub code_verifier: String,
}

/// Body of `POST /auth/refresh` and `POST /auth/logout`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RefreshRequest {
    /// Refresh token previously issued by the backend.
    pub refresh_token: String,
}

/// Response of `POST /auth/token` and `POST /auth/refresh`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenResponse {
    /// Short-lived token sent with every API request.
    pub access_token: String,
    /// Long-lived token used to obtain a new access token.
    pub refresh_token: String,
    /// Lifetime of the access token, in seconds.
    pub expires_in: u64,
}

/// Response of `GET /api/v1/me`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Me {
    /// Email of the signed-in user.
    pub email: String,
    /// Display name, when the account has one.
    pub display_name: Option<String>,
}

/// Answer of an attendee to a meeting invitation, spelled as Google does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResponseStatus {
    /// No answer yet.
    NeedsAction,
    /// The attendee declined.
    Declined,
    /// The attendee answered maybe.
    Tentative,
    /// The attendee accepted.
    Accepted,
}

/// A person invited to a meeting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Attendee {
    /// Email address of the attendee.
    pub email: String,
    /// Display name; may be empty when the calendar provides none.
    pub name: String,
    /// True when the attendee is the signed-in user.
    pub is_self: bool,
    /// True when attendance is optional.
    pub optional: bool,
    /// Answer to the invitation.
    pub response: ResponseStatus,
}

/// Completeness of the participant list after group resolution.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "snake_case")]
pub struct ParticipantResolution {
    /// True when at least one invited address could not be fully resolved.
    pub incomplete: bool,
    /// True when granting additional Google access may allow resolution.
    pub reauthorization_required: bool,
}

/// A calendar event as returned by `GET /api/v1/meetings`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Meeting {
    /// Identifier of the event in the calendar.
    pub event_id: String,
    /// Title of the event.
    pub title: String,
    /// Start instant, with the offset reported by the calendar.
    pub start: DateTime<FixedOffset>,
    /// End instant, with the offset reported by the calendar.
    pub end: DateTime<FixedOffset>,
    /// Invited people; may be incomplete when `attendees_omitted` is true.
    pub attendees: Vec<Attendee>,
    /// True when the calendar left out some attendees (very large events).
    pub attendees_omitted: bool,
    /// Group resolution status; independent of Calendar's omitted attendees.
    #[serde(default)]
    pub participant_resolution: ParticipantResolution,
    /// Identifier of the recurring series this event belongs to, if any.
    pub recurring_event_id: Option<String>,
    /// True when the signed-in user saved a slot preference for this meeting's title.
    #[serde(default)]
    pub has_template: bool,
    /// The signed-in user's saved preference for this meeting's normalized title.
    #[serde(default)]
    pub preference: Option<MeetingPreference>,
}

/// Kind of a person saved in a meeting preference or shown in the participant editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PersonKind {
    /// A meeting attendee identified by email; the saved entry renames them.
    Attendee,
    /// A person added by hand, not tied to an attendee.
    Manual,
}

/// One person saved in a meeting preference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PreferencePerson {
    /// Kind of the person.
    pub kind: PersonKind,
    /// Email of the attendee; `Some` only for [`PersonKind::Attendee`].
    pub email: Option<String>,
    /// Label shown for the person.
    pub label: String,
}

/// Body of `PUT /api/v1/preferences`; creates or replaces the preference of the normalized title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PutPreferenceRequest {
    /// Title of the meeting the preference applies to.
    pub meeting_title: String,
    /// Saved people: attendee renames first, then manual people.
    pub people: Vec<PreferencePerson>,
}

/// Response of `PUT /api/v1/preferences`, and the preference embedded in [`Meeting`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct MeetingPreference {
    /// Identifier of the preference.
    pub id: String,
    /// Title of the meeting the preference applies to.
    pub meeting_title: String,
    /// Saved people.
    pub people: Vec<PreferencePerson>,
}

/// Kind of an entry of a slot template or plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    /// A meeting attendee, identified by email.
    Attendee,
    /// A slot added by hand, not tied to an attendee.
    Manual,
    /// Time not assigned to anybody.
    FreeTime,
}

/// One saved entry of a slot template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TemplateEntry {
    /// Kind of the entry.
    pub kind: EntryKind,
    /// Email of the attendee; `Some` only for [`EntryKind::Attendee`].
    pub email: Option<String>,
    /// Label shown for the entry.
    pub label: String,
    /// Weight in seconds; 0 means the participant is excluded.
    pub weight_secs: u32,
}

/// Body of `PUT /api/v1/templates`; creates or replaces the template of the normalized title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PutTemplateRequest {
    /// Title of the meeting the template applies to.
    pub meeting_title: String,
    /// Entries in position order.
    pub entries: Vec<TemplateEntry>,
}

/// Response of `PUT /api/v1/templates`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Template {
    /// Identifier of the template.
    pub id: String,
    /// Title of the meeting the template applies to.
    pub meeting_title: String,
    /// Entries in position order.
    pub entries: Vec<TemplateEntry>,
}

/// Where a [`SlotPlan`] comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanSource {
    /// Built from a saved template.
    Template,
    /// No template: equal slots for everybody.
    Equal,
}

/// One entry of a computed slot plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct PlanEntry {
    /// Kind of the entry.
    pub kind: EntryKind,
    /// Email of the attendee; `Some` only for [`EntryKind::Attendee`].
    pub email: Option<String>,
    /// Label shown for the entry.
    pub label: String,
    /// Relative weight (seconds from a template, 1 for equal plans); 0 means excluded.
    pub weight: u64,
}

/// Response of `GET /api/v1/meetings/{event_id}/plan`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SlotPlan {
    /// Where the plan comes from.
    pub source: PlanSource,
    /// Identifier of the template used; `None` for equal plans.
    pub template_id: Option<String>,
    /// Entries in speaking order.
    pub entries: Vec<PlanEntry>,
    /// Freshly resolved attendees; absent in responses from older servers.
    #[serde(default)]
    pub attendees: Option<Vec<Attendee>>,
    /// True when Calendar omitted attendees from the freshly read event.
    #[serde(default)]
    pub attendees_omitted: bool,
    /// Group resolution status of the event read to build this plan.
    #[serde(default)]
    pub participant_resolution: ParticipantResolution,
}

/// Error body returned by the backend on failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApiError {
    /// Machine-readable code; see [`error_codes`] for the known values.
    pub code: String,
    /// Human-readable description.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;
    use serde_json::json;

    fn round_trip<T>(value: &T) -> T
    where
        T: Serialize + DeserializeOwned,
    {
        let text = serde_json::to_string(value).unwrap();
        serde_json::from_str(&text).unwrap()
    }

    fn sample_attendee() -> Attendee {
        Attendee {
            email: "anna.rossi@openmove.com".to_string(),
            name: "Anna Rossi".to_string(),
            is_self: true,
            optional: false,
            response: ResponseStatus::NeedsAction,
        }
    }

    fn sample_meeting() -> Meeting {
        Meeting {
            event_id: "evt1".to_string(),
            title: "Daily".to_string(),
            start: DateTime::parse_from_rfc3339("2026-11-05T09:30:00+01:00").unwrap(),
            end: DateTime::parse_from_rfc3339("2026-11-05T10:00:00+01:00").unwrap(),
            attendees: vec![sample_attendee()],
            attendees_omitted: false,
            participant_resolution: ParticipantResolution::default(),
            recurring_event_id: Some("series1".to_string()),
            has_template: false,
            preference: None,
        }
    }

    #[test]
    fn token_request_round_trips() {
        let value = TokenRequest {
            code: "c".to_string(),
            code_verifier: "v".to_string(),
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn refresh_request_round_trips() {
        let value = RefreshRequest {
            refresh_token: "r".to_string(),
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn token_response_round_trips() {
        let value = TokenResponse {
            access_token: "a".to_string(),
            refresh_token: "r".to_string(),
            expires_in: 3600,
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn me_round_trips() {
        let value = Me {
            email: "a@x.it".to_string(),
            display_name: None,
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn attendee_round_trips() {
        let value = sample_attendee();
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn meeting_round_trips_keeping_offset() {
        let value = sample_meeting();
        let back = round_trip(&value);
        assert_eq!(back, value);
        assert_eq!(back.start.offset().local_minus_utc(), 3600);
    }

    #[test]
    fn api_error_round_trips() {
        let value = ApiError {
            code: error_codes::UNAUTHORIZED.to_string(),
            message: "no".to_string(),
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn response_status_round_trips_every_variant() {
        for status in [
            ResponseStatus::NeedsAction,
            ResponseStatus::Declined,
            ResponseStatus::Tentative,
            ResponseStatus::Accepted,
        ] {
            assert_eq!(round_trip(&status), status);
        }
    }

    #[test]
    fn response_status_uses_google_spelling() {
        let names = [
            (ResponseStatus::NeedsAction, "needsAction"),
            (ResponseStatus::Declined, "declined"),
            (ResponseStatus::Tentative, "tentative"),
            (ResponseStatus::Accepted, "accepted"),
        ];
        for (status, name) in names {
            assert_eq!(serde_json::to_value(status).unwrap(), json!(name));
        }
    }

    #[test]
    fn attendee_json_field_names() {
        let value = serde_json::to_value(sample_attendee()).unwrap();
        let expected = json!({
            "email": "anna.rossi@openmove.com",
            "name": "Anna Rossi",
            "is_self": true,
            "optional": false,
            "response": "needsAction",
        });
        assert_eq!(value, expected);
    }

    #[test]
    fn meeting_json_field_names() {
        let value = serde_json::to_value(sample_meeting()).unwrap();
        let expected = json!({
            "event_id": "evt1",
            "title": "Daily",
            "start": "2026-11-05T09:30:00+01:00",
            "end": "2026-11-05T10:00:00+01:00",
            "attendees": [{
                "email": "anna.rossi@openmove.com",
                "name": "Anna Rossi",
                "is_self": true,
                "optional": false,
                "response": "needsAction",
            }],
            "attendees_omitted": false,
            "participant_resolution": {
                "incomplete": false,
                "reauthorization_required": false,
            },
            "recurring_event_id": "series1",
            "has_template": false,
            "preference": null,
        });
        assert_eq!(value, expected);
    }

    #[test]
    fn token_response_json_field_names() {
        let value = TokenResponse {
            access_token: "a".to_string(),
            refresh_token: "r".to_string(),
            expires_in: 60,
        };
        let expected = json!({"access_token": "a", "refresh_token": "r", "expires_in": 60});
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }

    fn sample_entry(kind: EntryKind, email: Option<&str>, secs: u32) -> TemplateEntry {
        TemplateEntry {
            kind,
            email: email.map(str::to_string),
            label: "Anna".to_string(),
            weight_secs: secs,
        }
    }

    fn sample_plan() -> SlotPlan {
        SlotPlan {
            source: PlanSource::Template,
            template_id: Some("t1".to_string()),
            entries: vec![PlanEntry {
                kind: EntryKind::FreeTime,
                email: None,
                label: "Tempo libero".to_string(),
                weight: 1200,
            }],
            attendees: Some(vec![sample_attendee()]),
            attendees_omitted: false,
            participant_resolution: ParticipantResolution::default(),
        }
    }

    #[test]
    fn entry_kind_round_trips_every_variant() {
        for kind in [EntryKind::Attendee, EntryKind::Manual, EntryKind::FreeTime] {
            assert_eq!(round_trip(&kind), kind);
        }
    }

    #[test]
    fn template_entry_round_trips() {
        let value = sample_entry(EntryKind::Attendee, Some("a@x.it"), 300);
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn put_template_request_round_trips() {
        let value = PutTemplateRequest {
            meeting_title: "Retro".to_string(),
            entries: vec![sample_entry(EntryKind::Manual, None, 60)],
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn template_round_trips() {
        let value = Template {
            id: "t1".to_string(),
            meeting_title: "Retro".to_string(),
            entries: vec![sample_entry(EntryKind::Manual, None, 60)],
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn plan_source_round_trips_every_variant() {
        for source in [PlanSource::Template, PlanSource::Equal] {
            assert_eq!(round_trip(&source), source);
        }
    }

    #[test]
    fn plan_entry_round_trips() {
        let value = PlanEntry {
            kind: EntryKind::Attendee,
            email: Some("a@x.it".to_string()),
            label: "Anna".to_string(),
            weight: 1,
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn slot_plan_round_trips() {
        let value = sample_plan();
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn template_entry_json_field_names() {
        let attendee = sample_entry(EntryKind::Attendee, Some("a@x.it"), 300);
        let expected = json!({
            "kind": "attendee",
            "email": "a@x.it",
            "label": "Anna",
            "weight_secs": 300,
        });
        assert_eq!(serde_json::to_value(attendee).unwrap(), expected);
    }

    #[test]
    fn template_entry_free_time_kind_is_snake_case() {
        let free = sample_entry(EntryKind::FreeTime, None, 1200);
        let expected = json!({
            "kind": "free_time",
            "email": null,
            "label": "Anna",
            "weight_secs": 1200,
        });
        assert_eq!(serde_json::to_value(free).unwrap(), expected);
    }

    #[test]
    fn slot_plan_json_field_names() {
        let expected = json!({
            "source": "template",
            "template_id": "t1",
            "attendees": [{
                "email": "anna.rossi@openmove.com",
                "name": "Anna Rossi",
                "is_self": true,
                "optional": false,
                "response": "needsAction",
            }],
            "attendees_omitted": false,
            "participant_resolution": {
                "incomplete": false,
                "reauthorization_required": false,
            },
            "entries": [{
                "kind": "free_time",
                "email": null,
                "label": "Tempo libero",
                "weight": 1200,
            }],
        });
        assert_eq!(serde_json::to_value(sample_plan()).unwrap(), expected);
    }

    #[test]
    fn meeting_without_resolution_uses_complete_default() {
        let expected = sample_meeting();
        let mut value = serde_json::to_value(&expected).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("participant_resolution");
        let actual: Meeting = serde_json::from_value(value).unwrap();
        assert_eq!(actual, expected);
    }

    #[test]
    fn old_slot_plan_has_no_fresh_attendees_or_resolution_warning() {
        let value = json!({ "source": "equal", "template_id": null, "entries": [] });
        let actual: SlotPlan = serde_json::from_value(value).unwrap();
        assert_eq!(actual.attendees, None);
        assert!(!actual.attendees_omitted);
        assert_eq!(
            actual.participant_resolution,
            ParticipantResolution::default()
        );
    }

    #[test]
    fn fresh_empty_attendees_are_distinct_from_missing_attendees() {
        let value = json!({
            "source": "equal", "template_id": null, "entries": [], "attendees": [],
            "participant_resolution": { "incomplete": true },
        });
        let actual: SlotPlan = serde_json::from_value(value).unwrap();
        assert_eq!(actual.attendees, Some(Vec::new()));
        assert!(actual.participant_resolution.incomplete);
        assert!(!actual.participant_resolution.reauthorization_required);
    }

    #[test]
    fn put_template_request_json_field_names() {
        let value = PutTemplateRequest {
            meeting_title: "Retro".to_string(),
            entries: vec![sample_entry(EntryKind::Manual, None, 60)],
        };
        let expected = json!({
            "meeting_title": "Retro",
            "entries": [{
                "kind": "manual",
                "email": null,
                "label": "Anna",
                "weight_secs": 60,
            }],
        });
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }

    #[test]
    fn meeting_plan_path_inserts_the_event_id() {
        assert_eq!(meeting_plan_path("evt1"), "/api/v1/meetings/evt1/plan");
    }

    #[test]
    fn template_path_inserts_the_id() {
        assert_eq!(template_path("t1"), "/api/v1/templates/t1");
    }

    #[test]
    fn preference_path_inserts_the_id() {
        assert_eq!(preference_path("p1"), "/api/v1/preferences/p1");
    }

    fn sample_person(kind: PersonKind, email: Option<&str>) -> PreferencePerson {
        PreferencePerson {
            kind,
            email: email.map(str::to_string),
            label: "Anna".to_string(),
        }
    }

    #[test]
    fn person_kind_round_trips_every_variant() {
        for kind in [PersonKind::Attendee, PersonKind::Manual] {
            assert_eq!(round_trip(&kind), kind);
        }
    }

    #[test]
    fn person_kind_is_snake_case() {
        assert_eq!(
            serde_json::to_value(PersonKind::Attendee).unwrap(),
            json!("attendee")
        );
        assert_eq!(
            serde_json::to_value(PersonKind::Manual).unwrap(),
            json!("manual")
        );
    }

    #[test]
    fn preference_person_json_field_names() {
        let value = sample_person(PersonKind::Attendee, Some("a@x.it"));
        let expected = json!({"kind": "attendee", "email": "a@x.it", "label": "Anna"});
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }

    #[test]
    fn put_preference_request_round_trips() {
        let value = PutPreferenceRequest {
            meeting_title: "Retro".to_string(),
            people: vec![
                sample_person(PersonKind::Attendee, Some("a@x.it")),
                sample_person(PersonKind::Manual, None),
            ],
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn put_preference_request_json_field_names() {
        let value = PutPreferenceRequest {
            meeting_title: "Retro".to_string(),
            people: vec![sample_person(PersonKind::Manual, None)],
        };
        let expected = json!({
            "meeting_title": "Retro",
            "people": [{"kind": "manual", "email": null, "label": "Anna"}],
        });
        assert_eq!(serde_json::to_value(value).unwrap(), expected);
    }

    #[test]
    fn meeting_preference_round_trips() {
        let value = MeetingPreference {
            id: "p1".to_string(),
            meeting_title: "Retro".to_string(),
            people: vec![sample_person(PersonKind::Manual, None)],
        };
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn meeting_with_preference_round_trips() {
        let mut value = sample_meeting();
        value.preference = Some(MeetingPreference {
            id: "p1".to_string(),
            meeting_title: "Daily".to_string(),
            people: vec![sample_person(PersonKind::Attendee, Some("a@x.it"))],
        });
        assert_eq!(round_trip(&value), value);
    }

    #[test]
    fn meeting_without_preference_field_deserializes_to_none() {
        let mut value = serde_json::to_value(sample_meeting()).unwrap();
        value.as_object_mut().unwrap().remove("preference");
        let actual: Meeting = serde_json::from_value(value).unwrap();
        assert_eq!(actual.preference, None);
    }
}
