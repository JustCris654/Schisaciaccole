//! Backend session of the app: startup check, login and logout, run on worker threads.
//!
//! Threading model: the UI state (`Rc<RefCell<AppState>>`) lives only on the UI thread. Workers
//! share the backend client through an `Arc<Mutex<BackendClient>>`, locked only inside worker
//! threads, so the UI thread never waits on the network. A worker reports its result as an
//! [`AppEvent`] with [`post`], which queues it with `slint::invoke_from_event_loop`; on the UI
//! thread the event goes to the handler installed by [`set_event_handler`], kept in a
//! `thread_local!`.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

use log::{debug, info, warn};
use schisaciaccole_core::api::Me;

use crate::backend::{BackendClient, BackendError};
use crate::login::{self, LoginError, LoopbackServer};
use crate::pkce;
use crate::session_store::{self, KeyringStore, MemoryStore, SessionStore};
use crate::state::{AppState, OFFLINE_NOTICE};

/// How long the login waits for the browser to come back.
const LOGIN_TIMEOUT: Duration = Duration::from_secs(5 * 60);

/// Result of a worker, delivered to the UI thread.
#[derive(Debug)]
pub enum AppEvent {
    /// The saved session was checked at startup.
    StartupChecked(StartupOutcome),
    /// The login attempt `attempt` ended.
    LoginFinished {
        /// Id given by [`Session::start_login`].
        attempt: u64,
        /// Signed-in user, or why the login failed.
        result: Result<Me, LoginError>,
    },
    /// The session was revoked and forgotten.
    LoggedOut,
}

/// What the startup check found.
#[derive(Debug, PartialEq, Eq)]
pub enum StartupOutcome {
    /// The saved session is valid.
    SignedIn(Me),
    /// The user has to sign in; the message to show is empty when there is nothing to explain.
    LoginRequired(&'static str),
    /// The backend could not answer: the app runs in local mode.
    Offline,
}

/// Maps the result of the startup profile fetch to what the UI must show.
///
/// A missing or dead session asks for a login; a Google re-auth asks for it with an
/// explanation; any other failure (unreachable, rate limited, server, store) is offline.
pub fn startup_outcome(result: Result<Me, BackendError>) -> StartupOutcome {
    match result {
        Ok(me) => StartupOutcome::SignedIn(me),
        Err(BackendError::NotLoggedIn) => StartupOutcome::LoginRequired(""),
        Err(BackendError::ReauthRequired) => StartupOutcome::LoginRequired(login::REAUTH_MESSAGE),
        Err(e) => {
            warn!("Startup session check failed: {e}");
            StartupOutcome::Offline
        }
    }
}

/// UI-thread callback receiving the worker events.
type EventHandler = Box<dyn Fn(AppEvent)>;

thread_local! {
    static EVENT_HANDLER: RefCell<Option<EventHandler>> = const { RefCell::new(None) };
}

/// Installs the UI-thread handler of the events posted by the workers, replacing any previous one.
///
/// Call it on the UI thread: events are delivered on that thread only.
pub fn set_event_handler(handler: impl Fn(AppEvent) + 'static) {
    EVENT_HANDLER.with(|slot| *slot.borrow_mut() = Some(Box::new(handler)));
}

/// Removes the event handler, dropping what it captured. Later events are ignored.
pub fn clear_event_handler() {
    EVENT_HANDLER.with(|slot| *slot.borrow_mut() = None);
}

/// Sends `event` from any thread to the UI-thread handler.
///
/// When the event loop is no longer running the event is dropped and logged.
fn post(event: AppEvent) {
    let delivered = slint::invoke_from_event_loop(move || {
        EVENT_HANDLER.with(|slot| match slot.borrow().as_ref() {
            Some(handler) => handler(event),
            None => debug!("No handler for {event:?}"),
        });
    });
    if let Err(e) = delivered {
        debug!("Event loop gone, dropping a session event: {e}");
    }
}

/// Login attempt waiting for the browser.
struct ActiveLogin {
    attempt: u64,
    cancel: Arc<AtomicBool>,
}

/// UI-thread handle of the backend session. Not `Send`: it stays on the UI thread and hands the
/// shared client to the workers it spawns.
pub struct Session {
    client: Arc<Mutex<BackendClient>>,
    login: RefCell<Option<ActiveLogin>>,
    next_attempt: Cell<u64>,
}

impl Session {
    /// Opens the OS keychain and creates the backend client for `base_url`.
    ///
    /// When the keychain cannot be opened a warning is logged and the session is kept in
    /// memory only, so it is lost when the app exits. No network call is made.
    ///
    /// # Errors
    ///
    /// Returns [`BackendError::ClientBuild`] when the HTTP client cannot be created.
    pub fn connect(base_url: &str) -> Result<Self, BackendError> {
        let store: Box<dyn SessionStore + Send> = match session_store::init_keyring() {
            Ok(()) => Box::new(KeyringStore),
            Err(e) => {
                warn!("Keychain unavailable, the session will not be saved: {e}");
                Box::new(MemoryStore::new())
            }
        };
        let client = BackendClient::new(base_url, store)?;
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            login: RefCell::new(None),
            next_attempt: Cell::new(0),
        })
    }

    /// Checks the saved session on a worker and posts [`AppEvent::StartupChecked`].
    ///
    /// When the worker cannot be started the outcome is posted as offline right away.
    pub fn spawn_startup_check(&self) {
        let client = self.client.clone();
        let spawned = spawn("session-check", move || {
            let outcome = check_saved_session(&client);
            post(AppEvent::StartupChecked(outcome));
        });
        if let Err(e) = spawned {
            warn!("Cannot start the session check: {e}");
            post(AppEvent::StartupChecked(StartupOutcome::Offline));
        }
    }

    /// Starts a browser login on a worker; it ends with [`AppEvent::LoginFinished`].
    ///
    /// Returns `Ok(false)` without doing anything when a login is already waiting.
    ///
    /// # Errors
    ///
    /// Returns the I/O error when the worker thread cannot be started.
    pub fn start_login(&self) -> std::io::Result<bool> {
        if self.login.borrow().is_some() {
            return Ok(false);
        }

        let attempt = self.next_attempt.get().wrapping_add(1);
        self.next_attempt.set(attempt);
        let cancel = Arc::new(AtomicBool::new(false));

        let client = self.client.clone();
        let worker_cancel = cancel.clone();
        spawn("login", move || {
            let result = run_login(&client, &worker_cancel, open_system_browser);
            match &result {
                Ok(_) => info!("Login completed"),
                Err(e) => warn!("Login failed: {e}"),
            }
            post(AppEvent::LoginFinished { attempt, result });
        })?;

        *self.login.borrow_mut() = Some(ActiveLogin { attempt, cancel });
        Ok(true)
    }

    /// Cancels the waiting login, if any; its result will be ignored.
    pub fn cancel_login(&self) {
        if let Some(login) = self.login.borrow_mut().take() {
            login.cancel.store(true, Ordering::Relaxed);
        }
    }

    /// Marks login `attempt` as ended. Returns `false` when it is not the waiting one
    /// (cancelled or superseded), so its result must be ignored.
    pub fn finish_login(&self, attempt: u64) -> bool {
        let mut login = self.login.borrow_mut();
        match login.as_ref() {
            Some(active) if active.attempt == attempt => {
                *login = None;
                true
            }
            _ => false,
        }
    }

    /// Revokes the session on a worker and posts [`AppEvent::LoggedOut`].
    ///
    /// A failure to forget the local session is logged and the event is posted anyway. When the
    /// worker cannot be started nothing is revoked and the event is posted right away.
    pub fn spawn_logout(&self) {
        let client = self.client.clone();
        let spawned = spawn("logout", move || {
            if let Err(e) = lock(&client).logout() {
                warn!("Logout failed: {e}");
            }
            post(AppEvent::LoggedOut);
        });
        if let Err(e) = spawned {
            warn!("Cannot start the logout: {e}");
            post(AppEvent::LoggedOut);
        }
    }
}

/// Applies a worker event to the UI state. Results of stale login attempts are dropped.
pub fn apply_event(state: &mut AppState, session: &Session, event: AppEvent) {
    match event {
        AppEvent::StartupChecked(StartupOutcome::SignedIn(me)) => state.on_signed_in(&me),
        AppEvent::StartupChecked(StartupOutcome::LoginRequired(message)) => {
            state.on_login_required(message);
        }
        AppEvent::StartupChecked(StartupOutcome::Offline) => {
            state.on_local_mode(OFFLINE_NOTICE);
        }
        AppEvent::LoginFinished { attempt, result } => {
            if !session.finish_login(attempt) {
                debug!("Ignoring the result of login attempt {attempt}");
                return;
            }
            match result {
                Ok(me) => state.on_signed_in(&me),
                Err(e) => state.on_login_failed(login::login_error_message(&e)),
            }
        }
        AppEvent::LoggedOut => state.on_logged_out(),
    }
}

/// Runs the startup check: no saved session asks for a login, otherwise the profile is fetched.
fn check_saved_session(client: &Mutex<BackendClient>) -> StartupOutcome {
    let mut client = lock(client);
    if !client.has_session() {
        info!("No saved session");
        return StartupOutcome::LoginRequired("");
    }
    startup_outcome(client.fetch_me())
}

/// Runs a whole browser login: PKCE secrets, loopback listener, browser, wait for the callback,
/// code exchange and profile fetch. The client is locked only for the backend calls.
///
/// `open_browser` receives the login URL; it is injectable so tests can play the browser.
///
/// # Errors
///
/// Returns [`LoginError::Random`] or [`LoginError::Listener`] when the login cannot start,
/// [`LoginError::Browser`] when `open_browser` fails, the errors of
/// [`LoopbackServer::wait_for_callback`] and [`LoginError::Backend`] when the exchange or the
/// profile fetch fails.
fn run_login(
    client: &Mutex<BackendClient>,
    cancel: &AtomicBool,
    open_browser: impl FnOnce(&str) -> std::io::Result<()>,
) -> Result<Me, LoginError> {
    let verifier = pkce::generate_verifier()?;
    let state = pkce::generate_state()?;
    let challenge = pkce::challenge_for(&verifier);

    let server = LoopbackServer::bind()?;
    let url = lock(client).login_url(server.port(), &state, &challenge);
    open_browser(&url).map_err(LoginError::Browser)?;
    let code = server.wait_for_callback(&state, cancel, LOGIN_TIMEOUT)?;
    drop(server);

    let mut client = lock(client);
    client.exchange_code(&code, &verifier)?;
    Ok(client.fetch_me()?)
}

/// Opens `url` in the default browser.
///
/// # Errors
///
/// Returns the I/O error of the `webbrowser` crate when no browser can be started.
fn open_system_browser(url: &str) -> std::io::Result<()> {
    webbrowser::open(url)
}

/// Locks the shared client. A poisoned lock is recovered, never an error.
fn lock(client: &Mutex<BackendClient>) -> MutexGuard<'_, BackendClient> {
    client.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Starts a named worker thread running `job`.
///
/// # Errors
///
/// Returns the I/O error when the OS cannot create the thread.
fn spawn(name: &str, job: impl FnOnce() + Send + 'static) -> std::io::Result<()> {
    thread::Builder::new().name(name.to_string()).spawn(job)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::tests::sample_state;
    use crate::ui::Page;
    use schisaciaccole_core::api::{AUTH_TOKEN_PATH, ME_PATH};
    use serde_json::json;
    use url::Url;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn me() -> Me {
        Me {
            email: "a@x.it".to_string(),
            display_name: None,
        }
    }

    fn offline_session() -> Session {
        let client =
            BackendClient::new("http://127.0.0.1:1", Box::new(MemoryStore::new())).unwrap();
        Session {
            client: Arc::new(Mutex::new(client)),
            login: RefCell::new(None),
            next_attempt: Cell::new(0),
        }
    }

    fn active_session(attempt: u64) -> Session {
        let session = offline_session();
        *session.login.borrow_mut() = Some(ActiveLogin {
            attempt,
            cancel: Arc::new(AtomicBool::new(false)),
        });
        session
    }

    #[test]
    fn startup_with_valid_session_is_signed_in() {
        assert_eq!(startup_outcome(Ok(me())), StartupOutcome::SignedIn(me()));
    }

    #[test]
    fn startup_without_session_requires_login() {
        let outcome = startup_outcome(Err(BackendError::NotLoggedIn));
        assert_eq!(outcome, StartupOutcome::LoginRequired(""));
    }

    #[test]
    fn startup_reauth_requires_login_with_message() {
        let outcome = startup_outcome(Err(BackendError::ReauthRequired));
        assert_eq!(
            outcome,
            StartupOutcome::LoginRequired(login::REAUTH_MESSAGE)
        );
    }

    #[test]
    fn startup_unreachable_is_offline() {
        let outcome = startup_outcome(Err(BackendError::Unreachable));
        assert_eq!(outcome, StartupOutcome::Offline);
    }

    #[test]
    fn startup_without_saved_token_requires_login_without_network() {
        let session = offline_session();
        let outcome = check_saved_session(&session.client);
        assert_eq!(outcome, StartupOutcome::LoginRequired(""));
    }

    #[test]
    fn finish_accepts_only_the_waiting_attempt() {
        let session = active_session(2);
        assert!(!session.finish_login(1));
        assert!(session.finish_login(2));
        assert!(!session.finish_login(2));
    }

    #[test]
    fn cancel_sets_the_flag_and_forgets_the_attempt() {
        let session = active_session(1);
        let cancel = session.login.borrow().as_ref().unwrap().cancel.clone();

        session.cancel_login();

        assert!(cancel.load(Ordering::Relaxed));
        assert!(!session.finish_login(1));
    }

    #[test]
    fn stale_login_result_is_ignored() {
        let session = offline_session();
        let mut state = sample_state();
        state.on_login_required("");
        state.on_login_started();

        let event = AppEvent::LoginFinished {
            attempt: 7,
            result: Ok(me()),
        };
        apply_event(&mut state, &session, event);

        assert_eq!(state.page(), Page::LoginPage);
        assert!(state.is_login_waiting());
    }

    #[test]
    fn current_login_result_signs_in() {
        let session = active_session(3);
        let mut state = sample_state();
        state.on_login_required("");
        state.on_login_started();

        let event = AppEvent::LoginFinished {
            attempt: 3,
            result: Ok(me()),
        };
        apply_event(&mut state, &session, event);

        assert_eq!(state.page(), Page::SelectionPage);
        assert_eq!(state.account(), Some("a@x.it"));
    }

    #[test]
    fn browser_failure_ends_the_login() {
        let session = offline_session();
        let result = run_login(&session.client, &AtomicBool::new(false), |_| {
            Err(std::io::Error::other("no browser"))
        });
        assert!(matches!(result, Err(LoginError::Browser(_))));
    }

    /// Plays the browser: reads port and state from the login URL and hits the callback.
    fn fake_browser(code: &'static str) -> impl FnOnce(&str) -> std::io::Result<()> {
        move |login_url: &str| {
            let url = Url::parse(login_url).unwrap();
            let param = |name: &str| {
                url.query_pairs()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
                    .unwrap()
            };
            let callback = format!(
                "http://127.0.0.1:{}/callback?code={code}&state={}",
                param("port"),
                param("state")
            );
            thread::spawn(move || reqwest::blocking::get(callback).unwrap());
            Ok(())
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn login_exchanges_the_callback_code() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path(AUTH_TOKEN_PATH))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "a1",
                "refresh_token": "r1",
                "expires_in": 900,
            })))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path(ME_PATH))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({"email": "a@x.it", "display_name": "Ada"})),
            )
            .expect(1)
            .mount(&server)
            .await;
        let store = MemoryStore::new();
        let backend_url = server.uri();
        let client_store = store.clone();

        let result = tokio::task::spawn_blocking(move || {
            let client = BackendClient::new(&backend_url, Box::new(client_store)).unwrap();
            run_login(
                &Mutex::new(client),
                &AtomicBool::new(false),
                fake_browser("c1"),
            )
        })
        .await
        .unwrap();

        assert_eq!(result.unwrap().display_name.as_deref(), Some("Ada"));
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("r1"));
        let requests = server.received_requests().await.unwrap();
        let exchange = requests
            .iter()
            .find(|request| request.url.path() == AUTH_TOKEN_PATH)
            .unwrap();
        let body: serde_json::Value = exchange.body_json().unwrap();
        assert_eq!(body["code"], "c1");
    }
}
