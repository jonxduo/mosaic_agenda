use data_encoding::BASE64;
use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::json;
use tauri::plugin::{PluginApi, PluginHandle};
use tauri::{AppHandle, Runtime};

use crate::{Error, Result};

#[cfg(target_os = "ios")]
tauri::ios_plugin_binding!(init_plugin_secret_store);

pub fn init<R: Runtime, C: DeserializeOwned>(
    _app: &AppHandle<R>,
    api: PluginApi<R, C>,
) -> Result<SecretStore<R>> {
    #[cfg(target_os = "android")]
    let handle = api.register_android_plugin("it.briccolab.secretstore", "SecretStorePlugin")?;
    #[cfg(target_os = "ios")]
    let handle = api.register_ios_plugin(init_plugin_secret_store)?;
    Ok(SecretStore(handle))
}

#[derive(Deserialize, Default)]
struct Value {
    value: Option<String>,
}

/// Keystore (Android) or Keychain (iOS).
pub struct SecretStore<R: Runtime>(PluginHandle<R>);

impl<R: Runtime> SecretStore<R> {
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        let v: Value = self.0.run_mobile_plugin("get", json!({ "name": name }))?;
        v.value
            .map(|b| BASE64.decode(b.as_bytes()).map_err(|_| Error::Malformed))
            .transpose()
    }

    pub fn put(&self, name: &str, value: &[u8]) -> Result<()> {
        // The native side answers with nothing (null).
        let _: Option<serde_json::Value> = self.0.run_mobile_plugin(
            "put",
            json!({ "name": name, "value": BASE64.encode(value) }),
        )?;
        Ok(())
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let _: Option<serde_json::Value> = self
            .0
            .run_mobile_plugin("delete", json!({ "name": name }))?;
        Ok(())
    }
}
