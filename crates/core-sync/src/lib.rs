//! Synchronization between devices (PROGETTO.md §9).
//!
//! - [`protocol`]: reconciliation of two event stores, transport independent;
//! - [`receipt`] and [`delivery`]: signed receipts and the delivery status;
//! - [`pairing`]: tickets and verification codes for new devices;
//! - [`blobs`]: photo preparation, encryption and blob storage;
//! - [`schedule`]: whom to contact and when;
//! - [`net`]: the iroh transport (QUIC, relay, gossip announcements).

pub mod blobs;
pub mod delivery;
pub mod net;
pub mod pairing;
pub mod protocol;
pub mod receipt;
pub mod schedule;

pub use protocol::{Msg, Peer, Session, SessionStats, SyncError, sync_in_memory};
