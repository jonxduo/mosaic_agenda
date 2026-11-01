//! Payloads of the public events defined in phase F2. Payloads of the other
//! kinds are defined with the engine (F3).

use core_crypto::{ChannelDescriptor, ChannelRef, DeviceIdentity, DevicePublicKey};
use core_model::{Id, PersonId};
use serde::{Deserialize, Serialize};

/// A root administrator declared at genesis (no personal data in clear:
/// names arrive later in encrypted `person.added`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootAdmin {
    pub person: PersonId,
    pub devices: Vec<DeviceIdentity>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NetworkParams {
    /// iroh relay URLs; empty = library defaults (development).
    pub relay_urls: Vec<String>,
    /// pkarr/DNS discovery URL.
    pub dns_url: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecurityParams {
    /// Default auto-lock after inactivity, in minutes (1..=30).
    pub auto_lock_minutes: u8,
    /// Wipe local data after this many failed PIN attempts (None = never).
    pub wipe_after_failed_pins: Option<u8>,
}

impl Default for SecurityParams {
    fn default() -> Self {
        Self {
            auto_lock_minutes: 5,
            wipe_after_failed_pins: None,
        }
    }
}

/// `org.genesis` (§7.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GenesisPayload {
    /// Organization name (shown at join time; keep it non-identifying if needed).
    pub name: String,
    pub root_admins: Vec<RootAdmin>,
    /// Compiled configuration, version 1 (`core-config` bundle bytes).
    pub config_bundle: Vec<u8>,
    /// BLAKE3 of `config_bundle`.
    pub config_hash: [u8; 32],
    pub network: NetworkParams,
    pub security: SecurityParams,
    /// Random bytes so that two organizations never share an id.
    pub nonce: [u8; 16],
}

/// `device.invited` (§9.6).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceInvited {
    pub invite: Id,
    pub person: PersonId,
    /// Milliseconds since the Unix epoch.
    pub expires_at: i64,
}

/// `device.added`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceAdded {
    pub person: PersonId,
    pub device: DeviceIdentity,
    pub invite: Option<Id>,
}

/// `device.revoked`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceRevoked {
    pub device: DevicePublicKey,
}

/// One device's copy of a channel key, sealed to its X25519 key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SealedKeyEntry {
    pub device: DevicePublicKey,
    pub sealed_key: Vec<u8>,
}

/// `channel.keys_distributed` (new members of an epoch) and
/// `channel.rotated` (first distribution of a new epoch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeyDistribution {
    /// What the channel is for: lets every device recompute its expected
    /// members and check that `channel.id` derives from it.
    pub descriptor: ChannelDescriptor,
    pub channel: ChannelRef,
    pub entries: Vec<SealedKeyEntry>,
}
