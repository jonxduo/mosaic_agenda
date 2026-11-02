//! Reconciliation in memory and over iroh (§9.2, §9.6, F4 acceptance:
//! two and ten nodes converge). Uses the neutral "club" fixture.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use core_crypto::DeviceKeys;
use core_engine::channels::derive_keyring;
use core_engine::ops::Values;
use core_engine::rebuild;
use core_engine::time::{PeriodKey, parse_date};
use core_events::{DbKey, EventStore, SignedEvent};
use core_model::{Scalar, Slug};
use core_sync::blobs::MemBlobStore;
use core_sync::delivery::{Delivery, delivery};
use core_sync::local::LocalLog;
use core_sync::net::{AlwaysConfirm, Network, Node, NodeState};
use core_sync::pairing::{Invitations, PairResponse};
use core_sync::protocol::{Msg, Peer, Session, sync_in_memory};
use core_testkit::{ADMIN, World};
use proptest::prelude::*;

fn slug(s: &str) -> Slug {
    Slug::new(s).unwrap()
}

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

/// A small organization with some activity: admin, three members with logs.
fn world() -> World {
    let mut w = World::new(club_bundle(), parse_date("2026-11-02").unwrap()).unwrap();
    let team = w
        .act_with(ADMIN, |c| c.create_unit(&slug("team"), "t1"))
        .unwrap();
    for (i, name) in ["a", "b", "c"].iter().enumerate() {
        let attrs: Values = BTreeMap::from([(slug("level"), Scalar::Int(1 + i as i64))]);
        let p = w.act_with(ADMIN, |c| c.add_person(name, attrs)).unwrap();
        w.persons.insert((*name).to_owned(), p);
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
    for name in ["a", "b", "c"] {
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
    for e in events {
        if e.is_genesis() {
            s.insert(e, 0).unwrap();
        }
    }
    for e in events {
        s.insert(e, 0).unwrap();
    }
    LocalLog::Open(s)
}

fn ids(s: &LocalLog) -> BTreeSet<core_events::EventHash> {
    s.ids_with_depth()
        .unwrap()
        .into_iter()
        .map(|(id, _)| id)
        .collect()
}

struct Dev {
    store: LocalLog,
    blobs: MemBlobStore,
    keys: DeviceKeys,
}

fn sync_pair(a: &mut Dev, b: &mut Dev) {
    let (mut sa, mut sb) = (Session::new(BTreeSet::new()), Session::new(BTreeSet::new()));
    let mut pa = Peer {
        store: &mut a.store,
        blobs: &mut a.blobs,
        keys: &a.keys,
        now: 1,
    };
    let mut pb = Peer {
        store: &mut b.store,
        blobs: &mut b.blobs,
        keys: &b.keys,
        now: 1,
    };
    sync_in_memory(&mut sa, &mut pa, &mut sb, &mut pb).unwrap();
}

/// Splits the events among `n` devices: each knows the genesis and a random subset.
fn split(events: &[SignedEvent], n: usize, picks: &[u8]) -> Vec<Dev> {
    (0..n)
        .map(|i| {
            let subset: Vec<SignedEvent> = events
                .iter()
                .enumerate()
                .filter(|(j, e)| {
                    // Every event belongs to at least one device; others get random extras.
                    e.is_genesis()
                        || j % n == i
                        || picks
                            .get((i * 7 + j) % picks.len().max(1))
                            .is_some_and(|p| p % (n as u8 + 1) == i as u8 % (n as u8 + 1))
                })
                .map(|(_, e)| e.clone())
                .collect();
            Dev {
                store: store_with(&subset),
                blobs: MemBlobStore::default(),
                keys: DeviceKeys::generate().unwrap(),
            }
        })
        .collect()
}

#[test]
fn two_stores_converge_with_receipts() {
    let w = world();
    let half = w.events.len() / 2;
    let mut a = Dev {
        store: store_with(&w.events[..half]),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    let mut later: Vec<SignedEvent> = vec![w.events[0].clone()];
    later.extend(w.events[half..].iter().cloned());
    // b has the second half only: those events wait for missing parents.
    let mut b = Dev {
        store: store_with(&later),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    assert!(!b.store.missing().unwrap().is_empty());
    sync_pair(&mut a, &mut b);
    assert_eq!(ids(&a.store), ids(&b.store));
    assert_eq!(ids(&a.store).len(), w.events.len());
    assert!(b.store.missing().unwrap().is_empty());
    assert_eq!(
        a.store
            .store()
            .unwrap()
            .ordered_ids(&[core_events::EventStatus::PendingParents])
            .unwrap()
            .len(),
        0
    );
    // a received receipts from b for what it sent (an event of the first half).
    let sent = &w.events[1];
    assert!(
        a.store
            .store()
            .unwrap()
            .receipts(&sent.id)
            .unwrap()
            .contains(&b.keys.public())
    );
    // Nothing more to exchange: a second sync sends only hellos.
    let (mut sa, _) = (Session::new(BTreeSet::new()), ());
    let mut pa = Peer {
        store: &mut a.store,
        blobs: &mut a.blobs,
        keys: &a.keys,
        now: 2,
    };
    let hello = sa.start(&mut pa).unwrap();
    let mut sb = Session::new(BTreeSet::new());
    let mut pb = Peer {
        store: &mut b.store,
        blobs: &mut b.blobs,
        keys: &b.keys,
        now: 2,
    };
    for m in hello {
        assert!(
            sb.handle(&mut pb, m).unwrap().is_empty(),
            "identical stores need no reply"
        );
    }
}

#[test]
fn other_organizations_are_refused() {
    let w1 = world();
    let w2 = world();
    let mut a = Dev {
        store: store_with(&w1.events),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    let mut b = Dev {
        store: store_with(&w2.events),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    let mut sa = Session::new(BTreeSet::new());
    let mut sb = Session::new(BTreeSet::new());
    let mut pa = Peer {
        store: &mut a.store,
        blobs: &mut a.blobs,
        keys: &a.keys,
        now: 1,
    };
    let mut pb = Peer {
        store: &mut b.store,
        blobs: &mut b.blobs,
        keys: &b.keys,
        now: 1,
    };
    let hello = sa.start(&mut pa).unwrap();
    let reply = sb.handle(&mut pb, hello[0].clone()).unwrap();
    assert_eq!(reply, vec![Msg::WrongOrg]);
    assert!(sa.handle(&mut pa, Msg::WrongOrg).is_err());
}

#[test]
fn blobs_are_sent_to_who_wants_them_and_checked() {
    let w = world();
    let mut a = Dev {
        store: store_with(&w.events),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    let mut b = Dev {
        store: store_with(&w.events[..1]),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    let good = b"encrypted photo".to_vec();
    let h = *blake3::hash(&good).as_bytes();
    use core_sync::blobs::BlobStore;
    a.blobs.put(&h, &good).unwrap();
    let mut sa = Session::new(BTreeSet::new());
    let mut sb = Session::new([h].into());
    let mut pa = Peer {
        store: &mut a.store,
        blobs: &mut a.blobs,
        keys: &a.keys,
        now: 1,
    };
    let mut pb = Peer {
        store: &mut b.store,
        blobs: &mut b.blobs,
        keys: &b.keys,
        now: 1,
    };
    sync_in_memory(&mut sa, &mut pa, &mut sb, &mut pb).unwrap();
    assert!(b.blobs.contains(&h));
    // A blob whose content does not match its hash is dropped.
    let mut c = Dev {
        store: store_with(&w.events[..1]),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    let mut sc = Session::new([[9u8; 32]].into());
    let mut pc = Peer {
        store: &mut c.store,
        blobs: &mut c.blobs,
        keys: &c.keys,
        now: 1,
    };
    sc.handle(
        &mut pc,
        Msg::Blob {
            hash: [9; 32],
            data: good,
        },
    )
    .unwrap();
    assert!(!c.blobs.contains(&[9; 32]));
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 8, ..ProptestConfig::default() })]

    /// Ten devices with random subsets converge through random pairwise syncs.
    #[test]
    fn ten_devices_converge(picks in prop::collection::vec(any::<u8>(), 20..60), order in prop::collection::vec((0usize..10, 0usize..10), 30..60)) {
        let w = world();
        let mut devs = split(&w.events, 10, &picks);
        for (i, j) in order {
            if i == j {
                continue;
            }
            let (lo, hi) = (i.min(j), i.max(j));
            let (left, right) = devs.split_at_mut(hi);
            sync_pair(&mut left[lo], &mut right[0]);
            prop_assert_eq!(ids(&left[lo].store), ids(&right[0].store), "pair {} {}", lo, hi);
        }
        // A final pass along the chain and back guarantees full propagation:
        // forward, the last device collects everything; backward, it flows
        // to all the others.
        let pairs: Vec<usize> = (0..9).chain((0..9).rev()).collect();
        for i in pairs {
            let (left, right) = devs.split_at_mut(i + 1);
            sync_pair(&mut left[i], &mut right[0]);
            prop_assert_eq!(ids(&left[i].store), ids(&right[0].store), "chain {}", i);
        }
        let all: BTreeSet<_> = w.events.iter().map(|e| e.id).collect();
        for d in &devs {
            prop_assert_eq!(&ids(&d.store), &all);
        }
        // Same events → same projection on each device (same keys used).
        let keys = &w.device("a", None).unwrap().keys;
        let ring = derive_keyring(&w.events, keys);
        let reference = rebuild(&w.events, &ring).unwrap().state_hash();
        for d in &devs {
            let events = d.store.store().unwrap().all_events().unwrap();
            prop_assert_eq!(rebuild(&events, &ring).unwrap().state_hash(), reference);
        }
    }
}

#[test]
fn delivery_status_follows_receipts() {
    let w = world();
    let keys_a = &w.device("a", None).unwrap().keys;
    let ring = derive_keyring(&w.events, keys_a);
    let proj = rebuild(&w.events, &ring).unwrap();
    let LocalLog::Open(mut store) = store_with(&w.events) else {
        unreachable!()
    };
    // a's document event (submitted log): audience = readers of the log.
    let a_dev = keys_a.public();
    let ev = w
        .events
        .iter()
        .rev()
        .find(|e| e.body.author == a_dev)
        .unwrap()
        .clone();
    assert_eq!(delivery(&store, &proj, &ev).unwrap(), Delivery::Pending);
    let b = w.device("b", None).unwrap().keys.public();
    let c = w.device("c", None).unwrap().keys.public();
    store.add_receipt(&ev.id, &b, 1).unwrap();
    store.add_receipt(&ev.id, &a_dev, 1).unwrap(); // own receipts do not count
    assert_eq!(delivery(&store, &proj, &ev).unwrap(), Delivery::Pending);
    store.add_receipt(&ev.id, &c, 1).unwrap();
    assert!(delivery(&store, &proj, &ev).unwrap() >= Delivery::Sent);
}

// ----- over iroh -----------------------------------------------------------------

fn node_state(store: LocalLog) -> NodeState {
    NodeState {
        store,
        blobs: Box::new(MemBlobStore::default()),
        wanted_blobs: BTreeSet::new(),
        allowed: None,
        invitations: Invitations::default(),
        paired: Vec::new(),
    }
}

async fn local_node(store: LocalLog) -> Arc<Node> {
    Arc::new(
        Node::spawn(
            Arc::new(DeviceKeys::generate().unwrap()),
            Arc::new(std::sync::Mutex::new(node_state(store))),
            Network::Local,
            Arc::new(AlwaysConfirm),
        )
        .await
        .unwrap(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn two_nodes_sync_over_quic() {
    let w = world();
    let half = w.events.len() / 2;
    let a = local_node(store_with(&w.events[..half])).await;
    let mut later = vec![w.events[0].clone()];
    later.extend(w.events[half..].iter().cloned());
    let b = local_node(store_with(&later)).await;
    a.learn(b.addr());
    let stats = tokio::time::timeout(Duration::from_secs(20), a.sync_with(&b.device()))
        .await
        .unwrap()
        .unwrap();
    assert!(stats.events_new > 0 && stats.events_sent > 0, "{stats:?}");
    let ia = ids(&a.state.lock().unwrap().store);
    let ib = ids(&b.state.lock().unwrap().store);
    assert_eq!(ia, ib);
    assert_eq!(ia.len(), w.events.len());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pairing_a_new_device_then_syncing() {
    let mut w = world();
    // The inviter has the whole log.
    let inviter = local_node(store_with(&w.events)).await;
    let a_person = w.person("a").unwrap();
    let org = inviter.state.lock().unwrap().store.org().unwrap();
    let ticket = {
        let mut st = inviter.state.lock().unwrap();
        let addrs = inviter.addr().ip_addrs().map(|s| s.to_string()).collect();
        st.invitations
            .create(org, inviter.device(), a_person, 0, None, addrs)
            .unwrap()
    };
    let ticket = core_sync::pairing::Ticket::decode(&ticket.encode()).unwrap();
    // The new device starts from an empty store.
    let joiner = local_node(LocalLog::Open(
        EventStore::open_in_memory(&DbKey::generate().unwrap()).unwrap(),
    ))
    .await;
    // Ticket times are relative to the inviter's clock: make it valid now.
    {
        let mut st = inviter.state.lock().unwrap();
        st.invitations = Invitations::default();
    }
    let ticket = {
        let mut st = inviter.state.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let mut t = st
            .invitations
            .create(
                org,
                inviter.device(),
                a_person,
                now,
                None,
                ticket.addrs.clone(),
            )
            .unwrap();
        t.addrs = ticket.addrs.clone();
        t
    };
    let response = tokio::time::timeout(Duration::from_secs(20), joiner.pair(&ticket))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response, PairResponse::Accepted { org });
    let (person, identity) = inviter.state.lock().unwrap().paired.pop().unwrap();
    assert_eq!(person, a_person);
    assert_eq!(identity, joiner.keys().identity());
    // The inviter side publishes device.added (here through the world, as the admin).
    w.act(ADMIN, |c| c.add_device(person, identity)).unwrap();
    // Then the existing devices give it the channel keys (§8.3).
    w.complete_channels().unwrap();
    {
        let mut st = inviter.state.lock().unwrap();
        for e in &w.events {
            let _ = st.store.insert(e, 0);
        }
    }
    joiner.learn(inviter.addr());
    tokio::time::timeout(Duration::from_secs(20), joiner.sync_with(&inviter.device()))
        .await
        .unwrap()
        .unwrap();
    let events = joiner
        .state
        .lock()
        .unwrap()
        .store
        .store()
        .unwrap()
        .all_events()
        .unwrap();
    assert_eq!(events.len(), w.events.len());
    let proj = rebuild(&events, &derive_keyring(&events, joiner.keys())).unwrap();
    assert_eq!(
        proj.auth.author(&joiner.device()),
        Ok(a_person),
        "the new device belongs to the person"
    );
    // A replayed ticket is refused.
    let again = tokio::time::timeout(Duration::from_secs(20), joiner.pair(&ticket))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(again, PairResponse::Rejected { .. }));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn gossip_announcement_triggers_sync() {
    let w = world();
    let n = w.events.len();
    let a = local_node(store_with(&w.events[..n - 2])).await;
    let b = local_node(store_with(&w.events[..n - 2])).await;
    a.learn(b.addr());
    b.learn(a.addr());
    a.start_gossip(vec![b.device()]).await.unwrap();
    b.start_gossip(vec![a.device()]).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    // a creates (receives) two new events and announces them.
    {
        let mut st = a.state.lock().unwrap();
        for e in &w.events[n - 2..] {
            st.store.insert(e, 0).unwrap();
        }
    }
    let ann: Vec<_> = w.events[n - 2..]
        .iter()
        .map(|e| (e.id, e.body.depth))
        .collect();
    let mut ok = false;
    for _ in 0..40 {
        a.announce(ann.clone()).await.unwrap();
        tokio::time::sleep(Duration::from_millis(250)).await;
        if ids(&b.state.lock().unwrap().store).len() == n {
            ok = true;
            break;
        }
    }
    assert!(ok, "b synced after the announcement");
}

/// A locked device receives into its sealed inbox, serves nothing, and gets
/// everything into its store when unlocked.
#[test]
fn locked_device_receives_into_the_inbox() {
    use core_crypto::pin::InboxKey;
    use core_sync::local::Inbox;

    let w = world();
    let half = w.events.len() / 2;
    let db_key = DbKey::generate().unwrap();
    let inbox_key = InboxKey::derive(db_key.as_bytes());
    let dir = tempfile::tempdir().unwrap();

    // b knows the first half, then locks.
    let LocalLog::Open(b_store) = store_with(&w.events[..half]) else {
        unreachable!()
    };
    let inbox = Inbox::create(dir.path(), &b_store, inbox_key.public()).unwrap();
    drop(b_store);
    let mut b = Dev {
        store: LocalLog::Locked(inbox),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    let mut a = Dev {
        store: store_with(&w.events),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    sync_pair(&mut a, &mut b);
    let LocalLog::Locked(inbox) = &b.store else {
        unreachable!()
    };
    assert_eq!(inbox.pending(), w.events.len() - half);
    // Nothing readable on disk: sealed files only.
    for f in std::fs::read_dir(dir.path()).unwrap() {
        let bytes = std::fs::read(f.unwrap().path()).unwrap();
        for e in &w.events[half..] {
            let raw = e.to_bytes().unwrap();
            assert!(
                !bytes
                    .windows(32)
                    .any(|win| raw.windows(32).next() == Some(win))
            );
        }
    }
    // A third device syncing with the locked one gets nothing from it.
    let mut c = Dev {
        store: store_with(&w.events[..1]),
        blobs: MemBlobStore::default(),
        keys: DeviceKeys::generate().unwrap(),
    };
    sync_pair(&mut c, &mut b);
    assert_eq!(ids(&c.store).len(), 1);

    // Restart while locked: the inbox reopens with what it received.
    let reopened = Inbox::open(dir.path()).unwrap().unwrap();
    assert_eq!(reopened.pending(), w.events.len() - half);

    // Unlock: the store gets everything.
    let LocalLog::Open(mut store) = store_with(&w.events[..half]) else {
        unreachable!()
    };
    let added = reopened.drain_into(&mut store, &inbox_key, 2).unwrap();
    assert_eq!(added, w.events.len() - half);
    assert_eq!(ids(&LocalLog::Open(store)).len(), w.events.len());
    assert_eq!(Inbox::open(dir.path()).unwrap().unwrap().pending(), 0);
}
