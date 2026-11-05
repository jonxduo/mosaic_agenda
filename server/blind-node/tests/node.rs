//! F11: the blind backup node keeps what one device leaves and hands it to
//! another that never met the first, without any channel key.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use blind_node::{BlindNode, Config, attachments, authorized};
use core_crypto::DeviceKeys;
use core_engine::channels::derive_keyring;
use core_engine::ops::Values;
use core_engine::rebuild;
use core_engine::time::{PeriodKey, parse_date};
use core_events::{DbKey, EventStore, SignedEvent};
use core_model::{Scalar, Slug};
use core_sync::blobs::MemBlobStore;
use core_sync::local::LocalLog;
use core_sync::net::{AlwaysConfirm, Network, Node, NodeState};
use core_sync::pairing::Invitations;
use core_testkit::{ADMIN, World};

fn slug(s: &str) -> Slug {
    Slug::new(s).unwrap()
}

fn club_bundle() -> Vec<u8> {
    let dir =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../crates/core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

/// An organization with an admin and two members who sent a log each.
fn world() -> World {
    let mut w = World::new(club_bundle(), parse_date("2026-11-02").unwrap()).unwrap();
    let team = w
        .act_with(ADMIN, |c| c.create_unit(&slug("team"), "t1"))
        .unwrap();
    for name in ["a", "b"] {
        let p = w
            .act_with(ADMIN, |c| c.add_person(name, Values::new()))
            .unwrap();
        w.persons.insert(name.to_owned(), p);
        w.act_with(ADMIN, |c| {
            c.grant_qualification(p, &slug("member"), parse_date("2020-01-01").unwrap(), None)
        })
        .unwrap();
        w.act_with(ADMIN, |c| {
            c.add_membership(p, team, parse_date("2020-01-01").unwrap())
        })
        .unwrap();
        w.add_device(name).unwrap();
    }
    for name in ["a", "b"] {
        let p = w.person(name).unwrap();
        let v: Values = BTreeMap::from([(slug("done"), Scalar::Bool(true))]);
        let doc = w
            .act_with(name, |c| {
                c.fill_document(&slug("log"), p, PeriodKey::month(2026, 10), v)
            })
            .unwrap();
        w.act(name, |c| c.step_document(doc, &slug("send"), None))
            .unwrap();
    }
    w
}

fn store_with(events: &[SignedEvent]) -> LocalLog {
    let mut s = EventStore::open_in_memory(&DbKey::generate().unwrap()).unwrap();
    for e in events.iter().filter(|e| e.is_genesis()).chain(events) {
        s.insert(e, 0).unwrap();
    }
    LocalLog::Open(s)
}

async fn device(store: LocalLog) -> Arc<Node> {
    let state = NodeState {
        store,
        blobs: Box::new(MemBlobStore::default()),
        wanted_blobs: BTreeSet::new(),
        allowed: None,
        invitations: Invitations::default(),
        paired: Vec::new(),
    };
    Arc::new(
        Node::spawn(
            Arc::new(DeviceKeys::generate().unwrap()),
            Arc::new(std::sync::Mutex::new(state)),
            Network::Local,
            Arc::new(AlwaysConfirm),
        )
        .await
        .unwrap(),
    )
}

fn count(n: &Node) -> u64 {
    n.state
        .lock()
        .unwrap()
        .store
        .store()
        .unwrap()
        .count()
        .unwrap()
}

#[test]
fn allowed_devices_from_public_events_match_the_projection() {
    let w = world();
    let keys = &w.device("a", None).unwrap().keys;
    let proj = rebuild(&w.events, &derive_keyring(&w.events, keys)).unwrap();
    let exact: BTreeSet<_> = proj
        .auth
        .devices
        .iter()
        .filter(|(_, d)| !d.revoked)
        .map(|(k, _)| *k)
        .collect();
    assert_eq!(authorized(&w.events), Some(exact));
    assert_eq!(authorized(&w.events[1..]), None, "no genesis yet");
    assert!(attachments(&w.events).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn devices_never_online_together_meet_through_the_node() {
    let w = world();
    let dir = tempfile::tempdir().unwrap();
    let org = w.events[0].id;
    let blind = BlindNode::start(Config {
        data: dir.path().to_path_buf(),
        org: core_events::OrgId(org.0),
        network: Network::Local,
        quota_bytes: 1 << 20,
    })
    .await
    .unwrap();

    // The first device leaves everything with the node, then goes away.
    let first = device(store_with(&w.events)).await;
    first.learn(blind.node.addr());
    tokio::time::timeout(Duration::from_secs(20), first.sync_with(&blind.id()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blind.events().unwrap(), w.events.len() as u64);
    // The node signed receipts with its own key.
    let last = w.events.last().unwrap().id;
    let receipts = first
        .state
        .lock()
        .unwrap()
        .store
        .store()
        .unwrap()
        .receipts(&last)
        .unwrap();
    assert!(receipts.contains(&blind.id()));
    drop(first);

    // Now it knows the organization's devices: strangers are turned away.
    blind.refresh().unwrap();
    let stranger = device(store_with(&w.events[..1])).await;
    stranger.learn(blind.node.addr());
    let _ = tokio::time::timeout(Duration::from_secs(10), stranger.sync_with(&blind.id())).await;
    assert_eq!(count(&stranger), 1);

    // A later device of the organization gets everything from the node.
    let later = {
        let k = &w.device("b", None).unwrap().keys;
        let keys = DeviceKeys::from_secret_bytes(&k.signing_secret(), &k.exchange_secret());
        let state = NodeState {
            store: store_with(&w.events[..1]),
            blobs: Box::new(MemBlobStore::default()),
            wanted_blobs: BTreeSet::new(),
            allowed: None,
            invitations: Invitations::default(),
            paired: Vec::new(),
        };
        Arc::new(
            Node::spawn(
                Arc::new(keys),
                Arc::new(std::sync::Mutex::new(state)),
                Network::Local,
                Arc::new(AlwaysConfirm),
            )
            .await
            .unwrap(),
        )
    };
    later.learn(blind.node.addr());
    tokio::time::timeout(Duration::from_secs(20), later.sync_with(&blind.id()))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(count(&later), w.events.len() as u64);
}
