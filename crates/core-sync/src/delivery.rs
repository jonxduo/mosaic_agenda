//! Delivery status of local events (§9.3) and the outbox.

use std::collections::BTreeSet;

use core_crypto::ChannelDescriptor;
use core_engine::Projection;
use core_engine::channels::members;
use core_engine::time::date_of;
use core_events::{EventStore, PayloadChannel, SignedEvent, StoreError};
use core_model::PersonId;
use serde::Serialize;

/// Receipts needed from other persons' devices to count as sent.
pub const SENT_THRESHOLD: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Delivery {
    /// No receipt yet.
    Pending,
    /// Kept by the blind backup node (§10.3), no other person has it yet.
    Stored,
    /// Received by at least two devices of other persons.
    Sent,
    /// Received by at least one device of every person of the audience.
    Delivered,
}

/// Persons the event is meant for (the members of its channel), except the author.
pub fn audience_of(proj: &Projection, event: &SignedEvent) -> BTreeSet<PersonId> {
    let Some(cfg) = proj.cfg() else {
        return BTreeSet::new();
    };
    let desc = match event.body.channel {
        PayloadChannel::Public => ChannelDescriptor::Organization,
        PayloadChannel::Channel(c) => match proj.channels.get(&c.id) {
            Some(info) => info.descriptor.clone(),
            None => return BTreeSet::new(),
        },
    };
    let author = proj.auth.devices.get(&event.body.author).map(|d| d.person);
    members(&proj.auth, cfg, &desc, date_of(event.body.declared_time))
        .unwrap_or_default()
        .into_iter()
        .filter(|p| Some(*p) != author)
        .collect()
}

pub fn delivery(
    store: &EventStore,
    proj: &Projection,
    event: &SignedEvent,
) -> Result<Delivery, StoreError> {
    let author = proj.auth.devices.get(&event.body.author).map(|d| d.person);
    let receipts = store.receipts(&event.id)?;
    let persons: Vec<PersonId> = receipts
        .iter()
        .filter_map(|d| proj.auth.devices.get(d).map(|r| r.person))
        .filter(|p| Some(*p) != author)
        .collect();
    let audience = audience_of(proj, event);
    let reached: BTreeSet<PersonId> = persons.iter().copied().collect();
    if !audience.is_empty() && audience.is_subset(&reached) {
        return Ok(Delivery::Delivered);
    }
    let other_devices = receipts
        .iter()
        .filter(|d| {
            proj.auth
                .devices
                .get(*d)
                .is_some_and(|r| Some(r.person) != author)
        })
        .count();
    let stored = proj
        .auth
        .backup_node
        .is_some_and(|n| receipts.contains(&n));
    Ok(if other_devices >= SENT_THRESHOLD {
        Delivery::Sent
    } else if stored {
        Delivery::Stored
    } else {
        Delivery::Pending
    })
}

/// Drops from the outbox the events that reached `Sent` (§7.4: kept until
/// confirmed by two devices). Returns how many are still pending.
pub fn update_outbox(store: &mut EventStore, proj: &Projection) -> Result<usize, StoreError> {
    let mut pending = 0;
    for id in store.outbox()? {
        let Some(e) = store.get(&id)? else {
            store.outbox_remove(&id)?;
            continue;
        };
        if delivery(store, proj, &e)? >= Delivery::Sent {
            store.outbox_remove(&id)?;
        } else {
            pending += 1;
        }
    }
    Ok(pending)
}
