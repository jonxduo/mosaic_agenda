//! Device secrets: the operating system's keyring on desktop (macOS
//! Keychain, Windows Credential Manager, Secret Service on Linux); Android
//! Keystore or iOS Keychain through the `secret-store` plugin on mobile.

use core_crypto::{SecretStore, SecretStoreError};
use zeroize::Zeroizing;

#[cfg(desktop)]
pub struct KeyringSecretStore {
    service: String,
}

#[cfg(desktop)]
impl KeyringSecretStore {
    pub fn new(service: &str) -> Self {
        Self {
            service: service.to_owned(),
        }
    }

    fn entry(&self, name: &str) -> Result<keyring::Entry, SecretStoreError> {
        keyring::Entry::new(&self.service, name).map_err(|_| SecretStoreError::Unavailable)
    }
}

#[cfg(desktop)]
impl SecretStore for KeyringSecretStore {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        match self.entry(name)?.get_secret() {
            Ok(v) => Ok(Some(Zeroizing::new(v))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(SecretStoreError::Unavailable),
        }
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        self.entry(name)?
            .set_secret(value)
            .map_err(|_| SecretStoreError::Unavailable)
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        match self.entry(name)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(SecretStoreError::Unavailable),
        }
    }
}

/// Mobile: Keystore (Android) or Keychain (iOS).
#[cfg(mobile)]
pub struct PluginSecretStore(pub tauri::AppHandle);

#[cfg(mobile)]
impl SecretStore for PluginSecretStore {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>, SecretStoreError> {
        use tauri_plugin_secret_store::SecretStoreExt;
        self.0
            .secret_store()
            .get(name)
            .map(|v| v.map(Zeroizing::new))
            .map_err(|_| SecretStoreError::Unavailable)
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<(), SecretStoreError> {
        use tauri_plugin_secret_store::SecretStoreExt;
        self.0
            .secret_store()
            .put(name, value)
            .map_err(|_| SecretStoreError::Unavailable)
    }

    fn delete(&self, name: &str) -> Result<(), SecretStoreError> {
        use tauri_plugin_secret_store::SecretStoreExt;
        self.0
            .secret_store()
            .delete(name)
            .map_err(|_| SecretStoreError::Unavailable)
    }
}
