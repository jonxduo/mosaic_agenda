//! F8 acceptance: a slot and a role changed in the configuration editor,
//! simulated and published as a new version, reach another device without a
//! new build of the app.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use core_api::config::ConfigFile;
use core_api::{AppState, Core, CoreOptions};
use core_crypto::MemorySecretStore;
use core_crypto::pin::PinParams;
use core_sync::net::Network;

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    let sources = core_config::source_files(&dir).unwrap();
    core_config::compile_with_sources(pkg, sources)
        .unwrap()
        .bytes
}

fn core(dir: &Path, name: &str) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join(name),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: Some(Network::Local),
        clock: None,
        pin_params: Some(PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap()
}

fn edit(files: &mut [ConfigFile], path: &str, from: &str, to: &str) {
    let f = files.iter_mut().find(|f| f.path == path).unwrap();
    assert!(f.text.contains(from), "{path} has `{from}`");
    f.text = f.text.replacen(from, to, 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn slot_and_role_changes_published_without_a_new_build() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path(), "a");
    let b = core(dir.path(), "b");
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
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

    // The active version, with its source files (comments included).
    let current = a.config_files().unwrap();
    assert_eq!(current.version, 1);
    assert!(current.can_publish);
    assert!(current.files.iter().any(|f| f.path == "roles.yaml"));
    assert!(
        !b.config_files().unwrap().can_publish,
        "Bob may not publish"
    );

    // A slot (one more host) and a role (two leads per team).
    let mut files = current.files.clone();
    edit(
        &mut files,
        "programs.yaml",
        "{ id: host, label: s.host, requires: *senior }",
        "{ id: host, label: s.host, requires: *senior, repeat: { count: 2 } }",
    );
    edit(
        &mut files,
        "roles.yaml",
        "holders: { min: 0, max: 1 }",
        "holders: { min: 0, max: 2 }",
    );
    assert!(
        a.config_check(files.clone())
            .unwrap()
            .iter()
            .all(|i| i.severity != "error")
    );
    let sim = a.config_simulate(files.clone()).unwrap();
    assert!(
        sim.changes
            .iter()
            .any(|c| c.kind == "slot" && c.id == "weekly_plan.host" && c.what == "changed"),
        "{:?}",
        sim.changes
    );
    assert!(
        sim.changes
            .iter()
            .any(|c| c.kind == "role" && c.id == "lead" && c.what == "changed")
    );
    // Simulation publishes nothing.
    assert_eq!(a.config_files().unwrap().version, 1);

    // A removal without a migration is reported before publishing.
    let mut bad = files.clone();
    let roles = bad.iter_mut().find(|f| f.path == "roles.yaml").unwrap();
    roles.text = roles
        .text
        .replace("label: r.lead", "label: r.lead\n    x-note: kept")
        .replacen("  - id: lead\n", "  - id: leader\n", 1);
    let issues = a.config_check(bad.clone()).unwrap();
    assert!(issues.iter().any(|i| i.severity == "error"), "{issues:?}");
    assert!(a.config_publish(bad).is_err());

    assert_eq!(a.config_publish(files.clone()).unwrap(), 2);
    let after = a.config_files().unwrap();
    assert_eq!(after.version, 2);
    assert!(
        after
            .files
            .iter()
            .any(|f| f.path == "roles.yaml" && f.text.contains("max: 2"))
    );

    // Bob's device gets version 2 by sync, same app build.
    for _ in 0..60 {
        let _ = a.sync_round().await;
        let _ = b.sync_round().await;
        if b.config_files().unwrap().version == 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    let seen = b.config_files().unwrap();
    assert_eq!(seen.version, 2);
    assert!(
        seen.files
            .iter()
            .any(|f| f.path == "programs.yaml" && f.text.contains("repeat: { count: 2 } }"))
    );
}
