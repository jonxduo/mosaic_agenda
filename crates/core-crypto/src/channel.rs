//! Channels: who a payload is encrypted for (PROGETTO.md §8.2).

use std::fmt;

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use core_model::{PersonId, Slug, UnitId};
use serde::{Deserialize, Serialize};
use zeroize::Zeroizing;

use crate::error::CryptoError;
use crate::random_array;

/// Scope of an audience channel.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ChannelScope {
    Organization,
    Unit(UnitId),
}

/// What a channel is for. Its canonical form (postcard) determines the id.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ChannelDescriptor {
    /// A configured audience in a scope.
    Audience { audience: Slug, scope: ChannelScope },
    /// Personal documents: read audience plus the subject, one channel per person.
    Subject {
        audience: Slug,
        subject: PersonId,
        scope: ChannelScope,
    },
    /// Program drafts: owner and assistants.
    Program { program_type: Slug, stage: Slug },
    /// An individual assignment: owners of the program type plus the assignee.
    Assignment {
        program_type: Slug,
        person: PersonId,
    },
    /// Resources and their attachments, optionally for the current holder.
    Resource {
        resource_type: Slug,
        holder: Option<PersonId>,
    },
    /// Administrators.
    Admin,
    /// Every member of the organization (organizational events: persons,
    /// qualifications, units, roles). Appended last: positions are part of ids.
    Organization,
    /// Persons holding `permission` (dotted name) on `object` (`kind:id`),
    /// e.g. the drafts of an aggregation before it is distributed.
    PermissionHolders { permission: String, object: String },
}

/// `blake3("channel" || org || canonical descriptor)`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub [u8; 32]);

impl ChannelId {
    pub fn derive(org: &[u8; 32], descriptor: &ChannelDescriptor) -> Result<Self, CryptoError> {
        let canonical = postcard::to_allocvec(descriptor)
            .map_err(|_| CryptoError::Malformed("channel descriptor"))?;
        let mut h = blake3::Hasher::new();
        h.update(b"channel");
        h.update(org);
        h.update(&canonical);
        Ok(Self(*h.finalize().as_bytes()))
    }
}

impl fmt::Debug for ChannelId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Channel(")?;
        for b in &self.0[..6] {
            write!(f, "{b:02x}")?;
        }
        write!(f, ")")
    }
}

/// A channel at a key epoch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChannelRef {
    pub id: ChannelId,
    pub epoch: u32,
}

/// Associated data of a payload: `org || channel || epoch (LE) || kind`.
pub fn aad(org: &[u8; 32], channel: &ChannelRef, kind: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 + 32 + 4 + kind.len());
    out.extend_from_slice(org);
    out.extend_from_slice(&channel.id.0);
    out.extend_from_slice(&channel.epoch.to_le_bytes());
    out.extend_from_slice(kind.as_bytes());
    out
}

/// A ciphertext with its random nonce.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sealed {
    pub nonce: [u8; 24],
    pub ciphertext: Vec<u8>,
}

/// Symmetric key of a channel epoch (XChaCha20-Poly1305).
#[derive(Clone)]
pub struct ChannelKey(Zeroizing<[u8; 32]>);

impl ChannelKey {
    pub fn generate() -> Result<Self, CryptoError> {
        Ok(Self(Zeroizing::new(random_array::<32>()?)))
    }

    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(Zeroizing::new(bytes))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn encrypt(&self, aad: &[u8], plaintext: &[u8]) -> Result<Sealed, CryptoError> {
        self.encrypt_with_nonce(random_array::<24>()?, aad, plaintext)
    }

    pub(crate) fn encrypt_with_nonce(
        &self,
        nonce: [u8; 24],
        aad: &[u8],
        plaintext: &[u8],
    ) -> Result<Sealed, CryptoError> {
        let cipher = XChaCha20Poly1305::new_from_slice(self.0.as_slice())
            .map_err(|_| CryptoError::Encrypt)?;
        let ciphertext = cipher
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::Encrypt)?;
        Ok(Sealed { nonce, ciphertext })
    }

    pub fn decrypt(&self, aad: &[u8], sealed: &Sealed) -> Result<Vec<u8>, CryptoError> {
        let cipher = XChaCha20Poly1305::new_from_slice(self.0.as_slice())
            .map_err(|_| CryptoError::Decrypt)?;
        cipher
            .decrypt(
                XNonce::from_slice(&sealed.nonce),
                Payload {
                    msg: &sealed.ciphertext,
                    aad,
                },
            )
            .map_err(|_| CryptoError::Decrypt)
    }
}

impl fmt::Debug for ChannelKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ChannelKey(..)")
    }
}

impl PartialEq for ChannelKey {
    fn eq(&self, other: &Self) -> bool {
        // Not constant-time; only used in tests and deduplication of known keys.
        self.0.as_slice() == other.0.as_slice()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_model::Id;

    fn h(s: &str) -> Vec<u8> {
        hex::decode(s).unwrap()
    }

    /// draft-irtf-cfrg-xchacha-03, appendix A.3.1.
    #[test]
    fn xchacha20poly1305_reference_vector() {
        let key = ChannelKey::from_bytes(
            h("808182838485868788898a8b8c8d8e8f909192939495969798999a9b9c9d9e9f")
                .try_into()
                .unwrap(),
        );
        let nonce: [u8; 24] = h("404142434445464748494a4b4c4d4e4f5051525354555657")
            .try_into()
            .unwrap();
        let aad = h("50515253c0c1c2c3c4c5c6c7");
        let plaintext = b"Ladies and Gentlemen of the class of '99: If I could offer you only one tip for the future, sunscreen would be it.";
        let sealed = key.encrypt_with_nonce(nonce, &aad, plaintext).unwrap();
        let expected = [
            "bd6d179d3e83d43b9576579493c0e939572a1700252bfaccbed2902c21396cbb",
            "731c7f1b0b4aa6440bf3a82f4eda7e39ae64c6708c54c216cb96b72e1213b452",
            "2f8c9ba40db5d945b11b69b982c1bb9e3f3fac2bc369488f76b2383565d3fff9",
            "21f9664c97637da9768812f615c68b13b52e",
            "c0875924c1c7987947deafd8780acf49",
        ]
        .concat();
        assert_eq!(hex::encode(&sealed.ciphertext), expected);
        assert_eq!(key.decrypt(&aad, &sealed).unwrap(), plaintext);
    }

    fn sample_ref() -> ChannelRef {
        ChannelRef {
            id: ChannelId::derive(&[9; 32], &ChannelDescriptor::Admin).unwrap(),
            epoch: 3,
        }
    }

    #[test]
    fn altered_aad_or_ciphertext_is_rejected() {
        let key = ChannelKey::generate().unwrap();
        let r = sample_ref();
        let a = aad(&[9; 32], &r, "document.created");
        let sealed = key.encrypt(&a, b"secret").unwrap();
        assert_eq!(key.decrypt(&a, &sealed).unwrap(), b"secret");
        // Same channel, other kind.
        let other_kind = aad(&[9; 32], &r, "document.updated");
        assert_eq!(key.decrypt(&other_kind, &sealed), Err(CryptoError::Decrypt));
        // Other epoch.
        let other_epoch = aad(&[9; 32], &ChannelRef { epoch: 4, ..r }, "document.created");
        assert_eq!(
            key.decrypt(&other_epoch, &sealed),
            Err(CryptoError::Decrypt)
        );
        // Other organization.
        let other_org = aad(&[8; 32], &r, "document.created");
        assert_eq!(key.decrypt(&other_org, &sealed), Err(CryptoError::Decrypt));
        // Flipped bit.
        let mut bad = sealed.clone();
        bad.ciphertext[0] ^= 1;
        assert_eq!(key.decrypt(&a, &bad), Err(CryptoError::Decrypt));
        // Wrong key.
        let other = ChannelKey::generate().unwrap();
        assert_eq!(other.decrypt(&a, &sealed), Err(CryptoError::Decrypt));
    }

    #[test]
    fn nonces_are_fresh() {
        let key = ChannelKey::generate().unwrap();
        let a = key.encrypt(b"", b"x").unwrap();
        let b = key.encrypt(b"", b"x").unwrap();
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    /// Regression vector for our own derivations: changing them would make
    /// every existing channel unreachable.
    #[test]
    fn channel_id_and_aad_vectors() {
        let person = PersonId(Id::from_bytes([1; 16]));
        let id = ChannelId::derive(
            &[7; 32],
            &ChannelDescriptor::Subject {
                audience: Slug::new("readers").unwrap(),
                subject: person,
                scope: ChannelScope::Organization,
            },
        )
        .unwrap();
        assert_eq!(hex::encode(id.0), CHANNEL_ID_VECTOR);
        let r = ChannelRef { id, epoch: 2 };
        let a = aad(&[7; 32], &r, "k");
        assert_eq!(a.len(), 32 + 32 + 4 + 1);
        assert_eq!(&a[64..68], &[2, 0, 0, 0]);
        let key = ChannelKey::from_bytes([5; 32]);
        let sealed = key.encrypt_with_nonce([6; 24], &a, b"payload").unwrap();
        assert_eq!(hex::encode(&sealed.ciphertext), SEALED_VECTOR);
    }

    const CHANNEL_ID_VECTOR: &str =
        "5240f900afbdde8c89cc533f66dea68931a29695feb0afc4ee3babf0b2f1e1bb";
    const SEALED_VECTOR: &str = "b0301b5e5e3d872336bea99f72603dd1934fd30ceaec15";

    #[test]
    fn distinct_descriptors_give_distinct_ids() {
        let org = [1; 32];
        let a = ChannelId::derive(&org, &ChannelDescriptor::Admin).unwrap();
        let b = ChannelId::derive(
            &org,
            &ChannelDescriptor::Assignment {
                program_type: Slug::new("plan").unwrap(),
                person: PersonId(Id::from_bytes([0; 16])),
            },
        )
        .unwrap();
        let c = ChannelId::derive(&[2; 32], &ChannelDescriptor::Admin).unwrap();
        assert_ne!(a, b);
        assert_ne!(a, c);
    }
}
