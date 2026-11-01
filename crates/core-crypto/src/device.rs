//! Device keys (PROGETTO.md §8.1).
//!
//! The Ed25519 key signs events and is the device's iroh identity
//! (`NodeId` = its public key). The X25519 key only receives channel keys.

use std::fmt;

use ed25519_dalek::{Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey as XPublic, StaticSecret};
use zeroize::Zeroizing;

use crate::error::CryptoError;
use crate::random_array;
use crate::secret_store::{SecretStore, SecretStoreError};

/// Ed25519 public key of a device (= iroh node id).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct DevicePublicKey(pub [u8; 32]);

impl DevicePublicKey {
    pub fn verify(&self, message: &[u8], signature: &Signature) -> Result<(), CryptoError> {
        let key = VerifyingKey::from_bytes(&self.0).map_err(|_| CryptoError::InvalidPublicKey)?;
        let sig = ed25519_dalek::Signature::from_bytes(&signature.0);
        key.verify_strict(message, &sig)
            .map_err(|_| CryptoError::BadSignature)
    }
}

impl fmt::Display for DevicePublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for b in self.0 {
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for DevicePublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Device({})", &self.to_string()[..12])
    }
}

/// X25519 public key a device receives channel keys on.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct ExchangePublicKey(pub [u8; 32]);

/// Ed25519 signature.
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature(#[serde(with = "crate::bytes")] pub [u8; 64]);

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Signature({:02x}{:02x}..)", self.0[0], self.0[1])
    }
}

/// Public identity of a device, as published in `device.added`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct DeviceIdentity {
    pub signing: DevicePublicKey,
    pub exchange: ExchangePublicKey,
}

/// Secret keys of this device.
pub struct DeviceKeys {
    signing: SigningKey,
    exchange: StaticSecret,
}

const STORE_SIGNING: &str = "device.signing_key";
const STORE_EXCHANGE: &str = "device.exchange_key";

impl DeviceKeys {
    /// Fresh random keys.
    pub fn generate() -> Result<Self, CryptoError> {
        let s = Zeroizing::new(random_array::<32>()?);
        let x = Zeroizing::new(random_array::<32>()?);
        Ok(Self::from_secret_bytes(&s, &x))
    }

    /// Keys from their 32-byte secrets (Ed25519 seed, X25519 scalar).
    pub fn from_secret_bytes(signing: &[u8; 32], exchange: &[u8; 32]) -> Self {
        Self {
            signing: SigningKey::from_bytes(signing),
            exchange: StaticSecret::from(*exchange),
        }
    }

    /// Loads the keys from the store, creating and saving them on first use.
    pub fn load_or_create(store: &dyn SecretStore) -> Result<Self, SecretStoreError> {
        let signing = store.get(STORE_SIGNING)?;
        let exchange = store.get(STORE_EXCHANGE)?;
        match (signing, exchange) {
            (Some(s), Some(x)) => {
                let s: [u8; 32] = s
                    .as_slice()
                    .try_into()
                    .map_err(|_| SecretStoreError::Corrupt)?;
                let x: [u8; 32] = x
                    .as_slice()
                    .try_into()
                    .map_err(|_| SecretStoreError::Corrupt)?;
                Ok(Self::from_secret_bytes(
                    &Zeroizing::new(s),
                    &Zeroizing::new(x),
                ))
            }
            (None, None) => {
                let keys = Self::generate().map_err(|_| SecretStoreError::Unavailable)?;
                store.put(STORE_SIGNING, &keys.signing.to_bytes())?;
                store.put(STORE_EXCHANGE, keys.exchange.as_bytes())?;
                Ok(keys)
            }
            _ => Err(SecretStoreError::Corrupt),
        }
    }

    pub fn public(&self) -> DevicePublicKey {
        DevicePublicKey(self.signing.verifying_key().to_bytes())
    }

    pub fn exchange_public(&self) -> ExchangePublicKey {
        ExchangePublicKey(XPublic::from(&self.exchange).to_bytes())
    }

    pub fn identity(&self) -> DeviceIdentity {
        DeviceIdentity {
            signing: self.public(),
            exchange: self.exchange_public(),
        }
    }

    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature(self.signing.sign(message).to_bytes())
    }

    /// Ed25519 seed, e.g. to build the iroh `SecretKey` of the endpoint.
    pub fn signing_secret(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.signing.to_bytes())
    }

    pub(crate) fn exchange_secret(&self) -> Zeroizing<[u8; 32]> {
        Zeroizing::new(self.exchange.to_bytes())
    }
}

impl fmt::Debug for DeviceKeys {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "DeviceKeys({:?})", self.public())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secret_store::MemorySecretStore;

    fn h<const N: usize>(s: &str) -> [u8; N] {
        hex::decode(s).unwrap().try_into().unwrap()
    }

    /// RFC 8032 §7.1, test 1 (empty message).
    #[test]
    fn ed25519_rfc8032_vector() {
        let keys = DeviceKeys::from_secret_bytes(
            &h("9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60"),
            &[0; 32],
        );
        assert_eq!(
            keys.public().0,
            h::<32>("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a")
        );
        let sig = keys.sign(b"");
        assert_eq!(
            sig.0,
            h::<64>(
                "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b"
            )
        );
        assert!(keys.public().verify(b"", &sig).is_ok());
    }

    /// RFC 7748 §6.1 (Alice's key pair).
    #[test]
    fn x25519_rfc7748_vector() {
        let keys = DeviceKeys::from_secret_bytes(
            &[0; 32],
            &h("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a"),
        );
        assert_eq!(
            keys.exchange_public().0,
            h::<32>("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a")
        );
    }

    #[test]
    fn tampered_message_or_signature_fails() {
        let keys = DeviceKeys::generate().unwrap();
        let mut sig = keys.sign(b"event id");
        assert!(keys.public().verify(b"event id", &sig).is_ok());
        assert_eq!(
            keys.public().verify(b"event iD", &sig),
            Err(CryptoError::BadSignature)
        );
        sig.0[10] ^= 1;
        assert_eq!(
            keys.public().verify(b"event id", &sig),
            Err(CryptoError::BadSignature)
        );
        let other = DeviceKeys::generate().unwrap();
        assert!(
            other
                .public()
                .verify(b"event id", &keys.sign(b"event id"))
                .is_err()
        );
    }

    #[test]
    fn load_or_create_is_stable() {
        let store = MemorySecretStore::default();
        let a = DeviceKeys::load_or_create(&store).unwrap();
        let b = DeviceKeys::load_or_create(&store).unwrap();
        assert_eq!(a.identity(), b.identity());
        let other = MemorySecretStore::default();
        assert_ne!(
            DeviceKeys::load_or_create(&other).unwrap().identity(),
            a.identity()
        );
    }

    #[test]
    fn signature_serde_round_trip() {
        let keys = DeviceKeys::generate().unwrap();
        let sig = keys.sign(b"x");
        let bytes = postcard::to_allocvec(&sig).unwrap();
        assert_eq!(bytes.len(), 65, "length prefix + 64 bytes");
        assert_eq!(postcard::from_bytes::<Signature>(&bytes).unwrap(), sig);
    }
}
