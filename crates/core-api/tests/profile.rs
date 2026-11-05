//! Everyone changes their own profile: the attributes marked `self_edit`
//! and the dates of `self_service` qualifications, never other people's
//! data nor attributes that are not theirs to change.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use core_api::{AppState, Core, CoreOptions};
use core_crypto::MemorySecretStore;
use core_sync::net::Network;

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

fn core(dir: &Path, name: &str) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join(name),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: Some(Network::Local),
        clock: None,
        pin_params: Some(core_crypto::pin::PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap()
}

async fn pair(admin: &Arc<Core>, joiner: &Arc<Core>, person: &str) {
    let inv = admin.invite_device(person.to_owned()).await.unwrap();
    joiner.start_network().await.unwrap();
    let code = joiner.join_org(inv.ticket).await.unwrap();
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
    admin
        .confirm_pairing(found.expect("pairing request").code, true)
        .await
        .unwrap();
    for _ in 0..150 {
        if joiner.status().unwrap().state == AppState::Ready {
            return;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    panic!("the new device did not become ready");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn everyone_edits_their_own_profile_only() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path(), "a");
    let b = core(dir.path(), "b");
    let alice = a
        .create_org("Club".into(), "Alice".into())
        .await
        .unwrap()
        .me
        .unwrap()
        .person_id;
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
    pair(&a, &b, &bob).await;

    // Bob, with no permission, changes his phone and the date he joined.
    let p = b
        .save_profile(
            None,
            None,
            [("phone".to_owned(), serde_json::json!("555"))]
                .into_iter()
                .collect(),
            [("joined".to_owned(), "2020-05-01".to_owned())]
                .into_iter()
                .collect(),
        )
        .unwrap();
    assert_eq!(p.fields[0].value, serde_json::json!("555"));
    assert_eq!(p.dated[0].date.as_deref(), Some("2020-05-01"));
    // A corrected date replaces the old one.
    let p = b
        .save_profile(
            None,
            None,
            Default::default(),
            [("joined".to_owned(), "2019-01-01".to_owned())]
                .into_iter()
                .collect(),
        )
        .unwrap();
    assert_eq!(p.dated[0].date.as_deref(), Some("2019-01-01"));

    // Not an attribute of his profile, not someone else's data.
    let size = [("size".to_owned(), serde_json::json!("l"))]
        .into_iter()
        .collect();
    assert_eq!(
        b.update_person(bob.clone(), None, size).unwrap_err().kind,
        "denied"
    );
    let phone = [("phone".to_owned(), serde_json::json!("1"))]
        .into_iter()
        .collect();
    assert_eq!(
        b.update_person(alice.clone(), None, phone)
            .unwrap_err()
            .kind,
        "denied"
    );
    assert_eq!(
        b.grant_qualification(alice, "joined".into(), None, None)
            .unwrap_err()
            .kind,
        "denied"
    );
    assert_eq!(
        b.grant_qualification(bob, "member".into(), None, None)
            .unwrap_err()
            .kind,
        "denied"
    );
}
