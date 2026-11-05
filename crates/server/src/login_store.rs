//! In-memory, short-lived state of the login flow: logins waiting for the
//! Google callback and one-time codes waiting for `POST /auth/token`.
//!
//! Entries live only in this process: a restart drops logins in progress,
//! and running several server replicas needs sticky routing.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use openidconnect::{Nonce, PkceCodeVerifier};
use uuid::Uuid;

/// Maximum age of a pending login: the time the user has to complete the
/// Google consent screen.
pub const PENDING_LOGIN_TTL: Duration = Duration::from_secs(10 * 60);

/// Maximum age of a one-time app code.
pub const APP_CODE_TTL: Duration = Duration::from_secs(60);

/// A login started by `GET /auth/login`, waiting for the Google callback.
pub struct PendingLogin {
    /// Port of the app's loopback listener.
    pub app_port: u16,
    /// Opaque state chosen by the app, echoed back on its callback.
    pub app_state: String,
    /// PKCE `S256` challenge chosen by the app.
    pub app_challenge: String,
    /// PKCE verifier of the Google authorization request.
    pub google_pkce_verifier: PkceCodeVerifier,
    /// Nonce expected in the Google ID token.
    pub nonce: Nonce,
}

/// A one-time code handed to the app, exchanged by `POST /auth/token`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCode {
    /// User who completed the login.
    pub user_id: Uuid,
    /// PKCE `S256` challenge the code verifier must match.
    pub app_challenge: String,
}

/// Map whose entries expire `ttl` after insertion.
struct ExpiringMap<V> {
    ttl: Duration,
    entries: HashMap<String, (Instant, V)>,
}

impl<V> ExpiringMap<V> {
    fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            entries: HashMap::new(),
        }
    }

    /// Inserts `value` created at `now`, first dropping every entry expired
    /// at `now`. An entry with the same key is replaced.
    fn insert(&mut self, key: String, value: V, now: Instant) {
        let ttl = self.ttl;
        self.entries
            .retain(|_, (created, _)| !is_expired(ttl, *created, now));
        self.entries.insert(key, (now, value));
    }

    /// Removes the entry for `key` and returns it if not expired at `now`.
    /// An expired entry is removed too.
    fn take(&mut self, key: &str, now: Instant) -> Option<V> {
        let (created, value) = self.entries.remove(key)?;
        if is_expired(self.ttl, created, now) {
            return None;
        }
        Some(value)
    }
}

/// True when an entry created at `created` is expired at `now`. An entry
/// whose `created` is after `now` is not expired.
fn is_expired(ttl: Duration, created: Instant, now: Instant) -> bool {
    now.saturating_duration_since(created) >= ttl
}

/// Pending logins and one-time codes, shared by all handlers.
///
/// Every operation takes the current instant explicitly so that expiry can
/// be tested without waiting. Each lock is held only for a map operation.
pub struct LoginStore {
    pending: Mutex<ExpiringMap<PendingLogin>>,
    codes: Mutex<ExpiringMap<AppCode>>,
}

impl LoginStore {
    /// Creates an empty store with the [`PENDING_LOGIN_TTL`] and
    /// [`APP_CODE_TTL`] lifetimes.
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(ExpiringMap::new(PENDING_LOGIN_TTL)),
            codes: Mutex::new(ExpiringMap::new(APP_CODE_TTL)),
        }
    }

    /// Stores a pending login under the Google `state`, replacing any login
    /// with the same state. Expired pending logins are purged first.
    pub fn insert_pending(&self, google_state: String, login: PendingLogin, now: Instant) {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.insert(google_state, login, now);
    }

    /// Removes and returns the pending login for `google_state`.
    ///
    /// Returns `None` when the state is unknown, already taken, or older
    /// than [`PENDING_LOGIN_TTL`] at `now`.
    pub fn take_pending(&self, google_state: &str, now: Instant) -> Option<PendingLogin> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.take(google_state, now)
    }

    /// Stores a one-time code, replacing any entry with the same code.
    /// Expired codes are purged first.
    pub fn insert_code(&self, code: String, value: AppCode, now: Instant) {
        let mut codes = self.codes.lock().unwrap_or_else(PoisonError::into_inner);
        codes.insert(code, value, now);
    }

    /// Removes and returns the one-time code entry for `code`.
    ///
    /// Returns `None` when the code is unknown, already used, or older than
    /// [`APP_CODE_TTL`] at `now`.
    pub fn take_code(&self, code: &str, now: Instant) -> Option<AppCode> {
        let mut codes = self.codes.lock().unwrap_or_else(PoisonError::into_inner);
        codes.take(code, now)
    }
}

impl Default for LoginStore {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for LoginStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoginStore").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pending(port: u16) -> PendingLogin {
        PendingLogin {
            app_port: port,
            app_state: "app-state".to_string(),
            app_challenge: "challenge".to_string(),
            google_pkce_verifier: PkceCodeVerifier::new("verifier".to_string()),
            nonce: Nonce::new("nonce".to_string()),
        }
    }

    fn code() -> AppCode {
        AppCode {
            user_id: Uuid::new_v4(),
            app_challenge: "challenge".to_string(),
        }
    }

    #[test]
    fn pending_login_is_returned_once() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_pending("s".to_string(), pending(5000), now);

        let first = store.take_pending("s", now);
        let second = store.take_pending("s", now);

        assert_eq!(first.map(|p| p.app_port), Some(5000));
        assert!(second.is_none());
    }

    #[test]
    fn unknown_pending_state_returns_none() {
        let store = LoginStore::new();

        assert!(store.take_pending("missing", Instant::now()).is_none());
    }

    #[test]
    fn pending_login_is_valid_just_before_ttl() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_pending("s".to_string(), pending(5000), now);

        let later = now + PENDING_LOGIN_TTL - Duration::from_millis(1);

        assert!(store.take_pending("s", later).is_some());
    }

    #[test]
    fn pending_login_expires_at_ttl() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_pending("s".to_string(), pending(5000), now);

        assert!(store.take_pending("s", now + PENDING_LOGIN_TTL).is_none());
    }

    #[test]
    fn insert_purges_expired_pending_logins() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_pending("old".to_string(), pending(5000), now);

        let later = now + PENDING_LOGIN_TTL;
        store.insert_pending("new".to_string(), pending(5001), later);

        let pending = store.pending.lock().unwrap();
        assert!(!pending.entries.contains_key("old"));
        assert!(pending.entries.contains_key("new"));
    }

    #[test]
    fn code_is_returned_once() {
        let store = LoginStore::new();
        let now = Instant::now();
        let value = code();
        store.insert_code("c".to_string(), value.clone(), now);

        assert_eq!(store.take_code("c", now), Some(value));
        assert_eq!(store.take_code("c", now), None);
    }

    #[test]
    fn code_expires_after_sixty_seconds() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_code("c".to_string(), code(), now);

        assert_eq!(store.take_code("c", now + Duration::from_secs(60)), None);
    }

    #[test]
    fn code_is_valid_before_sixty_seconds() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_code("c".to_string(), code(), now);

        assert!(
            store
                .take_code("c", now + Duration::from_secs(59))
                .is_some()
        );
    }
}
