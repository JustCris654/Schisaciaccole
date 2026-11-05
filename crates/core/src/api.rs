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
/// Path of the health check endpoint.
pub const HEALTH_PATH: &str = "/healthz";

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
    /// Identifier of the recurring series this event belongs to, if any.
    pub recurring_event_id: Option<String>,
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
            recurring_event_id: Some("series1".to_string()),
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
            "recurring_event_id": "series1",
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
}
