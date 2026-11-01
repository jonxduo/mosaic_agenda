//! Local wrapping of secrets stored in the database (e.g. channel keys),
//! with a key derived from the database key (HKDF-SHA256).

use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::channel::{ChannelKey, Sealed};
use crate::error::CryptoError;

const INFO: &[u8] = b"org-app/local-wrap/1";

pub struct LocalKey(ChannelKey);

impl LocalKey {
    pub fn derive(db_key: &[u8; 32]) -> Result<Self, CryptoError> {
        let hk = Hkdf::<Sha256>::new(None, db_key);
        let mut okm = Zeroizing::new([0u8; 32]);
        hk.expand(INFO, okm.as_mut_slice())
            .map_err(|_| CryptoError::Encrypt)?;
        Ok(Self(ChannelKey::from_bytes(*okm)))
    }

    /// Encrypts `secret`, bound to `context` (e.g. channel and epoch).
    pub fn wrap(&self, context: &[u8], secret: &[u8]) -> Result<Vec<u8>, CryptoError> {
        let sealed = self.0.encrypt(context, secret)?;
        postcard::to_allocvec(&sealed).map_err(|_| CryptoError::Encrypt)
    }

    pub fn unwrap_secret(
        &self,
        context: &[u8],
        wrapped: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, CryptoError> {
        let sealed: Sealed =
            postcard::from_bytes(wrapped).map_err(|_| CryptoError::Malformed("wrapped secret"))?;
        self.0.decrypt(context, &sealed).map(Zeroizing::new)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_round_trip_and_context_binding() {
        let k = LocalKey::derive(&[1; 32]).unwrap();
        let w = k.wrap(b"ctx", b"secret").unwrap();
        assert_eq!(k.unwrap_secret(b"ctx", &w).unwrap().as_slice(), b"secret");
        assert!(k.unwrap_secret(b"other", &w).is_err());
        let other = LocalKey::derive(&[2; 32]).unwrap();
        assert!(other.unwrap_secret(b"ctx", &w).is_err());
    }
}
