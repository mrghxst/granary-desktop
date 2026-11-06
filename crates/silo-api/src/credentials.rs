//! Refresh-token storage (ADR-0004): Windows Credential Manager in the app, memory in tests.
//! Only refresh tokens are stored; access tokens and profile tokens stay in memory.
use crate::error::{ApiError, Result};
use crate::secret::Secret;
use std::collections::HashMap;
use std::sync::Mutex;

/// Credential Manager "service" part of every entry.
pub const SERVICE: &str = "Granary";

/// Key of a stored refresh token: `<server_id>/<user_id>`.
pub fn account_key(server_id: &str, user_id: &str) -> String {
    format!("{server_id}/{user_id}")
}

pub trait CredentialStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<Secret>>;
    fn set(&self, key: &str, secret: &Secret) -> Result<()>;
    /// Deleting a missing entry is not an error.
    fn delete(&self, key: &str) -> Result<()>;
}

/// In-memory store for tests and the mock server.
#[derive(Default)]
pub struct MemoryStore(Mutex<HashMap<String, Secret>>);

impl MemoryStore {
    fn map(&self) -> Result<std::sync::MutexGuard<'_, HashMap<String, Secret>>> {
        self.0
            .lock()
            .map_err(|_| ApiError::Credentials("memory store poisoned".into()))
    }

    pub fn len(&self) -> usize {
        self.map().map(|m| m.len()).unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl CredentialStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        Ok(self.map()?.get(key).cloned())
    }
    fn set(&self, key: &str, secret: &Secret) -> Result<()> {
        self.map()?.insert(key.to_owned(), secret.clone());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.map()?.remove(key);
        Ok(())
    }
}

/// Windows Credential Manager (generic credentials, DPAPI-protected per user) through `keyring-core` and
/// `windows-native-keyring-store`. Target name is `<key>.Granary` with the store's default delimiters.
#[cfg(windows)]
pub struct WindowsCredentialStore {
    store: std::sync::Arc<windows_native_keyring_store::Store>,
}

#[cfg(windows)]
impl WindowsCredentialStore {
    pub fn new() -> Result<Self> {
        let store = windows_native_keyring_store::Store::new().map_err(|e| ApiError::Credentials(e.to_string()))?;
        Ok(Self { store })
    }

    fn entry(&self, key: &str) -> Result<keyring_core::Entry> {
        use keyring_core::api::CredentialStoreApi;
        self.store
            .build(SERVICE, key, None)
            .map_err(|e| ApiError::Credentials(e.to_string()))
    }
}

#[cfg(windows)]
impl CredentialStore for WindowsCredentialStore {
    fn get(&self, key: &str) -> Result<Option<Secret>> {
        match self.entry(key)?.get_password() {
            Ok(p) => Ok(Some(Secret::new(p))),
            Err(keyring_core::Error::NoEntry) => Ok(None),
            Err(e) => Err(ApiError::Credentials(e.to_string())),
        }
    }
    fn set(&self, key: &str, secret: &Secret) -> Result<()> {
        self.entry(key)?
            .set_password(secret.expose())
            .map_err(|e| ApiError::Credentials(e.to_string()))
    }
    fn delete(&self, key: &str) -> Result<()> {
        match self.entry(key)?.delete_credential() {
            Ok(()) | Err(keyring_core::Error::NoEntry) => Ok(()),
            Err(e) => Err(ApiError::Credentials(e.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(store: &dyn CredentialStore, key: &str) {
        assert!(matches!(store.get(key), Ok(None)));
        assert!(store.set(key, &Secret::new("refresh-1")).is_ok());
        assert!(store.set(key, &Secret::new("refresh-2")).is_ok());
        assert_eq!(
            store.get(key).ok().flatten().map(|s| s.expose().to_owned()).as_deref(),
            Some("refresh-2")
        );
        assert!(store.delete(key).is_ok());
        assert!(store.delete(key).is_ok(), "deleting twice is fine");
        assert!(matches!(store.get(key), Ok(None)));
    }

    #[test]
    fn memory_store_round_trip() {
        round_trip(&MemoryStore::default(), "srv/1");
    }

    /// Writes, reads and deletes one real Credential Manager entry with a random key.
    #[cfg(windows)]
    #[test]
    fn windows_store_round_trip() {
        let Ok(store) = WindowsCredentialStore::new() else {
            panic!("credential store unavailable");
        };
        round_trip(
            &store,
            &account_key(&format!("granary-test-{}", uuid::Uuid::new_v4()), "1"),
        );
    }
}
