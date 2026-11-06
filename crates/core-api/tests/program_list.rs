//! The list of programs (menu > Programmi) with the package: programs of a
//! publish group, handed in and then published as a group, stay listed.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use core_api::{Core, CoreOptions};
use core_crypto::MemorySecretStore;
use core_crypto::pin::PinParams;

fn bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packages/congregazione");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    let sources = core_config::source_files(&dir).unwrap();
    core_config::compile_with_sources(pkg, sources).unwrap().bytes
}

fn open(dir: &Path, secrets: &Arc<MemorySecretStore>, clock: i64) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join("a"),
        bundle: bundle(),
        secrets: secrets.clone(),
        network: None,
        clock: Some(clock),
        pin_params: Some(PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap()
}

// 2026-10-20 and 2026-10-22, 10:00 UTC.
const DAY1: i64 = 1_792_490_400_000;
const DAY3: i64 = 1_792_663_200_000;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn group_published_stays_listed() {
    let dir = tempfile::tempdir().unwrap();
    let secrets = Arc::new(MemorySecretStore::default());
    let program_ids;
    {
        // Who prepares audio/video and attendants hands both in.
        let a = open(dir.path(), &secrets, DAY1);
        a.create_org("Org".into(), "Alice".into()).await.unwrap();
        let alice = a.people().unwrap().persons[0].person_id.clone();
        let male: std::collections::BTreeMap<String, serde_json::Value> =
            [("sex".to_owned(), serde_json::json!("m"))].into();
        a.update_person(alice.clone(), None, male).unwrap();
        for q in ["baptized", "elder"] {
            a.grant_qualification(alice.clone(), q.into(), None, None).unwrap();
        }
        for r in ["av_manager", "attendant_manager"] {
            a.grant_role(alice.clone(), r.into(), None).unwrap();
        }
        let mut ids = Vec::new();
        for ty in ["av", "attendants"] {
            let d = a.program_dates(ty.into(), Some("2026-11".into())).unwrap();
            let id = a.draft_program(ty.into(), d.dates).unwrap();
            a.send_program(id.clone()).unwrap();
            ids.push(id);
        }
        program_ids = ids;
        let me = a.people().unwrap().persons.into_iter().find(|p| p.person_id == alice).unwrap();
        for r in me.held_roles {
            a.end_grant("role".into(), r.grant_id, None).unwrap();
        }
        a.grant_role(alice, "board_manager".into(), None).unwrap();
    }
    // Two days later Alice only publishes them (here she may still prepare
    // them, as the founder: they are listed one by one).
    let a = open(dir.path(), &secrets, DAY3);
    let status = |a: &Arc<Core>| {
        let mut v: Vec<_> = a
            .programs(None)
            .unwrap()
            .programs
            .into_iter()
            .filter(|p| program_ids.contains(&p.program_id))
            .map(|p| p.status)
            .collect();
        v.sort();
        v
    };
    assert_eq!(status(&a), ["to_publish", "to_publish"]);
    a.publish_program_group("assignments".into(), "2026-11".into()).unwrap();
    assert_eq!(status(&a), ["published", "published"]);
}
