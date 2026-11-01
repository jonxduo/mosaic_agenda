//! Device keys, channel encryption, key distribution and rotation
//! (PROGETTO.md §8).
//!
//! - [`device`]: Ed25519 signing key (also the iroh node identity) and a
//!   separate X25519 key to receive channel keys.
//! - [`channel`]: channel ids, symmetric keys (XChaCha20-Poly1305) and the
//!   associated data binding a ciphertext to org, channel, epoch and event kind.
//! - [`distribution`]: channel keys sealed to a device's X25519 key.
//! - [`keyring`]: keys by channel and epoch, and the deterministic rules for
//!   concurrent rotations.
//! - [`secret_store`]: where device secrets live.

pub mod bytes;
pub mod channel;
pub mod device;
pub mod distribution;
pub mod error;
pub mod keyring;
pub mod local;
pub mod secret_store;

pub use channel::{
    ChannelDescriptor, ChannelId, ChannelKey, ChannelRef, ChannelScope, Sealed, aad,
};
pub use device::{DeviceIdentity, DeviceKeys, DevicePublicKey, ExchangePublicKey, Signature};
pub use error::CryptoError;
pub use keyring::ChannelKeyring;
pub use secret_store::{MemorySecretStore, SecretStore, SecretStoreError};

/// Fills a fresh array with OS randomness.
pub fn random_array<const N: usize>() -> Result<[u8; N], CryptoError> {
    let mut out = [0u8; N];
    getrandom::fill(&mut out).map_err(|_| CryptoError::Random)?;
    Ok(out)
}
