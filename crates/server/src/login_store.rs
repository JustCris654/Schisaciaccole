//! In-memory, short-lived state of the login flow: logins waiting for the
//! Google callback and one-time codes waiting for `POST /auth/token`.
//!
//! Entries live only in this process: a restart drops logins in progress,
//! and running several server replicas needs sticky routing.

use std::collections::HashMap;
use std::fmt;
use std::net::IpAddr;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use openidconnect::{Nonce, PkceCodeVerifier};
use thiserror::Error;
use uuid::Uuid;

/// Maximum age of a pending login: the time the user has to complete the
/// Google consent screen.
pub const PENDING_LOGIN_TTL: Duration = Duration::from_secs(10 * 60);

/// Maximum age of a one-time app code.
pub const APP_CODE_TTL: Duration = Duration::from_secs(60);

/// Maximum number of pending logins kept at the same time.
pub const MAX_PENDING_LOGINS: usize = 10_000;

/// Maximum number of pending logins of one client kept at the same time.
pub const MAX_PENDING_LOGINS_PER_CLIENT: usize = 20;

/// Maximum number of one-time app codes kept at the same time.
pub const MAX_APP_CODES: usize = 10_000;

/// The store holds the maximum number of live entries and refuses new ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("login store is full")]
pub struct StoreFull;

/// Why [`LoginStore::insert_pending`] refused a pending login.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PendingLoginRefused {
    /// The client already has [`MAX_PENDING_LOGINS_PER_CLIENT`] pending
    /// logins.
    #[error("too many logins in progress for this client")]
    ClientLimit,
    /// The store holds [`MAX_PENDING_LOGINS`] pending logins.
    #[error(transparent)]
    StoreFull(#[from] StoreFull),
}

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
    /// IP address of the client that started the login; `None` when it is
    /// unknown. All unknown clients share one quota.
    pub client: Option<IpAddr>,
}

/// A one-time code handed to the app, exchanged by `POST /auth/token`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppCode {
    /// User who completed the login.
    pub user_id: Uuid,
    /// PKCE `S256` challenge the code verifier must match.
    pub app_challenge: String,
}

/// Map whose entries expire `ttl` after insertion and which holds at most
/// `max_entries` entries.
struct ExpiringMap<V> {
    ttl: Duration,
    max_entries: usize,
    entries: HashMap<String, (Instant, V)>,
}

impl<V> ExpiringMap<V> {
    fn new(ttl: Duration, max_entries: usize) -> Self {
        Self {
            ttl,
            max_entries,
            entries: HashMap::new(),
        }
    }

    /// Inserts `value` created at `now`, first dropping every entry expired
    /// at `now`. An entry with the same key is replaced.
    ///
    /// # Errors
    ///
    /// Returns [`StoreFull`] when, after the purge, the map holds
    /// `max_entries` entries and `key` is not one of them; `value` is
    /// dropped.
    fn insert(&mut self, key: String, value: V, now: Instant) -> Result<(), StoreFull> {
        self.purge_expired(now);
        if self.entries.len() >= self.max_entries && !self.entries.contains_key(&key) {
            return Err(StoreFull);
        }
        self.entries.insert(key, (now, value));
        Ok(())
    }

    /// Drops every entry expired at `now`.
    fn purge_expired(&mut self, now: Instant) {
        let ttl = self.ttl;
        self.entries
            .retain(|_, (created, _)| !is_expired(ttl, *created, now));
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
    /// [`APP_CODE_TTL`] lifetimes and the [`MAX_PENDING_LOGINS`],
    /// [`MAX_PENDING_LOGINS_PER_CLIENT`] and [`MAX_APP_CODES`] caps.
    pub fn new() -> Self {
        Self {
            pending: Mutex::new(ExpiringMap::new(PENDING_LOGIN_TTL, MAX_PENDING_LOGINS)),
            codes: Mutex::new(ExpiringMap::new(APP_CODE_TTL, MAX_APP_CODES)),
        }
    }

    /// Stores a pending login under the Google `state`, replacing any login
    /// with the same state. Expired pending logins are purged first.
    ///
    /// The per-client quota counts the valid logins whose
    /// [`PendingLogin::client`] equals `login.client`, except the one stored
    /// under `google_state`; logins removed by [`Self::take_pending`] or
    /// expired no longer count.
    ///
    /// # Errors
    ///
    /// Returns, checked in this order and dropping `login`:
    /// - [`PendingLoginRefused::ClientLimit`] when
    ///   [`MAX_PENDING_LOGINS_PER_CLIENT`] logins of the same client are
    ///   valid at `now`;
    /// - [`PendingLoginRefused::StoreFull`] when [`MAX_PENDING_LOGINS`]
    ///   logins are valid at `now` and `google_state` is not one of them.
    pub fn insert_pending(
        &self,
        google_state: String,
        login: PendingLogin,
        now: Instant,
    ) -> Result<(), PendingLoginRefused> {
        let mut pending = self.pending.lock().unwrap_or_else(PoisonError::into_inner);
        pending.purge_expired(now);
        let client_logins = pending
            .entries
            .iter()
            .filter(|(state, (_, other))| *state != &google_state && other.client == login.client)
            .count();
        if client_logins >= MAX_PENDING_LOGINS_PER_CLIENT {
            return Err(PendingLoginRefused::ClientLimit);
        }
        pending.insert(google_state, login, now)?;
        Ok(())
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
    ///
    /// # Errors
    ///
    /// Returns [`StoreFull`] when [`MAX_APP_CODES`] codes are still valid at
    /// `now` and `code` is not one of them.
    pub fn insert_code(&self, code: String, value: AppCode, now: Instant) -> Result<(), StoreFull> {
        let mut codes = self.codes.lock().unwrap_or_else(PoisonError::into_inner);
        codes.insert(code, value, now)
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
        pending_from(port, None)
    }

    fn pending_from(port: u16, client: Option<IpAddr>) -> PendingLogin {
        PendingLogin {
            app_port: port,
            app_state: "app-state".to_string(),
            app_challenge: "challenge".to_string(),
            google_pkce_verifier: PkceCodeVerifier::new("verifier".to_string()),
            nonce: Nonce::new("nonce".to_string()),
            client,
        }
    }

    fn client(text: &str) -> Option<IpAddr> {
        Some(text.parse().unwrap())
    }

    /// Fills the quota of `who` with logins `{prefix}0..` created at `now`.
    fn fill_client_quota(store: &LoginStore, prefix: &str, who: Option<IpAddr>, now: Instant) {
        for i in 0..MAX_PENDING_LOGINS_PER_CLIENT {
            store
                .insert_pending(format!("{prefix}{i}"), pending_from(5000, who), now)
                .unwrap();
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
        store
            .insert_pending("s".to_string(), pending(5000), now)
            .unwrap();

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
        store
            .insert_pending("s".to_string(), pending(5000), now)
            .unwrap();

        let later = now + PENDING_LOGIN_TTL - Duration::from_millis(1);

        assert!(store.take_pending("s", later).is_some());
    }

    #[test]
    fn pending_login_expires_at_ttl() {
        let store = LoginStore::new();
        let now = Instant::now();
        store
            .insert_pending("s".to_string(), pending(5000), now)
            .unwrap();

        assert!(store.take_pending("s", now + PENDING_LOGIN_TTL).is_none());
    }

    #[test]
    fn insert_purges_expired_pending_logins() {
        let store = LoginStore::new();
        let now = Instant::now();
        store
            .insert_pending("old".to_string(), pending(5000), now)
            .unwrap();

        let later = now + PENDING_LOGIN_TTL;
        store
            .insert_pending("new".to_string(), pending(5001), later)
            .unwrap();

        let pending = store.pending.lock().unwrap();
        assert!(!pending.entries.contains_key("old"));
        assert!(pending.entries.contains_key("new"));
    }

    #[test]
    fn code_is_returned_once() {
        let store = LoginStore::new();
        let now = Instant::now();
        let value = code();
        store
            .insert_code("c".to_string(), value.clone(), now)
            .unwrap();

        assert_eq!(store.take_code("c", now), Some(value));
        assert_eq!(store.take_code("c", now), None);
    }

    #[test]
    fn code_expires_after_sixty_seconds() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_code("c".to_string(), code(), now).unwrap();

        assert_eq!(store.take_code("c", now + Duration::from_secs(60)), None);
    }

    #[test]
    fn code_is_valid_before_sixty_seconds() {
        let store = LoginStore::new();
        let now = Instant::now();
        store.insert_code("c".to_string(), code(), now).unwrap();

        assert!(
            store
                .take_code("c", now + Duration::from_secs(59))
                .is_some()
        );
    }

    #[test]
    fn expiring_map_refuses_new_key_when_full() {
        let mut map = ExpiringMap::new(Duration::from_secs(60), 2);
        let now = Instant::now();
        map.insert("a".to_string(), 1, now).unwrap();
        map.insert("b".to_string(), 2, now).unwrap();

        let result = map.insert("c".to_string(), 3, now);

        assert_eq!(result, Err(StoreFull));
        assert!(!map.entries.contains_key("c"));
    }

    // A replacement does not grow the map, so it is allowed at the cap.
    #[test]
    fn expiring_map_replaces_existing_key_when_full() {
        let mut map = ExpiringMap::new(Duration::from_secs(60), 1);
        let now = Instant::now();
        map.insert("a".to_string(), 1, now).unwrap();

        map.insert("a".to_string(), 2, now).unwrap();

        assert_eq!(map.take("a", now), Some(2));
    }

    #[test]
    fn expiring_map_accepts_new_key_when_full_of_expired_entries() {
        let mut map = ExpiringMap::new(Duration::from_secs(60), 1);
        let now = Instant::now();
        map.insert("a".to_string(), 1, now).unwrap();

        let later = now + Duration::from_secs(60);

        assert_eq!(map.insert("b".to_string(), 2, later), Ok(()));
    }

    // Each login comes from its own client, so only the global cap applies.
    #[test]
    fn insert_pending_refuses_login_at_cap() {
        let store = LoginStore::new();
        let now = Instant::now();
        for i in 0..MAX_PENDING_LOGINS {
            let who = Some(IpAddr::from(u32::try_from(i).unwrap().to_be_bytes()));
            store
                .insert_pending(format!("s{i}"), pending_from(5000, who), now)
                .unwrap();
        }

        let result = store.insert_pending("extra".to_string(), pending(5000), now);

        assert_eq!(result, Err(PendingLoginRefused::StoreFull(StoreFull)));
    }

    #[test]
    fn insert_pending_refuses_client_over_its_quota() {
        let store = LoginStore::new();
        let now = Instant::now();
        let who = client("203.0.113.1");
        fill_client_quota(&store, "s", who, now);

        let result = store.insert_pending("extra".to_string(), pending_from(5000, who), now);

        assert_eq!(result, Err(PendingLoginRefused::ClientLimit));
    }

    #[test]
    fn client_quota_does_not_affect_other_clients() {
        let store = LoginStore::new();
        let now = Instant::now();
        fill_client_quota(&store, "s", client("203.0.113.1"), now);

        let result = store.insert_pending(
            "other".to_string(),
            pending_from(5000, client("203.0.113.2")),
            now,
        );

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn taken_login_frees_client_quota() {
        let store = LoginStore::new();
        let now = Instant::now();
        let who = client("203.0.113.1");
        fill_client_quota(&store, "s", who, now);
        store.take_pending("s0", now).unwrap();

        let result = store.insert_pending("extra".to_string(), pending_from(5000, who), now);

        assert_eq!(result, Ok(()));
    }

    #[test]
    fn expired_logins_free_client_quota() {
        let store = LoginStore::new();
        let now = Instant::now();
        let who = client("203.0.113.1");
        fill_client_quota(&store, "s", who, now);

        let later = now + PENDING_LOGIN_TTL;
        let result = store.insert_pending("extra".to_string(), pending_from(5000, who), later);

        assert_eq!(result, Ok(()));
    }

    // Logins of unknown clients share one quota.
    #[test]
    fn unknown_clients_share_one_quota() {
        let store = LoginStore::new();
        let now = Instant::now();
        fill_client_quota(&store, "s", None, now);

        let result = store.insert_pending("extra".to_string(), pending(5000), now);

        assert_eq!(result, Err(PendingLoginRefused::ClientLimit));
    }

    #[test]
    fn unknown_client_quota_does_not_affect_known_clients() {
        let store = LoginStore::new();
        let now = Instant::now();
        fill_client_quota(&store, "s", None, now);

        let result = store.insert_pending(
            "extra".to_string(),
            pending_from(5000, client("203.0.113.1")),
            now,
        );

        assert_eq!(result, Ok(()));
    }

    // A replacement does not grow the client's logins, so it is allowed at
    // the quota.
    #[test]
    fn insert_pending_replaces_existing_state_at_client_quota() {
        let store = LoginStore::new();
        let now = Instant::now();
        let who = client("203.0.113.1");
        fill_client_quota(&store, "s", who, now);

        let result = store.insert_pending("s0".to_string(), pending_from(6000, who), now);

        assert_eq!(result, Ok(()));
        assert_eq!(
            store.take_pending("s0", now).map(|p| p.app_port),
            Some(6000)
        );
    }

    #[test]
    fn insert_code_refuses_code_at_cap() {
        let store = LoginStore::new();
        let now = Instant::now();
        for i in 0..MAX_APP_CODES {
            store.insert_code(format!("c{i}"), code(), now).unwrap();
        }

        let result = store.insert_code("extra".to_string(), code(), now);

        assert_eq!(result, Err(StoreFull));
    }
}
