//! Package scenario tests (PROGETTO.md §15.4).
//!
//! ```yaml
//! name: ...
//! given:
//!   date: 2026-12-01
//!   people:
//!     - { id: anna, qualifications: [q1, "q2:2026-10"], group: g1, roles: [r1@g1, r2],
//!         attributes: { a: x }, devices: 1 }
//! when:
//!   - { as: anna, do: document.fill, form: f, period: 2026-11, values: { x: 1 } }
//!   - { check: { cannot_read: { who: luca, document: anna/f/2026-11 } } }
//!   - concurrent:
//!       - { as: marco, do: batch.forward, group: g1, period: 2026-11 }
//!       - { as: sara, do: batch.forward, group: g1, period: 2026-11 }
//!   - { as: anna, do: batch.forward, group: g1, period: 2026-11, expect: denied }
//! then:
//!   - { state: { document: anna/f/2026-11, is: registered } }
//! ```
//!
//! Checks look at the projection of one device (`who`, optional `device`
//! index), so readability is checked cryptographically: a person "can read"
//! a document only if their device could decrypt it.

use std::collections::{BTreeMap, BTreeSet};

use core_config::CompiledConfig;
use core_engine::Verdict;
use core_engine::ops::{Assignee, AssignmentSpec, Response, SlotKey, Values};
use core_engine::projection::Projection;
use core_engine::time::{PeriodKey, parse_date};
use core_model::config::StepKind;
use core_model::{Id, Scalar, Slug};
use serde_norway::{Mapping, Value};
use time::Date;

use crate::world::{ADMIN, BoxedAction, World};

#[derive(Debug, thiserror::Error)]
#[error("{context}: {message}")]
pub struct ScenarioError {
    pub context: String,
    pub message: String,
}

fn err(context: &str, message: impl Into<String>) -> ScenarioError {
    ScenarioError {
        context: context.to_owned(),
        message: message.into(),
    }
}

type R<T> = Result<T, String>;

/// Grants given at setup start here, long before any tested period.
fn setup_date() -> Date {
    parse_date("2020-01-01").unwrap_or(Date::MIN)
}

fn s(m: &Mapping, key: &str) -> Option<String> {
    match m.get(key)? {
        Value::String(x) => Some(x.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn req(m: &Mapping, key: &str) -> R<String> {
    s(m, key).ok_or_else(|| format!("missing `{key}`"))
}

fn slug(x: &str) -> R<Slug> {
    Slug::new(x).map_err(|e| e.to_string())
}

fn date(x: &str) -> R<Date> {
    parse_date(x).ok_or_else(|| format!("invalid date `{x}`"))
}

fn period(x: &str) -> R<PeriodKey> {
    x.parse()
        .map_err(|e: core_engine::time::PeriodParseError| e.to_string())
}

fn uint(m: &Mapping, key: &str, default: u64) -> R<u64> {
    match m.get(key) {
        None => Ok(default),
        Some(Value::Number(n)) => n
            .as_u64()
            .ok_or_else(|| format!("`{key}` must be a positive integer")),
        Some(_) => Err(format!("`{key}` must be a number")),
    }
}

fn scalar(v: &Value) -> R<Scalar> {
    Ok(match v {
        Value::Null => Scalar::Null,
        Value::Bool(b) => Scalar::Bool(*b),
        Value::Number(n) => match n.as_i64() {
            Some(i) => Scalar::Int(i),
            None => Scalar::Num(n.as_f64().unwrap_or(0.0)),
        },
        Value::String(x) => Scalar::Str(x.clone()),
        Value::Sequence(items) => Scalar::List(items.iter().map(scalar).collect::<R<_>>()?),
        _ => return Err("unsupported value".into()),
    })
}

fn values(m: &Mapping, key: &str) -> R<Values> {
    let mut out = BTreeMap::new();
    if let Some(Value::Mapping(map)) = m.get(key) {
        for (k, v) in map {
            let k = k.as_str().ok_or("value keys must be strings")?;
            out.insert(slug(k)?, scalar(v)?);
        }
    }
    Ok(out)
}

fn strings(m: &Mapping, key: &str) -> Vec<String> {
    match m.get(key) {
        Some(Value::Sequence(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(str::to_owned))
            .collect(),
        Some(Value::String(x)) => vec![x.clone()],
        _ => Vec::new(),
    }
}

pub struct Runner {
    pub world: World,
    cfg: CompiledConfig,
    default_date: Date,
}

impl Runner {
    pub fn new(bundle: Vec<u8>, date: Date) -> Result<Self, ScenarioError> {
        let cfg = core_config::decode(&bundle).map_err(|e| err("bundle", e.to_string()))?;
        let world = World::new(bundle, date).map_err(|e| err("genesis", e.to_string()))?;
        Ok(Self {
            world,
            cfg,
            default_date: date,
        })
    }

    fn unit_type(&self) -> R<Slug> {
        self.cfg
            .package
            .forms
            .forms
            .iter()
            .find_map(|f| f.batching.as_ref().map(|b| b.unit_type.clone()))
            .or_else(|| {
                self.cfg
                    .package
                    .units
                    .unit_types
                    .first()
                    .map(|u| u.id.clone())
            })
            .ok_or_else(|| "the package has no unit types".to_owned())
    }

    fn default_form(&self) -> R<Slug> {
        self.cfg
            .package
            .forms
            .forms
            .first()
            .map(|f| f.id.clone())
            .ok_or_else(|| "no forms".into())
    }

    fn unit(&mut self, name: &str) -> R<core_model::UnitId> {
        if let Some(u) = self.world.units.get(name) {
            return Ok(*u);
        }
        let t = self.unit_type()?;
        let u = self
            .world
            .act_with(ADMIN, |c| c.create_unit(&t, name))
            .map_err(|e| e.to_string())?;
        self.world.units.insert(name.to_owned(), u);
        Ok(u)
    }

    /// `q`, `q:YYYY-MM` (one month) or `q:YYYY-MM-DD..YYYY-MM-DD`.
    fn qualification_spec(spec: &str) -> R<(Slug, Date, Option<Date>)> {
        match spec.split_once(':') {
            None => Ok((slug(spec)?, setup_date(), None)),
            Some((q, range)) => {
                if let Some((a, b)) = range.split_once("..") {
                    return Ok((slug(q)?, date(a)?, Some(date(b)?)));
                }
                let p = period(range)?;
                let start = p.start().ok_or("bad month")?;
                let end = p.next().and_then(|n| n.start()).ok_or("bad month")?;
                Ok((slug(q)?, start, Some(end)))
            }
        }
    }

    /// `role` or `role@group`.
    fn role_spec(&mut self, spec: &str) -> R<(Slug, Option<core_model::UnitId>)> {
        match spec.split_once('@') {
            None => Ok((slug(spec)?, None)),
            Some((r, g)) => Ok((slug(r)?, Some(self.unit(g)?))),
        }
    }

    fn add_person(&mut self, m: &Mapping) -> R<()> {
        let name = req(m, "id")?;
        let attributes = values(m, "attributes")?;
        let display = name.clone();
        let person = self
            .world
            .act_with(ADMIN, |c| c.add_person(&display, attributes))
            .map_err(|e| e.to_string())?;
        self.world.persons.insert(name.clone(), person);
        for q in strings(m, "qualifications") {
            self.grant_qualification(&name, &q)?;
        }
        if let Some(g) = s(m, "group") {
            let unit = self.unit(&g)?;
            let id = self
                .world
                .act_with(ADMIN, |c| c.add_membership(person, unit, setup_date()))
                .map_err(|e| format!("membership: {e}"))?;
            self.world.memberships.insert(name.clone(), id);
        }
        for r in strings(m, "roles") {
            self.grant_role(&name, &r, setup_date())?;
        }
        let devices = uint(m, "devices", 1)?;
        for _ in 0..devices {
            self.world.add_device(&name).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn grant_qualification(&mut self, name: &str, spec: &str) -> R<()> {
        let person = self.world.person(name).map_err(|e| e.to_string())?;
        let (q, start, end) = Self::qualification_spec(spec)?;
        let id = self
            .world
            .act_with(ADMIN, |c| c.grant_qualification(person, &q, start, end))
            .map_err(|e| format!("qualification {spec}: {e}"))?;
        self.world
            .qualification_grants
            .insert((name.to_owned(), q.to_string()), id);
        Ok(())
    }

    fn grant_role(&mut self, name: &str, spec: &str, start: Date) -> R<()> {
        let person = self.world.person(name).map_err(|e| e.to_string())?;
        let (role, unit) = self.role_spec(spec)?;
        let id = self
            .world
            .act_with(ADMIN, |c| c.grant_role(person, &role, unit, start))
            .map_err(|e| format!("role {spec}: {e}"))?;
        self.world
            .role_grants
            .insert((name.to_owned(), spec.to_owned()), id);
        Ok(())
    }

    /// `subject/form/period`.
    fn document(&self, who: &str, reference: &str) -> R<(Id, Projection)> {
        let parts: Vec<&str> = reference.split('/').collect();
        let [subject, form, p] = parts.as_slice() else {
            return Err(format!(
                "document reference `{reference}` must be subject/form/period"
            ));
        };
        let person = self.world.person(subject).map_err(|e| e.to_string())?;
        let proj = self.world.view(who, None).map_err(|e| e.to_string())?;
        let id = proj
            .document_by(&slug(form)?, person, period(p)?)
            .map(|(id, _)| *id)
            .ok_or_else(|| format!("{who} does not see {reference}"))?;
        Ok((id, proj))
    }

    fn slot_key(m: &Mapping) -> R<SlotKey> {
        Ok(SlotKey {
            occurrence: u16::try_from(uint(m, "occurrence", 0)?).map_err(|e| e.to_string())?,
            slot: slug(&req(m, "slot")?)?,
            index: u16::try_from(uint(m, "index", 0)?).map_err(|e| e.to_string())?,
        })
    }

    fn assignee(&self, m: &Mapping, key: &str) -> R<Option<Assignee>> {
        if let Some(p) = s(m, key) {
            if let Some(x) = self.world.externals.get(&p) {
                return Ok(Some(Assignee::External(*x)));
            }
            return Ok(Some(Assignee::Person(
                self.world.person(&p).map_err(|e| e.to_string())?,
            )));
        }
        Ok(None)
    }

    fn program(&self, m: &Mapping) -> R<Id> {
        let name = req(m, "program")?;
        self.world
            .programs
            .get(&name)
            .copied()
            .ok_or_else(|| format!("unknown program `{name}`"))
    }

    fn resource(&self, m: &Mapping) -> R<Id> {
        let name = req(m, "resource")?;
        self.world
            .resources
            .get(&name)
            .copied()
            .ok_or_else(|| format!("unknown resource `{name}`"))
    }

    fn batch_step_id(&self, form: &Slug, kind: StepKind) -> R<Slug> {
        let f = self
            .cfg
            .package
            .forms
            .forms
            .iter()
            .find(|x| &x.id == form)
            .ok_or("unknown form")?;
        let wf = self
            .cfg
            .package
            .workflows
            .workflows
            .iter()
            .find(|w| w.id == f.workflow)
            .ok_or("unknown workflow")?;
        wf.steps
            .iter()
            .find(|s| s.kind == kind)
            .map(|s| s.id.clone())
            .ok_or_else(|| "no such step".into())
    }

    /// Builds the closure for one action.
    fn action(&mut self, m: &Mapping) -> R<(String, BoxedAction)> {
        let who = req(m, "as")?;
        let what = req(m, "do")?;
        let f: BoxedAction = match what.as_str() {
            "document.fill" => {
                let form = match s(m, "form") {
                    Some(f) => slug(&f)?,
                    None => self.default_form()?,
                };
                let subject = self
                    .world
                    .person(&s(m, "subject").unwrap_or_else(|| who.clone()))
                    .map_err(|e| e.to_string())?;
                let p = period(&req(m, "period")?)?;
                let v = values(m, "values")?;
                Box::new(move |c| c.fill_document(&form, subject, p, v).map(|(_, e)| e))
            }
            "document.step" => {
                let reference = match s(m, "document") {
                    Some(r) => r,
                    None => {
                        let form = self.default_form()?;
                        format!(
                            "{}/{}/{}",
                            s(m, "subject").unwrap_or_else(|| who.clone()),
                            form,
                            req(m, "period")?
                        )
                    }
                };
                let (id, _) = self.document(&who, &reference)?;
                let step = slug(&req(m, "step")?)?;
                let note = s(m, "note");
                Box::new(move |c| c.step_document(id, &step, note))
            }
            "batch.forward" | "batch.register" => {
                let form = match s(m, "form") {
                    Some(f) => slug(&f)?,
                    None => self.default_form()?,
                };
                let kind = if what == "batch.forward" {
                    StepKind::Forward
                } else {
                    StepKind::Register
                };
                let step = self.batch_step_id(&form, kind)?;
                let unit = self.unit(&req(m, "group")?)?;
                let p = period(&req(m, "period")?)?;
                Box::new(move |c| c.batch_step(&form, unit, p, &step))
            }
            "aggregate.generate" | "aggregate.distribute" => {
                let a = slug(&req(m, "aggregate")?)?;
                let p = period(&req(m, "period")?)?;
                let note = s(m, "note");
                if what == "aggregate.generate" {
                    Box::new(move |c| c.generate_aggregate(&a, p, note))
                } else {
                    Box::new(move |c| c.distribute_aggregate(&a, p))
                }
            }
            "program.draft" => {
                let t = slug(&req(m, "type")?)?;
                let dates: Vec<Date> = strings(m, "dates")
                    .iter()
                    .map(|d| date(d))
                    .collect::<R<_>>()?;
                let name = req(m, "name")?;
                let w = &mut self.world;
                let id = w
                    .act_with(&who, |c| c.draft_program(&t, dates))
                    .map_err(|e| e.to_string())?;
                w.programs.insert(name, id);
                return Ok((who, Box::new(|_| Ok(Vec::new()))));
            }
            "program.assign" => {
                let program = self.program(m)?;
                let key = Self::slot_key(m)?;
                let assignee = self.assignee(m, "person")?.ok_or("missing `person`")?;
                let extra = values(m, "extra")?;
                Box::new(move |c| {
                    c.assign(
                        program,
                        vec![AssignmentSpec {
                            key,
                            assignee,
                            extra,
                        }],
                    )
                })
            }
            "program.send" => {
                let program = self.program(m)?;
                Box::new(move |c| c.send_assignments(program))
            }
            "program.publish" => {
                let program = self.program(m)?;
                Box::new(move |c| c.publish_program(program))
            }
            "assignment.respond" => {
                let program = self.program(m)?;
                let key = Self::slot_key(m)?;
                let assignee = match self.assignee(m, "assignee")? {
                    Some(a) => a,
                    None => Assignee::Person(self.world.person(&who).map_err(|e| e.to_string())?),
                };
                let t = slug(&req(m, "type")?)?;
                let response = match req(m, "response")?.as_str() {
                    "accepted" => Response::Accepted,
                    "declined" => Response::Declined { note: s(m, "note") },
                    other => return Err(format!("unknown response `{other}`")),
                };
                Box::new(move |c| c.respond(program, &t, key, assignee, response))
            }
            "external.add" => {
                let name = req(m, "name")?;
                let display = name.clone();
                let id = self
                    .world
                    .act_with(&who, |c| c.add_external(&display))
                    .map_err(|e| e.to_string())?;
                self.world.externals.insert(name, id);
                return Ok((who, Box::new(|_| Ok(Vec::new()))));
            }
            "resource.create" => {
                let t = slug(&req(m, "type")?)?;
                let attrs = values(m, "attributes")?;
                let name = req(m, "name")?;
                let id = self
                    .world
                    .act_with(&who, |c| c.upsert_resource(None, &t, attrs))
                    .map_err(|e| e.to_string())?;
                self.world.resources.insert(name, id);
                return Ok((who, Box::new(|_| Ok(Vec::new()))));
            }
            "resource.assign" => {
                let r = self.resource(m)?;
                let holder = self
                    .world
                    .person(&req(m, "holder")?)
                    .map_err(|e| e.to_string())?;
                Box::new(move |c| c.assign_resource(r, holder))
            }
            "resource.request" => {
                let r = self.resource(m)?;
                let t = slug(&req(m, "request")?)?;
                let days = uint(m, "due_days", 14)?;
                let name = req(m, "resource")?;
                let due =
                    core_engine::time::add_days(self.today(), i64::try_from(days).unwrap_or(14));
                let id = self
                    .world
                    .act_with(&who, |c| c.open_request(r, &t, due))
                    .map_err(|e| e.to_string())?;
                self.world.requests.insert(name, id);
                return Ok((who, Box::new(|_| Ok(Vec::new()))));
            }
            "resource.answer" => {
                let name = req(m, "resource")?;
                let request = *self.world.requests.get(&name).ok_or("no open request")?;
                let answer = slug(&req(m, "answer")?)?;
                Box::new(move |c| c.answer_request(request, &answer))
            }
            "resource.log" => {
                let r = self.resource(m)?;
                let v = values(m, "values")?;
                Box::new(move |c| c.log_resource(r, v))
            }
            "resource.return" => {
                let r = self.resource(m)?;
                Box::new(move |c| c.return_resource(r))
            }
            other => return Err(format!("unknown action `{other}`")),
        };
        Ok((who, f))
    }

    fn today(&self) -> Date {
        core_engine::time::date_of(self.world.events.last().map_or(0, |e| e.body.declared_time))
    }

    /// Organizational actions run directly on the world.
    fn org_action(&mut self, m: &Mapping, what: &str) -> R<bool> {
        match what {
            "role.grant" => {
                let person = req(m, "person")?;
                let start = match s(m, "from") {
                    Some(d) => date(&d)?,
                    None => self.today(),
                };
                self.grant_role(&person, &req(m, "role")?, start)?;
            }
            "role.end" => {
                let person = req(m, "person")?;
                let spec = req(m, "role")?;
                let grant = *self
                    .world
                    .role_grants
                    .get(&(person.clone(), spec.clone()))
                    .ok_or("unknown role grant")?;
                let end = date(&req(m, "at")?)?;
                self.world
                    .act(ADMIN, |c| c.end_role(grant, end))
                    .map_err(|e| e.to_string())?;
            }
            "qualification.grant" => {
                self.grant_qualification(&req(m, "person")?, &req(m, "qualification")?)?;
            }
            "qualification.end" => {
                let person = req(m, "person")?;
                let q = req(m, "qualification")?;
                let grant = *self
                    .world
                    .qualification_grants
                    .get(&(person, q))
                    .ok_or("unknown qualification grant")?;
                let end = date(&req(m, "at")?)?;
                self.world
                    .act(ADMIN, |c| c.end_qualification(grant, end))
                    .map_err(|e| e.to_string())?;
            }
            "membership.move" => {
                let name = req(m, "person")?;
                let person = self.world.person(&name).map_err(|e| e.to_string())?;
                let at = date(&req(m, "at")?)?;
                let unit = self.unit(&req(m, "group")?)?;
                if let Some(old) = self.world.memberships.get(&name).copied() {
                    self.world
                        .act(ADMIN, |c| c.end_membership(old, at))
                        .map_err(|e| e.to_string())?;
                }
                let id = self
                    .world
                    .act_with(ADMIN, |c| c.add_membership(person, unit, at))
                    .map_err(|e| e.to_string())?;
                self.world.memberships.insert(name, id);
            }
            "device.add" => {
                self.world
                    .add_device(&req(m, "person")?)
                    .map_err(|e| e.to_string())?;
            }
            "device.revoke" => {
                let index = usize::try_from(uint(m, "device", 0)?).map_err(|e| e.to_string())?;
                self.world
                    .revoke_device(&req(m, "person")?, index)
                    .map_err(|e| e.to_string())?;
            }
            "keys.complete" => {
                self.world.complete_channels().map_err(|e| e.to_string())?;
            }
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn run_action(&mut self, m: &Mapping) -> R<()> {
        if let Some(at) = s(m, "at") {
            self.world.set_date(date(&at)?);
        }
        let expect_denied = s(m, "expect").as_deref() == Some("denied");
        let what = req(m, "do")?;
        let result = match self.org_action(m, &what) {
            Ok(true) => Ok(()),
            Ok(false) => self.action(m).and_then(|(who, f)| {
                self.world
                    .act(&who, f)
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            }),
            Err(e) => Err(e),
        };
        match (result, expect_denied) {
            (Ok(()), false) => Ok(()),
            (Err(e), true) => {
                if std::env::var_os("SCENARIO_VERBOSE").is_some() {
                    eprintln!(
                        "    denied as expected ({}): {e}",
                        s(m, "do").unwrap_or_default()
                    );
                }
                if let Some(reason) = s(m, "reason") {
                    if !e.contains(&reason) {
                        return Err(format!("denied as expected but for another reason: {e}"));
                    }
                }
                Ok(())
            }
            (Ok(()), true) => Err("expected the action to be denied, but it succeeded".into()),
            (Err(e), false) => Err(e),
        }
    }

    fn run_concurrent(&mut self, items: &[Value]) -> R<()> {
        let mut actions = Vec::new();
        for item in items {
            let m = item
                .as_mapping()
                .ok_or("concurrent items must be actions")?;
            if let Some(at) = s(m, "at") {
                self.world.set_date(date(&at)?);
            }
            actions.push(self.action(m)?);
        }
        self.world.concurrent(actions).map_err(|e| e.to_string())
    }

    // ----- checks ---------------------------------------------------------------

    fn view(&self, m: &Mapping) -> R<Projection> {
        let who = req(m, "who")?;
        let device = match m.get("device") {
            Some(_) => Some(usize::try_from(uint(m, "device", 0)?).map_err(|e| e.to_string())?),
            None => None,
        };
        self.world.view(&who, device).map_err(|e| e.to_string())
    }

    fn sees(&self, m: &Mapping, proj: &Projection) -> R<bool> {
        if let Some(r) = s(m, "document") {
            let parts: Vec<&str> = r.split('/').collect();
            let [subject, form, p] = parts.as_slice() else {
                return Err("document reference must be subject/form/period".into());
            };
            let person = self.world.person(subject).map_err(|e| e.to_string())?;
            return Ok(proj.document_by(&slug(form)?, person, period(p)?).is_some());
        }
        if s(m, "program").is_some() {
            let id = self.program(m)?;
            return Ok(proj.published.contains_key(&id)
                || proj.programs.contains_key(&id)
                || proj.assigned_snapshots.contains_key(&id));
        }
        if let Some(a) = s(m, "aggregate") {
            let (id, p) = a
                .split_once('/')
                .ok_or("aggregate reference must be id/period")?;
            let agg = proj.aggregates.get(&(slug(id)?, period(p)?));
            return Ok(agg.is_some_and(|x| x.distributed.is_some() || x.generated.is_some()));
        }
        if s(m, "resource").is_some() {
            let id = self.resource(m)?;
            return Ok(proj.resources.contains_key(&id));
        }
        Err("nothing to read: give document, program, aggregate or resource".into())
    }

    fn check(&self, c: &Value) -> R<()> {
        let map = c.as_mapping().ok_or("a check must be a mapping")?;
        let (kind, body) = map.iter().next().ok_or("empty check")?;
        let kind = kind.as_str().ok_or("check name must be a string")?;
        let m = body
            .as_mapping()
            .ok_or("check arguments must be a mapping")?;
        match kind {
            "can_read" | "cannot_read" => {
                let proj = self.view(m)?;
                let sees = self.sees(m, &proj)?;
                let want = kind == "can_read";
                if sees != want {
                    return Err(format!(
                        "{} {} read it",
                        req(m, "who")?,
                        if sees { "can" } else { "cannot" }
                    ));
                }
            }
            "state" | "variant" | "value" => {
                let reference = req(m, "document")?;
                let who = s(m, "who")
                    .unwrap_or_else(|| reference.split('/').next().unwrap_or_default().to_owned());
                let (id, proj) = self.document(&who, &reference)?;
                let doc = proj.documents.get(&id).ok_or("document vanished")?;
                match kind {
                    "state" => {
                        let want = req(m, "is")?;
                        if doc.state.as_str() != want {
                            return Err(format!("state is {}, expected {want}", doc.state));
                        }
                    }
                    "variant" => {
                        let want = req(m, "is")?;
                        if doc.content.variant.as_str() != want {
                            return Err(format!(
                                "variant is {}, expected {want}",
                                doc.content.variant
                            ));
                        }
                    }
                    _ => {
                        let field = slug(&req(m, "field")?)?;
                        let want = scalar(m.get("is").ok_or("missing `is`")?)?;
                        let have = doc.content.values.get(&field).cloned().unwrap_or_default();
                        if !have.loose_eq(&want) {
                            return Err(format!("{field} is {have:?}, expected {want:?}"));
                        }
                    }
                }
            }
            "aggregate" => {
                let proj = self.view(m)?;
                let id = slug(&req(m, "id")?)?;
                let p = period(&req(m, "period")?)?;
                let agg = proj
                    .aggregates
                    .get(&(id, p))
                    .ok_or("aggregate not visible")?;
                let snap = agg
                    .distributed
                    .as_ref()
                    .or(agg.generated.as_ref())
                    .ok_or("aggregate not visible")?;
                if let Some(v) = m.get("version") {
                    let want = v.as_u64().ok_or("version must be a number")?;
                    if u64::from(snap.version) != want {
                        return Err(format!("version is {}, expected {want}", snap.version));
                    }
                }
                if let Some(Value::Mapping(measures)) = m.get("measures") {
                    let group = s(m, "group").unwrap_or_default();
                    let g = snap
                        .result
                        .groups
                        .get(&group)
                        .ok_or_else(|| format!("no group `{group}`"))?;
                    for (k, v) in measures {
                        let k = slug(k.as_str().ok_or("measure names are strings")?)?;
                        let want = v.as_f64().ok_or("measure values are numbers")?;
                        let have = g.get(&k).copied().unwrap_or(0.0);
                        if (have - want).abs() > 1e-9 {
                            return Err(format!("{k} is {have}, expected {want}"));
                        }
                    }
                }
                let missing: BTreeSet<&str> = snap
                    .result
                    .missing
                    .iter()
                    .filter_map(|p| self.world.name_of(*p))
                    .collect();
                for n in strings(m, "missing") {
                    if !missing.contains(n.as_str()) {
                        return Err(format!(
                            "{n} is not listed as missing (missing: {missing:?})"
                        ));
                    }
                }
                for n in strings(m, "not_missing") {
                    if missing.contains(n.as_str()) {
                        return Err(format!("{n} is listed as missing"));
                    }
                }
                if let Some(d) = m.get("distributed") {
                    let want = d.as_bool().ok_or("distributed must be a boolean")?;
                    if agg.distributed.is_some() != want {
                        return Err(format!("distributed is {}", agg.distributed.is_some()));
                    }
                }
            }
            "batch_steps" => {
                let proj = self.view(m)?;
                let form = match s(m, "form") {
                    Some(f) => slug(&f)?,
                    None => self.default_form()?,
                };
                let unit = *self
                    .world
                    .units
                    .get(&req(m, "group")?)
                    .ok_or("unknown group")?;
                let p = period(&req(m, "period")?)?;
                let step = req(m, "step")?;
                let count = proj
                    .batches
                    .get(&core_engine::ops::BatchKey {
                        form,
                        unit,
                        period: p,
                    })
                    .map_or(0, |b| {
                        b.records.iter().filter(|r| r.step.as_str() == step).count()
                    });
                let want = uint(m, "count", 1)?;
                if count as u64 != want {
                    return Err(format!("{count} `{step}` records, expected {want}"));
                }
            }
            "superseded" => {
                let proj = self.view(m)?;
                let n = proj
                    .verdicts
                    .values()
                    .filter(|v| **v == Verdict::Superseded)
                    .count();
                let want = uint(m, "count", 1)?;
                if n as u64 != want {
                    return Err(format!("{n} superseded events, expected {want}"));
                }
            }
            "holder" => {
                let proj = self.view(m)?;
                let id = self.resource(m)?;
                let r = proj.resources.get(&id).ok_or("resource not visible")?;
                let have = r
                    .holding
                    .as_ref()
                    .and_then(|h| self.world.name_of(h.holder))
                    .unwrap_or("none");
                let want = req(m, "is")?;
                if have != want {
                    return Err(format!("holder is {have}, expected {want}"));
                }
                if let Some(n) = m.get("history") {
                    let want = n.as_u64().ok_or("history must be a number")?;
                    if r.history.len() as u64 != want {
                        return Err(format!(
                            "history has {} entries, expected {want}",
                            r.history.len()
                        ));
                    }
                }
                if let Some(n) = m.get("logs") {
                    let want = n.as_u64().ok_or("logs must be a number")?;
                    let logs = r
                        .history
                        .iter()
                        .chain(r.holding.iter())
                        .map(|h| h.logs.len())
                        .sum::<usize>();
                    if logs as u64 != want {
                        return Err(format!("{logs} logs, expected {want}"));
                    }
                }
            }
            "response" => {
                let proj = self.view(m)?;
                let id = self.program(m)?;
                let key = Self::slot_key(m)?;
                let have = match proj.responses.get(&(id, key)) {
                    None => "none",
                    Some(Response::Accepted) => "accepted",
                    Some(Response::Declined { .. }) => "declined",
                };
                let want = req(m, "is")?;
                if have != want {
                    return Err(format!("response is {have}, expected {want}"));
                }
            }
            "on_board" => {
                let proj = self.view(m)?;
                let id = self.program(m)?;
                let have = proj.board.get(&id).is_some_and(|b| !b.withdrawn);
                let want = m.get("is").and_then(Value::as_bool).unwrap_or(true);
                if have != want {
                    return Err(format!("on the board: {have}, expected {want}"));
                }
            }
            "notified" => {
                let proj = self.view(m)?;
                let person = self
                    .world
                    .person(&req(m, "who")?)
                    .map_err(|e| e.to_string())?;
                let text = req(m, "text")?;
                let have = core_engine::notify::notifications_for(&proj, person)
                    .iter()
                    .any(|t| t.text.as_str() == text);
                let want = m.get("is").and_then(Value::as_bool).unwrap_or(true);
                if have != want {
                    return Err(format!("notified with {text}: {have}, expected {want}"));
                }
            }
            "reminded" => {
                let proj = self.view(m)?;
                let person = self
                    .world
                    .person(&req(m, "who")?)
                    .map_err(|e| e.to_string())?;
                let on = date(&req(m, "on")?)?;
                let text = req(m, "text")?;
                let have = core_engine::notify::due_reminders(&proj, on)
                    .iter()
                    .any(|n| n.text.as_str() == text && n.recipients.contains(&person));
                let want = m.get("is").and_then(Value::as_bool).unwrap_or(true);
                if have != want {
                    return Err(format!(
                        "reminded with {text} on {on}: {have}, expected {want}"
                    ));
                }
            }
            "uncovered" => {
                let proj = self.view(m)?;
                let on = date(&req(m, "on")?)?;
                let role = req(m, "role")?;
                let unit = match s(m, "group") {
                    Some(g) => Some(*self.world.units.get(&g).ok_or("unknown group")?),
                    None => None,
                };
                let have = core_engine::notify::coverage_alerts(&proj, on).iter().any(|a| {
                    matches!(a, core_engine::notify::CoverageAlert::Uncovered { role: r, unit: u } if r.as_str() == role && *u == unit)
                });
                let want = m.get("is").and_then(Value::as_bool).unwrap_or(true);
                if have != want {
                    return Err(format!("{role} uncovered on {on}: {have}, expected {want}"));
                }
            }
            other => return Err(format!("unknown check `{other}`")),
        }
        Ok(())
    }

    /// Runs one parsed scenario.
    pub fn run(bundle: Vec<u8>, scenario: &Value) -> Result<(), ScenarioError> {
        let top = scenario
            .as_mapping()
            .ok_or_else(|| err("scenario", "must be a mapping"))?;
        let given = top
            .get("given")
            .and_then(Value::as_mapping)
            .ok_or_else(|| err("given", "missing"))?;
        let d = date(&req(given, "date").map_err(|e| err("given", e))?)
            .map_err(|e| err("given.date", e))?;
        let mut r = Runner::new(bundle, d)?;
        if let Some(Value::Sequence(people)) = given.get("people") {
            for (i, p) in people.iter().enumerate() {
                let m = p
                    .as_mapping()
                    .ok_or_else(|| err(&format!("given.people[{i}]"), "must be a mapping"))?;
                r.add_person(m)
                    .map_err(|e| err(&format!("given.people[{i}]"), e))?;
            }
        }
        for n in strings(given, "externals") {
            let display = n.clone();
            let id = r
                .world
                .act_with(ADMIN, |c| c.add_external(&display))
                .map_err(|e| err("given.externals", e.to_string()))?;
            r.world.externals.insert(n, id);
        }
        r.world
            .complete_channels()
            .map_err(|e| err("given", e.to_string()))?;
        r.world.set_date(r.default_date);
        if let Some(Value::Sequence(steps)) = top.get("when") {
            for (i, step) in steps.iter().enumerate() {
                let ctx = format!("when[{i}]");
                let m = step
                    .as_mapping()
                    .ok_or_else(|| err(&ctx, "must be a mapping"))?;
                if let Some(c) = m.get("check") {
                    r.check(c).map_err(|e| err(&ctx, e))?;
                } else if let Some(Value::Sequence(items)) = m.get("concurrent") {
                    r.run_concurrent(items).map_err(|e| err(&ctx, e))?;
                } else {
                    r.run_action(m).map_err(|e| {
                        err(&format!("{ctx} ({})", s(m, "do").unwrap_or_default()), e)
                    })?;
                }
            }
        }
        if let Some(Value::Sequence(checks)) = top.get("then") {
            for (i, c) in checks.iter().enumerate() {
                r.check(c).map_err(|e| err(&format!("then[{i}]"), e))?;
            }
        }
        Ok(())
    }
}

/// Parses and runs a scenario file's text.
pub fn run_scenario(bundle: Vec<u8>, text: &str) -> Result<String, ScenarioError> {
    let v: Value = serde_norway::from_str(text).map_err(|e| err("yaml", e.to_string()))?;
    let name = v
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("scenario")
        .to_owned();
    Runner::run(bundle, &v)?;
    Ok(name)
}
