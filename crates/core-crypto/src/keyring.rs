//! Channel keys by epoch, and the deterministic rotation rules (PROGETTO.md §8.4).
//!
//! - A new epoch is needed when a member leaves the channel's audience.
//! - Concurrent rotations to the same epoch: every key is kept (payloads
//!   already encrypted with any of them stay readable), and new payloads use
//!   the key introduced by the event with the smallest id.
//! - Members who leave keep what they already received.

use std::collections::{BTreeMap, BTreeSet};

use crate::channel::{ChannelId, ChannelKey, ChannelRef, Sealed};
use crate::error::CryptoError;

/// Id of the event that introduced a key (an event hash, compared bytewise).
pub type SourceId = [u8; 32];

#[derive(Debug, Clone, Default)]
pub struct ChannelKeyring {
    keys: BTreeMap<(ChannelId, u32), BTreeMap<SourceId, ChannelKey>>,
}

impl ChannelKeyring {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds a key; idempotent for the same source.
    pub fn insert(&mut self, channel: ChannelRef, source: SourceId, key: ChannelKey) {
        self.keys
            .entry((channel.id, channel.epoch))
            .or_default()
            .insert(source, key);
    }

    pub fn contains(&self, channel: &ChannelRef) -> bool {
        self.keys.contains_key(&(channel.id, channel.epoch))
    }

    /// The key to encrypt new payloads with: highest epoch, and within it the
    /// key from the smallest source id.
    pub fn current(&self, channel: &ChannelId) -> Option<(ChannelRef, &ChannelKey)> {
        self.keys
            .range((*channel, 0)..=(*channel, u32::MAX))
            .next_back()
            .and_then(|((id, epoch), by_source)| {
                by_source.values().next().map(|k| {
                    (
                        ChannelRef {
                            id: *id,
                            epoch: *epoch,
                        },
                        k,
                    )
                })
            })
    }

    /// Next epoch number for a rotation of `channel`.
    pub fn next_epoch(&self, channel: &ChannelId) -> u32 {
        self.current(channel)
            .map_or(0, |(r, _)| r.epoch.saturating_add(1))
    }

    /// Decrypts with any key known for that channel epoch.
    pub fn decrypt(
        &self,
        channel: &ChannelRef,
        aad: &[u8],
        sealed: &Sealed,
    ) -> Result<Vec<u8>, CryptoError> {
        let by_source = self
            .keys
            .get(&(channel.id, channel.epoch))
            .ok_or(CryptoError::MissingKey)?;
        by_source
            .values()
            .find_map(|k| k.decrypt(aad, sealed).ok())
            .ok_or(CryptoError::Decrypt)
    }

    /// Every (channel, epoch, source, key), in a stable order.
    pub fn entries(&self) -> impl Iterator<Item = (ChannelRef, &SourceId, &ChannelKey)> {
        self.keys.iter().flat_map(|((id, epoch), by_source)| {
            by_source.iter().map(move |(s, k)| {
                (
                    ChannelRef {
                        id: *id,
                        epoch: *epoch,
                    },
                    s,
                    k,
                )
            })
        })
    }
}

/// True when someone who was a member is no longer expected: rotate.
pub fn needs_rotation<T: Ord>(previous: &BTreeSet<T>, expected: &BTreeSet<T>) -> bool {
    previous.iter().any(|m| !expected.contains(m))
}

/// Expected recipients that have not received the key yet (to complete).
pub fn missing_recipients<T: Ord + Clone>(
    expected: &BTreeSet<T>,
    distributed: &BTreeSet<T>,
) -> BTreeSet<T> {
    expected.difference(distributed).cloned().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::channel::{ChannelDescriptor, aad};

    fn chan() -> ChannelId {
        ChannelId::derive(&[1; 32], &ChannelDescriptor::Admin).unwrap()
    }

    #[test]
    fn current_is_highest_epoch_then_smallest_source() {
        let mut ring = ChannelKeyring::new();
        assert!(ring.current(&chan()).is_none());
        assert_eq!(ring.next_epoch(&chan()), 0);
        let k0 = ChannelKey::from_bytes([10; 32]);
        let k1a = ChannelKey::from_bytes([11; 32]);
        let k1b = ChannelKey::from_bytes([12; 32]);
        ring.insert(
            ChannelRef {
                id: chan(),
                epoch: 0,
            },
            [5; 32],
            k0,
        );
        ring.insert(
            ChannelRef {
                id: chan(),
                epoch: 1,
            },
            [9; 32],
            k1b.clone(),
        );
        ring.insert(
            ChannelRef {
                id: chan(),
                epoch: 1,
            },
            [2; 32],
            k1a.clone(),
        );
        let (r, k) = ring.current(&chan()).unwrap();
        assert_eq!(r.epoch, 1);
        assert_eq!(
            k, &k1a,
            "smallest source id wins among concurrent rotations"
        );
        assert_eq!(ring.next_epoch(&chan()), 2);
        // A different channel is unaffected.
        let other = ChannelId::derive(&[2; 32], &ChannelDescriptor::Admin).unwrap();
        assert!(ring.current(&other).is_none());
    }

    #[test]
    fn payloads_from_losing_rotation_stay_readable() {
        let mut ring = ChannelKeyring::new();
        let r1 = ChannelRef {
            id: chan(),
            epoch: 1,
        };
        let winner = ChannelKey::from_bytes([1; 32]);
        let loser = ChannelKey::from_bytes([2; 32]);
        ring.insert(r1, [1; 32], winner);
        ring.insert(r1, [7; 32], loser.clone());
        let a = aad(&[0; 32], &r1, "k");
        let sealed = loser.encrypt(&a, b"written with the losing key").unwrap();
        assert_eq!(
            ring.decrypt(&r1, &a, &sealed).unwrap(),
            b"written with the losing key"
        );
        let missing = ChannelRef {
            id: chan(),
            epoch: 9,
        };
        assert_eq!(
            ring.decrypt(&missing, &a, &sealed),
            Err(CryptoError::MissingKey)
        );
    }

    #[test]
    fn rotation_and_completion_rules() {
        let prev: BTreeSet<u8> = [1, 2, 3].into();
        assert!(
            !needs_rotation(&prev, &[1, 2, 3, 4].into()),
            "joining needs no rotation"
        );
        assert!(
            needs_rotation(&prev, &[1, 3].into()),
            "leaving needs a rotation"
        );
        assert_eq!(missing_recipients(&[1, 2, 3, 4].into(), &prev), [4].into());
    }
}
