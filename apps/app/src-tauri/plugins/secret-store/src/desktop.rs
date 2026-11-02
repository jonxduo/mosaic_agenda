use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use tauri::plugin::PluginApi;
use tauri::{AppHandle, Runtime};

use crate::{Error, Result};

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    _api: PluginApi<R, C>,
) -> Result<SecretStore<R>> {
    Ok(SecretStore(PhantomData))
}

/// Desktop: not used (the app keeps secrets with the `keyring` crate).
pub struct SecretStore<R: Runtime>(PhantomData<fn() -> R>);

impl<R: Runtime> SecretStore<R> {
    pub fn get(&self, _name: &str) -> Result<Option<Vec<u8>>> {
        Err(Error::Unavailable)
    }

    pub fn put(&self, _name: &str, _value: &[u8]) -> Result<()> {
        Err(Error::Unavailable)
    }

    pub fn delete(&self, _name: &str) -> Result<()> {
        Err(Error::Unavailable)
    }
}
