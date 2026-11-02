//! F8 acceptance: a backup restored on a new desktop brings back the device
//! with all its data.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use core_api::{AppState, Core, CoreOptions};
use core_crypto::MemorySecretStore;
use core_crypto::pin::PinParams;

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
        server: None,
        security: None,
    })
    .unwrap()
}

#[tokio::test]
async fn backup_restored_on_a_new_desktop() {
    let old_dir = tempfile::tempdir().unwrap();
    let a = open(old_dir.path(), Arc::new(MemorySecretStore::default()));
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    a.add_person("Bob".into(), Default::default()).unwrap();
    a.add_person("Carol".into(), Default::default()).unwrap();
    let me = a.status().unwrap().me.unwrap();

    a.export_backup("short").unwrap_err();
    let file = a.export_backup("a long passphrase").unwrap();
    assert!(file.starts_with(b"ORGBACKUP1\n"));

    // A new computer: new folder, new keychain.
    let new_dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    let b = open(new_dir.path(), secrets.clone());
    assert_eq!(b.status().unwrap().state, AppState::Empty);
    assert_eq!(
        b.restore_backup(&file, "wrong passphrase!")
            .unwrap_err()
            .kind,
        "denied"
    );
    b.restore_backup(&file, "a long passphrase").unwrap();
    assert_eq!(b.status().unwrap().state, AppState::Restart);
    drop(b);

    // After the restart: same person, same device, same data, and it can act.
    let b = open(new_dir.path(), secrets);
    let st = b.status().unwrap();
    assert_eq!(st.state, AppState::Ready);
    let restored = st.me.unwrap();
    assert_eq!(restored.person_id, me.person_id);
    assert_eq!(restored.device_id, me.device_id);
    let names: Vec<String> = b
        .people()
        .unwrap()
        .persons
        .into_iter()
        .map(|p| p.name)
        .collect();
    assert!(
        names.contains(&"Bob".to_owned()) && names.contains(&"Carol".to_owned()),
        "{names:?}"
    );
    b.add_person("Dave".into(), Default::default()).unwrap();
    // Restoring twice is refused.
    assert!(b.restore_backup(&file, "a long passphrase").is_err());
}

#[tokio::test]
async fn devices_can_be_marked_as_archive() {
    let dir = tempfile::tempdir().unwrap();
    let a = open(dir.path(), Arc::new(MemorySecretStore::default()));
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let device = |a: &Core| a.people().unwrap().persons[0].devices[0].clone();
    let d = device(&a);
    assert_eq!(d.label, "");
    a.set_archive(d.id.clone(), true).unwrap();
    assert_eq!(device(&a).label, "archive");
    a.set_archive(d.id.clone(), false).unwrap();
    assert_eq!(device(&a).label, "");
    assert_eq!(
        a.set_archive("00".into(), true).unwrap_err().kind,
        "invalid"
    );
}

/// Every command of the list reaches its handler (the dev server and the
/// browser bridge use this table).
#[tokio::test]
async fn every_listed_command_is_dispatched() {
    let dir = tempfile::tempdir().unwrap();
    let a = open(dir.path(), Arc::new(MemorySecretStore::default()));
    for cmd in core_api::dispatch::COMMANDS {
        // Skip the ones that would wait on the network or stop the device.
        if [
            "sync_now",
            "join_org",
            "create_org",
            "wipe",
            "invite_device",
        ]
        .contains(cmd)
        {
            continue;
        }
        let r = core_api::dispatch::dispatch(&a, cmd, serde_json::json!({})).await;
        if let Err(e) = r {
            assert!(
                !(e.kind == "not_found" && e.message.starts_with("command")),
                "{cmd} has no handler"
            );
        }
    }
}
