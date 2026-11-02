//! Encrypted backup (PROGETTO.md §9.5): a `.orgbackup` file with this
//! device's identity, every event and every attachment, sealed with a
//! passphrase (Argon2id + XChaCha20-Poly1305). Restoring it on a new
//! computer brings back this device as it was: the old one must not be used
//! any more (revoke it if it still exists).

use core_crypto::pin::PassphraseBox;
use core_events::{Inserted, SignedEvent, StoreError};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::{ApiError, ApiResult, Core, DEVICE_SECRETS, lock, locked_error};

const MAGIC: &[u8] = b"ORGBACKUP1\n";
/// Shortest passphrase accepted.
pub const MIN_PASSPHRASE: usize = 8;

#[derive(Serialize, Deserialize)]
struct Backup {
    created_at: i64,
    signing: [u8; 32],
    exchange: [u8; 32],
    /// Events as stored (bytes of each signed event).
    events: Vec<Vec<u8>>,
    /// Attachments (already encrypted for their channel).
    blobs: Vec<([u8; 32], Vec<u8>)>,
}

impl Drop for Backup {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.signing.zeroize();
        self.exchange.zeroize();
    }
}

impl Core {
    /// The backup file, sealed with `passphrase`.
    pub fn export_backup(&self, passphrase: &str) -> ApiResult<Vec<u8>> {
        self.ensure_unlocked()?;
        if passphrase.chars().count() < MIN_PASSPHRASE {
            return Err(ApiError::invalid(format!(
                "the passphrase needs at least {MIN_PASSPHRASE} characters"
            )));
        }
        let (events, blobs) = {
            let st = lock(&self.state)?;
            let store = st.store.store().ok_or_else(locked_error)?;
            let events = store
                .all_events()
                .map_err(ApiError::internal)?
                .iter()
                .map(|e| e.to_bytes())
                .collect::<Result<Vec<_>, _>>()
                .map_err(ApiError::internal)?;
            let blobs = st
                .blobs
                .hashes()
                .into_iter()
                .filter_map(|h| st.blobs.get(&h).ok().flatten().map(|d| (h, d)))
                .collect();
            (events, blobs)
        };
        let backup = Backup {
            created_at: self.now(),
            signing: *self.keys.signing_secret(),
            exchange: *self.keys.exchange_secret(),
            events,
            blobs,
        };
        let plain = Zeroizing::new(postcard::to_allocvec(&backup).map_err(ApiError::internal)?);
        let sealed =
            PassphraseBox::seal(passphrase, &plain, self.pin_params).map_err(ApiError::internal)?;
        let mut out = MAGIC.to_vec();
        out.extend(postcard::to_allocvec(&sealed).map_err(ApiError::internal)?);
        Ok(out)
    }

    /// The backup as base64 (browser development: the page downloads it).
    pub fn export_backup_base64(&self, passphrase: &str) -> ApiResult<String> {
        Ok(data_encoding::BASE64.encode(&self.export_backup(passphrase)?))
    }

    /// Restores from base64 (file read by the page).
    pub fn restore_backup_base64(&self, data: &str, passphrase: &str) -> ApiResult<()> {
        let bytes = data_encoding::BASE64
            .decode(data.as_bytes())
            .map_err(|_| ApiError::invalid("not a backup file"))?;
        self.restore_backup(&bytes, passphrase)
    }

    /// Writes the backup to a file chosen by the person (desktop).
    pub fn export_backup_to(&self, path: &str, passphrase: &str) -> ApiResult<()> {
        let bytes = self.export_backup(passphrase)?;
        std::fs::write(path, bytes).map_err(ApiError::internal)
    }

    /// Restores a backup on a device without organization. The app must be
    /// restarted afterwards (the device identity changes).
    pub fn restore_backup(&self, bytes: &[u8], passphrase: &str) -> ApiResult<()> {
        if self.has_org() {
            return Err(ApiError::invalid("this device already has an organization"));
        }
        let body = bytes
            .strip_prefix(MAGIC)
            .ok_or_else(|| ApiError::invalid("not a backup file"))?;
        let sealed: PassphraseBox =
            postcard::from_bytes(body).map_err(|_| ApiError::invalid("damaged backup file"))?;
        let plain = sealed
            .open(passphrase)
            .map_err(|_| ApiError::new("denied", "wrong passphrase"))?;
        let backup: Backup =
            postcard::from_bytes(&plain).map_err(|_| ApiError::invalid("damaged backup file"))?;
        let mut events: Vec<SignedEvent> = backup
            .events
            .iter()
            .map(|b| SignedEvent::from_bytes(b))
            .collect::<Result<_, _>>()
            .map_err(|_| ApiError::invalid("damaged backup file"))?;
        core_events::order::sort_events(&mut events);
        {
            let mut st = lock(&self.state)?;
            let store = st.store.store_mut().ok_or_else(locked_error)?;
            for e in &events {
                match store.insert(e, self.now()) {
                    Ok(Inserted::New(_) | Inserted::Duplicate) => {}
                    Err(StoreError::Event(_)) => {}
                    Err(e) => return Err(ApiError::internal(e)),
                }
            }
            for (h, d) in &backup.blobs {
                st.blobs.put(h, d).map_err(ApiError::internal)?;
            }
        }
        // The device identity of the backup replaces the one made at start.
        self.secrets
            .put(DEVICE_SECRETS[0], &backup.signing)
            .map_err(ApiError::internal)?;
        self.secrets
            .put(DEVICE_SECRETS[1], &backup.exchange)
            .map_err(ApiError::internal)?;
        self.restart_needed
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self.invalidate();
        Ok(())
    }
}
