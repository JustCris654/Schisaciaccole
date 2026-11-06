//! Client of the Google Calendar API: lists the events of the user's
//! primary calendar and reads one of them.

use std::time::Duration;

use chrono::{DateTime, FixedOffset};
use openidconnect::AccessToken;
use openidconnect::reqwest::{self, StatusCode};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;
use tracing::warn;

use crate::google::error_chain;

/// Maximum number of result pages read for one listing.
pub const MAX_PAGES: usize = 5;

const PAGE_SIZE: &str = "250";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

/// Error returned by the Google Calendar client.
///
/// No variant carries a response body, which may contain personal data.
#[derive(Debug, Error)]
pub enum CalendarError {
    /// Google answered 401: the access token is invalid or expired.
    #[error("Google Calendar rejected the access token")]
    Unauthorized,

    /// Google answered 404 or 410: the event was deleted or the id is unknown.
    /// Only [`fetch_event`] returns it.
    #[error("Google Calendar event not found")]
    NotFound,

    /// Google answered with another non-success status.
    #[error("Google Calendar answered with status {0}")]
    Status(u16),

    /// Google could not be reached, or the response body could not be read.
    #[error("Google Calendar request failed: {0}")]
    Request(String),

    /// The response is not a JSON event list.
    #[error("unparsable Google Calendar response at line {line} column {column}")]
    Parse { line: usize, column: usize },
}

/// One page of `GET /calendars/primary/events`.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct EventsPage {
    #[serde(default)]
    items: Vec<Value>,
    next_page_token: Option<String>,
}

/// Lists the events of the primary calendar overlapping `from..to`, as raw
/// JSON items in Google's order (start time).
///
/// Recurring events are expanded into single instances and only events of
/// type `default` are requested. `nextPageToken` is followed for at most
/// [`MAX_PAGES`] pages of 250 events; further pages are ignored with a
/// warning. `base_url` may end with a slash. Each request times out after
/// 15 seconds.
///
/// # Errors
///
/// Returns `Err` when, for any page:
/// - Google answers 401 (`CalendarError::Unauthorized`);
/// - Google answers another non-2xx status (`CalendarError::Status`);
/// - the request fails or times out (`CalendarError::Request`);
/// - the body is not a JSON event list (`CalendarError::Parse`).
pub async fn fetch_events(
    http: &reqwest::Client,
    base_url: &str,
    access_token: &AccessToken,
    from: DateTime<FixedOffset>,
    to: DateTime<FixedOffset>,
) -> Result<Vec<Value>, CalendarError> {
    let url = format!(
        "{}/calendars/primary/events",
        base_url.trim_end_matches('/')
    );
    let time_min = from.to_rfc3339();
    let time_max = to.to_rfc3339();
    let base_query = [
        ("timeMin", time_min.as_str()),
        ("timeMax", time_max.as_str()),
        ("singleEvents", "true"),
        ("orderBy", "startTime"),
        ("eventTypes", "default"),
        ("maxResults", PAGE_SIZE),
    ];

    let mut items = Vec::new();
    let mut page_token: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let mut query = base_query.to_vec();
        if let Some(token) = page_token.as_deref() {
            query.push(("pageToken", token));
        }
        let page = fetch_page(http, &url, access_token, &query).await?;
        items.extend(page.items);
        match page.next_page_token {
            Some(token) => page_token = Some(token),
            None => return Ok(items),
        }
    }
    warn!(
        max_pages = MAX_PAGES,
        "Google Calendar has more events than read, ignoring the rest"
    );
    Ok(items)
}

/// Reads one event of the primary calendar as raw JSON.
///
/// `event_id` is added to the URL as one escaped path segment, so characters
/// such as `/` or `?` cannot change the path. `base_url` may end with a
/// slash. The request times out after 15 seconds. A cancelled event that
/// Google still serves is returned like any other; the caller checks its
/// `status`.
///
/// # Errors
///
/// Returns `Err` when:
/// - `base_url` is not a valid absolute URL (`CalendarError::Request`);
/// - Google answers 401 (`CalendarError::Unauthorized`);
/// - Google answers 404 or 410 (`CalendarError::NotFound`);
/// - Google answers another non-2xx status (`CalendarError::Status`);
/// - the request fails or times out (`CalendarError::Request`);
/// - the body is not JSON (`CalendarError::Parse`).
pub async fn fetch_event(
    http: &reqwest::Client,
    base_url: &str,
    access_token: &AccessToken,
    event_id: &str,
) -> Result<Value, CalendarError> {
    let url = event_url(base_url, event_id)?;
    let request = http.get(url).bearer_auth(access_token.secret());
    match send(request).await {
        Err(CalendarError::Status(404 | 410)) => Err(CalendarError::NotFound),
        Err(err) => Err(err),
        Ok(response) => read_json(response).await,
    }
}

/// Builds `{base_url}/calendars/primary/events/{event_id}` with `event_id`
/// escaped as a single path segment.
///
/// # Errors
///
/// Returns `Err(CalendarError::Request)` when `base_url` is not a valid
/// absolute URL that can have path segments.
fn event_url(base_url: &str, event_id: &str) -> Result<reqwest::Url, CalendarError> {
    let invalid = || CalendarError::Request("invalid Google Calendar base URL".to_string());
    let mut url = reqwest::Url::parse(base_url).map_err(|_| invalid())?;
    url.path_segments_mut()
        .map_err(|_| invalid())?
        .pop_if_empty()
        .extend(["calendars", "primary", "events"])
        .push(event_id);
    Ok(url)
}

/// Requests one page of events.
///
/// # Errors
///
/// Returns `Err` in the cases listed on [`fetch_events`].
async fn fetch_page(
    http: &reqwest::Client,
    url: &str,
    access_token: &AccessToken,
    query: &[(&str, &str)],
) -> Result<EventsPage, CalendarError> {
    let request = http
        .get(url)
        .bearer_auth(access_token.secret())
        .query(query);
    let response = send(request).await?;
    read_json(response).await
}

/// Sends `request` with the Calendar timeout and returns the response when
/// its status is 2xx.
///
/// # Errors
///
/// Returns `Err(CalendarError::Unauthorized)` on 401,
/// `Err(CalendarError::Status)` on any other non-2xx status and
/// `Err(CalendarError::Request)` when the request fails or times out.
async fn send(request: reqwest::RequestBuilder) -> Result<reqwest::Response, CalendarError> {
    let response = request
        .timeout(REQUEST_TIMEOUT)
        .send()
        .await
        .map_err(|err| CalendarError::Request(error_chain(&err.without_url())))?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED {
        return Err(CalendarError::Unauthorized);
    }
    if !status.is_success() {
        return Err(CalendarError::Status(status.as_u16()));
    }
    Ok(response)
}

/// Reads the body of `response` as JSON of type `T`.
///
/// # Errors
///
/// Returns `Err(CalendarError::Request)` when the body cannot be read and
/// `Err(CalendarError::Parse)` when it is not valid JSON of type `T`.
async fn read_json<T: serde::de::DeserializeOwned>(
    response: reqwest::Response,
) -> Result<T, CalendarError> {
    let body = response
        .bytes()
        .await
        .map_err(|err| CalendarError::Request(error_chain(&err.without_url())))?;
    serde_json::from_slice(&body).map_err(|err| CalendarError::Parse {
        line: err.line(),
        column: err.column(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url_string(base_url: &str, event_id: &str) -> String {
        event_url(base_url, event_id).unwrap().to_string()
    }

    #[test]
    fn event_url_appends_the_event_path() {
        assert_eq!(
            url_string("http://localhost:1234/calendar/v3", "abc"),
            "http://localhost:1234/calendar/v3/calendars/primary/events/abc"
        );
    }

    #[test]
    fn event_url_accepts_a_base_with_trailing_slash() {
        assert_eq!(
            url_string("http://localhost:1234/calendar/v3/", "abc"),
            "http://localhost:1234/calendar/v3/calendars/primary/events/abc"
        );
    }

    #[test]
    fn event_url_escapes_the_event_id() {
        assert_eq!(
            url_string("http://localhost:1234/calendar/v3", "a/b?c"),
            "http://localhost:1234/calendar/v3/calendars/primary/events/a%2Fb%3Fc"
        );
    }

    #[test]
    fn event_url_rejects_a_relative_base() {
        assert!(matches!(
            event_url("calendar/v3", "abc"),
            Err(CalendarError::Request(_))
        ));
    }
}
