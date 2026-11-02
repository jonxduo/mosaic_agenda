//! Calendar and rotations: the days of the events give the dates of a new
//! program, and a monthly rotation assigns the same person to every event
//! of the month with one choice.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use std::sync::Arc;

use core_api::{Core, CoreOptions};
use core_crypto::MemorySecretStore;
use core_crypto::pin::PinParams;

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    let sources = core_config::source_files(&dir).unwrap();
    core_config::compile_with_sources(pkg, sources)
        .unwrap()
        .bytes
}

fn core(dir: &Path) -> Arc<Core> {
    Core::open(CoreOptions {
        data_dir: dir.join("a"),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: None,
        clock: None,
        pin_params: Some(PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn monthly_rotation_assigns_the_whole_month() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
    let alice = a
        .people()
        .unwrap()
        .persons
        .into_iter()
        .find(|p| p.name == "Alice")
        .unwrap()
        .person_id;
    a.grant_qualification(alice.clone(), "member".into(), None, None)
        .unwrap();
    a.grant_role(alice, "keeper".into(), None).unwrap();

    // Monday only: four or five dates in November 2026.
    let d = a
        .program_dates("weekly_plan".into(), Some("week-2026-11-02".into()))
        .unwrap();
    assert_eq!(d.dates, vec!["2026-11-02"]);

    // Events on Monday and Thursday, one program per month, same persons all month.
    let mut files = a.config_files().unwrap().files;
    let edit = |files: &mut Vec<core_api::config::ConfigFile>, path: &str, from: &str, to: &str| {
        let f = files.iter_mut().find(|f| f.path == path).unwrap();
        assert!(f.text.contains(from), "{path}: {from}");
        f.text = f.text.replacen(from, to, 1);
    };
    edit(&mut files, "calendar.yaml", "BYDAY=MO", "BYDAY=MO,TH");
    edit(
        &mut files,
        "programs.yaml",
        "    period: week\n",
        "    period: month\n    rotation: month\n",
    );
    edit(
        &mut files,
        "programs.yaml",
        "{ id: helper, label: s.helper, repeat: { count: 2 } }",
        "{ id: helper, label: s.helper, repeat: { count: 2 }, enabled: false }",
    );
    // A monthly rotation in a weekly program is refused.
    let mut bad = files.clone();
    edit(
        &mut bad,
        "programs.yaml",
        "    period: month\n",
        "    period: week\n",
    );
    assert!(a.config_publish(bad).is_err());
    assert_eq!(a.config_publish(files).unwrap(), 2);

    let d = a
        .program_dates("weekly_plan".into(), Some("2026-11".into()))
        .unwrap();
    assert_eq!(d.dates.len(), 9, "{:?}", d.dates);
    assert_eq!(d.dates[0], "2026-11-02");
    assert!(!d.exists);
    let id = a.draft_program("weekly_plan".into(), d.dates).unwrap();
    assert!(
        a.program_dates("weekly_plan".into(), Some("2026-11".into()))
            .unwrap()
            .exists
    );
    assert_eq!(
        a.program_dates("weekly_plan".into(), None).unwrap().period,
        "2026-12",
        "the next proposal follows the last program"
    );

    let p = a.program(id.clone()).unwrap();
    assert_eq!(p.groups.len(), 1, "one group for the month");
    assert_eq!(p.occurrences.len(), 9);
    assert!(
        p.cells.iter().all(|c| c.slot != "helper"),
        "disabled slot hidden"
    );

    let p = a
        .assign_slot(id.clone(), 0, "talker".into(), 0, bob.clone())
        .unwrap();
    let talker = p
        .cells
        .iter()
        .find(|c| c.slot == "talker" && c.index == 0)
        .unwrap();
    assert_eq!(talker.assignee.as_deref(), Some(bob.as_str()));
    // Every event of the month got the same person.
    let export = a.export_person(bob.clone()).unwrap();
    assert_eq!(export.assignments.len(), 9);

    // Emptying the slot empties it for the whole month.
    a.assign_slot(id.clone(), 0, "talker".into(), 0, String::new())
        .unwrap();
    assert!(a.export_person(bob).unwrap().assignments.is_empty());
}
