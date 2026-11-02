//! F9 acceptance (§14): a person's data exported, then erased on three
//! devices that sync over the network; the erasure reaches every copy.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use core_api::{AppState, Core, CoreOptions};
use core_crypto::MemorySecretStore;
use core_sync::net::Network;
use serde_json::json;

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

fn core(dir: &Path, name: &str, network: Network) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join(name),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: Some(network),
        clock: None,
        pin_params: Some(core_crypto::pin::PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap()
}

fn map(v: serde_json::Value) -> BTreeMap<String, serde_json::Value> {
    serde_json::from_value(v).unwrap()
}

/// `admin` invites a device for `person`; `joiner` joins and is confirmed.
async fn pair(admin: &Arc<Core>, joiner: &Arc<Core>, person: &str, local: bool) {
    let inv = admin.invite_device(person.to_owned()).await.unwrap();
    assert!(inv.qr_svg.starts_with("<?xml") || inv.qr_svg.contains("<svg"));
    if local {
        joiner.start_network().await.unwrap();
    }
    let code = joiner.join_org(inv.ticket).await.unwrap();
    // The admin sees the request with the same code and confirms it.
    let mut found = None;
    for _ in 0..100 {
        if let Some(p) = admin
            .pairings()
            .unwrap()
            .into_iter()
            .find(|p| p.code == code)
        {
            found = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let p = found.expect("pairing request reached the admin");
    admin.confirm_pairing(p.code, true).await.unwrap();
    for _ in 0..150 {
        if joiner.status().unwrap().state == AppState::Ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("the new device did not become ready");
}

/// Pushes and pulls between `a` and the admin until `check` holds.
async fn sync_until(devices: &[&Arc<Core>], check: impl Fn() -> bool) {
    for _ in 0..60 {
        for d in devices {
            let _ = d.sync_round().await;
        }
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    panic!("condition not reached after syncing");
}

use core_model::PersonId;

fn bob_gone(d: &Arc<Core>, bob: PersonId) -> bool {
    let Ok(v) = d.view() else { return false };
    let erased_name = v
        .proj
        .auth
        .org
        .persons
        .get(&bob)
        .is_some_and(|r| r.display_name.is_empty() && r.attributes.is_empty());
    erased_name
        && v.proj.documents.values().all(|x| x.content.subject != bob)
        && v.events.iter().any(|e| e.is_erased())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn erasure_reaches_three_devices() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path(), "a", Network::Local);
    let b = core(dir.path(), "b", Network::Local);
    let c = core(dir.path(), "c", Network::Local);
    let me = a
        .create_org("Club".into(), "Alice Admin".into())
        .await
        .unwrap()
        .me
        .unwrap()
        .person_id;
    a.update_person(me.clone(), None, map(json!({ "level": 3 })))
        .unwrap();
    let team = a.create_unit("team".into(), "Team 1".into()).unwrap();
    for q in ["member", "senior"] {
        a.grant_qualification(me.clone(), q.into(), Some("2020-01-01".into()), None)
            .unwrap();
    }
    a.set_membership(me.clone(), team.clone(), Some("2020-01-01".into()))
        .unwrap();
    a.grant_role(me.clone(), "lead".into(), Some(team.clone()))
        .unwrap();
    let bob = a
        .add_person("Bob".into(), map(json!({ "level": 1 })))
        .unwrap();
    a.grant_qualification(
        bob.clone(),
        "member".into(),
        Some("2020-01-01".into()),
        None,
    )
    .unwrap();
    a.set_membership(bob.clone(), team.clone(), Some("2020-01-01".into()))
        .unwrap();
    let carol = a
        .add_person("Carol".into(), map(json!({ "level": 1 })))
        .unwrap();
    a.grant_qualification(
        carol.clone(),
        "member".into(),
        Some("2020-01-01".into()),
        None,
    )
    .unwrap();
    pair(&a, &b, &bob, true).await;
    pair(&a, &c, &carol, true).await;

    // Bob sends his log; the lead (admin) receives it.
    let period = "2026-10".to_owned();
    b.save_document(
        "log".into(),
        None,
        period.clone(),
        map(json!({ "done": true, "count": 4 })),
    )
    .unwrap();
    let doc = b.form("log".into(), None, period.clone()).unwrap();
    b.step_document(doc.document_id.unwrap(), "send".into(), None)
        .unwrap();
    let bob_id = PersonId(core_api::parse_id(&bob).unwrap());
    sync_until(&[&a, &b, &c], || {
        a.view().is_ok_and(|v| {
            v.proj
                .documents
                .values()
                .any(|d| d.content.subject == bob_id)
        })
    })
    .await;

    // Export: by Bob himself and by the administrator, not by Carol.
    let ex = a.export_person(bob.clone()).unwrap();
    assert_eq!(ex.person.name, "Bob");
    assert_eq!(ex.documents.len(), 1);
    assert!(
        ex.documents[0].values.iter().any(|l| l.value == "4"),
        "{ex:?}"
    );
    assert!(!ex.erasure_requested);
    assert_eq!(b.export_person(bob.clone()).unwrap().documents.len(), 1);
    assert_eq!(c.export_person(bob.clone()).unwrap_err().kind, "denied");
    let json = serde_json::to_string(&ex).unwrap();
    assert!(json.contains("Bob"));

    // Erasure: Carol may not ask for it, the root administrator does.
    assert!(c.request_erasure(bob.clone()).is_err());
    a.request_erasure(bob.clone()).unwrap();
    assert!(bob_gone(&a, bob_id));
    sync_until(&[&a, &b, &c], || {
        bob_gone(&a, bob_id) && bob_gone(&b, bob_id) && bob_gone(&c, bob_id)
    })
    .await;
    let ex = a.export_person(bob.clone()).unwrap();
    assert!(ex.erasure_requested);
    assert!(ex.documents.is_empty());
    assert!(!serde_json::to_string(&ex).unwrap().contains("Bob"));
    // Shells: the person's event everywhere; the document's events where
    // they could be read (Bob's device and the lead), not on Carol's.
    let shells = |d: &Arc<Core>| {
        d.view()
            .unwrap()
            .events
            .iter()
            .filter(|e| e.is_erased())
            .count()
    };
    assert!(
        shells(&a) >= 3 && shells(&b) >= 3,
        "{} {}",
        shells(&a),
        shells(&b)
    );
    assert!(shells(&c) >= 1);
    // The organization goes on: the lead still files their own log.
    a.save_document(
        "log".into(),
        None,
        period.clone(),
        map(json!({ "done": true, "count": 1 })),
    )
    .unwrap();
}
