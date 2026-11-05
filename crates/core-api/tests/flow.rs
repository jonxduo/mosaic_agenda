//! End-to-end through the facade: three devices with their own endpoint and
//! encrypted database pair over the network, then run the whole flow from
//! the individual document to the distributed aggregate (F5 acceptance).
//! Uses the neutral "club" fixture; `three_devices_over_public_relays` does
//! the same through n0's public relays (run with `--ignored`, needs internet).

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

async fn run_flow(network: Network) {
    let dir = tempfile::tempdir().unwrap();
    let local = matches!(network, Network::Local);
    let a = core(dir.path(), "a", network.clone());
    let b = core(dir.path(), "b", network.clone());
    let c = core(dir.path(), "c", network);
    assert_eq!(a.status().unwrap().state, AppState::Empty);
    let st = a
        .create_org("Club".into(), "Alice Admin".into())
        .await
        .unwrap();
    assert_eq!(st.state, AppState::Ready);
    let me = st.me.unwrap().person_id;

    // Organization set up by the admin.
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
    a.grant_role(me.clone(), "keeper".into(), None).unwrap();
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
    a.grant_role(carol.clone(), "keeper".into(), None).unwrap();

    pair(&a, &b, &bob, local).await;
    pair(&a, &c, &carol, local).await;
    assert_eq!(b.status().unwrap().me.unwrap().display_name, "Bob");

    // Bob fills and sends his log; the admin does the same.
    let period = "2026-10".to_owned();
    let form = b.form("log".into(), None, period.clone()).unwrap();
    assert!(form.editable);
    assert!(form.fields.iter().any(|f| f.id == "done"));
    b.save_document(
        "log".into(),
        None,
        period.clone(),
        map(json!({ "done": true, "count": 2 })),
    )
    .unwrap();
    let doc = b.form("log".into(), None, period.clone()).unwrap();
    let send = doc
        .actions
        .iter()
        .find(|x| x.id == "send")
        .expect("send action")
        .id
        .clone();
    let doc = b
        .step_document(doc.document_id.unwrap(), send, None)
        .unwrap();
    assert_eq!(doc.state.as_deref(), Some("collected"));
    a.save_document(
        "log".into(),
        None,
        period.clone(),
        map(json!({ "done": false, "count": 1 })),
    )
    .unwrap();
    let mine = a.form("log".into(), None, period.clone()).unwrap();
    a.step_document(mine.document_id.unwrap(), "send".into(), None)
        .unwrap();

    // The lead sees the complete batch after syncing with Bob, forwards and files it.
    sync_until(&[&b, &c], || {
        a.work_queue("log".into(), None).is_ok_and(|q| {
            q.batches
                .iter()
                .any(|x| x.period == period && x.missing.is_empty() && x.received.len() == 2)
        })
    })
    .await;
    let q = a.work_queue("log".into(), None).unwrap();
    let batch = q.batches.iter().find(|x| x.period == period).unwrap();
    assert!(batch.actions.iter().any(|x| x.id == "forward"), "{batch:?}");
    // The same unit in the overview: members, and the month with both logs.
    let r = a.unit_reports("log".into(), "team".into()).unwrap();
    let unit = r.units.iter().find(|u| u.unit_id == team).unwrap();
    assert!(unit.members.len() >= 2, "{:?}", unit.members.len());
    let month = unit.months.iter().find(|m| m.period == period).unwrap();
    assert_eq!(month.rows.len(), 2);
    // Totals of the month: counts summed.
    assert_eq!(month.received, 2);
    let count = month.totals.iter().find(|m| m.label.to_lowercase().contains("count"));
    assert_eq!(count.map(|m| m.value), Some(3.0), "{:?}", month.totals);
    assert!(
        b.unit_reports("log".into(), "team".into())
            .is_ok_and(|r| r.units.is_empty()),
        "a member does not oversee the unit"
    );
    a.batch_step("log".into(), team.clone(), period.clone(), "forward".into())
        .unwrap();
    a.batch_step("log".into(), team.clone(), period.clone(), "file".into())
        .unwrap();
    let agg = a
        .generate_aggregate("log_summary".into(), period.clone(), None)
        .unwrap();
    assert_eq!(agg.version, Some(1));
    assert!(
        agg.groups
            .iter()
            .any(|g| g.measures.iter().any(|m| m.value == 3.0)),
        "count sum {agg:?}"
    );
    a.distribute_aggregate("log_summary".into(), period.clone())
        .unwrap();

    // Carol, a keeper on another device, reads the distributed aggregate; Bob cannot.
    sync_until(&[&c, &b], || {
        c.aggregate("log_summary".into(), Some(period.clone()))
            .is_ok_and(|v| v.distributed)
    })
    .await;
    let seen = c
        .aggregate("log_summary".into(), Some(period.clone()))
        .unwrap();
    assert_eq!(seen.version, Some(1));
    assert_eq!(seen.registered_count, 2, "{seen:?}");
    assert!(
        seen.groups
            .iter()
            .any(|g| g.measures.iter().any(|m| m.value == 3.0)),
        "{seen:?}"
    );
    let bob_view = b
        .aggregate("log_summary".into(), Some(period.clone()))
        .unwrap();
    assert!(
        bob_view.groups.is_empty(),
        "Bob is not in the aggregate's audience"
    );
    // Bob's document reached others: its delivery status moved past pending.
    sync_until(&[&b], || {
        b.my_documents(None).is_ok_and(|d| {
            d.iter().any(|x| {
                x.delivery.as_deref() != Some("In attesa di sincronizzazione")
                    && x.delivery.is_some()
            })
        })
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn three_devices_from_document_to_aggregate() {
    run_flow(Network::Local).await;
}

/// Over the organization's own relay (F6): `OWN_RELAY_URL` points to an
/// `iroh-relay` (e.g. `http://localhost:3340` from `iroh-relay --dev` or the
/// CI compose stack).
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running iroh-relay (OWN_RELAY_URL)"]
async fn three_devices_over_own_relay() {
    let url = std::env::var("OWN_RELAY_URL").unwrap_or_else(|_| "http://localhost:3340".into());
    run_flow(Network::Server {
        relays: vec![url],
        dns: None,
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs internet access to n0's public relays"]
async fn three_devices_over_public_relays() {
    run_flow(Network::PublicRelays).await;
}
