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
    edit(
        &mut files,
        "programs.yaml",
        "{ id: host, label: s.host, requires: *senior }",
        "{ id: host, label: s.host, requires: *senior, each_event: true }",
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

/// A privilege ticked today counts for the events of a program already
/// started: the editor offers the person and the engine accepts the choice.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn qualification_granted_today_counts_for_past_events() {
    let dir = tempfile::tempdir().unwrap();
    let a = Core::open(CoreOptions {
        data_dir: dir.path().join("a"),
        bundle: club_bundle(),
        secrets: Arc::new(MemorySecretStore::default()),
        network: None,
        // 10 November 2026.
        clock: Some(1_794_301_200_000),
        pin_params: Some(PinParams::insecure_for_tests()),
        server: None,
        security: None,
    })
    .unwrap();
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let alice = a.people().unwrap().persons[0].person_id.clone();
    a.grant_qualification(alice.clone(), "member".into(), None, None)
        .unwrap();
    a.grant_role(alice.clone(), "keeper".into(), None).unwrap();
    let bob = a
        .add_person(
            "Bob".into(),
            core_api::dto::JsonMap(
                [("level".to_owned(), serde_json::json!(2))]
                    .into_iter()
                    .collect(),
            )
            .0,
        )
        .unwrap();
    a.grant_qualification(bob.clone(), "member".into(), None, None)
        .unwrap();
    let id = a
        .draft_program(
            "weekly_plan".into(),
            vec!["2026-11-02".into(), "2026-11-16".into()],
        )
        .unwrap();
    let hosts = |a: &Arc<Core>| {
        a.program(id.clone())
            .unwrap()
            .cells
            .into_iter()
            .filter(|c| c.slot == "host")
            .map(|c| c.candidates.iter().any(|x| x.id == bob))
            .collect::<Vec<_>>()
    };
    assert_eq!(hosts(&a), vec![false, false], "not senior yet");
    assert!(
        a.assign_slot(id.clone(), 0, "host".into(), 0, bob.clone())
            .is_err()
    );

    // Senior from today: offered and accepted for the past event too, and
    // the administrator is a candidate like everyone else.
    a.grant_qualification(bob.clone(), "senior".into(), None, None)
        .unwrap();
    assert_eq!(hosts(&a), vec![true, true]);
    a.assign_slot(id.clone(), 0, "host".into(), 0, bob.clone())
        .unwrap();
    a.assign_slot(id.clone(), 1, "host".into(), 0, bob).unwrap();
    let talker = a
        .program(id)
        .unwrap()
        .cells
        .into_iter()
        .find(|c| c.slot == "talker")
        .unwrap();
    assert!(talker.candidates.iter().any(|x| x.id == alice));
}

/// The details of a slot (its extra fields) are written without an
/// assignee, kept when someone is assigned, and travel with the version sent.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slot_details_without_assignee() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let alice = a.people().unwrap().persons[0].person_id.clone();
    a.grant_qualification(alice.clone(), "member".into(), None, None)
        .unwrap();
    a.grant_role(alice.clone(), "keeper".into(), None).unwrap();
    let id = a
        .draft_program("weekly_plan".into(), vec!["2026-11-02".into()])
        .unwrap();
    let detail = |e: &core_api::dto::ProgramEditor| {
        e.cells
            .iter()
            .find(|c| c.slot == "talker" && c.index == 1)
            .and_then(|c| c.details.iter().find(|f| f.id == "topic"))
            .map(|f| f.value.clone())
    };
    let change = |text: &str| core_api::dto::DetailChange {
        occurrence: 0,
        slot: "talker".into(),
        index: 1,
        values: [("topic".to_owned(), text.to_owned())].into(),
    };
    let e = a
        .assign_slots(id.clone(), vec![], vec![change("Patience")], vec![])
        .unwrap();
    assert_eq!(detail(&e), Some(serde_json::json!("Patience")));
    assert!(!e.needs_prefill, "no prefill for this type");

    // Assigning keeps the details; the version sent carries them.
    let e = a
        .assign_slots(
            id.clone(),
            vec![core_api::dto::SlotChange {
                occurrence: 0,
                slot: "talker".into(),
                index: 1,
                person: alice,
            }],
            vec![],
            vec![],
        )
        .unwrap();
    assert_eq!(detail(&e), Some(serde_json::json!("Patience")));
    a.send_program(id.clone()).unwrap();
    let v = a.view().unwrap();
    let snap = v.proj.assigned_snapshots.values().next().unwrap();
    assert_eq!(
        snap.assignments[0].extra.values().next(),
        Some(&core_model::Scalar::Str("Patience".into()))
    );
    drop(v);

    // An empty text clears the field.
    let e = a
        .assign_slots(id, vec![], vec![change("  ")], vec![])
        .unwrap();
    assert_eq!(detail(&e), Some(serde_json::Value::Null));
}

/// The editor's side list: who never had a slot of this type first, then
/// the longest without one, then by name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn people_by_weeks_since_their_last_slot() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let alice = a.people().unwrap().persons[0].person_id.clone();
    a.grant_qualification(alice.clone(), "member".into(), None, None)
        .unwrap();
    a.grant_role(alice.clone(), "keeper".into(), None).unwrap();
    for name in ["Bob", "Carla"] {
        let p = a.add_person(name.into(), Default::default()).unwrap();
        a.grant_qualification(p, "member".into(), None, None)
            .unwrap();
    }
    let first = a
        .draft_program("weekly_plan".into(), vec!["2026-11-02".into()])
        .unwrap();
    a.assign_slots(
        first,
        vec![core_api::dto::SlotChange {
            occurrence: 0,
            slot: "talker".into(),
            index: 0,
            person: alice,
        }],
        vec![],
        vec![],
    )
    .unwrap();
    let next = a
        .draft_program("weekly_plan".into(), vec!["2026-11-16".into()])
        .unwrap();
    let list: Vec<(String, Option<u32>)> = a
        .program(next)
        .unwrap()
        .people
        .into_iter()
        .map(|p| (p.name, p.weeks))
        .collect();
    assert_eq!(
        list,
        vec![
            ("Bob".into(), None),
            ("Carla".into(), None),
            ("Alice".into(), Some(2)),
        ]
    );
}

/// Parts of a single event removed and added by the editor stay so.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn parts_removed_and_added_per_event() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let alice = a.people().unwrap().persons[0].person_id.clone();
    a.grant_qualification(alice.clone(), "member".into(), None, None)
        .unwrap();
    a.grant_role(alice, "keeper".into(), None).unwrap();
    let id = a
        .draft_program("weekly_plan".into(), vec!["2026-11-02".into()])
        .unwrap();
    let shown = |e: &core_api::dto::ProgramEditor, i: u32| {
        e.cells
            .iter()
            .find(|c| c.slot == "talker" && c.index == i)
            .unwrap()
            .shown
    };
    let e = a.program(id.clone()).unwrap();
    assert!(shown(&e, 0) && shown(&e, 1), "all shown by default");
    let part = |i: u32, on: bool| core_api::dto::PartChange {
        occurrence: 0,
        slot: "talker".into(),
        index: i,
        shown: on,
    };
    let e = a
        .assign_slots(id.clone(), vec![], vec![], vec![part(1, false)])
        .unwrap();
    assert!(shown(&e, 0) && !shown(&e, 1));
    let e = a
        .assign_slots(id, vec![], vec![], vec![part(1, true)])
        .unwrap();
    assert!(shown(&e, 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn programs_listed_newest_first() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let alice = a.people().unwrap().persons[0].person_id.clone();
    a.grant_qualification(alice.clone(), "member".into(), None, None)
        .unwrap();
    a.grant_role(alice, "keeper".into(), None).unwrap();
    // Created in a different order from their dates: creation decides.
    let mut made = Vec::new();
    for day in ["2026-11-16", "2026-11-02", "2026-11-09"] {
        made.push(a.draft_program("weekly_plan".into(), vec![day.into()]).unwrap());
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    let listed: Vec<_> = a
        .programs(None)
        .unwrap()
        .programs
        .into_iter()
        .map(|p| p.program_id)
        .collect();
    made.reverse();
    assert_eq!(listed, made);
}

/// A slot on dedicated days: not in the events' rows, rows of date and
/// person added in the program, dated on the board.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn dedicated_days_rows() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
    let alice = a.people().unwrap().persons.into_iter().find(|p| p.name == "Alice").unwrap().person_id;
    a.grant_qualification(alice.clone(), "member".into(), None, None).unwrap();
    a.grant_role(alice, "keeper".into(), None).unwrap();
    let mut files = a.config_files().unwrap().files;
    let f = files.iter_mut().find(|f| f.path == "programs.yaml").unwrap();
    let from = "{ id: helper, label: s.helper, repeat: { count: 2 } }";
    assert!(f.text.contains(from));
    f.text = f.text.replacen(from, "{ id: helper, label: s.helper, repeat: { count: 2 }, mode: dedicated }", 1);
    a.config_publish(files).unwrap();

    let d = a.program_dates("weekly_plan".into(), Some("week-2026-11-02".into())).unwrap();
    let id = a.draft_program("weekly_plan".into(), d.dates).unwrap();
    let p = a.program(id.clone()).unwrap();
    assert!(p.cells.iter().all(|c| c.slot != "helper"), "not at the events");
    assert_eq!(p.dedicated_slots.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["helper"]);
    assert!(p.dedicated.is_empty());

    // Saturday 7: a day of its own; the second row takes the next place.
    let p = a.add_dedicated(id.clone(), "helper".into(), "2026-11-07".into(), bob.clone()).unwrap();
    assert_eq!(p.dedicated.len(), 1);
    assert_eq!(p.dedicated[0].date, "2026-11-07");
    assert_eq!(p.dedicated[0].assignee, "Bob");
    let p = a.add_dedicated(id.clone(), "helper".into(), "2026-11-07".into(), bob.clone()).unwrap();
    assert_eq!(p.dedicated.len(), 2);
    assert_eq!(p.dedicated[1].index, 1);
    // Removing a row empties its place.
    let r = &p.dedicated[1];
    let p = a
        .assign_slots(
            id.clone(),
            vec![core_api::dto::SlotChange { occurrence: r.occurrence, slot: r.slot.clone(), index: r.index, person: String::new() }],
            Vec::new(),
            Vec::new(),
        )
        .unwrap();
    assert_eq!(p.dedicated.len(), 1);
    // Bob's assignment carries the dedicated day.
    let export = a.export_person(bob).unwrap();
    assert_eq!(export.assignments.len(), 1, "{:?}", export.assignments);
}
