//! In-memory cache of Google access tokens, one per user, so that a Google
//! refresh is not needed for every Calendar request.
//!
//! Entries live only in this process: a restart empties the cache.

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use openidconnect::AccessToken;
use uuid::Uuid;

/// A cached token is no longer handed out when it expires within this
/// margin, so it cannot expire while a Calendar request is in flight.
pub const EXPIRY_MARGIN: Duration = Duration::from_secs(60);

struct CachedToken {
    token: AccessToken,
    expires_at: Instant,
}

/// Google access tokens of the users, shared by all handlers.
///
/// Every operation takes the current instant explicitly so that expiry can
/// be tested without waiting. The lock is held only for a map operation,
/// never across an `.await`.
pub struct GoogleTokenCache {
    entries: Mutex<HashMap<Uuid, CachedToken>>,
}

impl GoogleTokenCache {
    /// Creates an empty cache.
    pub fn new() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
        }
    }

    /// Returns the token of `user_id` when it is still valid for more than
    /// [`EXPIRY_MARGIN`] at `now`.
    ///
    /// Returns `None` when no token is cached or the cached one is too close
    /// to its expiry; such an entry is removed.
    pub fn get(&self, user_id: Uuid, now: Instant) -> Option<AccessToken> {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        let entry = entries.get(&user_id)?;
        if is_usable(entry.expires_at, now) {
            return Some(entry.token.clone());
        }
        entries.remove(&user_id);
        None
    }

    /// Stores the token of `user_id`, valid until `expires_at`, replacing
    /// any previous one. Entries no longer usable at `now` are purged first.
    pub fn insert(&self, user_id: Uuid, token: AccessToken, expires_at: Instant, now: Instant) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.retain(|_, entry| is_usable(entry.expires_at, now));
        entries.insert(user_id, CachedToken { token, expires_at });
    }

    /// Drops the token of `user_id`, if any.
    pub fn remove(&self, user_id: Uuid) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        entries.remove(&user_id);
    }
}

impl Default for GoogleTokenCache {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for GoogleTokenCache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GoogleTokenCache").finish_non_exhaustive()
    }
}

/// True when a token expiring at `expires_at` is still valid for more than
/// [`EXPIRY_MARGIN`] at `now`. False when `now + EXPIRY_MARGIN` overflows.
fn is_usable(expires_at: Instant, now: Instant) -> bool {
    now.checked_add(EXPIRY_MARGIN)
        .is_some_and(|limit| limit < expires_at)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token(value: &str) -> AccessToken {
        AccessToken::new(value.to_string())
    }

    fn secret(token: Option<AccessToken>) -> Option<String> {
        token.map(|token| token.secret().clone())
    }

    #[test]
    fn returns_token_valid_beyond_margin() {
        let cache = GoogleTokenCache::new();
        let user = Uuid::new_v4();
        let now = Instant::now();
        cache.insert(user, token("a"), now + Duration::from_secs(3600), now);

        assert_eq!(secret(cache.get(user, now)), Some("a".to_string()));
    }

    #[test]
    fn returns_none_for_unknown_user() {
        let cache = GoogleTokenCache::new();

        assert!(cache.get(Uuid::new_v4(), Instant::now()).is_none());
    }

    #[test]
    fn rejects_token_expiring_exactly_at_margin() {
        let cache = GoogleTokenCache::new();
        let user = Uuid::new_v4();
        let now = Instant::now();
        cache.insert(user, token("a"), now + EXPIRY_MARGIN, now);

        assert!(cache.get(user, now).is_none());
    }

    #[test]
    fn accepts_token_expiring_just_after_margin() {
        let cache = GoogleTokenCache::new();
        let user = Uuid::new_v4();
        let now = Instant::now();
        let expires_at = now + EXPIRY_MARGIN + Duration::from_secs(1);
        cache.insert(user, token("a"), expires_at, now);

        assert!(cache.get(user, now).is_some());
    }

    #[test]
    fn token_becomes_unusable_60_seconds_before_expiry() {
        let cache = GoogleTokenCache::new();
        let user = Uuid::new_v4();
        let now = Instant::now();
        let expires_at = now + Duration::from_secs(3600);
        cache.insert(user, token("a"), expires_at, now);

        let later = expires_at - Duration::from_secs(59);

        assert!(cache.get(user, later).is_none());
    }

    #[test]
    fn unusable_entry_is_removed_on_get() {
        let cache = GoogleTokenCache::new();
        let user = Uuid::new_v4();
        let now = Instant::now();
        cache.insert(user, token("a"), now + Duration::from_secs(3600), now);
        cache.get(user, now + Duration::from_secs(3600));

        assert!(cache.get(user, now).is_none());
    }

    #[test]
    fn insert_replaces_previous_token() {
        let cache = GoogleTokenCache::new();
        let user = Uuid::new_v4();
        let now = Instant::now();
        let expires_at = now + Duration::from_secs(3600);
        cache.insert(user, token("a"), expires_at, now);

        cache.insert(user, token("b"), expires_at, now);

        assert_eq!(secret(cache.get(user, now)), Some("b".to_string()));
    }

    #[test]
    fn remove_drops_token() {
        let cache = GoogleTokenCache::new();
        let user = Uuid::new_v4();
        let now = Instant::now();
        cache.insert(user, token("a"), now + Duration::from_secs(3600), now);

        cache.remove(user);

        assert!(cache.get(user, now).is_none());
    }

    #[test]
    fn tokens_of_different_users_are_separate() {
        let cache = GoogleTokenCache::new();
        let (ada, bob) = (Uuid::new_v4(), Uuid::new_v4());
        let now = Instant::now();
        let expires_at = now + Duration::from_secs(3600);
        cache.insert(ada, token("a"), expires_at, now);
        cache.insert(bob, token("b"), expires_at, now);

        cache.remove(ada);

        assert_eq!(secret(cache.get(bob, now)), Some("b".to_string()));
    }
}
