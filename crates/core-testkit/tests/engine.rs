//! Engine rules the client cannot be trusted with: forged events, revoked
//! devices, concurrent configurations and grants, convergence (§7.6, §15.2).
//! Uses the neutral "club" fixture package.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::Path;

use core_crypto::distribution::seal_channel_key;
use core_crypto::{ChannelId, ChannelKey, ChannelRef, ChannelScope};
use core_engine::channels::{audience_desc, derive_keyring};
use core_engine::ops::{BatchKey, BatchStep, RoleGranted, Values, encode};
use core_engine::projection::AlertKind;
use core_engine::time::{PeriodKey, ms_of, parse_date};
use core_engine::{Engine, Update, Verdict, rebuild};
use core_events::payloads::{DeviceInvited, KeyDistribution, SealedKeyEntry};
use core_events::{Draft, EventKind, SignedEvent};
use core_model::{Id, PersonId, Scalar, Slug, UnitId};
use core_testkit::{ADMIN, World};
use proptest::prelude::*;
use time::Date;

fn slug(s: &str) -> Slug {
    Slug::new(s).unwrap()
}

fn day(s: &str) -> Date {
    parse_date(s).unwrap()
}

fn club_bundle() -> Vec<u8> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../core-config/tests/fixtures/club");
    let (pkg, _) = core_config::load_and_validate(&dir).unwrap();
    core_config::compile(pkg).unwrap().bytes
}

fn add(
    w: &mut World,
    name: &str,
    level: i64,
    quals: &[&str],
    team: Option<UnitId>,
    roles: &[(&str, Option<UnitId>)],
    devices: usize,
) -> PersonId {
    let attrs: Values = BTreeMap::from([(slug("level"), Scalar::Int(level))]);
    let display = name.to_owned();
    let p = w
        .act_with(ADMIN, |c| c.add_person(&display, attrs))
        .unwrap();
    w.persons.insert(name.to_owned(), p);
    for q in quals {
        w.act_with(ADMIN, |c| {
            c.grant_qualification(p, &slug(q), day("2020-01-01"), None)
        })
        .unwrap();
    }
    if let Some(t) = team {
        let m = w
            .act_with(ADMIN, |c| c.add_membership(p, t, day("2020-01-01")))
            .unwrap();
        w.memberships.insert(name.to_owned(), m);
    }
    for (r, u) in roles {
        w.act_with(ADMIN, |c| c.grant_role(p, &slug(r), *u, day("2020-01-01")))
            .unwrap();
    }
    for _ in 0..devices {
        w.add_device(name).unwrap();
    }
    p
}

/// admin + `lead` (lead of the team, keeper, 2 devices) + members m1, m2 (m2 with 2 devices).
fn club() -> (World, UnitId) {
    let mut w = World::new(club_bundle(), day("2026-11-02")).unwrap();
    let team = w
        .act_with(ADMIN, |c| c.create_unit(&slug("team"), "t1"))
        .unwrap();
    w.units.insert("t1".into(), team);
    add(
        &mut w,
        "lead",
        3,
        &["member", "senior"],
        Some(team),
        &[("lead", Some(team)), ("keeper", None)],
        2,
    );
    add(&mut w, "m1", 1, &["member"], Some(team), &[], 1);
    add(&mut w, "m2", 1, &["member"], Some(team), &[], 2);
    w.complete_channels().unwrap();
    (w, team)
}

fn verdict(w: &World, who: &str, e: &SignedEvent) -> Verdict {
    w.view(who, None).unwrap().verdict(&e.id).cloned().unwrap()
}

fn nov() -> PeriodKey {
    PeriodKey::month(2026, 11)
}

fn fill_and_send(w: &mut World, who: &str) {
    let p = w.person(who).unwrap();
    let values: Values = BTreeMap::from([(slug("done"), Scalar::Bool(true))]);
    let doc = w
        .act_with(who, |c| c.fill_document(&slug("log"), p, nov(), values))
        .unwrap();
    w.act(who, |c| c.step_document(doc, &slug("send"), None))
        .unwrap();
}

#[test]
fn forged_batch_step_is_rejected_by_the_engine() {
    let (mut w, team) = club();
    fill_and_send(&mut w, "m1");
    // m1 bypasses the client: makes its own key for the batch channel and forwards.
    let keys = &w.device("m1", None).unwrap().keys;
    let (proj, _) = w.view_of(&w.events, keys).unwrap();
    let doc = *proj.documents.keys().next().unwrap();
    let desc = audience_desc(&slug("log_readers"), ChannelScope::Unit(team));
    let channel = ChannelRef {
        id: ChannelId::derive(&proj.org.0, &desc).unwrap(),
        epoch: 0,
    };
    let key = ChannelKey::generate().unwrap();
    let dist = KeyDistribution {
        descriptor: desc,
        channel,
        entries: vec![SealedKeyEntry {
            device: keys.public(),
            sealed_key: seal_channel_key(&keys.exchange_public(), &channel, &key).unwrap(),
        }],
    };
    let now = ms_of(day("2026-12-10"));
    let e1 = Draft::after(
        proj.org,
        EventKind::ChannelKeysDistributed,
        &proj.heads,
        now,
    )
    .sign_public(&dist, keys)
    .unwrap();
    let step = BatchStep {
        batch: BatchKey {
            form: slug("log"),
            unit: team,
            period: nov(),
        },
        step: slug("forward"),
        documents: vec![doc],
    };
    let e2 = Draft::after(
        proj.org,
        EventKind::BatchForwarded,
        &[(e1.id, e1.body.depth)],
        now,
    )
    .sign_sealed(&encode(&step).unwrap(), channel, &key, keys)
    .unwrap();
    w.events.extend([e1.clone(), e2.clone()]);
    // Even the forger's own device, which can decrypt, rejects both.
    assert!(matches!(verdict(&w, "m1", &e1), Verdict::Invalid(m) if m.contains("members")));
    assert!(matches!(verdict(&w, "m1", &e2), Verdict::Invalid(m) if m.contains("not allowed")));
    let view = w.view("m1", None).unwrap();
    assert_eq!(view.documents[&doc].state.as_str(), "collected");
    // Others cannot even decrypt it.
    assert_eq!(verdict(&w, "lead", &e2), Verdict::PendingKey);
}

#[test]
fn forged_role_grant_is_rejected() {
    let (mut w, team) = club();
    let m1 = w.person("m1").unwrap();
    let keys = &w.device("m1", None).unwrap().keys;
    let ring = derive_keyring(&w.events, keys);
    let proj = rebuild(&w.events, &ring).unwrap();
    let org_channel =
        ChannelId::derive(&proj.org.0, &core_crypto::ChannelDescriptor::Organization).unwrap();
    let (r, key) = ring.current(&org_channel).unwrap();
    let grant = RoleGranted {
        grant: Id::from_bytes([9; 16]),
        person: m1,
        role: slug("lead"),
        unit: Some(team),
        start: day("2026-01-01"),
        end: None,
    };
    let e = Draft::after(
        proj.org,
        EventKind::RoleGranted,
        &proj.heads,
        ms_of(day("2026-11-03")),
    )
    .sign_sealed(&encode(&grant).unwrap(), r, key, keys)
    .unwrap();
    w.events.push(e.clone());
    assert!(matches!(verdict(&w, "lead", &e), Verdict::Invalid(m) if m.contains("roles.grant")));
    assert!(
        w.view("lead", None)
            .unwrap()
            .auth
            .org
            .role_grants
            .iter()
            .all(|g| g.person != m1)
    );
}

#[test]
fn revoked_device_events_are_invalid_or_quarantined() {
    let (mut w, _) = club();
    let m2 = w.person("m2").unwrap();
    let before = w.events.clone();
    w.set_date(day("2026-11-10"));
    w.revoke_device("m2", 1).unwrap();
    let revocation_time = w.events.last().unwrap().body.declared_time;
    let keys = &w.device("m2", Some(1)).unwrap().keys;
    let (old_view, _) = w.view_of(&before, keys).unwrap();
    let invite = |t: i64, heads: &[(core_events::EventHash, u64)]| {
        Draft::after(old_view.org, EventKind::DeviceInvited, heads, t)
            .sign_public(
                &DeviceInvited {
                    invite: Id::from_bytes([t as u8; 16]),
                    person: m2,
                    expires_at: t + 1,
                },
                keys,
            )
            .unwrap()
    };
    // Offline, unaware of the revocation, declared after it: quarantined.
    let late = invite(revocation_time + 60_000, &old_view.heads);
    // Offline, declared before the revocation: valid.
    let early = invite(revocation_time - 60_000, &old_view.heads);
    // Aware of the revocation (it is in its causal past): invalid.
    let (now_view, _) = w.view_of(&w.events, keys).unwrap();
    let after = invite(revocation_time + 120_000, &now_view.heads);
    w.events
        .extend([late.clone(), early.clone(), after.clone()]);
    assert_eq!(verdict(&w, "lead", &late), Verdict::Quarantined);
    assert_eq!(verdict(&w, "lead", &early), Verdict::Valid);
    assert!(matches!(verdict(&w, "lead", &after), Verdict::Invalid(m) if m.contains("revoked")));
    assert!(
        w.view("lead", None)
            .unwrap()
            .alerts
            .iter()
            .any(|a| a.kind == AlertKind::Quarantined)
    );
}

#[test]
fn concurrent_configurations_need_a_merge() {
    let (mut w, _) = club();
    let bundle = club_bundle();
    let (b2, b3) = (bundle.clone(), bundle.clone());
    w.concurrent(vec![
        (ADMIN.to_owned(), Box::new(move |c| c.publish_config(2, b2))),
        (ADMIN.to_owned(), Box::new(move |c| c.publish_config(3, b3))),
    ])
    .unwrap();
    let view = w.view("lead", None).unwrap();
    assert_eq!(
        view.active_config, 1,
        "neither concurrent version is active"
    );
    assert!(
        view.alerts
            .iter()
            .any(|a| a.kind == AlertKind::ConfigConflict)
    );
    w.act(ADMIN, |c| c.publish_config(4, bundle)).unwrap();
    assert_eq!(w.view("lead", None).unwrap().active_config, 4);
}

#[test]
fn concurrent_grants_beyond_max_keep_the_smallest_id() {
    let (mut w, team) = club();
    let a = add(&mut w, "sa", 3, &["member", "senior"], Some(team), &[], 1);
    let b = add(&mut w, "sb", 3, &["member", "senior"], Some(team), &[], 1);
    // The existing lead leaves, then two admins' devices grant the role offline.
    let lead_grant = w
        .view(ADMIN, None)
        .unwrap()
        .auth
        .role_ids
        .keys()
        .copied()
        .next();
    let _ = lead_grant;
    let lead = w.person("lead").unwrap();
    let grant_id = {
        let v = w.view(ADMIN, None).unwrap();
        *v.auth
            .role_ids
            .iter()
            .find(|(_, (i, _))| {
                v.auth.org.role_grants[*i].person == lead
                    && v.auth.org.role_grants[*i].role.as_str() == "lead"
            })
            .unwrap()
            .0
    };
    w.act(ADMIN, |c| c.end_role(grant_id, day("2026-11-02")))
        .unwrap();
    w.concurrent(vec![
        (
            ADMIN.to_owned(),
            Box::new(move |c| {
                c.grant_role(a, &slug("lead"), Some(team), day("2026-11-03"))
                    .map(|(_, e)| e)
            }),
        ),
        (
            ADMIN.to_owned(),
            Box::new(move |c| {
                c.grant_role(b, &slug("lead"), Some(team), day("2026-11-03"))
                    .map(|(_, e)| e)
            }),
        ),
    ])
    .unwrap();
    let view = w.view("lead", None).unwrap();
    let holders: Vec<PersonId> = view
        .auth
        .org
        .active_role_grants(day("2026-11-10"))
        .filter(|g| g.role.as_str() == "lead")
        .map(|g| g.person)
        .collect();
    assert_eq!(holders.len(), 1);
    let superseded: Vec<_> = view
        .verdicts
        .iter()
        .filter(|(_, v)| **v == Verdict::Superseded)
        .map(|(id, _)| *id)
        .collect();
    assert_eq!(superseded.len(), 1);
    let grants: Vec<&SignedEvent> = w
        .events
        .iter()
        .filter(|e| e.body.kind == EventKind::RoleGranted)
        .rev()
        .take(2)
        .collect();
    let winner = grants.iter().map(|e| e.id).min().unwrap();
    assert!(superseded[0] > winner, "the smallest event id wins");
}

// ----- convergence ---------------------------------------------------------------

#[derive(Debug, Clone)]
enum Act {
    Fill(&'static str, u8),
    Send(&'static str, u8),
    Forward(u8),
    ConcurrentFills(u8),
    ConcurrentEditsSamePerson(u8),
    RevokeConcurrentWithWrite(u8),
}

fn act_strategy() -> impl Strategy<Value = Act> {
    prop_oneof![
        (prop::sample::select(vec!["lead", "m1", "m2"]), 9u8..12)
            .prop_map(|(p, m)| Act::Fill(p, m)),
        (prop::sample::select(vec!["lead", "m1", "m2"]), 9u8..12)
            .prop_map(|(p, m)| Act::Send(p, m)),
        (9u8..12).prop_map(Act::Forward),
        (9u8..12).prop_map(Act::ConcurrentFills),
        (9u8..12).prop_map(Act::ConcurrentEditsSamePerson),
        (9u8..12).prop_map(Act::RevokeConcurrentWithWrite),
    ]
}

fn values(n: i64) -> Values {
    BTreeMap::from([
        (slug("done"), Scalar::Bool(n % 2 == 0)),
        (slug("count"), Scalar::Int(n)),
    ])
}

/// Applies an action; actions the rules refuse are simply skipped.
fn apply(w: &mut World, team: UnitId, a: &Act, n: i64) {
    let period = |m: u8| PeriodKey::month(2026, m);
    match a.clone() {
        Act::Fill(who, m) => {
            let p = w.person(who).unwrap();
            let _ = w.act_with(who, |c| {
                c.fill_document(&slug("log"), p, period(m), values(n))
            });
        }
        Act::Send(who, m) => {
            let p = w.person(who).unwrap();
            if let Ok(view) = w.view(who, None) {
                if let Some((id, _)) = view.document_by(&slug("log"), p, period(m)) {
                    let id = *id;
                    let _ = w.act(who, |c| c.step_document(id, &slug("send"), None));
                }
            }
        }
        Act::Forward(m) => {
            let _ = w.act("lead", |c| {
                c.batch_step(&slug("log"), team, period(m), &slug("forward"))
            });
        }
        Act::ConcurrentFills(m) => {
            let (p1, p2) = (w.person("m1").unwrap(), w.person("m2").unwrap());
            let _ = w.concurrent(vec![
                (
                    "m1".into(),
                    Box::new(move |c| {
                        c.fill_document(&slug("log"), p1, period(m), values(n))
                            .map(|(_, e)| e)
                    }),
                ),
                (
                    "m2".into(),
                    Box::new(move |c| {
                        c.fill_document(&slug("log"), p2, period(m), values(n + 1))
                            .map(|(_, e)| e)
                    }),
                ),
            ]);
        }
        Act::ConcurrentEditsSamePerson(m) => {
            // Two devices of `lead` edit the same draft offline (field by field, last in order wins).
            let p = w.person("lead").unwrap();
            let _ = w.act_with("lead", |c| {
                c.fill_document(&slug("log"), p, period(m), values(n))
            });
            let base = w.events.clone();
            let mut new = Vec::new();
            for (i, v) in [(0usize, values(n + 1)), (1usize, values(n + 2))] {
                let Ok(d) = w.device("lead", Some(i)) else {
                    continue;
                };
                if d.revoked {
                    continue;
                }
                let Ok((proj, ring)) = w.view_of(&base, &d.keys) else {
                    continue;
                };
                let client = core_engine::actions::Client {
                    events: &base,
                    proj: &proj,
                    keys: &d.keys,
                    keyring: &ring,
                    now: ms_of(day("2026-12-01")) + n,
                    engine: None,
                };
                if let Ok((_, e)) = client.fill_document(&slug("log"), p, period(m), v) {
                    new.extend(e);
                }
            }
            w.events.extend(new);
        }
        Act::RevokeConcurrentWithWrite(m) => {
            let already = w.device("m2", Some(1)).map(|d| d.revoked).unwrap_or(true);
            if !already {
                let p = w.person("m2").unwrap();
                let base = w.events.clone();
                let _ = w.revoke_device("m2", 1);
                if let Ok(d) = w.device("m2", Some(1)) {
                    if let Ok((proj, ring)) = w.view_of(&base, &d.keys) {
                        let client = core_engine::actions::Client {
                            events: &base,
                            proj: &proj,
                            keys: &d.keys,
                            keyring: &ring,
                            now: ms_of(day("2026-12-31")),
                            engine: None,
                        };
                        if let Ok((_, e)) =
                            client.fill_document(&slug("log"), p, period(m), values(n))
                        {
                            w.events.extend(e);
                        }
                    }
                }
            }
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 6, ..ProptestConfig::default() })]

    /// Same events, any order, any device with the same access: identical state.
    #[test]
    fn projections_converge(acts in prop::collection::vec(act_strategy(), 1..7), seed in any::<u64>()) {
        let (mut w, team) = club();
        for (i, a) in acts.iter().enumerate() {
            apply(&mut w, team, a, i as i64);
        }
        let d0 = &w.device("lead", Some(0)).unwrap().keys;
        let d1 = &w.device("lead", Some(1)).unwrap().keys;
        let ring0 = derive_keyring(&w.events, d0);
        let ring1 = derive_keyring(&w.events, d1);
        let reference = rebuild(&w.events, &ring0).unwrap().state_hash();
        // Another device of the same person.
        prop_assert_eq!(rebuild(&w.events, &ring1).unwrap().state_hash(), reference);
        // Shuffled and reversed delivery orders.
        let mut shuffled = w.events.clone();
        let mut x = seed | 1;
        for i in (1..shuffled.len()).rev() {
            x ^= x << 13; x ^= x >> 7; x ^= x << 17;
            shuffled.swap(i, (x % (i as u64 + 1)) as usize);
        }
        prop_assert_eq!(rebuild(&shuffled, &ring0).unwrap().state_hash(), reference);
        let mut reversed = w.events.clone();
        reversed.reverse();
        prop_assert_eq!(rebuild(&reversed, &ring0).unwrap().state_hash(), reference);

        // Incremental engine: same state whatever the delivery order and chunks.
        let genesis = &w.events[..1];
        for (label, order, chunk) in [("natural", &w.events, 1usize), ("shuffled/1", &shuffled, 1), ("shuffled/3", &shuffled, 3), ("reversed/2", &reversed, 2)] {
            let mut engine = Engine::build_for(genesis.to_vec(), d0).unwrap();
            engine.set_checkpoint_interval(2);
            for c in order.chunks(chunk) {
                engine.extend(c, d0).unwrap();
            }
            prop_assert_eq!(engine.projection().state_hash(), reference, "{}", label);
        }
        // Random causal order (each event after its parents), as sync
        // delivers them: out-of-order events resume from a saved state.
        let causal = causal_shuffle(&w.events, seed);
        let mut engine = Engine::build_for(causal[..1].to_vec(), d0).unwrap();
        engine.set_checkpoint_interval(2);
        for e in &causal[1..] {
            engine.extend(std::slice::from_ref(e), d0).unwrap();
        }
        prop_assert_eq!(engine.projection().state_hash(), reference, "causal order");
    }
}

/// A random order in which every event comes after its parents.
fn causal_shuffle(events: &[SignedEvent], seed: u64) -> Vec<SignedEvent> {
    let mut left: Vec<&SignedEvent> = events.iter().collect();
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    let mut x = seed | 1;
    while !left.is_empty() {
        let ready: Vec<usize> = (0..left.len())
            .filter(|&i| left[i].body.parents.iter().all(|p| seen.contains(p)))
            .collect();
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        let pick = ready[(x % ready.len() as u64) as usize];
        let e = left.remove(pick);
        seen.insert(e.id);
        out.push(e.clone());
    }
    out
}

/// Late events that sort before others resume from a saved state, without
/// a full rebuild, and give the same state.
#[test]
fn late_events_resume_from_a_saved_state() {
    let (mut w, team) = club();
    let acts = [
        Act::Fill("m1", 10),
        Act::ConcurrentFills(11),
        Act::Send("m1", 10),
        Act::Fill("m2", 10),
        Act::ConcurrentEditsSamePerson(10),
        Act::Send("m2", 10),
        Act::Send("lead", 10),
    ];
    for (i, a) in acts.iter().enumerate() {
        apply(&mut w, team, a, i as i64);
    }
    let keys = &w.device("lead", Some(0)).unwrap().keys;
    let ring = derive_keyring(&w.events, keys);
    let reference = rebuild(&w.events, &ring).unwrap().state_hash();
    let mut kinds = std::collections::BTreeMap::new();
    for seed in 1..20u64 {
        let causal = causal_shuffle(&w.events, seed);
        let mut engine = Engine::build_for(causal[..1].to_vec(), keys).unwrap();
        engine.set_checkpoint_interval(2);
        for e in &causal[1..] {
            let u = engine.extend(std::slice::from_ref(e), keys).unwrap();
            *kinds.entry(format!("{u:?}")).or_insert(0) += 1;
        }
        assert_eq!(engine.projection().state_hash(), reference, "seed {seed}");
    }
    assert!(kinds.get("Replayed").copied().unwrap_or(0) > 0, "{kinds:?}");
    assert!(kinds.get("Appended").copied().unwrap_or(0) > 0, "{kinds:?}");
}

/// Dry run of new events: same verdicts as a rebuild with them.
#[test]
fn engine_preview_matches_rebuild() {
    let (mut w, team) = club();
    for (i, a) in [
        Act::Fill("m1", 10),
        Act::Send("m1", 10),
        Act::Fill("m2", 10),
    ]
    .iter()
    .enumerate()
    {
        apply(&mut w, team, a, i as i64);
    }
    let keys = &w.device("lead", Some(0)).unwrap().keys;
    let n = w.events.len();
    let base = w.events[..n - 1].to_vec();
    let last = w.events[n - 1..].to_vec();
    let mut engine = Engine::build_for(base.clone(), keys).unwrap();
    let ring = derive_keyring(&w.events, keys);
    let before = engine.projection().state_hash();
    let verdicts = engine.preview(&last, &ring).unwrap();
    let full = rebuild(&w.events, &ring).unwrap();
    assert_eq!(verdicts, vec![full.verdict(&last[0].id).cloned().unwrap()]);
    // The preview left the engine untouched.
    assert_eq!(engine.projection().state_hash(), before);
    assert_eq!(engine.events().len(), n - 1);
    assert_eq!(engine.extend(&last, keys).unwrap(), Update::Appended);
    assert_eq!(engine.projection().state_hash(), full.state_hash());
}

#[test]
fn convergence_actions_do_real_work() {
    let (mut w, team) = club();
    let start = w.events.len();
    let acts = [
        Act::Fill("m1", 10),
        Act::Send("m1", 10),
        Act::ConcurrentFills(11),
        Act::ConcurrentEditsSamePerson(10),
        Act::Send("lead", 10),
        Act::Send("m2", 10),
        Act::Fill("m2", 10),
        Act::Send("m2", 10),
        Act::RevokeConcurrentWithWrite(9),
        Act::Forward(10),
    ];
    for (i, a) in acts.iter().enumerate() {
        apply(&mut w, team, a, i as i64);
    }
    let view = w.view("lead", None).unwrap();
    let oct = PeriodKey::month(2026, 10);
    let sent = view
        .documents
        .values()
        .filter(|d| d.content.period == oct && d.state.as_str() == "forwarded")
        .count();
    assert_eq!(sent, 3, "three October logs forwarded");
    assert!(w.device("m2", Some(1)).unwrap().revoked);
    assert!(
        view.verdicts.values().any(|v| *v == Verdict::Quarantined),
        "the offline write after revocation is quarantined"
    );
    // Concurrent edits of the same draft from two devices: field by field, last in order wins.
    let lead = w.person("lead").unwrap();
    let (_, doc) = view.document_by(&slug("log"), lead, oct).unwrap();
    assert!(doc.version >= 3);
    assert!(w.events.len() > start + 15);
    let ring1 = derive_keyring(&w.events, &w.device("lead", Some(1)).unwrap().keys);
    assert_eq!(
        rebuild(&w.events, &ring1).unwrap().state_hash(),
        view.state_hash()
    );
}
