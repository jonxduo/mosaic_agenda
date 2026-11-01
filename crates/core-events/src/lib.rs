//! Signed, immutable events and their local store (PROGETTO.md §7).
//!
//! - [`event`]: envelope, signature, id, payload (clear or sealed for a channel).
//! - [`kind`]: closed set of event kinds and which of them travel in clear.
//! - [`payloads`]: payloads of the events defined so far (genesis, devices, keys).
//! - [`genesis`]: creation and checks of the first event of an organization.
//! - [`order`]: the deterministic total order used to apply events.
//! - [`store`]: SQLite store encrypted with SQLCipher.

pub mod error;
pub mod event;
pub mod genesis;
pub mod hash;
pub mod kind;
pub mod order;
pub mod payloads;
pub mod store;

pub use error::EventError;
pub use event::{Content, Draft, ObjectHeader, ObjectType, Payload, PayloadChannel, SignedEvent};
pub use hash::{BlobRef, EventHash, OrgId};
pub use kind::EventKind;
pub use store::{DbKey, EventStatus, EventStore, Inserted, StoreError};

/// Hard limit for a serialized event (§7.7).
pub const MAX_EVENT_SIZE: usize = 64 * 1024;
/// Maximum number of parents of an event.
pub const MAX_PARENTS: usize = 16;
