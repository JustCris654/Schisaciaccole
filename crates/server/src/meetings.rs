//! `GET /api/v1/meetings`: the signed-in user's meetings in a time range,
//! read from Google Calendar with the user's stored Google refresh token.

use std::future::Future;
use std::time::Instant;

use axum::Json;
use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::http::header;
use axum::response::{IntoResponse, Response};
use chrono::{DateTime, FixedOffset, TimeDelta};
use openidconnect::AccessToken;
use schisaciaccole_core::plan::normalize_title;
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;

use crate::app::AppState;
use crate::auth::AuthUser;
use crate::calendar::{self, CalendarError};
use crate::crypto;
use crate::error::ApiFailure;
use crate::events::map_events;
use crate::google::GoogleError;
use crate::store;

/// Longest time range accepted by `GET /api/v1/meetings`, in hours.
pub const MAX_RANGE_HOURS: i64 = 48;

/// Query parameters of `GET /api/v1/meetings`, as received.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MeetingsQuery {
    /// Start of the range, RFC 3339 with offset.
    pub from: Option<String>,
    /// End of the range, RFC 3339 with offset.
    pub to: Option<String>,
}

/// Validated time range of `GET /api/v1/meetings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeRange {
    /// Start of the range.
    pub from: DateTime<FixedOffset>,
    /// End of the range, after `from` and at most 48 hours later.
    pub to: DateTime<FixedOffset>,
}

/// `GET /api/v1/meetings?from=<RFC3339>&to=<RFC3339>`: lists the meetings
/// of the user's primary Google calendar overlapping the range, in start
/// order (see [`map_events`] for the events left out). Answers 200 with a
/// JSON array of meetings, marked as not cacheable. A meeting has
/// `has_template` set when the user saved a template for its normalized
/// title.
///
/// The Google access token is cached per user. When Google Calendar
/// answers 401, the token is refreshed and the request retried once.
///
/// # Errors
///
/// - 401 `unauthorized` when [`AuthUser`] cannot be extracted;
/// - 400 `invalid_request` when the range is invalid (see
///   [`validate_range`]);
/// - 401 `google_reauth_required` when no Google credentials are stored,
///   they lack the Calendar scope, Google answers `invalid_grant` (the
///   credentials are then deleted), or Calendar answers 401 even with a
///   fresh token;
/// - 502 `internal` when Google fails in any other way or is unreachable;
/// - 500 `internal` on database or decryption failure.
pub async fn list_meetings(
    State(state): State<AppState>,
    user: AuthUser,
    query: Result<Query<MeetingsQuery>, QueryRejection>,
) -> Result<Response, ApiFailure> {
    let Query(query) =
        query.map_err(|rejection| ApiFailure::InvalidRequest(rejection.body_text()))?;
    let range = validate_range(&query)?;
    let items = fetch_calendar_events(&state, user.user_id, range).await?;
    let mut meetings = map_events(&items);
    crate::groups::resolve_meetings(&state, user.user_id, &mut meetings).await?;
    let keys = store::template_keys(&state.pool, user.user_id).await?;
    for meeting in &mut meetings {
        meeting.has_template = keys.contains(&normalize_title(&meeting.title));
    }
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(meetings)).into_response())
}

/// Checks the parameters of `GET /api/v1/meetings`.
///
/// Both `from` and `to` are required and must be RFC 3339 timestamps with
/// an offset (`Z` or `±hh:mm`); `to` must be after `from` and at most
/// [`MAX_RANGE_HOURS`] hours later. The two offsets may differ.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` naming the first rule broken.
pub fn validate_range(query: &MeetingsQuery) -> Result<TimeRange, ApiFailure> {
    let from = parse_timestamp(query.from.as_deref(), "from")?;
    let to = parse_timestamp(query.to.as_deref(), "to")?;
    if to <= from {
        return Err(invalid_request("to must be after from"));
    }
    if to - from > TimeDelta::hours(MAX_RANGE_HOURS) {
        return Err(invalid_request("the range must not exceed 48 hours"));
    }
    Ok(TimeRange { from, to })
}

/// Parses the RFC 3339 parameter `name`.
///
/// # Errors
///
/// Returns `Err(ApiFailure::InvalidRequest)` when `value` is missing or not
/// RFC 3339 with an offset.
fn parse_timestamp(value: Option<&str>, name: &str) -> Result<DateTime<FixedOffset>, ApiFailure> {
    value
        .and_then(|value| DateTime::parse_from_rfc3339(value).ok())
        .ok_or_else(|| invalid_request(&format!("{name} must be an RFC 3339 timestamp")))
}

/// Lists the raw Calendar events of `user_id` in `range`, retrying once
/// with a fresh access token when Calendar answers 401.
///
/// # Errors
///
/// Returns `Err` in the Google, database and decryption cases listed on
/// [`list_meetings`].
async fn fetch_calendar_events(
    state: &AppState,
    user_id: Uuid,
    range: TimeRange,
) -> Result<Vec<Value>, ApiFailure> {
    let result = with_google_token(state, user_id, |token| async move {
        calendar::fetch_events(
            state.google.http(),
            &state.config.google_calendar_base_url,
            &token,
            range.from,
            range.to,
        )
        .await
    })
    .await?;
    result.map_err(calendar_failure)
}

/// Runs a Google Calendar request for `user_id` with the user's access
/// token, retrying once with a fresh token when Calendar answers 401.
///
/// The token comes from the per-user cache, or is refreshed when absent.
/// `request` is called with the token and may be called a second time. On a
/// first 401 the cached token is dropped, a new one is obtained and
/// `request` is called again.
///
/// The outer `Result` holds the failures the helper already maps: token,
/// database and decryption failures, and a second 401. The inner `Result`
/// is the outcome of the last call of `request`, other than a 401 after the
/// retry; the caller maps its errors (for example a not-found error to 404).
///
/// # Errors
///
/// - `ApiFailure::GoogleReauthRequired` in the cases listed on
///   [`refresh_access_token`], or when Calendar answers 401 even with a
///   fresh token (the cached token is then dropped too);
/// - the other `Err` cases listed on [`refresh_access_token`].
///
/// A `CalendarError` other than `Unauthorized` is returned in the inner
/// `Result`, not as `Err` here.
pub(crate) async fn with_google_token<T, F, Fut>(
    state: &AppState,
    user_id: Uuid,
    request: F,
) -> Result<Result<T, CalendarError>, ApiFailure>
where
    F: Fn(AccessToken) -> Fut,
    Fut: Future<Output = Result<T, CalendarError>>,
{
    let token = access_token(state, user_id).await?;
    match request(token).await {
        Err(CalendarError::Unauthorized) => {}
        other => return Ok(other),
    }

    state.google_tokens.remove(user_id);
    let token = refresh_access_token(state, user_id).await?;
    match request(token).await {
        Err(CalendarError::Unauthorized) => {
            state.google_tokens.remove(user_id);
            Err(ApiFailure::GoogleReauthRequired)
        }
        other => Ok(other),
    }
}

/// Returns the cached Google access token of `user_id`, or a new one.
///
/// # Errors
///
/// Returns `Err` in the cases listed on [`refresh_access_token`].
pub(crate) async fn access_token(
    state: &AppState,
    user_id: Uuid,
) -> Result<AccessToken, ApiFailure> {
    if let Some(token) = state.google_tokens.get(user_id, Instant::now()) {
        return Ok(token);
    }
    refresh_access_token(state, user_id).await
}

/// Obtains a new Google access token for `user_id` from the stored refresh
/// token and caches it until its expiry. A token whose response has no
/// lifetime is used once and not cached.
///
/// # Errors
///
/// - `ApiFailure::GoogleReauthRequired` when no credentials are stored,
///   their scopes lack `config.google_calendar_scope`, or Google answers
///   `invalid_grant`; in the last case the stored credentials and the
///   cached token are dropped;
/// - `ApiFailure::BadGateway` when Google fails in any other way;
/// - `ApiFailure::Database` or `ApiFailure::Crypto` when the credentials
///   cannot be read, deleted or decrypted;
/// - `ApiFailure::Internal` when the decrypted token is not UTF-8.
async fn refresh_access_token(state: &AppState, user_id: Uuid) -> Result<AccessToken, ApiFailure> {
    let config = &state.config;
    let mut credential_lock = store::lock_google_credentials(&state.pool, user_id).await?;
    let credentials = store::google_credentials(&mut *credential_lock, user_id)
        .await?
        .ok_or(ApiFailure::GoogleReauthRequired)?;
    if !has_scope(&credentials.scopes, &config.google_calendar_scope) {
        return Err(ApiFailure::GoogleReauthRequired);
    }
    let refresh_token = crypto::decrypt(&config.token_enc_key, &credentials.refresh_token_enc)?;
    let refresh_token = String::from_utf8(refresh_token).map_err(|_| {
        ApiFailure::Internal("stored Google refresh token is not UTF-8".to_string())
    })?;

    let started = Instant::now();
    let result = match state.google.refresh_access_token(&refresh_token).await {
        Ok(access) => {
            if let Some(scopes) = &access.scopes {
                let scopes = scopes.join(" ");
                store::save_google_credentials(
                    &mut *credential_lock,
                    user_id,
                    &credentials.refresh_token_enc,
                    &scopes,
                )
                .await?;
                if !has_scope(&scopes, &config.google_calendar_scope) {
                    state.google_tokens.remove(user_id);
                    credential_lock.commit().await?;
                    return Err(ApiFailure::GoogleReauthRequired);
                }
            }
            let expires_at = access
                .expires_in
                .and_then(|lifetime| started.checked_add(lifetime));
            if let Some(expires_at) = expires_at {
                state.google_tokens.insert(
                    user_id,
                    access.token.clone(),
                    expires_at,
                    Instant::now(),
                );
            }
            Ok(access.token)
        }
        Err(GoogleError::InvalidGrant) => {
            store::delete_google_credentials(&mut *credential_lock, user_id).await?;
            state.google_tokens.remove(user_id);
            Err(ApiFailure::GoogleReauthRequired)
        }
        Err(err) => Err(ApiFailure::BadGateway(err.to_string())),
    };
    credential_lock.commit().await?;
    result
}

/// True when the space-separated `scopes` contain exactly `wanted`.
fn has_scope(scopes: &str, wanted: &str) -> bool {
    scopes.split_whitespace().any(|scope| scope == wanted)
}

/// Maps a Calendar error the caller does not handle to a 502.
pub(crate) fn calendar_failure(err: CalendarError) -> ApiFailure {
    ApiFailure::BadGateway(err.to_string())
}

fn invalid_request(message: &str) -> ApiFailure {
    ApiFailure::InvalidRequest(message.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(from: Option<&str>, to: Option<&str>) -> MeetingsQuery {
        MeetingsQuery {
            from: from.map(str::to_string),
            to: to.map(str::to_string),
        }
    }

    fn rejects(from: Option<&str>, to: Option<&str>) -> bool {
        matches!(
            validate_range(&query(from, to)),
            Err(ApiFailure::InvalidRequest(_))
        )
    }

    #[test]
    fn accepts_valid_range() {
        let range = validate_range(&query(
            Some("2026-11-05T00:00:00+01:00"),
            Some("2026-11-06T00:00:00+01:00"),
        ))
        .unwrap();

        assert_eq!(range.from.to_rfc3339(), "2026-11-05T00:00:00+01:00");
        assert_eq!(range.to.to_rfc3339(), "2026-11-06T00:00:00+01:00");
    }

    #[test]
    fn accepts_exactly_48_hours() {
        assert!(!rejects(
            Some("2026-11-05T00:00:00Z"),
            Some("2026-11-07T00:00:00Z")
        ));
    }

    #[test]
    fn rejects_more_than_48_hours() {
        assert!(rejects(
            Some("2026-11-05T00:00:00Z"),
            Some("2026-11-07T00:00:01Z")
        ));
    }

    #[test]
    fn compares_instants_across_offsets() {
        // 23:00+01:00 is 22:00Z, so `to` is one hour after `from`.
        assert!(!rejects(
            Some("2026-11-05T21:00:00Z"),
            Some("2026-11-05T23:00:00+01:00")
        ));
    }

    #[test]
    fn rejects_to_equal_to_from() {
        assert!(rejects(
            Some("2026-11-05T09:00:00Z"),
            Some("2026-11-05T09:00:00Z")
        ));
    }

    #[test]
    fn rejects_to_before_from() {
        assert!(rejects(
            Some("2026-11-05T09:00:00Z"),
            Some("2026-11-05T08:00:00Z")
        ));
    }

    #[test]
    fn rejects_missing_from() {
        assert!(rejects(None, Some("2026-11-05T09:00:00Z")));
    }

    #[test]
    fn rejects_missing_to() {
        assert!(rejects(Some("2026-11-05T09:00:00Z"), None));
    }

    #[test]
    fn rejects_timestamp_without_offset() {
        assert!(rejects(
            Some("2026-11-05T09:00:00"),
            Some("2026-11-05T10:00:00Z")
        ));
    }

    #[test]
    fn rejects_date_only() {
        assert!(rejects(Some("2026-11-05"), Some("2026-11-06T00:00:00Z")));
    }

    #[test]
    fn has_scope_finds_listed_scope() {
        assert!(has_scope("openid email profile", "email"));
    }

    #[test]
    fn has_scope_rejects_prefix_of_listed_scope() {
        let scopes = "openid https://www.googleapis.com/auth/calendar.events.owned.readonly";

        assert!(!has_scope(
            scopes,
            "https://www.googleapis.com/auth/calendar.events"
        ));
    }

    #[test]
    fn has_scope_is_false_for_empty_scopes() {
        assert!(!has_scope("", "email"));
    }
}
