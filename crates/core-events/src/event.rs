//! Event envelope (PROGETTO.md §7.1).
//!
//! `id = blake3(postcard(digest of the body))`, `signature = Ed25519(author, id)`.
//! The payload is `postcard(Payload)`: a minimal clear header naming the
//! object (so devices outside the audience can relay and index the event)
//! and the content, in clear for public kinds, sealed for a channel otherwise.
//!
//! The id covers the content only through its hash: an event whose content
//! was erased (§14, erasure and retention) keeps that hash in its place and
//! stays a verifiable "shell" (id, author, kind, parents, object header).

use core_crypto::{
    ChannelKey, ChannelKeyring, ChannelRef, DeviceKeys, DevicePublicKey, Sealed, Signature, aad,
};
use core_model::{Id, Slug};
use serde::{Deserialize, Serialize};

use crate::error::EventError;
use crate::hash::{BlobRef, EventHash, OrgId};
use crate::kind::EventKind;
use crate::{MAX_EVENT_SIZE, MAX_PARENTS};

/// Channel the payload content is encrypted for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PayloadChannel {
    Public,
    Channel(ChannelRef),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EventBody {
    /// Organization (genesis: [`OrgId::GENESIS_PLACEHOLDER`]).
    pub org: OrgId,
    /// Author device (= iroh node id).
    pub author: DevicePublicKey,
    /// Graph heads known to the author, strictly increasing (1..=16; genesis: none).
    pub parents: Vec<EventHash>,
    /// 1 + max(depth of parents); genesis = 0.
    pub depth: u64,
    /// Milliseconds since the Unix epoch, informative only.
    pub declared_time: i64,
    pub kind: EventKind,
    pub channel: PayloadChannel,
    /// `postcard(Payload)`.
    pub payload: Vec<u8>,
    /// Hashes of encrypted blobs.
    pub attachments: Vec<BlobRef>,
    /// Configuration version used by the author.
    pub config_version: u32,
}

/// Kind of object an event is about, in the clear header.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub enum ObjectType {
    Organization,
    Config,
    Device,
    Channel,
    Person,
    ExternalPerson,
    Unit,
    Document,
    Batch,
    Aggregate,
    CalendarEvent,
    Program,
    Assignment,
    Resource,
    BoardPost,
}

/// Clear metadata needed to check permissions without reading the content:
/// object type, object id and, when relevant, the configured type it belongs
/// to (e.g. which form). Never field values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObjectHeader {
    pub object: ObjectType,
    pub id: Id,
    pub config: Option<Slug>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Content {
    Clear(Vec<u8>),
    Sealed(Sealed),
    /// Erased: only the hash of the original content is left.
    Erased([u8; 32]),
}

impl Content {
    /// Hash covered by the event id; the same before and after erasure.
    pub fn digest(&self) -> Result<[u8; 32], EventError> {
        let mut h = blake3::Hasher::new();
        match self {
            Self::Clear(bytes) => {
                h.update(b"org-app/content/clear");
                h.update(bytes);
            }
            Self::Sealed(sealed) => {
                h.update(b"org-app/content/sealed");
                h.update(&postcard::to_allocvec(sealed).map_err(|_| EventError::Encoding)?);
            }
            Self::Erased(digest) => return Ok(*digest),
        }
        Ok(*h.finalize().as_bytes())
    }
}

/// What the id is computed from: the body with the content replaced by its
/// hash.
#[derive(Serialize)]
struct BodyDigest<'a> {
    org: &'a OrgId,
    author: &'a DevicePublicKey,
    parents: &'a [EventHash],
    depth: u64,
    declared_time: i64,
    kind: EventKind,
    channel: &'a PayloadChannel,
    object: &'a Option<ObjectHeader>,
    content: [u8; 32],
    attachments: &'a [BlobRef],
    config_version: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    pub object: Option<ObjectHeader>,
    pub content: Content,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedEvent {
    pub id: EventHash,
    pub body: EventBody,
    pub signature: Signature,
}

fn body_hash(body: &EventBody) -> Result<EventHash, EventError> {
    let payload: Payload =
        postcard::from_bytes(&body.payload).map_err(|_| EventError::Structure("payload"))?;
    let digest = BodyDigest {
        org: &body.org,
        author: &body.author,
        parents: &body.parents,
        depth: body.depth,
        declared_time: body.declared_time,
        kind: body.kind,
        channel: &body.channel,
        object: &payload.object,
        content: payload.content.digest()?,
        attachments: &body.attachments,
        config_version: body.config_version,
    };
    let bytes = postcard::to_allocvec(&digest).map_err(|_| EventError::Encoding)?;
    Ok(EventHash(*blake3::hash(&bytes).as_bytes()))
}

impl SignedEvent {
    /// Signs a body.
    pub fn sign(body: EventBody, keys: &DeviceKeys) -> Result<Self, EventError> {
        if body.author != keys.public() {
            return Err(EventError::Structure("author is not the signing device"));
        }
        let id = body_hash(&body)?;
        let signature = keys.sign(&id.0);
        let event = Self {
            id,
            body,
            signature,
        };
        event.verify()?;
        Ok(event)
    }

    pub fn is_genesis(&self) -> bool {
        self.body.kind == EventKind::OrgGenesis
    }

    /// Organization of the event (for the genesis, its own id).
    pub fn org_id(&self) -> OrgId {
        if self.is_genesis() {
            OrgId::from(self.id)
        } else {
            self.body.org
        }
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, EventError> {
        postcard::to_allocvec(self).map_err(|_| EventError::Encoding)
    }

    /// Decodes and verifies an event received from elsewhere.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, EventError> {
        if bytes.len() > MAX_EVENT_SIZE {
            return Err(EventError::TooLarge {
                size: bytes.len(),
                max: MAX_EVENT_SIZE,
            });
        }
        let event: Self = postcard::from_bytes(bytes).map_err(|_| EventError::Encoding)?;
        event.verify()?;
        Ok(event)
    }

    /// Decodes an event read back from the local encrypted store, where it
    /// was fully verified on insert: only the id is checked again (the
    /// signature check dominates the cold start otherwise).
    pub fn from_stored_bytes(bytes: &[u8]) -> Result<Self, EventError> {
        let event: Self = postcard::from_bytes(bytes).map_err(|_| EventError::Encoding)?;
        if body_hash(&event.body)? != event.id {
            return Err(EventError::BadId);
        }
        Ok(event)
    }

    /// Self-contained checks: id, signature, structure, payload shape, size.
    /// Checks needing other events (depth, permissions) belong to the store
    /// and the engine.
    pub fn verify(&self) -> Result<(), EventError> {
        let b = &self.body;
        if body_hash(b)? != self.id {
            return Err(EventError::BadId);
        }
        b.author
            .verify(&self.id.0, &self.signature)
            .map_err(|_| EventError::BadSignature)?;
        let size = self.to_bytes()?.len();
        if size > MAX_EVENT_SIZE {
            return Err(EventError::TooLarge {
                size,
                max: MAX_EVENT_SIZE,
            });
        }
        if self.is_genesis() {
            if !b.parents.is_empty() || b.depth != 0 || b.org != OrgId::GENESIS_PLACEHOLDER {
                return Err(EventError::Structure(
                    "genesis must have no parents, depth 0 and a zero org",
                ));
            }
        } else {
            if b.parents.is_empty() || b.parents.len() > MAX_PARENTS {
                return Err(EventError::Structure("an event needs 1 to 16 parents"));
            }
            if b.parents.windows(2).any(|w| w[0] >= w[1]) {
                return Err(EventError::Structure("parents must be sorted and unique"));
            }
            if b.depth == 0 {
                return Err(EventError::Structure("only the genesis has depth 0"));
            }
            if b.org == OrgId::GENESIS_PLACEHOLDER {
                return Err(EventError::Structure("missing organization"));
            }
        }
        let payload = self.payload()?;
        match (b.kind.is_public(), b.channel, &payload.content) {
            (true, PayloadChannel::Public, Content::Clear(_) | Content::Erased(_)) => Ok(()),
            (false, PayloadChannel::Channel(_), Content::Sealed(_) | Content::Erased(_)) => Ok(()),
            (true, _, _) => Err(EventError::Structure("this kind must travel in clear")),
            (false, _, _) => Err(EventError::Structure(
                "this kind must be encrypted for a channel",
            )),
        }
    }

    /// The same event without its content (and attachments' data): same id,
    /// same signature, still verifiable.
    pub fn shell(&self) -> Result<Self, EventError> {
        let mut payload = self.payload()?;
        payload.content = Content::Erased(payload.content.digest()?);
        let mut body = self.body.clone();
        body.payload = postcard::to_allocvec(&payload).map_err(|_| EventError::Encoding)?;
        Ok(Self {
            id: self.id,
            body,
            signature: self.signature,
        })
    }

    pub fn is_erased(&self) -> bool {
        self.payload()
            .is_ok_and(|p| matches!(p.content, Content::Erased(_)))
    }

    pub fn payload(&self) -> Result<Payload, EventError> {
        postcard::from_bytes(&self.body.payload).map_err(|_| EventError::Structure("payload"))
    }

    /// Content in clear: as is for public events, decrypted otherwise.
    pub fn open(&self, keyring: &ChannelKeyring) -> Result<Vec<u8>, EventError> {
        match (self.payload()?.content, self.body.channel) {
            (Content::Clear(bytes), _) => Ok(bytes),
            (Content::Sealed(sealed), PayloadChannel::Channel(channel)) => {
                let a = aad(&self.org_id().0, &channel, self.body.kind.as_str());
                Ok(keyring.decrypt(&channel, &a, &sealed)?)
            }
            (Content::Sealed(_), PayloadChannel::Public) => Err(EventError::Structure(
                "sealed content on the public channel",
            )),
            (Content::Erased(_), _) => Err(EventError::Erased),
        }
    }

    /// Decodes the clear content of a public event as `T`.
    pub fn public_content<T: serde::de::DeserializeOwned>(&self) -> Result<T, EventError> {
        match self.payload()?.content {
            Content::Clear(bytes) => {
                postcard::from_bytes(&bytes).map_err(|_| EventError::Structure("content"))
            }
            Content::Sealed(_) => Err(EventError::Structure("content is encrypted")),
            Content::Erased(_) => Err(EventError::Erased),
        }
    }
}

/// Everything about a new event except its content.
#[derive(Debug, Clone)]
pub struct Draft {
    pub org: OrgId,
    pub kind: EventKind,
    pub parents: Vec<EventHash>,
    pub depth: u64,
    pub declared_time: i64,
    pub object: Option<ObjectHeader>,
    pub attachments: Vec<BlobRef>,
    pub config_version: u32,
}

impl Draft {
    /// A draft on top of the given heads `(id, depth)`.
    pub fn after(
        org: OrgId,
        kind: EventKind,
        heads: &[(EventHash, u64)],
        declared_time: i64,
    ) -> Self {
        let mut parents: Vec<EventHash> = heads.iter().map(|(h, _)| *h).collect();
        parents.sort_unstable();
        parents.dedup();
        let depth = heads.iter().map(|(_, d)| *d).max().map_or(0, |d| d + 1);
        Self {
            org,
            kind,
            parents,
            depth,
            declared_time,
            object: None,
            attachments: Vec::new(),
            config_version: 1,
        }
    }

    pub fn object(mut self, header: ObjectHeader) -> Self {
        self.object = Some(header);
        self
    }

    fn body(
        self,
        author: DevicePublicKey,
        channel: PayloadChannel,
        content: Content,
    ) -> Result<EventBody, EventError> {
        let payload = postcard::to_allocvec(&Payload {
            object: self.object,
            content,
        })
        .map_err(|_| EventError::Encoding)?;
        Ok(EventBody {
            org: self.org,
            author,
            parents: self.parents,
            depth: self.depth,
            declared_time: self.declared_time,
            kind: self.kind,
            channel,
            payload,
            attachments: self.attachments,
            config_version: self.config_version,
        })
    }

    /// Signs a public event with `content` serialized in clear.
    pub fn sign_public<T: Serialize>(
        self,
        content: &T,
        keys: &DeviceKeys,
    ) -> Result<SignedEvent, EventError> {
        let bytes = postcard::to_allocvec(content).map_err(|_| EventError::Encoding)?;
        let body = self.body(keys.public(), PayloadChannel::Public, Content::Clear(bytes))?;
        SignedEvent::sign(body, keys)
    }

    /// Signs an event whose content is encrypted for `channel` with `key`.
    pub fn sign_sealed(
        self,
        plaintext: &[u8],
        channel: ChannelRef,
        key: &ChannelKey,
        keys: &DeviceKeys,
    ) -> Result<SignedEvent, EventError> {
        let a = aad(&self.org.0, &channel, self.kind.as_str());
        let sealed = key.encrypt(&a, plaintext)?;
        let body = self.body(
            keys.public(),
            PayloadChannel::Channel(channel),
            Content::Sealed(sealed),
        )?;
        SignedEvent::sign(body, keys)
    }
}
