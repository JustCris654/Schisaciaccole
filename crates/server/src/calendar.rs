//! Client of the Google Calendar API: lists the events of the user's
//! primary calendar.

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
