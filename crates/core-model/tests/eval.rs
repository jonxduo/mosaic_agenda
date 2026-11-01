//! Condition evaluation and audience resolution on edge cases.
//! The fixture uses a neutral "club" domain.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::{BTreeMap, BTreeSet};

use core_model::config::{AudiencesFile, QualificationsFile, RolesFile};
use core_model::eval::{ConfigIndex, EvalContext, EvalError, Evaluator};
use core_model::state::{
    GrantScope, Membership, OrgState, Period, PersonRecord, QualificationGrant, RoleGrant,
    UnitRecord,
};
use core_model::{Cond, Id, PersonId, Scalar, Slug, UnitId};
use time::Date;
use time::macros::date;

fn slug(s: &str) -> Slug {
    Slug::new(s).unwrap()
}

fn pid(n: u8) -> PersonId {
    PersonId(Id::from_bytes([n; 16]))
}

fn uid(n: u8) -> UnitId {
    UnitId(Id::from_bytes([100 + n; 16]))
}

const QUALS: &str = r#"{
  "sets": ["extra"],
  "qualifications": [
    { "id": "member", "label": "q.member", "duration": "permanent" },
    { "id": "senior", "label": "q.senior", "duration": "permanent", "implies": ["member"] },
    { "id": "temp", "label": "q.temp", "duration": "fixed_period", "fixed_period_unit": "month",
      "sets": ["extra"], "implies": ["member"] },
    { "id": "steady", "label": "q.steady", "duration": "permanent", "sets": ["extra"] }
  ]
}"#;

const ROLES: &str = r#"{ "roles": [
  { "id": "lead", "label": "r.lead", "scope": "unit:team", "holders": { "min": 0, "max": 1 } },
  { "id": "helper", "label": "r.helper", "scope": "unit:team" },
  { "id": "watcher", "label": "r.watcher", "scope": "unit:team", "supervises": "helper" },
  { "id": "coordinator", "label": "r.coordinator", "scope": "organization" }
]}"#;

const AUDIENCES: &str = r#"{ "audiences": [
  { "id": "everyone_active", "select": { "everyone": true } },
  { "id": "members", "select": { "with_qualification": "member" } },
  { "id": "seniors", "select": { "with_qualification": "senior" } },
  { "id": "extra", "select": { "with_set": "extra" } },
  { "id": "oversight", "select": { "union": [
      { "role_holders": { "role": "lead", "where": "subject_unit" } },
      { "role_holders": { "role": "helper", "where": "subject_unit" } },
      { "supervisors_of": "subject_unit" } ] } },
  { "id": "readers", "select": { "union": [ { "subject": true }, { "audience": "oversight" } ] } },
  { "id": "juniors", "select": { "except": [ { "audience": "members" }, { "audience": "seniors" } ] } },
  { "id": "senior_extra", "select": { "intersect": [ { "audience": "seniors" }, { "audience": "extra" } ] } },
  { "id": "team_one", "select": { "unit_members": "subject_unit" } },
  { "id": "loop_a", "select": { "audience": "loop_b" } },
  { "id": "loop_b", "select": { "union": [ { "audience": "loop_a" } ] } }
]}"#;

struct Fixture {
    cfg: ConfigIndex,
    state: OrgState,
}

fn person(state: &mut OrgState, n: u8, attrs: &[(&str, Scalar)]) -> PersonId {
    let id = pid(n);
    state.persons.insert(
        id,
        PersonRecord {
            display_name: format!("p{n}"),
            attributes: attrs
                .iter()
                .map(|(k, v)| (slug(k), v.clone()))
                .collect::<BTreeMap<_, _>>(),
            active: true,
        },
    );
    id
}

fn grant(state: &mut OrgState, p: PersonId, q: &str, period: Period) {
    state.qualification_grants.push(QualificationGrant {
        person: p,
        qualification: slug(q),
        period,
    });
}

fn role(state: &mut OrgState, p: PersonId, r: &str, scope: GrantScope, period: Period) {
    state.role_grants.push(RoleGrant {
        person: p,
        role: slug(r),
        scope,
        period,
    });
}

const START: Date = date!(2026 - 01 - 01);

/// p1: senior, lead of team 1. p2: member + temp in October, team 1.
/// p3: member, team 1 until Oct 15 then team 2. p4: steady, watcher of team 1.
/// p5: helper of team 2, inactive person p6.
fn fixture() -> Fixture {
    let cfg = ConfigIndex::build(
        &serde_json::from_str::<QualificationsFile>(QUALS).unwrap(),
        &serde_json::from_str::<RolesFile>(ROLES).unwrap(),
        &serde_json::from_str::<AudiencesFile>(AUDIENCES).unwrap(),
    );
    let mut s = OrgState::default();
    for n in [1, 2] {
        s.units.insert(
            uid(n),
            UnitRecord {
                unit_type: slug("team"),
                name: format!("t{n}"),
                active: true,
            },
        );
    }
    let p1 = person(
        &mut s,
        1,
        &[("level", Scalar::Int(3)), ("kind", "a".into())],
    );
    let p2 = person(&mut s, 2, &[("level", Scalar::Int(1))]);
    let p3 = person(&mut s, 3, &[]);
    let p4 = person(&mut s, 4, &[]);
    let p5 = person(&mut s, 5, &[]);
    let p6 = person(&mut s, 6, &[]);
    if let Some(r) = s.persons.get_mut(&p6) {
        r.active = false;
    }
    grant(&mut s, p1, "senior", Period::from(START));
    grant(&mut s, p2, "member", Period::from(START));
    grant(
        &mut s,
        p2,
        "temp",
        Period::between(date!(2026 - 10 - 01), date!(2026 - 11 - 01)),
    );
    grant(&mut s, p3, "member", Period::from(START));
    grant(&mut s, p4, "steady", Period::from(START));
    grant(&mut s, p6, "member", Period::from(START));
    for p in [p1, p2, p4] {
        s.memberships.push(Membership {
            person: p,
            unit: uid(1),
            period: Period::from(START),
        });
    }
    s.memberships.push(Membership {
        person: p3,
        unit: uid(1),
        period: Period::between(START, date!(2026 - 10 - 15)),
    });
    s.memberships.push(Membership {
        person: p3,
        unit: uid(2),
        period: Period::from(date!(2026 - 10 - 15)),
    });
    s.memberships.push(Membership {
        person: p5,
        unit: uid(2),
        period: Period::from(START),
    });
    role(
        &mut s,
        p1,
        "lead",
        GrantScope::Unit(uid(1)),
        Period::from(START),
    );
    role(
        &mut s,
        p4,
        "watcher",
        GrantScope::Unit(uid(1)),
        Period::from(START),
    );
    role(
        &mut s,
        p5,
        "helper",
        GrantScope::Unit(uid(2)),
        Period::from(START),
    );
    role(
        &mut s,
        p1,
        "coordinator",
        GrantScope::Organization,
        Period::from(START),
    );
    Fixture { cfg, state: s }
}

fn set(ids: &[u8]) -> BTreeSet<PersonId> {
    ids.iter().map(|n| pid(*n)).collect()
}

#[test]
fn implication_is_transitive_and_dated() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    assert!(ev.has_qualification(pid(1), &slug("member"), date!(2026 - 05 - 01)));
    assert!(!ev.has_qualification(pid(1), &slug("member"), date!(2025 - 12 - 31)));
}

#[test]
fn fixed_period_qualification_boundaries() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    let extra = slug("extra");
    assert!(!ev.in_set(pid(2), &extra, date!(2026 - 09 - 30)));
    assert!(ev.in_set(pid(2), &extra, date!(2026 - 10 - 01)));
    assert!(ev.in_set(pid(2), &extra, date!(2026 - 10 - 31)));
    assert!(!ev.in_set(pid(2), &extra, date!(2026 - 11 - 01)));
    let oct = EvalContext::at(date!(2026 - 10 - 20));
    assert_eq!(ev.resolve(&slug("extra"), &oct), Ok(set(&[2, 4])));
    let nov = EvalContext::at(date!(2026 - 11 - 20));
    assert_eq!(ev.resolve(&slug("extra"), &nov), Ok(set(&[4])));
}

#[test]
fn exclusive_unit_change_mid_month() {
    let f = fixture();
    let team = slug("team");
    assert_eq!(
        f.state.unit_of_type(pid(3), &team, date!(2026 - 10 - 14)),
        Some(uid(1))
    );
    assert_eq!(
        f.state.unit_of_type(pid(3), &team, date!(2026 - 10 - 15)),
        Some(uid(2))
    );
    let ev = Evaluator::new(&f.cfg, &f.state);
    let ctx = EvalContext::at(date!(2026 - 10 - 31)).with_subject(pid(3), Some(uid(1)));
    assert_eq!(ev.resolve(&slug("team_one"), &ctx), Ok(set(&[1, 2, 4])));
}

#[test]
fn oversight_audience_by_subject_unit() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    // Team 1: lead p1; watcher p4 supervises "helper", which nobody holds in team 1.
    let ctx = EvalContext::at(date!(2026 - 10 - 10)).with_subject(pid(2), Some(uid(1)));
    assert_eq!(ev.resolve(&slug("oversight"), &ctx), Ok(set(&[1])));
    assert_eq!(ev.resolve(&slug("readers"), &ctx), Ok(set(&[1, 2])));
    // Team 2: helper p5.
    let ctx2 = EvalContext::at(date!(2026 - 10 - 20)).with_subject(pid(3), Some(uid(2)));
    assert_eq!(ev.resolve(&slug("readers"), &ctx2), Ok(set(&[3, 5])));
}

#[test]
fn supervisor_counts_only_when_supervised_role_is_held() {
    let mut f = fixture();
    role(
        &mut f.state,
        pid(2),
        "helper",
        GrantScope::Unit(uid(1)),
        Period::from(date!(2026 - 10 - 05)),
    );
    let ev = Evaluator::new(&f.cfg, &f.state);
    let before = EvalContext::at(date!(2026 - 10 - 04)).with_subject(pid(3), Some(uid(1)));
    let after = EvalContext::at(date!(2026 - 10 - 05)).with_subject(pid(3), Some(uid(1)));
    assert_eq!(ev.resolve(&slug("oversight"), &before), Ok(set(&[1])));
    assert_eq!(ev.resolve(&slug("oversight"), &after), Ok(set(&[1, 2, 4])));
}

#[test]
fn combinators_and_inactive_persons() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    let ctx = EvalContext::at(date!(2026 - 10 - 10));
    // p6 is inactive and excluded.
    assert_eq!(ev.resolve(&slug("members"), &ctx), Ok(set(&[1, 2, 3])));
    assert_eq!(ev.resolve(&slug("juniors"), &ctx), Ok(set(&[2, 3])));
    assert_eq!(ev.resolve(&slug("senior_extra"), &ctx), Ok(set(&[])));
    assert_eq!(
        ev.resolve(&slug("everyone_active"), &ctx),
        Ok(set(&[1, 2, 3, 4, 5]))
    );
}

#[test]
fn recursive_audience_is_an_error() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    let ctx = EvalContext::at(date!(2026 - 10 - 10));
    assert!(matches!(
        ev.resolve(&slug("loop_a"), &ctx),
        Err(EvalError::AudienceCycle(_))
    ));
    assert!(matches!(
        ev.resolve(&slug("nope"), &ctx),
        Err(EvalError::UnknownAudience(_))
    ));
}

#[test]
fn system_audiences() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    let mut ctx = EvalContext::at(date!(2026 - 10 - 10)).with_subject(pid(2), Some(uid(1)));
    assert_eq!(ev.resolve(&slug("subject_only"), &ctx), Ok(set(&[2])));
    assert_eq!(
        ev.resolve(&slug("subject_or_on_behalf"), &ctx),
        Ok(set(&[2]))
    );
    ctx.on_behalf = Some(slug("oversight"));
    assert_eq!(
        ev.resolve(&slug("subject_or_on_behalf"), &ctx),
        Ok(set(&[1, 2]))
    );
    ctx.missing_subjects = set(&[3]);
    assert_eq!(ev.resolve(&slug("subject_missing"), &ctx), Ok(set(&[3])));
}

#[test]
fn conditions() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    let ctx = EvalContext::at(date!(2026 - 10 - 10)).with_subject(pid(2), Some(uid(1)));
    let parse = |s: &str| serde_json::from_str::<Cond>(s).unwrap();
    let c = parse(r#"{ "attribute": { "name": "level", "op": "ge", "value": 2 } }"#);
    assert!(ev.eval(&c, Some(pid(1)), &ctx));
    assert!(!ev.eval(&c, Some(pid(2)), &ctx));
    assert!(
        !ev.eval(&c, Some(pid(3)), &ctx),
        "missing attribute is never >="
    );
    let c = parse(r#"{ "attribute": { "name": "kind", "op": "unset" } }"#);
    assert!(ev.eval(&c, Some(pid(2)), &ctx));
    let c = parse(r#"{ "holds_role": { "role": "lead", "where": "subject_unit" } }"#);
    assert!(ev.eval(&c, Some(pid(1)), &ctx));
    let c = parse(r#"{ "holds_role": { "role": "coordinator", "where": "subject_unit" } }"#);
    assert!(
        !ev.eval(&c, Some(pid(1)), &ctx),
        "organization roles are not in a unit"
    );
    let c = parse(r#"{ "holds_role": { "role": "lead", "where": "own_unit" } }"#);
    assert!(ev.eval(&c, Some(pid(1)), &ctx));
    let c = parse(r#"{ "member_of": "unit_type:team" }"#);
    assert!(ev.eval(&c, Some(pid(3)), &ctx));
    assert!(!ev.eval(&c, Some(pid(6)), &ctx));
    let c = parse(r#"{ "all": [ { "is_subject": true }, { "in_set": "extra" } ] }"#);
    assert!(ev.eval(&c, Some(pid(2)), &ctx));
    assert!(!ev.eval(&c, Some(pid(4)), &ctx));
    let c = parse(r#"{ "any": [] }"#);
    assert!(!ev.eval(&c, Some(pid(1)), &ctx));
    let c = parse(r#"{ "all": [] }"#);
    assert!(ev.eval(&c, None, &ctx));
}

#[test]
fn field_conditions_use_context_values() {
    let f = fixture();
    let ev = Evaluator::new(&f.cfg, &f.state);
    let mut ctx = EvalContext::at(date!(2026 - 10 - 10));
    let c: Cond =
        serde_json::from_str(r#"{ "field": { "name": "done", "op": "eq", "value": true } }"#)
            .unwrap();
    assert!(!ev.eval(&c, None, &ctx));
    ctx.fields.insert(slug("done"), Scalar::Bool(true));
    assert!(ev.eval(&c, None, &ctx));
}
