use std::fmt;

use serde::{Deserialize, Serialize};

fn hex(bytes: &[u8], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for b in bytes {
        write!(f, "{b:02x}")?;
    }
    Ok(())
}

/// `blake3(postcard(EventBody))`. Ordered bytewise.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct EventHash(pub [u8; 32]);

impl fmt::Display for EventHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        hex(&self.0, f)
    }
}

impl fmt::Debug for EventHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Event(")?;
        hex(&self.0[..6], f)?;
        write!(f, ")")
    }
}

/// Identifier of an organization: the id of its genesis event.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct OrgId(pub [u8; 32]);

impl OrgId {
    /// Value of `EventBody::org` inside the genesis itself (its id is not known yet).
    pub const GENESIS_PLACEHOLDER: OrgId = OrgId([0; 32]);
}

impl From<EventHash> for OrgId {
    fn from(h: EventHash) -> Self {
        Self(h.0)
    }
}

impl fmt::Display for OrgId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        hex(&self.0, f)
    }
}

impl fmt::Debug for OrgId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Org(")?;
        hex(&self.0[..6], f)?;
        write!(f, ")")
    }
}

/// Hash of an encrypted attachment blob (iroh-blobs content hash).
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize)]
pub struct BlobRef(pub [u8; 32]);
