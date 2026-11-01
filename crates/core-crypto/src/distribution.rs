//! Channel keys sealed to a device's X25519 key (PROGETTO.md §8.3).
//!
//! The sealed plaintext is `channel id || epoch (LE) || key`, so a sealed key
//! cannot be replayed as the key of another channel or epoch.

use crypto_box::aead::OsRng;
use crypto_box::{PublicKey, SecretKey};
use zeroize::Zeroizing;

use crate::channel::{ChannelKey, ChannelRef};
use crate::device::{DeviceKeys, ExchangePublicKey};
use crate::error::CryptoError;

const PLAIN_LEN: usize = 32 + 4 + 32;

/// Seals `key` (for `channel`) so that only `recipient` can open it.
pub fn seal_channel_key(
    recipient: &ExchangePublicKey,
    channel: &ChannelRef,
    key: &ChannelKey,
) -> Result<Vec<u8>, CryptoError> {
    let mut plain = Zeroizing::new(Vec::with_capacity(PLAIN_LEN));
    plain.extend_from_slice(&channel.id.0);
    plain.extend_from_slice(&channel.epoch.to_le_bytes());
    plain.extend_from_slice(key.as_bytes());
    PublicKey::from(recipient.0)
        .seal(&mut OsRng, &plain)
        .map_err(|_| CryptoError::Encrypt)
}

/// Opens a key sealed to this device, checking it belongs to `channel`.
pub fn open_channel_key(
    keys: &DeviceKeys,
    channel: &ChannelRef,
    sealed: &[u8],
) -> Result<ChannelKey, CryptoError> {
    let secret = SecretKey::from(*keys.exchange_secret());
    let plain = Zeroizing::new(secret.unseal(sealed).map_err(|_| CryptoError::Decrypt)?);
    if plain.len() != PLAIN_LEN {
        return Err(CryptoError::Malformed("sealed channel key"));
    }
    if plain[..32] != channel.id.0 || plain[32..36] != channel.epoch.to_le_bytes() {
        return Err(CryptoError::WrongChannel);
    }
    let mut key = [0u8; 32];
    key.copy_from_slice(&plain[36..]);
    Ok(ChannelKey::from_bytes(key))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{ChannelDescriptor, ChannelId};

    fn channel(epoch: u32) -> ChannelRef {
        ChannelRef {
            id: ChannelId::derive(&[3; 32], &ChannelDescriptor::Admin).unwrap(),
            epoch,
        }
    }

    #[test]
    fn only_the_recipient_opens_the_key() {
        let alice = DeviceKeys::generate().unwrap();
        let bob = DeviceKeys::generate().unwrap();
        let key = ChannelKey::generate().unwrap();
        let sealed = seal_channel_key(&alice.exchange_public(), &channel(1), &key).unwrap();
        assert_eq!(open_channel_key(&alice, &channel(1), &sealed).unwrap(), key);
        assert_eq!(
            open_channel_key(&bob, &channel(1), &sealed),
            Err(CryptoError::Decrypt)
        );
    }

    #[test]
    fn sealed_key_is_bound_to_channel_and_epoch() {
        let alice = DeviceKeys::generate().unwrap();
        let key = ChannelKey::generate().unwrap();
        let sealed = seal_channel_key(&alice.exchange_public(), &channel(1), &key).unwrap();
        assert_eq!(
            open_channel_key(&alice, &channel(2), &sealed),
            Err(CryptoError::WrongChannel)
        );
        let other = ChannelRef {
            id: ChannelId::derive(&[4; 32], &ChannelDescriptor::Admin).unwrap(),
            epoch: 1,
        };
        assert_eq!(
            open_channel_key(&alice, &other, &sealed),
            Err(CryptoError::WrongChannel)
        );
    }

    #[test]
    fn tampered_sealed_key_fails() {
        let alice = DeviceKeys::generate().unwrap();
        let key = ChannelKey::generate().unwrap();
        let mut sealed = seal_channel_key(&alice.exchange_public(), &channel(1), &key).unwrap();
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(open_channel_key(&alice, &channel(1), &sealed).is_err());
    }
}
