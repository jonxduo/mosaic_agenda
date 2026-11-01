//! Device secrets in the operating system's keyring (macOS Keychain,
//! Windows Credential Manager, Secret Service on Linux).

use core_crypto::{SecretStore, SecretStoreError};
use zeroize::Zeroizing;

pub struct KeyringSecretStore {
    service: String,
}

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
