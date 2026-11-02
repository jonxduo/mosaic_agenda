//! PIN protection of the local database (PROGETTO.md §8.6).
//!
//! - [`PinWrap`]: the database key encrypted with a key derived from the PIN
//!   (Argon2id) and a random *pepper* kept in the system keychain. Copying
//!   the data folder is not enough to try PINs: the pepper is needed too.
//! - [`InboxKey`]: an X25519 key derived from the database key. Its public
//!   half stays readable while the app is locked, so events received in the
//!   meantime are sealed to it and can only be read after unlocking.

use argon2::{Algorithm, Argon2, Params, Version};
use crypto_box::aead::OsRng;
use crypto_box::{PublicKey, SecretKey};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::channel::{ChannelKey, Sealed};
use crate::error::CryptoError;

const WRAP_CONTEXT: &[u8] = b"org-app/pin-wrap/1";
const INBOX_CONTEXT: &str = "org-app/inbox/1";

/// Argon2id cost.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinParams {
    pub memory_kib: u32,
    pub iterations: u32,
    pub lanes: u32,
}

impl Default for PinParams {
    /// 64 MiB, 3 passes: a few hundred ms on a laptop, about a second on a
    /// mid-range phone.
    fn default() -> Self {
        Self {
            memory_kib: 64 * 1024,
            iterations: 3,
            lanes: 1,
        }
    }
}

impl PinParams {
    /// Cheap parameters, for tests only.
    pub fn insecure_for_tests() -> Self {
        Self {
            memory_kib: 64,
            iterations: 1,
            lanes: 1,
        }
    }
}

/// A 32-byte secret encrypted with the PIN.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PinWrap {
    pub params: PinParams,
    pub salt: [u8; 16],
    pub sealed: Sealed,
}

fn derive(
    pin: &str,
    pepper: &[u8; 32],
    salt: &[u8; 16],
    params: PinParams,
) -> Result<ChannelKey, CryptoError> {
    let p = Params::new(params.memory_kib, params.iterations, params.lanes, Some(32))
        .map_err(|_| CryptoError::Malformed("PIN parameters"))?;
    let argon = Argon2::new_with_secret(pepper, Algorithm::Argon2id, Version::V0x13, p)
        .map_err(|_| CryptoError::Malformed("PIN parameters"))?;
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(pin.as_bytes(), salt, out.as_mut_slice())
        .map_err(|_| CryptoError::Encrypt)?;
    Ok(ChannelKey::from_bytes(*out))
}

impl PinWrap {
    pub fn seal(
        pin: &str,
        pepper: &[u8; 32],
        secret: &[u8; 32],
        params: PinParams,
    ) -> Result<Self, CryptoError> {
        let salt = crate::random_array::<16>()?;
        let key = derive(pin, pepper, &salt, params)?;
        let sealed = key.encrypt(WRAP_CONTEXT, secret)?;
        Ok(Self {
            params,
            salt,
            sealed,
        })
    }

    /// The secret, or [`CryptoError::Decrypt`] for a wrong PIN.
    pub fn open(&self, pin: &str, pepper: &[u8; 32]) -> Result<Zeroizing<[u8; 32]>, CryptoError> {
        let key = derive(pin, pepper, &self.salt, self.params)?;
        let plain = Zeroizing::new(key.decrypt(WRAP_CONTEXT, &self.sealed)?);
        let mut out = Zeroizing::new([0u8; 32]);
        if plain.len() != 32 {
            return Err(CryptoError::Malformed("wrapped secret"));
        }
        out.copy_from_slice(&plain);
        Ok(out)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, CryptoError> {
        postcard::to_allocvec(self).map_err(|_| CryptoError::Encrypt)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        postcard::from_bytes(bytes).map_err(|_| CryptoError::Malformed("PIN wrap"))
    }
}

/// Data encrypted with a passphrase (backups, §9.5): Argon2id without
/// pepper (the file must open on another computer) + XChaCha20-Poly1305.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassphraseBox {
    pub params: PinParams,
    pub salt: [u8; 16],
    pub sealed: Sealed,
}

const BOX_CONTEXT: &[u8] = b"org-app/backup/1";

fn passphrase_key(
    pass: &str,
    salt: &[u8; 16],
    params: PinParams,
) -> Result<ChannelKey, CryptoError> {
    let p = Params::new(params.memory_kib, params.iterations, params.lanes, Some(32))
        .map_err(|_| CryptoError::Malformed("backup parameters"))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, p);
    let mut out = Zeroizing::new([0u8; 32]);
    argon
        .hash_password_into(pass.as_bytes(), salt, out.as_mut_slice())
        .map_err(|_| CryptoError::Encrypt)?;
    Ok(ChannelKey::from_bytes(*out))
}

impl PassphraseBox {
    pub fn seal(pass: &str, data: &[u8], params: PinParams) -> Result<Self, CryptoError> {
        let salt = crate::random_array::<16>()?;
        let sealed = passphrase_key(pass, &salt, params)?.encrypt(BOX_CONTEXT, data)?;
        Ok(Self {
            params,
            salt,
            sealed,
        })
    }

    /// The data, or [`CryptoError::Decrypt`] for a wrong passphrase.
    pub fn open(&self, pass: &str) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        let key = passphrase_key(pass, &self.salt, self.params)?;
        key.decrypt(BOX_CONTEXT, &self.sealed).map(Zeroizing::new)
    }
}

/// Key pair for the locked inbox, derived from the database key.
pub struct InboxKey(SecretKey);

/// Public half of [`InboxKey`], kept readable while locked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxPublicKey(pub [u8; 32]);

impl InboxKey {
    pub fn derive(db_key: &[u8; 32]) -> Self {
        let bytes = Zeroizing::new(blake3::derive_key(INBOX_CONTEXT, db_key));
        Self(SecretKey::from(*bytes))
    }

    pub fn public(&self) -> InboxPublicKey {
        InboxPublicKey(*self.0.public_key().as_bytes())
    }

    pub fn open(&self, sealed: &[u8]) -> Result<Vec<u8>, CryptoError> {
        self.0.unseal(sealed).map_err(|_| CryptoError::Decrypt)
    }
}

impl InboxPublicKey {
    pub fn seal(&self, plain: &[u8]) -> Result<Vec<u8>, CryptoError> {
        PublicKey::from(self.0)
            .seal(&mut OsRng, plain)
            .map_err(|_| CryptoError::Encrypt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pin_wrap_round_trip() {
        let pepper = [7u8; 32];
        let secret = [9u8; 32];
        let w = PinWrap::seal("2468", &pepper, &secret, PinParams::insecure_for_tests()).unwrap();
        let back = PinWrap::from_bytes(&w.to_bytes().unwrap()).unwrap();
        assert_eq!(*back.open("2468", &pepper).unwrap(), secret);
        assert_eq!(
            back.open("2469", &pepper).unwrap_err(),
            CryptoError::Decrypt
        );
        // Without the pepper the PIN alone does not open it.
        assert_eq!(
            back.open("2468", &[8u8; 32]).unwrap_err(),
            CryptoError::Decrypt
        );
    }

    #[test]
    fn default_parameters_are_accepted() {
        let w = PinWrap::seal("123456", &[1; 32], &[2; 32], PinParams::default()).unwrap();
        assert_eq!(*w.open("123456", &[1; 32]).unwrap(), [2; 32]);
    }

    #[test]
    fn passphrase_box_round_trip() {
        let b = PassphraseBox::seal("correct horse", b"backup", PinParams::insecure_for_tests())
            .unwrap();
        assert_eq!(b.open("correct horse").unwrap().as_slice(), b"backup");
        assert_eq!(b.open("wrong").unwrap_err(), CryptoError::Decrypt);
    }

    #[test]
    fn inbox_seal_and_open() {
        let key = InboxKey::derive(&[5u8; 32]);
        let sealed = key.public().seal(b"event bytes").unwrap();
        assert_eq!(key.open(&sealed).unwrap(), b"event bytes");
        assert!(InboxKey::derive(&[6u8; 32]).open(&sealed).is_err());
    }
}
