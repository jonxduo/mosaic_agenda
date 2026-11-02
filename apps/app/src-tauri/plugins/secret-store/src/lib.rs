//! Device protection on mobile:
//! - secrets (PROGETTO.md §8.1): Android Keystore and iOS Keychain behind the
//!   same three calls. Desktop uses the `keyring` crate in the app instead;
//!   there this plugin only reports that it is unavailable;
//! - content hidden in the recent-apps screen (§8.6, mobile checklist #11).

use tauri::plugin::{Builder, TauriPlugin};
use tauri::{Manager, Runtime};

#[cfg(desktop)]
mod desktop;
mod error;
#[cfg(mobile)]
mod mobile;

pub use error::{Error, Result};

#[cfg(desktop)]
pub use desktop::SecretStore;
#[cfg(mobile)]
pub use mobile::SecretStore;

/// Access from [`tauri::App`], [`tauri::AppHandle`] and windows.
pub trait SecretStoreExt<R: Runtime> {
    fn secret_store(&self) -> &SecretStore<R>;
}

impl<R: Runtime, T: Manager<R>> SecretStoreExt<R> for T {
    fn secret_store(&self) -> &SecretStore<R> {
        self.state::<SecretStore<R>>().inner()
    }
}

pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("secret-store")
        .setup(|app, api| {
            #[cfg(mobile)]
            let store = mobile::init(app, api)?;
            #[cfg(desktop)]
            let store = desktop::init(app, api)?;
            app.manage(store);
            Ok(())
        })
        .build()
}
