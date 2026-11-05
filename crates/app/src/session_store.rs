//! Persistence of the refresh token: OS keychain in production, memory in tests and as fallback.

use std::sync::{Arc, Mutex, PoisonError};

use keyring_core::Entry;

/// Keychain service name under which the refresh token is saved.
const KEYRING_SERVICE: &str = "com.openmove.schisaciaccole";
/// Keychain account name of the refresh token entry.
const KEYRING_USER: &str = "refresh-token";

/// Failure of a session store.
#[derive(Debug, thiserror::Error)]
pub enum SessionStoreError {
    /// The keychain reported an error.
    #[error("keychain error: {0}")]
    Keyring(#[from] keyring_core::Error),
    /// No keychain implementation exists for this operating system.
    #[error("no keychain available on this platform")]
    #[cfg_attr(any(target_os = "macos", target_os = "linux"), allow(dead_code))]
    Unsupported,
}

/// Storage for the long-lived refresh token.
pub trait SessionStore {
    /// Returns the saved refresh token, or `None` when nothing is saved.
    ///
    /// # Errors
    ///
    /// Returns [`SessionStoreError`] when the storage cannot be read.
    fn load_refresh_token(&self) -> Result<Option<String>, SessionStoreError>;

    /// Saves the refresh token, replacing any previous one.
    ///
    /// # Errors
    ///
    /// Returns [`SessionStoreError`] when the storage cannot be written.
    fn save_refresh_token(&self, token: &str) -> Result<(), SessionStoreError>;

    /// Removes the saved refresh token. Succeeds when nothing is saved.
    ///
    /// # Errors
    ///
    /// Returns [`SessionStoreError`] when the storage cannot be modified.
    fn clear(&self) -> Result<(), SessionStoreError>;
}

/// Sets the process-wide default keychain used by [`KeyringStore`].
///
/// Call it once at startup, before creating a [`KeyringStore`]. Calling it again replaces
/// the default store.
///
/// # Errors
///
/// Returns [`SessionStoreError::Keyring`] when the platform keychain cannot be opened and
/// [`SessionStoreError::Unsupported`] on operating systems without a keychain backend. The
/// caller can fall back to [`MemoryStore`].
pub fn init_keyring() -> Result<(), SessionStoreError> {
    #[cfg(target_os = "macos")]
    {
        let store = apple_native_keyring_store::keychain::Store::new()?;
        keyring_core::set_default_store(store);
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let store = zbus_secret_service_keyring_store::Store::new()?;
        keyring_core::set_default_store(store);
        Ok(())
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(SessionStoreError::Unsupported)
    }
}

/// Session store backed by the OS keychain; requires a prior call to [`init_keyring`].
#[derive(Debug, Clone, Copy, Default)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry() -> Result<Entry, SessionStoreError> {
        Ok(Entry::new(KEYRING_SERVICE, KEYRING_USER)?)
    }
}

impl SessionStore for KeyringStore {
    /// A missing entry is `Ok(None)`. Fails with a keyring error when no default store was
    /// set, the keychain is locked, or the stored value is not valid UTF-8.
    fn load_refresh_token(&self) -> Result<Option<String>, SessionStoreError> {
        match Self::entry()?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    fn save_refresh_token(&self, token: &str) -> Result<(), SessionStoreError> {
        Self::entry()?.set_password(token)?;
        Ok(())
    }

    /// A missing entry is not an error.
    fn clear(&self) -> Result<(), SessionStoreError> {
        match Self::entry()?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

/// In-memory session store. Clones share the same slot, so a test can keep a handle
/// while the client owns another. The token is lost when the process exits.
#[derive(Debug, Clone, Default)]
pub struct MemoryStore {
    token: Arc<Mutex<Option<String>>>,
}

impl MemoryStore {
    /// Creates an empty store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the content of the slot. A poisoned lock is recovered, never an error.
    fn set(&self, value: Option<String>) {
        let mut slot = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        *slot = value;
    }
}

impl SessionStore for MemoryStore {
    fn load_refresh_token(&self) -> Result<Option<String>, SessionStoreError> {
        let slot = self.token.lock().unwrap_or_else(PoisonError::into_inner);
        Ok(slot.clone())
    }

    fn save_refresh_token(&self, token: &str) -> Result<(), SessionStoreError> {
        self.set(Some(token.to_string()));
        Ok(())
    }

    fn clear(&self) -> Result<(), SessionStoreError> {
        self.set(None);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_store_loads_none() {
        let store = MemoryStore::new();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[test]
    fn saved_token_is_loaded() {
        let store = MemoryStore::new();
        store.save_refresh_token("r1").unwrap();
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("r1"));
    }

    #[test]
    fn save_replaces_the_previous_token() {
        let store = MemoryStore::new();
        store.save_refresh_token("r1").unwrap();
        store.save_refresh_token("r2").unwrap();
        assert_eq!(store.load_refresh_token().unwrap().as_deref(), Some("r2"));
    }

    #[test]
    fn clear_removes_the_token() {
        let store = MemoryStore::new();
        store.save_refresh_token("r1").unwrap();
        store.clear().unwrap();
        assert_eq!(store.load_refresh_token().unwrap(), None);
    }

    #[test]
    fn clear_on_empty_store_succeeds() {
        let store = MemoryStore::new();
        assert!(store.clear().is_ok());
    }

    #[test]
    fn clones_share_the_same_slot() {
        let store = MemoryStore::new();
        let other = store.clone();
        store.save_refresh_token("r1").unwrap();
        assert_eq!(other.load_refresh_token().unwrap().as_deref(), Some("r1"));
    }
}
