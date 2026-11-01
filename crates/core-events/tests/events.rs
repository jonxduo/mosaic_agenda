//! Envelope, genesis, encryption and key distribution/rotation.

#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::collections::BTreeSet;

use common::{child, genesis, org_with, person};
use core_crypto::distribution::{open_channel_key, seal_channel_key};
use core_crypto::keyring::needs_rotation;
use core_crypto::{
    ChannelDescriptor, ChannelId, ChannelKey, ChannelKeyring, ChannelRef, CryptoError, DeviceKeys,
};
use core_events::genesis::{check_genesis, create_genesis};
use core_events::payloads::{KeyDistribution, SealedKeyEntry};
use core_events::{
    Draft, EventError, EventKind, MAX_EVENT_SIZE, ObjectHeader, ObjectType, OrgId, SignedEvent,
};
use core_model::{Id, Slug};

#[test]
fn genesis_is_self_identifying_and_checked() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    assert_eq!(org, OrgId::from(g.id));
    assert_eq!(g.org_id(), org);
    let info = check_genesis(&g).unwrap();
    assert_eq!(
        info.warnings.len(),
        1,
        "a single root admin is allowed with a warning"
    );
    let back = SignedEvent::from_bytes(&g.to_bytes().unwrap()).unwrap();
    assert_eq!(back, g);
    // Two organizations created alike still differ (random nonce).
    let (org2, _) = genesis(&admin);
    assert_ne!(org, org2);
}

#[test]
fn genesis_needs_a_root_admin_author() {
    let admin = DeviceKeys::generate().unwrap();
    let stranger = DeviceKeys::generate().unwrap();
    let err = create_genesis(org_with(&[(&admin, 1)]), &stranger, 0).unwrap_err();
    assert_eq!(
        err,
        EventError::Genesis("the author must be a root administrator's device")
    );
    let two = DeviceKeys::generate().unwrap();
    let (_, g) = create_genesis(org_with(&[(&admin, 1), (&two, 2)]), &two, 0).unwrap();
    assert!(check_genesis(&g).unwrap().warnings.is_empty());
}

#[test]
fn tampering_is_detected() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let e = child(&admin, org, &[(g.id, 0)], 1);
    let mut bad = e.clone();
    bad.body.declared_time += 1;
    assert_eq!(bad.verify(), Err(EventError::BadId));
    let mut bad = e.clone();
    bad.signature.0[0] ^= 1;
    assert_eq!(bad.verify(), Err(EventError::BadSignature));
    // Re-hashed by someone else but signature still from the author.
    let other = DeviceKeys::generate().unwrap();
    let mut bad = e.clone();
    bad.body.author = other.public();
    let forged = SignedEvent::sign(bad.body.clone(), &other).unwrap();
    assert_ne!(forged.id, e.id, "changing the author changes the id");
    // Reading back from the local store checks the id too.
    let mut bad = e.clone();
    bad.body.declared_time += 1;
    let bytes = bad.to_bytes().unwrap();
    assert_eq!(
        SignedEvent::from_stored_bytes(&bytes).unwrap_err(),
        EventError::BadId
    );
    assert_eq!(
        SignedEvent::from_stored_bytes(&e.to_bytes().unwrap()).unwrap(),
        e
    );
}

#[test]
fn structure_rules() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let a = child(&admin, org, &[(g.id, 0)], 1);
    let b = child(&admin, org, &[(g.id, 0)], 2);
    // Unsorted parents.
    let mut body = child(&admin, org, &[(a.id, 1), (b.id, 1)], 3).body;
    body.parents.reverse();
    assert_eq!(
        SignedEvent::sign(body, &admin).unwrap_err(),
        EventError::Structure("parents must be sorted and unique")
    );
    // No parents for a non-genesis event.
    let mut body = a.body.clone();
    body.parents.clear();
    assert!(matches!(
        SignedEvent::sign(body, &admin),
        Err(EventError::Structure(_))
    ));
    // Signing a body authored by another device.
    let other = DeviceKeys::generate().unwrap();
    assert!(SignedEvent::sign(a.body.clone(), &other).is_err());
}

#[test]
fn public_and_private_kinds_are_enforced() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let err = Draft::after(org, EventKind::DocumentCreated, &[(g.id, 0)], 1)
        .sign_public(&1u8, &admin)
        .unwrap_err();
    assert_eq!(
        err,
        EventError::Structure("this kind must be encrypted for a channel")
    );
    let channel = ChannelRef {
        id: ChannelId::derive(&org.0, &ChannelDescriptor::Admin).unwrap(),
        epoch: 0,
    };
    let key = ChannelKey::generate().unwrap();
    let err = Draft::after(org, EventKind::DeviceAdded, &[(g.id, 0)], 1)
        .sign_sealed(b"x", channel, &key, &admin)
        .unwrap_err();
    assert_eq!(err, EventError::Structure("this kind must travel in clear"));
}

#[test]
fn size_limit() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let big = vec![7u8; MAX_EVENT_SIZE];
    let err = Draft::after(org, EventKind::OrgSettingsChanged, &[(g.id, 0)], 1)
        .sign_public(&big, &admin)
        .unwrap_err();
    assert!(matches!(err, EventError::TooLarge { .. }));
    assert!(matches!(
        SignedEvent::from_bytes(&vec![0u8; MAX_EVENT_SIZE + 1]),
        Err(EventError::TooLarge { .. })
    ));
}

#[test]
fn sealed_content_with_clear_object_header() {
    let admin = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let channel = ChannelRef {
        id: ChannelId::derive(&org.0, &ChannelDescriptor::Admin).unwrap(),
        epoch: 0,
    };
    let key = ChannelKey::generate().unwrap();
    let header = ObjectHeader {
        object: ObjectType::Document,
        id: Id::from_bytes([3; 16]),
        config: Some(Slug::new("log").unwrap()),
    };
    let e = Draft::after(org, EventKind::DocumentCreated, &[(g.id, 0)], 1)
        .object(header.clone())
        .sign_sealed(b"field values", channel, &key, &admin)
        .unwrap();
    // Anyone can read the header (to relay and index), not the content.
    assert_eq!(e.payload().unwrap().object, Some(header));
    assert!(
        !e.to_bytes()
            .unwrap()
            .windows(12)
            .any(|w| w == b"field values")
    );
    let mut ring = ChannelKeyring::new();
    assert_eq!(
        e.open(&ring),
        Err(EventError::Crypto(CryptoError::MissingKey))
    );
    ring.insert(channel, [0; 32], key);
    assert_eq!(e.open(&ring).unwrap(), b"field values");
}

fn distribute(
    org: OrgId,
    kind: EventKind,
    parent: &SignedEvent,
    author: &DeviceKeys,
    channel: ChannelRef,
    key: &ChannelKey,
    to: &[&DeviceKeys],
) -> SignedEvent {
    let payload = KeyDistribution {
        descriptor: ChannelDescriptor::Admin,
        channel,
        entries: to
            .iter()
            .map(|d| SealedKeyEntry {
                device: d.public(),
                sealed_key: seal_channel_key(&d.exchange_public(), &channel, key).unwrap(),
            })
            .collect(),
    };
    Draft::after(
        org,
        kind,
        &[(parent.id, parent.body.depth)],
        parent.body.declared_time + 1,
    )
    .sign_public(&payload, author)
    .unwrap()
}

/// What a device learns from a distribution event: its own key, if any.
fn learn(ring: &mut ChannelKeyring, me: &DeviceKeys, event: &SignedEvent) {
    let d: KeyDistribution = event.public_content().unwrap();
    if let Some(entry) = d.entries.iter().find(|e| e.device == me.public()) {
        let key = open_channel_key(me, &d.channel, &entry.sealed_key).unwrap();
        ring.insert(d.channel, event.id.0, key);
    }
}

#[test]
fn distribution_and_rotation_after_a_member_leaves() {
    let admin = DeviceKeys::generate().unwrap();
    let b = DeviceKeys::generate().unwrap();
    let c = DeviceKeys::generate().unwrap();
    let (org, g) = genesis(&admin);
    let id = ChannelId::derive(&org.0, &ChannelDescriptor::Admin).unwrap();
    let e0 = ChannelRef { id, epoch: 0 };
    let k0 = ChannelKey::generate().unwrap();
    let dist0 = distribute(
        org,
        EventKind::ChannelKeysDistributed,
        &g,
        &admin,
        e0,
        &k0,
        &[&admin, &b, &c],
    );
    let (mut ring_b, mut ring_c) = (ChannelKeyring::new(), ChannelKeyring::new());
    learn(&mut ring_b, &b, &dist0);
    learn(&mut ring_c, &c, &dist0);
    let before = Draft::after(
        org,
        EventKind::DocumentCreated,
        &[(dist0.id, dist0.body.depth)],
        10,
    )
    .sign_sealed(b"before", e0, &k0, &admin)
    .unwrap();
    assert_eq!(before.open(&ring_b).unwrap(), b"before");
    assert_eq!(before.open(&ring_c).unwrap(), b"before");

    // c leaves the audience: rotate to epoch 1, distributed only to admin and b.
    let previous: BTreeSet<_> = [admin.public(), b.public(), c.public()].into();
    let expected: BTreeSet<_> = [admin.public(), b.public()].into();
    assert!(needs_rotation(&previous, &expected));
    let e1 = ChannelRef { id, epoch: 1 };
    let k1 = ChannelKey::generate().unwrap();
    let rot = distribute(
        org,
        EventKind::ChannelRotated,
        &before,
        &admin,
        e1,
        &k1,
        &[&admin, &b],
    );
    learn(&mut ring_b, &b, &rot);
    learn(&mut ring_c, &c, &rot);
    let (current, _) = ring_b.current(&id).unwrap();
    assert_eq!(current.epoch, 1);
    let after = Draft::after(
        org,
        EventKind::DocumentCreated,
        &[(rot.id, rot.body.depth)],
        20,
    )
    .sign_sealed(b"after", e1, &k1, &admin)
    .unwrap();
    assert_eq!(after.open(&ring_b).unwrap(), b"after");
    assert_eq!(
        after.open(&ring_c),
        Err(EventError::Crypto(CryptoError::MissingKey))
    );
    // c keeps what it already received.
    assert_eq!(before.open(&ring_c).unwrap(), b"before");
    // c cannot reuse a sealed entry meant for b.
    let d: KeyDistribution = rot.public_content().unwrap();
    assert!(open_channel_key(&c, &e1, &d.entries[1].sealed_key).is_err());
    let _ = person(0);
}
