//! Where device secrets live. Desktop (OS keyring) and mobile (Keystore /
//! Keychain) implementations arrive with the apps (F5, F7); the in-memory
//! one serves tests and the simulator.

use std::collections::BTreeMap;
use std::sync::Mutex;

use zeroize::Zeroizing;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SecretStoreError {
    #[error("the secret store is not available")]
    Unavailable,
    #[error("a stored secret is corrupt or incomplete")]
    Corrupt,
}

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError>;
    fn put(&self, name: &str, value: &[u8]) -> Result<(), SecretStoreError>;
    fn delete(&self, name: &str) -> Result<(), SecretStoreError>;
}

/// Secrets kept in memory only (tests, simulator).
#[derive(Default)]
pub struct MemorySecretStore {
    items: Mutex<BTreeMap<String, Zeroizing<Vec<u8>>>>,
}

impl SecretStore for MemorySecretStore {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        let items = self
            .items
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?;
        Ok(items.get(name).cloned())
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?;
        items.insert(name.to_owned(), Zeroizing::new(value.to_vec()));
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?;
        items.remove(name);
        Ok(())
    }
}
