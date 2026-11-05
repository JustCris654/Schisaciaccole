//! Blocking client of the Schisaciaccole backend: login code exchange, token refresh and API calls.
//!
//! Tokens are never logged. The refresh token lives in a [`SessionStore`]; the access token only
//! in memory.

use std::time::{Duration, Instant};

use chrono::{DateTime, FixedOffset};
use log::warn;
use reqwest::StatusCode;
use reqwest::blocking::{Client, Response};
use schisaciaccole_core::api::{
    AUTH_LOGIN_PATH, AUTH_LOGOUT_PATH, AUTH_REFRESH_PATH, AUTH_TOKEN_PATH, ApiError, HEALTH_PATH,
    ME_PATH, MEETINGS_PATH, Me, Meeting, RefreshRequest, TokenRequest, TokenResponse, error_codes,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::session_store::{SessionStore, SessionStoreError};

/// Connection timeout of every request.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Total timeout of every request.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
/// An access token expiring within this margin is renewed before use.
const REFRESH_MARGIN: Duration = Duration::from_secs(30);

/// Failure of a backend call.
#[derive(Debug, thiserror::Error)]
pub enum BackendError {
    /// The HTTP client could not be built.
    #[error("cannot build the HTTP client: {0}")]
    ClientBuild(reqwest::Error),
    /// The backend could not be reached (connection failure or timeout).
    #[error("backend unreachable")]
    Unreachable,
    /// There is no valid session: the user has to sign in.
    #[error("not logged in")]
    NotLoggedIn,
    /// Google rejected the stored credentials: the user has to sign in again.
    #[error("Google re-authentication required")]
    ReauthRequired,
    /// The backend answered 429.
    #[error("rate limited by the backend")]
    RateLimited,
    /// The backend answered with an error. `code` is empty when the body was not an `ApiError`.
    #[error("backend error {status} ({code}): {message}")]
    Server {
        /// HTTP status code.
        status: u16,
        /// Machine-readable error code.
        code: String,
        /// Human-readable description.
        message: String,
    },
    /// The session store failed.
    #[error("session store error: {0}")]
    Store(#[from] SessionStoreError),
    /// The response body could not be decoded.
    #[error("cannot decode the backend response")]
    Decode,
}

/// Access token kept in memory with its expiry.
struct AccessToken {
    token: String,
    expires_at: Instant,
}

/// Client of the backend API. Not thread-safe: use it from one thread at a time.
pub struct BackendClient {
    base_url: String,
    http: Client,
    store: Box<dyn SessionStore + Send>,
    access: Option<AccessToken>,
}

impl BackendClient {
    /// Creates a client for the backend at `base_url`; trailing slashes are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::ClientBuild`] when the HTTP client cannot be initialized
    /// (for example when the TLS backend fails to load).
    pub fn new(base_url: &str, store: Box<dyn SessionStore + Send>) -> Result<Self, BackendError> {
        let user_agent = format!(
            "schisaciaccole/{} ({})",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS
        );
        let http = Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .user_agent(user_agent)
            .build()
            .map_err(BackendError::ClientBuild)?;
        Ok(Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http,
            store,
            access: None,
        })
    }

    /// Builds the URL to open in the browser to start the login.
    ///
    /// `port` is the local port waiting for the redirect; all parameters are URL-encoded.
    pub fn login_url(&self, port: u16, state: &str, code_challenge: &str) -> String {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("port", &port.to_string())
            .append_pair("state", state)
            .append_pair("code_challenge", code_challenge)
            .append_pair("code_challenge_method", "S256")
            .finish();
        format!("{}{}?{}", self.base_url, AUTH_LOGIN_PATH, query)
    }

    /// Returns true when a refresh token is saved. A store failure counts as no session.
    pub fn has_session(&self) -> bool {
        matches!(self.store.load_refresh_token(), Ok(Some(_)))
    }

    /// Checks that the backend is up (`GET /healthz`).
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Unreachable`] on connection failure or timeout and
    /// [`BackendError::Server`] or [`BackendError::RateLimited`] on a non-success status.
    #[allow(dead_code)]
    pub fn health(&self) -> Result<(), BackendError> {
        let request = self.http.get(self.url(HEALTH_PATH));
        send(request)?;
        Ok(())
    }

    /// Exchanges the authorization code of the login for tokens and saves the refresh token.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Unreachable`] on connection failure or timeout,
    /// [`BackendError::Server`] (for example `invalid_code`) or [`BackendError::RateLimited`]
    /// when the backend refuses, [`BackendError::Decode`] on a malformed answer and
    /// [`BackendError::Store`] when the refresh token cannot be saved. When saving fails the
    /// access token is still kept in memory.
    pub fn exchange_code(&mut self, code: &str, code_verifier: &str) -> Result<(), BackendError> {
        let body = TokenRequest {
            code: code.to_string(),
            code_verifier: code_verifier.to_string(),
        };
        let response = self.post_json(AUTH_TOKEN_PATH, &body)?;
        let tokens: TokenResponse = decode(response)?;
        self.accept_tokens(tokens)?;
        Ok(())
    }

    /// Fetches the signed-in user.
    ///
    /// The access token is renewed first when missing or about to expire; on a 401
    /// `unauthorized` answer the token is renewed and the call retried once.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::NotLoggedIn`] when there is no refresh token or the session is
    /// gone (the store is cleared), [`BackendError::ReauthRequired`] when Google needs a new
    /// login (the store is cleared), [`BackendError::Unreachable`], [`BackendError::RateLimited`],
    /// [`BackendError::Server`], [`BackendError::Decode`] and [`BackendError::Store`] otherwise.
    pub fn fetch_me(&mut self) -> Result<Me, BackendError> {
        self.get_json(ME_PATH, &[])
    }

    /// Fetches the meetings between `from` and `to`.
    ///
    /// Token handling and errors are the same as [`BackendClient::fetch_me`]. The range is sent
    /// as given: an empty or inverted range is left to the backend to reject.
    ///
    /// # Errors
    ///
    /// See [`BackendClient::fetch_me`].
    #[allow(dead_code)]
    pub fn fetch_meetings(
        &mut self,
        from: DateTime<FixedOffset>,
        to: DateTime<FixedOffset>,
    ) -> Result<Vec<Meeting>, BackendError> {
        let from = from.to_rfc3339();
        let to = to.to_rfc3339();
        self.get_json(MEETINGS_PATH, &[("from", &from), ("to", &to)])
    }

    /// Revokes the session on the backend and forgets it locally.
    ///
    /// The backend call is best effort: any failure of it is ignored, and the local session
    /// is cleared anyway.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::Store`] when the refresh token cannot be removed from the store.
    pub fn logout(&mut self) -> Result<(), BackendError> {
        self.access = None;
        if let Ok(Some(refresh_token)) = self.store.load_refresh_token() {
            let body = RefreshRequest { refresh_token };
            if let Err(e) = self.post_json(AUTH_LOGOUT_PATH, &body) {
                warn!("Logout request failed: {e}");
            }
        }
        self.store.clear()?;
        Ok(())
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn post_json<B: Serialize>(&self, path: &str, body: &B) -> Result<Response, BackendError> {
        send(self.http.post(self.url(path)).json(body))
    }

    /// Keeps the access token in memory and saves the (possibly rotated) refresh token.
    fn accept_tokens(&mut self, tokens: TokenResponse) -> Result<(), SessionStoreError> {
        let now = Instant::now();
        let lifetime = Duration::from_secs(tokens.expires_in);
        self.access = Some(AccessToken {
            token: tokens.access_token,
            expires_at: now.checked_add(lifetime).unwrap_or(now),
        });
        self.store.save_refresh_token(&tokens.refresh_token)
    }

    /// Forgets the session in memory and in the store; a store failure is only logged.
    fn clear_session(&mut self) {
        self.access = None;
        if let Err(e) = self.store.clear() {
            warn!("Cannot clear the session store: {e}");
        }
    }

    /// Returns a valid access token, renewing it when missing or about to expire.
    fn ensure_access(&mut self) -> Result<String, BackendError> {
        if let Some(access) = &self.access {
            let left = access.expires_at.saturating_duration_since(Instant::now());
            if left > REFRESH_MARGIN {
                return Ok(access.token.clone());
            }
        }
        self.refresh()
    }

    /// Renews the access token with the stored refresh token, which rotates on every use.
    fn refresh(&mut self) -> Result<String, BackendError> {
        let Some(refresh_token) = self.store.load_refresh_token()? else {
            self.access = None;
            return Err(BackendError::NotLoggedIn);
        };
        let body = RefreshRequest { refresh_token };
        let response = match self.post_json(AUTH_REFRESH_PATH, &body) {
            Ok(response) => response,
            Err(e) => return Err(self.refresh_failure(e)),
        };
        let tokens: TokenResponse = decode(response)?;
        let access_token = tokens.access_token.clone();
        self.accept_tokens(tokens)?;
        Ok(access_token)
    }

    /// Maps a failed refresh: a dead session or a Google re-auth clears the store.
    fn refresh_failure(&mut self, error: BackendError) -> BackendError {
        if has_code(&error, error_codes::UNAUTHORIZED) {
            self.clear_session();
            return BackendError::NotLoggedIn;
        }
        self.api_failure(error)
    }

    /// Maps a failed API call: a Google re-auth request clears the store.
    fn api_failure(&mut self, error: BackendError) -> BackendError {
        if has_code(&error, error_codes::GOOGLE_REAUTH_REQUIRED) {
            self.clear_session();
            return BackendError::ReauthRequired;
        }
        error
    }

    fn get_once(
        &self,
        path: &str,
        query: &[(&str, &str)],
        token: &str,
    ) -> Result<Response, BackendError> {
        send(
            self.http
                .get(self.url(path))
                .query(query)
                .bearer_auth(token),
        )
    }

    /// Authenticated GET: renews the token when needed and retries once on 401 `unauthorized`.
    fn get_json<T: DeserializeOwned>(
        &mut self,
        path: &str,
        query: &[(&str, &str)],
    ) -> Result<T, BackendError> {
        let token = self.ensure_access()?;
        let first = self.get_once(path, query, &token);
        let result = if let Err(e) = &first
            && has_code(e, error_codes::UNAUTHORIZED)
        {
            self.access = None;
            let token = self.refresh()?;
            self.get_once(path, query, &token)
        } else {
            first
        };
        match result {
            Ok(response) => decode(response),
            Err(e) => Err(self.api_failure(e)),
        }
    }
}

/// True when `error` is a server error carrying the given API error code.
fn has_code(error: &BackendError, expected: &str) -> bool {
    matches!(error, BackendError::Server { code, .. } if code == expected)
}

/// Sends a request and turns connection failures and non-success statuses into errors.
fn send(request: reqwest::blocking::RequestBuilder) -> Result<Response, BackendError> {
    let response = request.send().map_err(|_| BackendError::Unreachable)?;
    if response.status().is_success() {
        return Ok(response);
    }
    Err(error_from(response))
}

/// Builds the error of a non-success response. A body that is not an `ApiError` gives an
/// empty code and message.
fn error_from(response: Response) -> BackendError {
    let status = response.status();
    if status == StatusCode::TOO_MANY_REQUESTS {
        return BackendError::RateLimited;
    }
    let (code, message) = match response.json::<ApiError>() {
        Ok(body) => (body.code, body.message),
        Err(_) => (String::new(), String::new()),
    };
    BackendError::Server {
        status: status.as_u16(),
        code,
        message,
    }
}

fn decode<T: DeserializeOwned>(response: Response) -> Result<T, BackendError> {
    response.json().map_err(|_| BackendError::Decode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session_store::MemoryStore;
    use schisaciaccole_core::api::Attendee;
    use serde_json::json;
    use wiremock::matchers::{body_json, header, method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn tokens(access: &str, refresh: &str, expires_in: u64) -> ResponseTemplate {
        ResponseTemplate::new(200).set_body_json(json!({
            "access_token": access,
            "refresh_token": refresh,
            "expires_in": expires_in,
        }))
    }

    fn api_error(status: u16, code: &str) -> ResponseTemplate {
        ResponseTemplate::new(status).set_body_json(json!({"code": code, "message": "m"}))
    }

    fn sample_meeting() -> Meeting {
        Meeting {
            event_id: "evt1".to_string(),
            title: "Daily".to_string(),
            start: DateTime::parse_from_rfc3339("2026-11-05T09:30:00+01:00").unwrap(),
            end: DateTime::parse_from_rfc3339("2026-11-05T10:00:00+01:00").unwrap(),
            attendees: Vec::<Attendee>::new(),
            attendees_omitted: false,
            recurring_event_id: None,
        }
    }

    fn range() -> (DateTime<FixedOffset>, DateTime<FixedOffset>) {
        let from = DateTime::parse_from_rfc3339("2026-11-05T00:00:00+01:00").unwrap();
        let to = DateTime::parse_from_rfc3339("2026-11-06T00:00:00+01:00").unwrap();
        (from, to)
    }

    fn store_with(token: &str) -> MemoryStore {
        let store = MemoryStore::new();
        store.save_refresh_token(token).unwrap();
        store
    }

    /// Runs `job` with a client on a blocking thread, as the blocking client cannot run
    /// inside the async test runtime.
    async fn run<T, F>(server: &MockServer, store: &MemoryStore, job: F) -> T
    where
        T: Send + 'static,
        F: FnOnce(BackendClient) -> T + Send + 'static,
    {
        let url = server.uri();
        let store = store.clone();
        tokio::task::spawn_blocking(move || {
            let client = BackendClient::new(&url, Box::new(store)).unwrap();
            job(client)
        })
        .await
        .unwrap()
    }

    fn offline_client() -> BackendClient {
        BackendClient::new("http://127.0.0.1:1", Box::new(MemoryStore::new())).unwrap()
    }

    #[test]
    fn login_url_encodes_parameters() {
        let client =
            BackendClient::new("https://api.example.com/", Box::new(MemoryStore::new())).unwrap();
        let url = client.login_url(5050, "a b&c", "x+y/z=");
        assert_eq!(
            url,
            "https://api.example.com/auth/login?port=5050&state=a+b%26c\
             &code_challenge=x%2By%2Fz%3D&code_challenge_method=S256"
        );
    }

    #[test]
    fn has_session_reflects_the_store() {
        let store = MemoryStore::new();
        let client = BackendClient::new("http://x", Box::new(store.clone())).unwrap();
        assert!(!client.has_session());
        store.save_refresh_token("r1").unwrap();
        assert!(client.has_session());
    }

    #[test]
    fn fetch_without_refresh_token_is_not_logged_in() {
        let mut client = offline_client();
        let (from, to) = range();
        let result = client.fetch_meetings(from, to);
        assert!(matches!(result, Err(BackendError::NotLoggedIn)));
    }

    #[test]
    fn unreachable_backend_is_reported() {
        let mut client = offline_client();
        let result = client.health();
        assert!(matches!(result, Err(BackendError::Unreachable)));
        client.store.save_refresh_token("r1").unwrap();
        assert!(matches!(client.fetch_me(), Err(BackendError::Unreachable)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn health_succeeds_on_200() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(200))
            .expect(1)
            .mount(&server)
            .await;
        let result = run(&server, &MemoryStore::new(), |client| client.health()).await;
        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn exchange_code_saves_the_refresh_token() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_TOKEN_PATH))
            .and(body_json(json!({"code": "c1", "code_verifier": "v1"})))
            .respond_with(tokens("a1", "r1", 900))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(ME_PATH))
            .and(header("authorization", "Bearer a1"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"email": "a@x.it", "display_name": null})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let store = MemoryStore::new();

        // The access token from the exchange is used directly: no refresh mock exists.
        let me = run(&server, &store, |mut client| {
            client.exchange_code("c1", "v1").unwrap();
            client.fetch_me()
        })
        .await
        .unwrap();

        assert_eq!(me.email, "a@x.it");
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("r1"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn exchange_code_reports_invalid_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_TOKEN_PATH))
            .respond_with(api_error(400, error_codes::INVALID_CODE))
            .mount(&server)
            .await;
        let store = MemoryStore::new();

        let result = run(&server, &store, |mut client| {
            client.exchange_code("c1", "v1")
        })
        .await;

        match result {
            Err(BackendError::Server { status, code, .. }) => {
                assert_eq!(status, 400);
                assert_eq!(code, error_codes::INVALID_CODE);
            }
            other => panic!("unexpected result: {other:?}"),
        }
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn fetch_meetings_refreshes_and_sends_bearer_and_range() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .and(body_json(json!({"refresh_token": "r1"})))
            .respond_with(tokens("a1", "r2", 900))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(MEETINGS_PATH))
            .and(header("authorization", "Bearer a1"))
            .and(query_param("from", "2026-11-05T00:00:00+01:00"))
            .and(query_param("to", "2026-11-06T00:00:00+01:00"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![sample_meeting()]))
            .expect(1)
            .mount(&server)
            .await;
        let store = store_with("r1");

        let meetings = run(&server, &store, |mut client| {
            let (from, to) = range();
            client.fetch_meetings(from, to)
        })
        .await
        .unwrap();

        assert_eq!(meetings, vec![sample_meeting()]);
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("r2"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn valid_access_token_is_reused() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(tokens("a1", "r2", 900))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(MEETINGS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(Vec::<Meeting>::new()))
            .expect(2)
            .mount(&server)
            .await;

        let result = run(&server, &store_with("r1"), |mut client| {
            let (from, to) = range();
            client.fetch_meetings(from, to)?;
            client.fetch_meetings(from, to)
        })
        .await;

        assert!(result.is_ok());
    }

    // An access token valid for less than the 30 s margin is renewed before every call.
    #[tokio::test(flavor = "multi_thread")]
    async fn expiring_access_token_is_renewed() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(tokens("a1", "r2", 20))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(MEETINGS_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(Vec::<Meeting>::new()))
            .expect(2)
            .mount(&server)
            .await;

        let result = run(&server, &store_with("r1"), |mut client| {
            let (from, to) = range();
            client.fetch_meetings(from, to)?;
            client.fetch_meetings(from, to)
        })
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn unauthorized_api_answer_refreshes_and_retries_once() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .and(body_json(json!({"refresh_token": "r1"})))
            .respond_with(tokens("a1", "r2", 900))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .and(body_json(json!({"refresh_token": "r2"})))
            .respond_with(tokens("a2", "r3", 900))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(MEETINGS_PATH))
            .and(header("authorization", "Bearer a1"))
            .respond_with(api_error(401, error_codes::UNAUTHORIZED))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(MEETINGS_PATH))
            .and(header("authorization", "Bearer a2"))
            .respond_with(ResponseTemplate::new(200).set_body_json(vec![sample_meeting()]))
            .expect(1)
            .mount(&server)
            .await;
        let store = store_with("r1");

        let meetings = run(&server, &store, |mut client| {
            let (from, to) = range();
            client.fetch_meetings(from, to)
        })
        .await
        .unwrap();

        assert_eq!(meetings.len(), 1);
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("r3"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn second_unauthorized_answer_is_not_retried_again() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(tokens("a1", "r2", 900))
            .expect(2)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(MEETINGS_PATH))
            .respond_with(api_error(401, error_codes::UNAUTHORIZED))
            .expect(2)
            .mount(&server)
            .await;

        let result = run(&server, &store_with("r1"), |mut client| {
            let (from, to) = range();
            client.fetch_meetings(from, to)
        })
        .await;

        assert!(matches!(
            result,
            Err(BackendError::Server { status: 401, .. })
        ));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn refresh_unauthorized_is_not_logged_in_and_clears_the_store() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(api_error(401, error_codes::UNAUTHORIZED))
            .mount(&server)
            .await;
        let store = store_with("r1");

        let result = run(&server, &store, |mut client| {
            let (from, to) = range();
            client.fetch_meetings(from, to)
        })
        .await;

        assert!(matches!(result, Err(BackendError::NotLoggedIn)));
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn google_reauth_on_refresh_clears_the_store() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(api_error(401, error_codes::GOOGLE_REAUTH_REQUIRED))
            .mount(&server)
            .await;
        let store = store_with("r1");

        let result = run(&server, &store, |mut client| client.fetch_me()).await;

        assert!(matches!(result, Err(BackendError::ReauthRequired)));
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn google_reauth_on_api_call_clears_the_store() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(tokens("a1", "r2", 900))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(ME_PATH))
            .respond_with(api_error(401, error_codes::GOOGLE_REAUTH_REQUIRED))
            .expect(1)
            .mount(&server)
            .await;
        let store = store_with("r1");

        let result = run(&server, &store, |mut client| client.fetch_me()).await;

        assert!(matches!(result, Err(BackendError::ReauthRequired)));
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn too_many_requests_is_rate_limited_and_keeps_the_session() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(ResponseTemplate::new(429))
            .mount(&server)
            .await;
        let store = store_with("r1");

        let result = run(&server, &store, |mut client| client.fetch_me()).await;

        assert!(matches!(result, Err(BackendError::RateLimited)));
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("r1"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn server_error_without_api_error_body_has_empty_code() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(HEALTH_PATH))
            .respond_with(ResponseTemplate::new(503).set_body_string("down"))
            .mount(&server)
            .await;

        let result = run(&server, &MemoryStore::new(), |client| client.health()).await;

        match result {
            Err(BackendError::Server { status, code, .. }) => {
                assert_eq!(status, 503);
                assert!(code.is_empty());
            }
            other => panic!("unexpected result: {other:?}"),
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn malformed_success_body_is_a_decode_error() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_REFRESH_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
            .mount(&server)
            .await;

        let result = run(&server, &store_with("r1"), |mut client| client.fetch_me()).await;

        assert!(matches!(result, Err(BackendError::Decode)));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn logout_revokes_and_clears_the_store() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_LOGOUT_PATH))
            .and(body_json(json!({"refresh_token": "r1"})))
            .respond_with(ResponseTemplate::new(204))
            .expect(1)
            .mount(&server)
            .await;
        let store = store_with("r1");

        let result = run(&server, &store, |mut client| client.logout()).await;

        assert!(result.is_ok());
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[test]
    fn logout_clears_the_store_even_if_the_backend_is_unreachable() {
        let mut client = offline_client();
        client.store.save_refresh_token("r1").unwrap();

        let result = client.logout();

        assert!(result.is_ok());
        assert!(!client.has_session());
    }
}
