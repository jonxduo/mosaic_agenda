//! Whom to contact and when (§9.2).

use std::collections::BTreeSet;

use core_crypto::DevicePublicKey;
use core_engine::Projection;

pub const MAX_PEERS_MOBILE: usize = 8;
pub const MAX_PEERS_DESKTOP: usize = 32;
/// Period of the foreground sync, in seconds.
pub const FOREGROUND_PERIOD_S: u64 = 60;
/// After creating events, how long to try direct delivery before asking for push wake-ups.
pub const DIRECT_DELIVERY_S: u64 = 10;
/// At most one wake-up request per recipient in this window.
pub const WAKE_COALESCE_S: u64 = 120;

/// Why a sync starts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    AppStart,
    Foreground,
    Push,
    Periodic,
    Background,
    Announcement,
}

/// Peers to contact, in priority order: the blind backup node (§10.3),
/// archive devices, devices of the recipients of our pending events, then
/// the others (rotated by `seed` so
/// that load spreads over the devices).
pub fn peers(
    proj: &Projection,
    me: &DevicePublicKey,
    outbox_targets: &BTreeSet<DevicePublicKey>,
    mobile: bool,
    seed: u64,
) -> Vec<DevicePublicKey> {
    let limit = if mobile {
        MAX_PEERS_MOBILE
    } else {
        MAX_PEERS_DESKTOP
    };
    let candidates: Vec<(&DevicePublicKey, bool)> = proj
        .auth
        .devices
        .iter()
        .filter(|(k, d)| *k != me && !d.revoked)
        .map(|(k, d)| (k, d.archive))
        .collect();
    let mut out: Vec<DevicePublicKey> = proj
        .auth
        .backup_node
        .filter(|n| n != me)
        .into_iter()
        .collect();
    out.extend(candidates.iter().filter(|(_, a)| *a).map(|(k, _)| **k));
    for (k, _) in &candidates {
        if outbox_targets.contains(*k) && !out.contains(*k) {
            out.push(**k);
        }
    }
    let mut rest: Vec<DevicePublicKey> = candidates
        .iter()
        .map(|(k, _)| **k)
        .filter(|k| !out.contains(k))
        .collect();
    if !rest.is_empty() {
        let shift = (seed as usize) % rest.len();
        rest.rotate_left(shift);
    }
    out.extend(rest);
    out.truncate(limit);
    out
}

/// Devices that should receive the pending events: the audiences' devices.
pub fn outbox_targets(
    proj: &Projection,
    events: &[core_events::SignedEvent],
) -> BTreeSet<DevicePublicKey> {
    let mut persons = BTreeSet::new();
    for e in events {
        persons.extend(crate::delivery::audience_of(proj, e));
    }
    proj.auth
        .devices
        .iter()
        .filter(|(_, d)| !d.revoked && persons.contains(&d.person))
        .map(|(k, _)| *k)
        .collect()
}
