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

/// A program assigned to units: the candidates are the units of that type,
/// a slot goes to a unit and its members hear of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn slots_to_units() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
    let alice = a.people().unwrap().persons.into_iter().find(|p| p.name == "Alice").unwrap().person_id;
    a.grant_qualification(alice.clone(), "member".into(), None, None).unwrap();
    a.grant_role(alice.clone(), "keeper".into(), None).unwrap();
    let team = a.create_unit("team".into(), "Team 1".into()).unwrap();
    a.grant_qualification(bob.clone(), "member".into(), None, None).unwrap();
    a.set_membership(bob.clone(), team.clone(), None).unwrap();
    a.set_membership(alice.clone(), team.clone(), None).unwrap();
    let mut files = a.config_files().unwrap().files;
    let f = files.iter_mut().find(|f| f.path == "programs.yaml").unwrap();
    assert!(f.text.contains("    period: week\n"));
    f.text = f.text.replacen("    period: week\n", "    period: week\n    assign_units: team\n", 1);
    a.config_publish(files).unwrap();

    let d = a.program_dates("weekly_plan".into(), Some("week-2026-11-16".into())).unwrap();
    let id = a.draft_program("weekly_plan".into(), d.dates).unwrap();
    let p = a.program(id.clone()).unwrap();
    assert!(p.assign_units);
    let cell = p.cells.iter().find(|c| c.slot == "host").unwrap();
    let unit = format!("unit:{team}");
    assert!(cell.candidates.iter().any(|c| c.id == unit && c.label == "Team 1"), "{:?}", cell.candidates);
    let p = a.assign_slot(id.clone(), cell.occurrence, "host".into(), 0, unit.clone()).unwrap();
    let cell = p.cells.iter().find(|c| c.slot == "host").unwrap();
    assert_eq!(cell.assignee.as_deref(), Some(unit.as_str()));
    assert_eq!(cell.assignee_name.as_deref(), Some("Team 1"));
    // Sent: Alice, a member, holds the slot through the team.
    a.send_program(id.clone()).unwrap();
    let mine = a.my_assignments().unwrap();
    assert!(!mine.is_empty(), "the team's slot reaches its members");
    let _ = bob;
}

/// "Non posso" with a substitute: who meets the slot's requirements is
/// offered; the manager sees the proposal on the slot.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn decline_with_a_substitute() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let bob = a.add_person("Bob".into(), Default::default()).unwrap();
    let carl = a.add_person("Carl".into(), Default::default()).unwrap();
    let alice = a.people().unwrap().persons.into_iter().find(|p| p.name == "Alice").unwrap().person_id;
    let level2: std::collections::BTreeMap<String, serde_json::Value> =
        [("level".to_owned(), serde_json::json!(2))].into();
    for p in [&alice, &bob] {
        a.update_person(p.clone(), None, level2.clone()).unwrap();
        a.grant_qualification(p.clone(), "senior".into(), None, None).unwrap();
    }
    a.grant_role(alice.clone(), "keeper".into(), None).unwrap();
    let d = a.program_dates("weekly_plan".into(), Some("week-2026-11-16".into())).unwrap();
    let id = a.draft_program("weekly_plan".into(), d.dates).unwrap();
    let p = a.assign_slot(id.clone(), 0, "host".into(), 0, alice.clone()).unwrap();
    assert!(p.cells.iter().any(|c| c.slot == "host" && c.assignee.as_deref() == Some(alice.as_str())));
    a.send_program(id.clone()).unwrap();

    // Who could take it: Bob (senior), not Carl, not Alice herself.
    let subs = a.substitutes(id.clone(), "weekly_plan".into(), 0, "host".into()).unwrap();
    assert_eq!(subs.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), [bob.as_str()]);
    let _ = carl;
    a.respond(id.clone(), "weekly_plan".into(), 0, "host".into(), 0, false, None, None, Some(bob.clone()))
        .unwrap();
    let cell = a.program(id).unwrap().cells.into_iter().find(|c| c.slot == "host").unwrap();
    assert_eq!(cell.response, "declined");
    assert_eq!(cell.substitute.map(|c| c.id), Some(bob));
}

/// Weekly series of an event type without a recurrence: the month's
/// program has one event per series and week, headed by its time, place
/// and units; a series conducted by a unit offers the units.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn weekly_series_give_the_program_dates() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let alice = a.people().unwrap().persons.into_iter().find(|p| p.name == "Alice").unwrap().person_id;
    a.grant_qualification(alice.clone(), "member".into(), None, None).unwrap();
    a.grant_role(alice.clone(), "keeper".into(), None).unwrap();
    let north = a.create_unit("team".into(), "North".into()).unwrap();
    let south = a.create_unit("team".into(), "South".into()).unwrap();
    let mut files = a.config_files().unwrap().files;
    let cal = files.iter_mut().find(|f| f.path == "calendar.yaml").unwrap();
    cal.text.push_str("  - { id: outing, label: e.gathering }\n");
    let f = files.iter_mut().find(|f| f.path == "programs.yaml").unwrap();
    f.text.push_str(concat!(
        "  - id: outings\n",
        "    label: p.weekly_plan\n",
        "    event_type: outing\n",
        "    period: month\n",
        "    owner_role: keeper\n",
        "    publish_audience: all_members\n",
        "    slots:\n",
        "      - { id: lead, label: s.host, allow_units: team, extra_fields: [\n",
        "          { id: time, type: text, label: fld.topic }, { id: place, type: text, label: fld.topic },\n",
        "          { id: groups, type: text, label: fld.topic }, { id: by_unit, type: boolean, label: fld.done } ] }\n",
    ));
    a.config_publish(files).unwrap();

    let m = a.calendar_series().unwrap();
    assert!(m.event_types.iter().any(|c| c.id == "outing"), "{:?}", m.event_types);
    assert!(m.items.is_empty());
    // Monday 18:00 for everybody; Saturday 10:00 for North, led by a unit.
    let all = a
        .save_series(None, "outing".into(), 1, "18:00".into(), "Hall".into(), vec![north.clone(), south.clone()], false)
        .unwrap();
    a.save_series(None, "outing".into(), 6, "10:00".into(), "Park".into(), vec![north.clone()], true).unwrap();
    assert_eq!(a.calendar_series().unwrap().items.len(), 2);
    assert!(a.save_series(None, "outing".into(), 8, "10:00".into(), "x".into(), Vec::new(), false).is_err());

    let d = a.program_dates("outings".into(), Some("2026-11".into())).unwrap();
    // Weeks from Monday 2 to Monday 30 November: 5 Mondays, 4 Saturdays.
    assert_eq!(d.dates.len(), 9, "{:?}", d.dates);
    let id = a.draft_program("outings".into(), d.dates).unwrap();
    let p = a.program(id.clone()).unwrap();
    let mon = p.groups.iter().find(|g| g.occurrence == 0).unwrap();
    assert!(mon.detail.as_deref().is_some_and(|x| x.contains("18:00") && x.contains("Hall")), "{:?}", mon.detail);
    let sat = p.groups.iter().find(|g| g.occurrence == 1).unwrap();
    assert!(sat.detail.as_deref().is_some_and(|x| x.contains("Park") && x.contains("North")), "{:?}", sat.detail);
    // The details head the group: the slot does not repeat them.
    let lead = |occ: u32| p.cells.iter().find(|c| c.slot == "lead" && c.occurrence == occ).unwrap();
    assert!(lead(0).details.is_empty());
    assert!(lead(0).candidates.iter().all(|c| !c.id.starts_with("unit:")), "a person leads");
    assert!(lead(1).candidates.iter().any(|c| c.id == format!("unit:{north}")), "{:?}", lead(1).candidates);

    // A series removed is gone from the next month.
    a.remove_series_item(all).unwrap();
    let d = a.program_dates("outings".into(), Some("2026-12".into())).unwrap();
    assert!(d.dates.iter().all(|x| x.as_str() != "2026-12-07"), "{:?}", d.dates);
}

/// Calendar and rotations exported (JSON and YAML) and imported into a
/// second organization: same calendar and programs, the labels, the weekly
/// series with the units matched by name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn calendar_export_and_import() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let north = a.create_unit("team".into(), "North".into()).unwrap();
    let mut files = a.config_files().unwrap().files;
    let cal = files.iter_mut().find(|f| f.path == "calendar.yaml").unwrap();
    cal.text.push_str("  - { id: outing, label: e.outing }\n");
    let tx = files.iter_mut().find(|f| f.path.starts_with("texts/")).unwrap();
    tx.text.push_str("e.outing: \"Outing\"\n");
    a.config_publish(files).unwrap();
    a.save_series(None, "outing".into(), 6, "10:00".into(), "Park".into(), vec![north], true).unwrap();

    let json = a.schedule_export("json".into()).unwrap();
    let yaml = a.schedule_export("yaml".into()).unwrap();
    assert!(json.contains("\"e.outing\": \"Outing\""), "{json}");
    assert!(yaml.contains("location: Park"), "{yaml}");
    assert!(a.schedule_export("xml".into()).is_err());

    // Another organization, with a unit of the same name.
    let dir2 = tempfile::tempdir().unwrap();
    let b = core(dir2.path());
    b.create_org("Other".into(), "Bea".into()).await.unwrap();
    let north2 = b.create_unit("team".into(), "North".into()).unwrap();
    assert!(b.schedule_import("{\"kind\": \"other\"}".into()).is_err());
    b.schedule_import(yaml).unwrap();
    let cal = b.config_files().unwrap().files.into_iter().find(|f| f.path == "calendar.yaml").unwrap();
    assert!(cal.text.contains("id: outing"));
    let s = b.calendar_series().unwrap();
    assert_eq!(s.items.len(), 1);
    assert_eq!(s.items[0].units, [north2]);
    assert!(s.items[0].conducted_by_unit);
    assert!(s.event_types.iter().any(|c| c.id == "outing" && c.label == "Outing"), "{:?}", s.event_types);
    // Imported again (JSON): the series are replaced, not added.
    b.schedule_import(json).unwrap();
    assert_eq!(b.calendar_series().unwrap().items.len(), 1);
}

/// A program sent stays in the list of programs; once published it leaves
/// it (it is on the board), where who publishes may open it again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn published_programs_leave_the_list() {
    let dir = tempfile::tempdir().unwrap();
    let a = core(dir.path());
    a.create_org("Club".into(), "Alice".into()).await.unwrap();
    let alice = a.people().unwrap().persons[0].person_id.clone();
    a.grant_qualification(alice.clone(), "member".into(), None, None).unwrap();
    a.grant_role(alice, "keeper".into(), None).unwrap();
    let id = a.draft_program("weekly_plan".into(), vec!["2026-11-16".into()]).unwrap();
    let status = |a: &std::sync::Arc<core_api::Core>| {
        a.programs(None).unwrap().programs.into_iter().find(|p| p.program_id == id).map(|p| p.status)
    };
    assert_eq!(status(&a).as_deref(), Some("draft"));
    a.send_program(id.clone()).unwrap();
    assert_eq!(status(&a).as_deref(), Some("assigned"));
    a.publish_program(id.clone()).unwrap();
    assert_eq!(status(&a), None);
    let board = a.board(Some("2026-11".into())).unwrap();
    let post = board.iter().find(|b| b.programs.contains(&id)).unwrap();
    assert_eq!(post.editable.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), [id.as_str()]);
}
