//! A management view is shown, and served, only to who can act in it: a
//! plain member does not find the people list or the qualification matrix
//! in the menu, and asking for them directly is refused.

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

fn views(c: &Core) -> Vec<String> {
    c.menu().unwrap().into_iter().map(|m| m.view).collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn management_views_hidden_without_permission() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path(), "a");
    let b = core(dir.path(), "b");
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
    a.grant_qualification(bob.clone(), "member".into(), None, None)
        .unwrap();

    // The administrator (root) sees and opens everything.
    let all = views(&a);
    assert!(all.contains(&"people".to_owned()), "{all:?}");
    assert!(all.contains(&"qualification_matrix".to_owned()), "{all:?}");
    assert!(a.qualification_matrix("member,senior".into()).is_ok());

    // Bob holds no management permission.
    pair(&a, &b, &bob).await;
    let mine = views(&b);
    assert!(!mine.contains(&"people".to_owned()), "{mine:?}");
    assert!(
        !mine.contains(&"qualification_matrix".to_owned()),
        "{mine:?}"
    );
    assert!(!mine.contains(&"config".to_owned()), "{mine:?}");
    assert_eq!(
        b.qualification_matrix("member".into()).unwrap_err().kind,
        "denied"
    );
    assert_eq!(b.people().unwrap_err().kind, "denied");
    assert_eq!(b.units().unwrap_err().kind, "denied");
    // His own data stays his to export.
    assert_eq!(b.export_person(bob).unwrap().person.name, "Bob");
}
