//! Secret stores for the apps that have no OS keyring at hand.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Mutex;

use core_crypto::{SecretStore, SecretStoreError};
use zeroize::Zeroizing;

/// Secrets in a JSON file, **for development and tests only**: the desktop
/// app uses the OS keyring, mobile apps Keystore/Keychain.
pub struct FileSecretStore {
    path: PathBuf,
    items: Mutex<BTreeMap<String, String>>,
}

impl FileSecretStore {
    pub fn open(path: PathBuf) -> Self {
        let items = std::fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Self {
            path,
            items: Mutex::new(items),
        }
    }

    fn save(&self, items: &BTreeMap<String, String>) -> Result<(), SecretStoreError> {
        let bytes = serde_json::to_vec(items).map_err(|_| SecretStoreError::Unavailable)?;
        std::fs::write(&self.path, bytes).map_err(|_| SecretStoreError::Unavailable)
    }
}

impl SecretStore for FileSecretStore {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        let items = self
            .items
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?;
        match items.get(name) {
            None => Ok(None),
            Some(hex) => data_encoding::HEXLOWER
                .decode(hex.as_bytes())
                .map(|v| Some(Zeroizing::new(v)))
                .map_err(|_| SecretStoreError::Corrupt),
        }
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?;
        items.insert(name.to_owned(), data_encoding::HEXLOWER.encode(value));
        self.save(&items)
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        let mut items = self
            .items
            .lock()
            .map_err(|_| SecretStoreError::Unavailable)?;
        items.remove(name);
        self.save(&items)
    }
}
