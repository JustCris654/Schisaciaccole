//! Desktop side of the Google login: the loopback listener that receives the backend redirect,
//! the parsing of the callback, the pages shown in the browser and the messages shown in the app.

use std::error::Error;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use log::warn;
use schisaciaccole_core::api::error_codes;
use tiny_http::{Header, Method, Request, Response, Server};

use crate::backend::BackendError;
use crate::pkce::PkceError;

/// Path on the loopback listener the backend redirects the browser to.
const CALLBACK_PATH: &str = "/callback";
/// How often the wait for the callback checks the cancel flag.
const POLL_INTERVAL: Duration = Duration::from_millis(500);

/// Shown in the browser when the callback carries a valid code.
pub const SUCCESS_MESSAGE: &str = "Accesso completato. Puoi tornare a Schisaciaccole.";
/// The account is outside the allowed domains.
pub const FORBIDDEN_DOMAIN_MESSAGE: &str = "Usa il tuo account aziendale.";
/// Google needs the user to grant the calendar access again.
pub const REAUTH_MESSAGE: &str = "Concedi l'accesso al calendario e riprova.";
/// The backend answered 429.
pub const RATE_LIMITED_MESSAGE: &str = "Troppi tentativi, riprova tra poco.";
/// The backend cannot be reached.
pub const UNREACHABLE_MESSAGE: &str = "Server non raggiungibile.";
/// Any other failure.
pub const FAILED_MESSAGE: &str = "Accesso non riuscito, riprova.";

/// Failure of a login attempt.
#[derive(Debug, thiserror::Error)]
pub enum LoginError {
    /// The PKCE verifier or the state could not be generated.
    #[error("cannot generate the login secrets: {0}")]
    Random(#[from] PkceError),
    /// The loopback listener could not be started.
    #[error("cannot start the loopback listener: {0}")]
    Listener(Box<dyn Error + Send + Sync>),
    /// The loopback listener failed while waiting.
    #[error("cannot receive the login callback: {0}")]
    Receive(std::io::Error),
    /// The browser could not be opened.
    #[error("cannot open the browser: {0}")]
    Browser(std::io::Error),
    /// No callback arrived in time.
    #[error("login timed out")]
    TimedOut,
    /// The user cancelled the login.
    #[error("login cancelled")]
    Cancelled,
    /// The callback carries a state different from the one sent.
    #[error("login callback with an unexpected state")]
    StateMismatch,
    /// The callback has neither a code nor an error, or no state.
    #[error("malformed login callback")]
    InvalidCallback,
    /// The backend redirected with an error code.
    #[error("login refused by the backend: {0}")]
    Refused(String),
    /// The code exchange or the profile fetch failed.
    #[error(transparent)]
    Backend(#[from] BackendError),
}

/// Query of a request to the callback path.
#[derive(Debug, PartialEq, Eq)]
pub enum Callback {
    /// The login succeeded.
    Code {
        /// One-time code to exchange for tokens.
        code: String,
        /// State echoed by the backend.
        state: String,
    },
    /// The login failed.
    Error {
        /// Backend error code (`forbidden_domain`, ...).
        code: String,
        /// State echoed by the backend.
        state: String,
    },
    /// Neither a non-empty code nor an error, or the state is missing.
    Invalid,
}

/// Parses the query string (without `?`) of a callback request.
///
/// An `error` parameter wins over `code` when both are present. Parameters are URL-decoded;
/// unknown ones are ignored and a repeated one keeps its last value.
pub fn parse_callback(query: &str) -> Callback {
    let mut code = None;
    let mut error = None;
    let mut state = None;
    for (key, value) in url::form_urlencoded::parse(query.as_bytes()) {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            _ => {}
        }
    }

    let Some(state) = state else {
        return Callback::Invalid;
    };
    match (error, code) {
        (Some(code), _) => Callback::Error { code, state },
        (None, Some(code)) if !code.is_empty() => Callback::Code { code, state },
        _ => Callback::Invalid,
    }
}

/// Checks a parsed callback against the state sent with the login and returns its code.
///
/// # Errors
///
/// Returns [`LoginError::StateMismatch`] when the state differs (also for an error callback),
/// [`LoginError::Refused`] for an error callback and [`LoginError::InvalidCallback`] for
/// [`Callback::Invalid`].
pub fn check_callback(callback: Callback, expected_state: &str) -> Result<String, LoginError> {
    match callback {
        Callback::Code { code, state } if state == expected_state => Ok(code),
        Callback::Error { code, state } if state == expected_state => {
            Err(LoginError::Refused(code))
        }
        Callback::Code { .. } | Callback::Error { .. } => Err(LoginError::StateMismatch),
        Callback::Invalid => Err(LoginError::InvalidCallback),
    }
}

/// Builds the UTF-8 HTML page shown in the browser at the end of the login.
///
/// `message` is inserted as is, without HTML escaping: only the static messages of this
/// module are meant to be passed.
pub fn callback_page(message: &'static str) -> String {
    format!(
        "<!DOCTYPE html>\n\
         <html lang=\"it\">\n\
         <head><meta charset=\"utf-8\"><title>Schisaciaccole</title></head>\n\
         <body style=\"font-family: sans-serif; text-align: center; margin-top: 4em;\">\
         <p>{message}</p></body>\n\
         </html>\n"
    )
}

/// Message for the user about a failed login.
///
/// [`LoginError::Cancelled`] and the local failures (listener, browser, timeout, malformed
/// callback) all give [`FAILED_MESSAGE`].
pub fn login_error_message(error: &LoginError) -> &'static str {
    match error {
        LoginError::Refused(code) => error_code_message(code),
        LoginError::Backend(error) => backend_error_message(error),
        _ => FAILED_MESSAGE,
    }
}

/// Message for the user about a failed backend call.
///
/// A server error is mapped through its API error code; anything not listed in
/// [`error_code_message`] gives [`FAILED_MESSAGE`].
pub fn backend_error_message(error: &BackendError) -> &'static str {
    match error {
        BackendError::Unreachable => UNREACHABLE_MESSAGE,
        BackendError::RateLimited => RATE_LIMITED_MESSAGE,
        BackendError::ReauthRequired => REAUTH_MESSAGE,
        BackendError::Server { code, .. } => error_code_message(code),
        _ => FAILED_MESSAGE,
    }
}

/// Message for the user about a backend error code (`forbidden_domain`, ...).
///
/// Unknown codes, including `invalid_code` and `internal`, give [`FAILED_MESSAGE`].
pub fn error_code_message(code: &str) -> &'static str {
    match code {
        error_codes::FORBIDDEN_DOMAIN => FORBIDDEN_DOMAIN_MESSAGE,
        error_codes::GOOGLE_REAUTH_REQUIRED => REAUTH_MESSAGE,
        _ => FAILED_MESSAGE,
    }
}

/// HTTP listener on `127.0.0.1` waiting for the redirect at the end of the browser login.
pub struct LoopbackServer {
    server: Server,
    port: u16,
}

impl LoopbackServer {
    /// Starts listening on `127.0.0.1` on a port chosen by the OS.
    ///
    /// # Errors
    ///
    /// Returns [`LoginError::Listener`] when the socket cannot be bound or its port read.
    pub fn bind() -> Result<Self, LoginError> {
        let server = Server::http("127.0.0.1:0").map_err(LoginError::Listener)?;
        let Some(address) = server.server_addr().to_ip() else {
            return Err(LoginError::Listener(
                "the listener has no IP address".into(),
            ));
        };
        Ok(Self {
            port: address.port(),
            server,
        })
    }

    /// Port the listener is bound to.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Waits for a `GET /callback` request and returns the code it carries.
    ///
    /// Other requests (e.g. `/favicon.ico`) get a 404 and the wait goes on. The first
    /// callback request ends the wait: the browser gets the success page or the error message,
    /// and its result is returned. `cancel` is checked about every 500 ms.
    ///
    /// # Errors
    ///
    /// Returns [`LoginError::Cancelled`] once `cancel` is set, [`LoginError::TimedOut`] after
    /// `timeout`, [`LoginError::Receive`] when the listener fails, and the errors of
    /// [`check_callback`] for a callback that is not a valid success. A failure to answer the
    /// browser is only logged.
    pub fn wait_for_callback(
        &self,
        expected_state: &str,
        cancel: &AtomicBool,
        timeout: Duration,
    ) -> Result<String, LoginError> {
        let start = Instant::now();
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(LoginError::Cancelled);
            }
            let left = timeout.saturating_sub(start.elapsed());
            if left.is_zero() {
                return Err(LoginError::TimedOut);
            }

            let received = self
                .server
                .recv_timeout(left.min(POLL_INTERVAL))
                .map_err(LoginError::Receive)?;
            let Some(request) = received else {
                continue;
            };

            let url = request.url().to_string();
            let (path, query) = url.split_once('?').unwrap_or((url.as_str(), ""));
            if *request.method() != Method::Get || path != CALLBACK_PATH {
                respond(request, Response::empty(404));
                continue;
            }

            let result = check_callback(parse_callback(query), expected_state);
            let message = match &result {
                Ok(_) => SUCCESS_MESSAGE,
                Err(error) => login_error_message(error),
            };
            respond(request, html_response(callback_page(message)));
            return result;
        }
    }
}

/// Wraps an HTML page in a 200 response with a UTF-8 HTML content type.
fn html_response(html: String) -> Response<std::io::Cursor<Vec<u8>>> {
    let response = Response::from_string(html);
    match Header::from_bytes("Content-Type", "text/html; charset=utf-8") {
        Ok(header) => response.with_header(header),
        Err(()) => response,
    }
}

/// Sends `response` to the browser; a failure is only logged.
fn respond<R: std::io::Read>(request: Request, response: Response<R>) {
    if let Err(e) = request.respond(response) {
        warn!("Cannot answer the browser: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::thread;

    fn backend_server_error(code: &str) -> BackendError {
        BackendError::Server {
            status: 400,
            code: code.to_string(),
            message: String::new(),
        }
    }

    #[test]
    fn parses_a_code_callback() {
        let callback = parse_callback("code=abc&state=s1");
        assert_eq!(
            callback,
            Callback::Code {
                code: "abc".to_string(),
                state: "s1".to_string()
            }
        );
    }

    #[test]
    fn parses_an_error_callback() {
        let callback = parse_callback("error=forbidden_domain&state=s1");
        assert_eq!(
            callback,
            Callback::Error {
                code: "forbidden_domain".to_string(),
                state: "s1".to_string()
            }
        );
    }

    #[test]
    fn error_wins_over_code() {
        let callback = parse_callback("code=abc&error=internal&state=s1");
        assert!(matches!(callback, Callback::Error { code, .. } if code == "internal"));
    }

    #[test]
    fn decodes_url_encoded_values() {
        let callback = parse_callback("code=a%2Bb&state=x%20y");
        assert_eq!(
            callback,
            Callback::Code {
                code: "a+b".to_string(),
                state: "x y".to_string()
            }
        );
    }

    #[test]
    fn callback_without_state_is_invalid() {
        assert_eq!(parse_callback("code=abc"), Callback::Invalid);
    }

    #[test]
    fn callback_with_empty_code_is_invalid() {
        assert_eq!(parse_callback("code=&state=s1"), Callback::Invalid);
    }

    #[test]
    fn empty_query_is_invalid() {
        assert_eq!(parse_callback(""), Callback::Invalid);
    }

    #[test]
    fn check_returns_the_code_on_matching_state() {
        let callback = parse_callback("code=abc&state=s1");
        assert_eq!(check_callback(callback, "s1").unwrap(), "abc");
    }

    #[test]
    fn check_rejects_a_wrong_state() {
        let callback = parse_callback("code=abc&state=other");
        let result = check_callback(callback, "s1");
        assert!(matches!(result, Err(LoginError::StateMismatch)));
    }

    #[test]
    fn check_rejects_an_error_with_a_wrong_state() {
        let callback = parse_callback("error=internal&state=other");
        let result = check_callback(callback, "s1");
        assert!(matches!(result, Err(LoginError::StateMismatch)));
    }

    #[test]
    fn check_reports_the_backend_error_code() {
        let callback = parse_callback("error=forbidden_domain&state=s1");
        let result = check_callback(callback, "s1");
        assert!(matches!(result, Err(LoginError::Refused(code)) if code == "forbidden_domain"));
    }

    #[test]
    fn check_rejects_an_invalid_callback() {
        let result = check_callback(Callback::Invalid, "s1");
        assert!(matches!(result, Err(LoginError::InvalidCallback)));
    }

    #[test]
    fn page_is_utf8_html_with_the_message() {
        let page = callback_page(SUCCESS_MESSAGE);
        assert!(page.starts_with("<!DOCTYPE html>"));
        assert!(page.contains("<meta charset=\"utf-8\">"));
        assert!(page.contains(SUCCESS_MESSAGE));
    }

    #[test]
    fn maps_error_codes_to_messages() {
        assert_eq!(
            error_code_message("forbidden_domain"),
            FORBIDDEN_DOMAIN_MESSAGE
        );
        assert_eq!(error_code_message("google_reauth_required"), REAUTH_MESSAGE);
        assert_eq!(error_code_message("invalid_code"), FAILED_MESSAGE);
        assert_eq!(error_code_message("internal"), FAILED_MESSAGE);
        assert_eq!(error_code_message("something_new"), FAILED_MESSAGE);
    }

    #[test]
    fn maps_backend_errors_to_messages() {
        assert_eq!(
            backend_error_message(&BackendError::Unreachable),
            UNREACHABLE_MESSAGE
        );
        assert_eq!(
            backend_error_message(&BackendError::RateLimited),
            RATE_LIMITED_MESSAGE
        );
        assert_eq!(
            backend_error_message(&BackendError::ReauthRequired),
            REAUTH_MESSAGE
        );
        assert_eq!(
            backend_error_message(&backend_server_error("forbidden_domain")),
            FORBIDDEN_DOMAIN_MESSAGE
        );
        assert_eq!(backend_error_message(&BackendError::Decode), FAILED_MESSAGE);
    }

    #[test]
    fn maps_login_errors_to_messages() {
        let refused = LoginError::Refused("forbidden_domain".to_string());
        assert_eq!(login_error_message(&refused), FORBIDDEN_DOMAIN_MESSAGE);
        let backend = LoginError::Backend(BackendError::RateLimited);
        assert_eq!(login_error_message(&backend), RATE_LIMITED_MESSAGE);
        assert_eq!(login_error_message(&LoginError::TimedOut), FAILED_MESSAGE);
        assert_eq!(
            login_error_message(&LoginError::StateMismatch),
            FAILED_MESSAGE
        );
    }

    #[test]
    fn binds_a_loopback_port() {
        let server = LoopbackServer::bind().unwrap();
        assert_ne!(server.port(), 0);
    }

    // A request to another path gets a 404 and does not end the wait.
    #[test]
    fn serves_the_callback_after_ignoring_other_paths() {
        let server = LoopbackServer::bind().unwrap();
        let base = format!("http://127.0.0.1:{}", server.port());
        let waiter = thread::spawn(move || {
            server.wait_for_callback("s", &AtomicBool::new(false), Duration::from_secs(10))
        });
        let http = reqwest::blocking::Client::new();

        let favicon = http.get(format!("{base}/favicon.ico")).send().unwrap();
        let callback = http
            .get(format!("{base}/callback?code=x&state=s"))
            .send()
            .unwrap();

        assert_eq!(favicon.status(), 404);
        assert_eq!(callback.status(), 200);
        let content_type = callback.headers()["content-type"].to_str().unwrap();
        assert_eq!(content_type, "text/html; charset=utf-8");
        assert!(callback.text().unwrap().contains(SUCCESS_MESSAGE));
        assert_eq!(waiter.join().unwrap().unwrap(), "x");
    }

    #[test]
    fn answers_an_error_callback_with_the_message() {
        let server = LoopbackServer::bind().unwrap();
        let url = format!(
            "http://127.0.0.1:{}/callback?error=forbidden_domain&state=s",
            server.port()
        );
        let waiter = thread::spawn(move || {
            server.wait_for_callback("s", &AtomicBool::new(false), Duration::from_secs(10))
        });

        let page = reqwest::blocking::get(url).unwrap().text().unwrap();

        assert!(page.contains(FORBIDDEN_DOMAIN_MESSAGE));
        let result = waiter.join().unwrap();
        assert!(matches!(result, Err(LoginError::Refused(code)) if code == "forbidden_domain"));
    }

    #[test]
    fn wait_stops_when_cancelled() {
        let server = LoopbackServer::bind().unwrap();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let waiter =
            thread::spawn(move || server.wait_for_callback("s", &flag, Duration::from_secs(10)));

        cancel.store(true, Ordering::Relaxed);

        let result = waiter.join().unwrap();
        assert!(matches!(result, Err(LoginError::Cancelled)));
    }

    #[test]
    fn wait_times_out() {
        let server = LoopbackServer::bind().unwrap();
        let result =
            server.wait_for_callback("s", &AtomicBool::new(false), Duration::from_millis(50));
        assert!(matches!(result, Err(LoginError::TimedOut)));
    }
}
