//! Encrypted store: persistence, graph bookkeeping and convergence.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{child, genesis};
use core_crypto::{ChannelDescriptor, ChannelId, ChannelKey, ChannelRef, DeviceKeys};
use core_events::{
    DbKey, Draft, EventKind, EventStatus, EventStore, Inserted, OrgId, SignedEvent, StoreError,
};
use proptest::prelude::*;

fn store() -> EventStore {
    EventStore::open_in_memory(&DbKey::generate().unwrap()).unwrap()
}

#[test]
fn file_is_encrypted_and_needs_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let key = [42u8; 32];
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    {
        let mut s = EventStore::open(&path, &DbKey::from_bytes(key)).unwrap();
        assert_eq!(s.insert(&g, 1).unwrap(), Inserted::New(EventStatus::Valid));
    }
    let bytes = std::fs::read(&path).unwrap();
    assert!(
        !bytes.starts_with(b"SQLite format 3"),
        "the file must not be a plain SQLite database"
    );
    assert!(matches!(
        EventStore::open(&path, &DbKey::from_bytes([1; 32])),
        Err(StoreError::WrongKey)
    ));
    let s = EventStore::open(&path, &DbKey::from_bytes(key)).unwrap();
    assert_eq!(s.org(), Some(org));
    assert_eq!(s.get(&g.id).unwrap(), Some(g));
}

#[test]
fn erased_events_keep_their_place() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let key = DbKey::from_bytes([7; 32]);
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let a = child(&admin, org, &[(g.id, 0)], 1);
    let b = child(&admin, org, &[(a.id, 1)], 2);
    let mut s = EventStore::open(&path, &key).unwrap();
    for e in [&g, &a, &b] {
        s.insert(e, 0).unwrap();
    }
    assert!(s.erase(&a.id).unwrap());
    assert!(!s.erase(&a.id).unwrap(), "already erased");
    let stored = s.get(&a.id).unwrap().unwrap();
    assert!(stored.is_erased());
    assert_eq!(stored, a.shell().unwrap());
    // The full event arriving again from a peer is a duplicate: it stays erased.
    assert_eq!(s.insert(&a, 0).unwrap(), Inserted::Duplicate);
    assert!(s.get(&a.id).unwrap().unwrap().is_erased());
    assert_eq!(s.heads().unwrap(), vec![b.id]);
    drop(s);
    // A new store receiving the shell before its child accepts both.
    let mut t = store();
    t.insert(&g, 0).unwrap();
    assert_eq!(
        t.insert(&stored, 0).unwrap(),
        Inserted::New(EventStatus::Valid)
    );
    assert_eq!(t.insert(&b, 0).unwrap(), Inserted::New(EventStatus::Valid));
    let s = EventStore::open(&path, &key).unwrap();
    assert!(s.get(&a.id).unwrap().unwrap().is_erased(), "persisted");
}

#[test]
fn out_of_order_arrival_waits_for_parents() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let a = child(&admin, org, &[(g.id, 0)], 1);
    let b = child(&admin, org, &[(a.id, 1)], 2);
    let mut s = store();
    s.insert(&g, 0).unwrap();
    assert_eq!(
        s.insert(&b, 0).unwrap(),
        Inserted::New(EventStatus::PendingParents)
    );
    assert_eq!(s.missing().unwrap(), vec![a.id]);
    assert_eq!(s.heads().unwrap(), vec![g.id]);
    assert_eq!(s.insert(&a, 0).unwrap(), Inserted::New(EventStatus::Valid));
    assert_eq!(s.status(&b.id).unwrap(), Some(EventStatus::Valid));
    assert_eq!(s.heads().unwrap(), vec![b.id]);
    assert!(s.missing().unwrap().is_empty());
    assert_eq!(s.insert(&a, 0).unwrap(), Inserted::Duplicate);
    assert_eq!(
        s.ordered_ids(&[EventStatus::Valid]).unwrap(),
        vec![g.id, a.id, b.id]
    );
    assert!(s.is_ancestor(&g.id, &b.id).unwrap());
    assert!(!s.is_ancestor(&b.id, &a.id).unwrap());
    assert_eq!(s.ancestors(&b.id).unwrap(), [g.id, a.id].into());
    assert_eq!(s.parents_for_new_event().unwrap(), vec![(b.id, 2)]);
}

#[test]
fn concurrent_branches_give_two_heads() {
    let admin = DeviceKeys::generate().unwrap();
    let other = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let a = child(&admin, org, &[(g.id, 0)], 1);
    let b = child(&other, org, &[(g.id, 0)], 1);
    let mut s = store();
    for e in [&g, &a, &b] {
        s.insert(e, 0).unwrap();
    }
    let mut expected = vec![a.id, b.id];
    expected.sort();
    assert_eq!(s.heads().unwrap(), expected);
    let m = Draft::after(
        org,
        EventKind::OrgSettingsChanged,
        &s.parents_for_new_event().unwrap(),
        2,
    )
    .sign_public(&2i64, &admin)
    .unwrap();
    assert_eq!(m.body.parents.len(), 2);
    assert_eq!(m.body.depth, 2);
    s.insert(&m, 0).unwrap();
    assert_eq!(s.heads().unwrap(), vec![m.id]);
}

#[test]
fn wrong_depth_is_marked_invalid() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let mut d = Draft::after(org, EventKind::OrgSettingsChanged, &[(g.id, 0)], 1);
    d.depth = 5;
    let bad = d.sign_public(&1i64, &admin).unwrap();
    let mut s = store();
    s.insert(&g, 0).unwrap();
    assert_eq!(
        s.insert(&bad, 0).unwrap(),
        Inserted::New(EventStatus::Invalid)
    );
    assert!(s.valid_events().unwrap().iter().all(|e| e.id != bad.id));
}

#[test]
fn organization_binding() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let (_, other_g) = genesis(&admin);
    let a = child(&admin, org, &[(g.id, 0)], 1);
    let mut s = store();
    assert!(matches!(s.insert(&a, 0), Err(StoreError::Unbound)));
    s.bind_org(org).unwrap();
    assert_eq!(
        s.insert(&a, 0).unwrap(),
        Inserted::New(EventStatus::PendingParents)
    );
    assert!(matches!(s.insert(&other_g, 0), Err(StoreError::WrongOrg)));
    assert_eq!(s.insert(&g, 0).unwrap(), Inserted::New(EventStatus::Valid));
    assert_eq!(s.status(&a.id).unwrap(), Some(EventStatus::Valid));
    assert!(matches!(
        s.bind_org(OrgId([9; 32])),
        Err(StoreError::WrongOrg)
    ));
}

#[test]
fn engine_statuses() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let a = child(&admin, org, &[(g.id, 0)], 1);
    let mut s = store();
    s.insert(&g, 0).unwrap();
    s.insert(&a, 0).unwrap();
    s.set_status(&a.id, EventStatus::Quarantined).unwrap();
    assert_eq!(s.status(&a.id).unwrap(), Some(EventStatus::Quarantined));
    assert!(s.set_status(&a.id, EventStatus::PendingParents).is_err());
    assert_eq!(
        s.ordered_ids(&[EventStatus::Quarantined]).unwrap(),
        vec![a.id]
    );
}

#[test]
fn channel_keys_outbox_and_receipts_persist() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("events.db");
    let key = DbKey::from_bytes([3; 32]);
    let admin = DeviceKeys::generate().unwrap();
    let peer = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let channel = ChannelRef {
        id: ChannelId::derive(&org.0, &ChannelDescriptor::Admin).unwrap(),
        epoch: 0,
    };
    let ck = ChannelKey::generate().unwrap();
    {
        let mut s = EventStore::open(&path, &key).unwrap();
        s.insert(&g, 0).unwrap();
        s.put_channel_key(channel, g.id, &ck).unwrap();
        s.put_channel_key(channel, g.id, &ck).unwrap();
        s.outbox_add(&g.id, 5).unwrap();
        s.add_receipt(&g.id, &peer.public(), 6).unwrap();
        s.add_receipt(&g.id, &peer.public(), 7).unwrap();
    }
    let mut s = EventStore::open(&path, &key).unwrap();
    let ring = s.keyring().unwrap();
    assert_eq!(ring.current(&channel.id).unwrap().1, &ck);
    assert_eq!(s.outbox().unwrap(), vec![g.id]);
    assert_eq!(s.receipts(&g.id).unwrap(), [peer.public()].into());
    s.outbox_remove(&g.id).unwrap();
    assert!(s.outbox().unwrap().is_empty());
}

/// A random graph: each event picks 1–3 earlier events as parents.
fn build_graph(spec: &[(u8, Vec<usize>)]) -> Vec<SignedEvent> {
    let devices: Vec<DeviceKeys> = (0..3).map(|_| DeviceKeys::generate().unwrap()).collect();
    let (org, g) = genesis(&devices[0]);
    let mut events = vec![g];
    for (i, (dev, picks)) in spec.iter().enumerate() {
        let mut parents: Vec<_> = picks
            .iter()
            .map(|p| {
                let e = &events[p % events.len()];
                (e.id, e.body.depth)
            })
            .collect();
        parents.sort();
        parents.dedup();
        let e = child(&devices[usize::from(*dev) % 3], org, &parents, i as i64 + 1);
        events.push(e);
    }
    events
}

fn insert_in_order(events: &[SignedEvent], order: &[u64]) -> EventStore {
    let mut idx: Vec<usize> = (0..events.len()).collect();
    idx.sort_by_key(|i| (order[*i % order.len()], *i));
    let mut s = store();
    // The genesis binds the store; the rest arrive in any order.
    s.insert(&events[0], 0).unwrap();
    for i in idx {
        s.insert(&events[i], 0).unwrap();
    }
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    /// Whatever the arrival order, every store ends with the same events,
    /// statuses, heads and application order.
    #[test]
    fn stores_converge_regardless_of_arrival_order(
        spec in prop::collection::vec((0u8..3, prop::collection::vec(any::<usize>(), 1..4)), 1..30),
        order_a in prop::collection::vec(any::<u64>(), 1..40),
        order_b in prop::collection::vec(any::<u64>(), 1..40),
    ) {
        let events = build_graph(&spec);
        let a = insert_in_order(&events, &order_a);
        let b = insert_in_order(&events, &order_b);
        let all = [EventStatus::Valid, EventStatus::PendingParents, EventStatus::Invalid];
        prop_assert_eq!(a.ordered_ids(&all).unwrap(), b.ordered_ids(&all).unwrap());
        prop_assert_eq!(a.heads().unwrap(), b.heads().unwrap());
        prop_assert_eq!(a.count().unwrap(), events.len() as u64);
        prop_assert_eq!(a.ordered_ids(&[EventStatus::Valid]).unwrap().len(), events.len());
        prop_assert!(a.missing().unwrap().is_empty());
        // The application order is topological: parents always come first.
        let order = a.ordered_ids(&[EventStatus::Valid]).unwrap();
        let pos = |h| order.iter().position(|x| *x == h).unwrap();
        for e in &events {
            for p in &e.body.parents {
                prop_assert!(pos(*p) < pos(e.id));
            }
        }
    }
}
