//! PIN protection (§8.6): the database key is stored only wrapped with the
//! PIN; locking closes the database; a restart starts locked.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use core_api::{AppState, Core, CoreOptions};
use core_crypto::pin::PinParams;
use core_crypto::{MemorySecretStore, SecretStore};

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

fn open(dir: &Path, secrets: Arc<MemorySecretStore>) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.to_path_buf(),
        bundle: club_bundle(),
        secrets,
        network: None,
        clock: None,
        pin_params: Some(PinParams::insecure_for_tests()),
    })
    .unwrap()
}

#[tokio::test]
async fn pin_wraps_the_database_key() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    let core = open(dir.path(), secrets.clone());
    core.accept_privacy().unwrap();
    core.create_org("Club".into(), "Alice".into())
        .await
        .unwrap();
    let me = core.status().unwrap().me.unwrap().person_id;
    core.add_person("Bob".into(), Default::default()).unwrap();
    assert!(secrets.get("store.db_key").unwrap().is_some());

    core.set_pin(Some("12".into())).unwrap_err();
    core.set_pin(Some("2468".into())).unwrap();
    assert!(
        secrets.get("store.db_key").unwrap().is_none(),
        "no plain key once a PIN is set"
    );
    assert!(secrets.get("store.db_key.pin").unwrap().is_some());

    // Locking closes the database: nothing can be read or written.
    core.lock_now();
    assert_eq!(core.status().unwrap().state, AppState::Locked);
    assert_eq!(core.people().unwrap_err().kind, "locked");
    assert_eq!(
        core.add_person("Carol".into(), Default::default())
            .unwrap_err()
            .kind,
        "locked"
    );
    assert!(!core.unlock("0000").unwrap());
    assert!(core.unlock("2468").unwrap());
    assert_eq!(core.people().unwrap().persons.len(), 2);

    // A restart starts locked; the PIN opens the same data.
    drop(core);
    let core = open(dir.path(), secrets.clone());
    assert_eq!(core.status().unwrap().state, AppState::Locked);
    assert!(!core.unlock("1357").unwrap());
    assert!(core.unlock("2468").unwrap());
    assert_eq!(core.status().unwrap().me.unwrap().person_id, me);
    assert_eq!(core.people().unwrap().persons.len(), 2);

    // Changing the PIN re-wraps the same key.
    core.set_pin(Some("97531".into())).unwrap();
    core.lock_now();
    assert!(!core.unlock("2468").unwrap());
    assert!(core.unlock("97531").unwrap());

    // Removing it puts the plain key back: the next start is unlocked.
    core.set_pin(None).unwrap();
    assert!(secrets.get("store.db_key.pin").unwrap().is_none());
    drop(core);
    let core = open(dir.path(), secrets);
    assert_eq!(core.status().unwrap().state, AppState::Ready);
    assert_eq!(core.people().unwrap().persons.len(), 2);
    assert!(!dir.path().join("inbox").exists());
}

fn open_net(dir: &Path, name: &str) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join(name),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: Some(core_sync::net::Network::Local),
        clock: None,
        pin_params: Some(PinParams::insecure_for_tests()),
    })
    .unwrap()
}

/// A locked device keeps receiving (sealed) and shows it after unlocking.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn locked_device_keeps_receiving() {
    use std::time::Duration;
    let dir = tempfile::tempdir().unwrap();
    let a = open_net(dir.path(), "a");
    let b = open_net(dir.path(), "b");
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();

    // Pair b as Bob.
    let inv = a.invite_device(bob).await.unwrap();
    b.start_network().await.unwrap();
    let code = b.join_org(inv.ticket).await.unwrap();
    for _ in 0..100 {
        if a.pairings().unwrap().iter().any(|p| p.code == code) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    a.confirm_pairing(code, true).await.unwrap();
    for _ in 0..150 {
        if b.status().unwrap().state == AppState::Ready {
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert_eq!(b.status().unwrap().state, AppState::Ready);
    b.sync_round().await.unwrap();

    b.set_pin(Some("2468".into())).unwrap();
    b.lock_now();
    a.add_person("Carol".into(), Default::default()).unwrap();
    for _ in 0..60 {
        let _ = a.sync_round().await;
        let _ = b.sync_round().await;
        if b.pending_while_locked() > 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    assert!(b.pending_while_locked() > 0, "received while locked");
    assert_eq!(b.people().unwrap_err().kind, "locked");
    assert!(b.unlock("2468").unwrap());
    let names: Vec<String> = b
        .people()
        .unwrap()
        .persons
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(names.contains(&"Carol".to_owned()), "{names:?}");
}
