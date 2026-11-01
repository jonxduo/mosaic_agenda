//! Multi-device simulation (§15.3).
//!
//! 1. **Activity**: a fictional organization is generated from the package
//!    (population profiles in `simulation.yaml`) and its activity is run
//!    through the real actions: every event is real (signed, encrypted,
//!    validated by the engine).
//! 2. **Propagation**: the events are replayed minute by minute over a
//!    realistic availability model. Archive desktops are always online;
//!    phones are online a few short sessions a day, plus push wake-ups
//!    (Android: background sync; iOS: a visible notification the person may
//!    open). Any two online devices can reach each other through the relay,
//!    and a sync makes both know the union of their events (the protocol
//!    that does this is tested in `core-sync`).
//!
//! The report gives delivery delays by event kind, the share of documents
//! reaching their new audience within 24 hours, storage per device and the
//! number of invalid or conflicting events.

use std::cmp::Reverse;
use std::collections::{BTreeMap, BTreeSet, BinaryHeap};
use std::fmt;

use core_crypto::DeviceKeys;
use core_engine::Verdict;
use core_engine::ops::{Assignee, AssignmentSpec, Response, SlotKey, Values};
use core_engine::rules::{expected_subjects, step_actors};
use core_engine::time::{PeriodKey, add_days, date_of, ms_of};
use core_events::{EventKind, PayloadChannel};
use core_model::config::{Periodicity, ProgramPeriod, Repeat, StepKind};
use core_model::strings::ScopeDef;
use core_model::{Id, Permission, Scalar, Slug, UnitId};
use serde::Deserialize;
use time::{Date, Weekday};

use crate::world::{ADMIN, Device, World};

const MINUTE: i64 = 60_000;
const DAY_MIN: i64 = 1440;

pub use core_engine::time::parse_date;

/// Small deterministic generator (xorshift64*).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.max(1))
    }
    pub fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        if n == 0 { 0 } else { self.next_u64() % n }
    }
    pub fn unit(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }
    pub fn chance(&mut self, p: f64) -> bool {
        self.unit() < p
    }
    pub fn range(&mut self, a: i64, b: i64) -> i64 {
        a + i64::try_from(self.below(u64::try_from((b - a).max(1)).unwrap_or(1))).unwrap_or(0)
    }
    fn pick<T: Clone>(&mut self, v: &[T]) -> Option<T> {
        if v.is_empty() {
            None
        } else {
            v.get(self.below(v.len() as u64) as usize).cloned()
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    weight: u32,
    qualifications: Vec<String>,
    #[serde(default)]
    attributes: BTreeMap<String, serde_norway::Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoleSpec {
    role: String,
    #[serde(default)]
    per_unit: bool,
}

fn default_ios() -> f64 {
    0.4
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Population {
    groups: usize,
    #[serde(default = "default_ios")]
    ios_share: f64,
    profiles: Vec<Profile>,
    #[serde(default)]
    roles: Vec<RoleSpec>,
    #[serde(default)]
    archive_roles: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SimOptions {
    /// Persons, one phone each.
    pub devices: usize,
    pub days: u32,
    pub seed: u64,
    pub start: Date,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KindStats {
    pub kind: String,
    pub events: usize,
    /// (event, recipient device) pairs.
    pub pairs: usize,
    pub median_min: Option<f64>,
    pub p95_min: Option<f64>,
    pub within_24h: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub persons: usize,
    pub phones: usize,
    pub archives: usize,
    pub days: u32,
    pub setup_events: usize,
    pub activity_events: usize,
    pub refused_actions: BTreeMap<String, usize>,
    pub kinds: Vec<KindStats>,
    /// Share of documents reaching their new audience within 24 hours.
    pub documents_within_24h: f64,
    pub storage_avg_bytes: u64,
    pub storage_per_year_mb: f64,
    pub invalid: usize,
    pub superseded: usize,
    pub quarantined: usize,
    pub generation_secs: f64,
    pub propagation_secs: f64,
}

impl Report {
    /// The F4 objective: 95% of documents within 24 hours.
    pub fn meets_target(&self) -> bool {
        self.documents_within_24h >= 0.95
    }
}

fn fmt_min(v: Option<f64>) -> String {
    match v {
        None => "-".into(),
        Some(m) if m >= 60.0 * 24.0 * 3.0 => "> 3 g".into(),
        Some(m) if m >= 120.0 => format!("{:.1} h", m / 60.0),
        Some(m) => format!("{m:.0} min"),
    }
}

impl fmt::Display for Report {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "Simulation: {} persons ({} phones, {} archive desktops), {} days",
            self.persons, self.phones, self.archives, self.days
        )?;
        writeln!(
            f,
            "Events: {} setup, {} activity",
            self.setup_events, self.activity_events
        )?;
        if !self.refused_actions.is_empty() {
            writeln!(f, "Refused actions: {:?}", self.refused_actions)?;
        }
        writeln!(
            f,
            "\n{:<28} {:>7} {:>8} {:>10} {:>10} {:>8}",
            "kind", "events", "pairs", "median", "p95", "<=24h"
        )?;
        for k in &self.kinds {
            writeln!(
                f,
                "{:<28} {:>7} {:>8} {:>10} {:>10} {:>7.1}%",
                k.kind,
                k.events,
                k.pairs,
                fmt_min(k.median_min),
                fmt_min(k.p95_min),
                k.within_24h * 100.0
            )?;
        }
        writeln!(
            f,
            "\nDocuments reaching their audience within 24 h: {:.1}% (target 95%: {})",
            self.documents_within_24h * 100.0,
            if self.meets_target() {
                "met"
            } else {
                "NOT met"
            }
        )?;
        writeln!(
            f,
            "Storage per device: {:.1} KB now, ~{:.1} MB per year",
            self.storage_avg_bytes as f64 / 1024.0,
            self.storage_per_year_mb
        )?;
        writeln!(
            f,
            "Invalid events: {}, superseded: {}, quarantined: {}",
            self.invalid, self.superseded, self.quarantined
        )?;
        write!(
            f,
            "Time: generation {:.1}s, propagation {:.1}s",
            self.generation_secs, self.propagation_secs
        )
    }
}

fn slug(s: &str) -> Result<Slug, String> {
    Slug::new(s).map_err(|e| e.to_string())
}

fn scalar(v: &serde_norway::Value) -> Scalar {
    match v {
        serde_norway::Value::Bool(b) => Scalar::Bool(*b),
        serde_norway::Value::Number(n) => n
            .as_i64()
            .map_or(Scalar::Num(n.as_f64().unwrap_or(0.0)), Scalar::Int),
        serde_norway::Value::String(s) => Scalar::Str(s.clone()),
        _ => Scalar::Null,
    }
}

/// Plausible values for a variant's fields.
fn values_for(fields: &[core_model::config::FieldDef], rng: &mut Rng) -> Values {
    use core_model::config::FieldType as T;
    let mut out = Values::new();
    for f in fields {
        let lo = f.min.unwrap_or(0.0) as i64;
        let v = match f.ty {
            T::Integer => Scalar::Int(rng.range(lo, lo + 4)),
            T::Duration => Scalar::Int(rng.range(lo.max(0), 6000)),
            T::Decimal => Scalar::Num(rng.range(lo, lo + 10) as f64),
            T::Boolean => Scalar::Bool(rng.chance(0.9)),
            _ => continue,
        };
        if f.max.is_some_and(|m| v.as_f64().unwrap_or(0.0) > m) {
            continue;
        }
        out.insert(f.id.clone(), v);
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Plan {
    Report {
        person: String,
        form: Slug,
        period: PeriodKey,
    },
    Batch {
        unit: UnitId,
        form: Slug,
        period: PeriodKey,
        kind: u8,
    },
    Aggregate {
        aggregate: Slug,
        period: PeriodKey,
    },
    Program {
        program_type: Slug,
        dates: Vec<Date>,
    },
    Respond {
        person: String,
        program: Id,
        program_type: Slug,
        key: SlotKey,
    },
    Publish {
        program: Id,
    },
}

fn weekday_of(rrule: &str) -> Option<Weekday> {
    let by = rrule.split(';').find_map(|p| p.strip_prefix("BYDAY="))?;
    Some(match by.split(',').next()? {
        "MO" => Weekday::Monday,
        "TU" => Weekday::Tuesday,
        "WE" => Weekday::Wednesday,
        "TH" => Weekday::Thursday,
        "FR" => Weekday::Friday,
        "SA" => Weekday::Saturday,
        _ => Weekday::Sunday,
    })
}

struct Gen {
    w: World,
    rng: Rng,
    names: Vec<String>,
    units: Vec<UnitId>,
    refused: BTreeMap<String, usize>,
}

impl Gen {
    fn refuse(&mut self, what: &str) {
        *self.refused.entry(what.to_owned()).or_default() += 1;
    }

    fn admin_view(&self) -> Result<core_engine::Projection, String> {
        self.w.view(ADMIN, None).map_err(|e| e.to_string())
    }

    /// Persons (with a device) able to perform a workflow step on a unit's batch.
    fn actors(
        &self,
        proj: &core_engine::Projection,
        form: &core_model::config::FormDef,
        by: &Slug,
        unit: Option<UnitId>,
        date: Date,
    ) -> Vec<String> {
        let cfg = match proj.cfg() {
            Some(c) => c,
            None => return Vec::new(),
        };
        let set = step_actors(&proj.auth, cfg, form, by, None, unit, date).unwrap_or_default();
        self.names
            .iter()
            .filter(|n| {
                self.w
                    .persons
                    .get(*n)
                    .is_some_and(|p| set.contains(p) && !proj.auth.is_root(*p))
            })
            .cloned()
            .collect()
    }

    fn holders(
        &self,
        proj: &core_engine::Projection,
        perm: Permission,
        object: &core_model::strings::ObjectRef,
        date: Date,
    ) -> Vec<String> {
        let Some(cfg) = proj.cfg() else {
            return Vec::new();
        };
        self.names
            .iter()
            .filter(|n| {
                self.w.persons.get(*n).is_some_and(|p| {
                    !proj.auth.is_root(*p)
                        && proj
                            .auth
                            .is_allowed(cfg, *p, perm, Some(object), None, date)
                })
            })
            .cloned()
            .collect()
    }
}

/// Runs the simulation.
pub fn run(bundle: Vec<u8>, population_yaml: &str, opts: &SimOptions) -> Result<Report, String> {
    let started = std::time::Instant::now();
    let pop: Population =
        serde_norway::from_str(population_yaml).map_err(|e| format!("simulation.yaml: {e}"))?;
    let compiled = core_config::decode(&bundle).map_err(|e| e.to_string())?;
    let pkg = compiled.package.clone();
    let setup_date = add_days(opts.start, -1);
    let mut g = Gen {
        w: World::new(bundle, setup_date).map_err(|e| e.to_string())?,
        rng: Rng::new(opts.seed),
        names: Vec::new(),
        units: Vec::new(),
        refused: BTreeMap::new(),
    };
    let since = core_engine::time::parse_date("2020-01-01").unwrap_or(Date::MIN);

    // ----- organization -------------------------------------------------------------
    let unit_type = pkg
        .forms
        .forms
        .iter()
        .find_map(|f| f.batching.as_ref().map(|b| b.unit_type.clone()))
        .or_else(|| pkg.units.unit_types.first().map(|u| u.id.clone()));
    let n = opts.devices.max(2);
    if let Some(t) = &unit_type {
        for i in 0..pop.groups.clamp(1, n) {
            let u =
                g.w.act_with(ADMIN, |c| c.create_unit(t, &format!("u{i}")))
                    .map_err(|e| e.to_string())?;
            g.units.push(u);
        }
    }
    let total_weight: u32 = pop.profiles.iter().map(|p| p.weight).sum::<u32>().max(1);
    for i in 0..n {
        let mut roll = g.rng.below(u64::from(total_weight)) as u32;
        let profile = pop
            .profiles
            .iter()
            .find(|p| {
                if roll < p.weight {
                    true
                } else {
                    roll -= p.weight;
                    false
                }
            })
            .or(pop.profiles.first())
            .ok_or("no profiles")?;
        let name = format!("p{i}");
        let attrs: Values = profile
            .attributes
            .iter()
            .filter_map(|(k, v)| Some((Slug::new(k).ok()?, scalar(v))))
            .collect();
        let person =
            g.w.act_with(ADMIN, |c| c.add_person(&name, attrs))
                .map_err(|e| e.to_string())?;
        g.w.persons.insert(name.clone(), person);
        for q in &profile.qualifications {
            let q = slug(q)?;
            if g.w
                .act_with(ADMIN, |c| c.grant_qualification(person, &q, since, None))
                .is_err()
            {
                g.refuse("qualification.grant");
            }
        }
        if let Some(u) = g.units.get(i % g.units.len().max(1)).copied() {
            if g.w
                .act_with(ADMIN, |c| c.add_membership(person, u, since))
                .is_err()
            {
                g.refuse("membership.start");
            }
        }
        g.names.push(name);
    }
    // Roles to those who meet the requirements.
    let mut holders_of: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for spec in &pop.roles {
        let role = slug(&spec.role)?;
        let Some(def) = pkg.roles.roles.iter().find(|r| r.id == role) else {
            return Err(format!("simulation.yaml: unknown role `{}`", spec.role));
        };
        let scopes: Vec<Option<UnitId>> = match (&def.scope, spec.per_unit) {
            (ScopeDef::UnitType(_), _) => g.units.iter().map(|u| Some(*u)).collect(),
            _ => vec![None],
        };
        for scope in scopes {
            let proj = g.admin_view()?;
            let Some(cfg) = proj.cfg().cloned() else {
                continue;
            };
            let mut candidates: Vec<String> = g
                .names
                .iter()
                .filter(|n| {
                    let Some(p) = g.w.persons.get(*n) else {
                        return false;
                    };
                    let in_unit = scope.is_none_or(|u| proj.auth.org.is_member(*p, u, opts.start));
                    in_unit && proj.auth.satisfies(&cfg, *p, def.requires.as_ref(), since)
                })
                .cloned()
                .collect();
            for i in (1..candidates.len()).rev() {
                let j = g.rng.below(i as u64 + 1) as usize;
                candidates.swap(i, j);
            }
            for c in candidates.iter().take(6) {
                let p = g.w.persons[c];
                if g.w
                    .act_with(ADMIN, |cl| cl.grant_role(p, &role, scope, since))
                    .is_ok()
                {
                    holders_of
                        .entry(spec.role.clone())
                        .or_default()
                        .insert(c.clone());
                    break;
                }
            }
        }
    }
    // Devices: a phone each, plus archive desktops.
    let mut archives: BTreeSet<(String, usize)> = BTreeSet::new();
    let archive_people: BTreeSet<String> = pop
        .archive_roles
        .iter()
        .flat_map(|r| holders_of.get(r).cloned().unwrap_or_default())
        .collect();
    for name in g.names.clone() {
        let person = g.w.persons[&name];
        let count = if archive_people.contains(&name) { 2 } else { 1 };
        for idx in 0..count {
            let keys = DeviceKeys::generate().map_err(|e| e.to_string())?;
            let identity = keys.identity();
            g.w.act(ADMIN, |c| c.add_device(person, identity))
                .map_err(|e| e.to_string())?;
            if idx == 1 {
                g.w.act(ADMIN, |c| c.set_archive(identity.signing, true))
                    .map_err(|e| e.to_string())?;
                archives.insert((name.clone(), idx));
            }
            g.w.devices.entry(name.clone()).or_default().push(Device {
                keys,
                revoked: false,
            });
        }
    }
    g.w.complete_channels().map_err(|e| e.to_string())?;
    let setup_len = g.w.events.len();

    // ----- activity plan ------------------------------------------------------------
    let t0 = ms_of(opts.start);
    let end = t0 + i64::from(opts.days) * DAY_MIN * MINUTE;
    let mut heap: BinaryHeap<Reverse<(i64, u64, Plan)>> = BinaryHeap::new();
    let mut seq = 0u64;
    let mut push = |heap: &mut BinaryHeap<Reverse<(i64, u64, Plan)>>, t: i64, p: Plan| {
        seq += 1;
        heap.push(Reverse((t, seq, p)));
    };
    let at = |d: Date, minute: i64| ms_of(d) + minute * MINUTE;
    let proj = g.admin_view()?;
    let cfg = proj.cfg().cloned().ok_or("no configuration")?;
    // Periodic forms: the month (week) before each window inside the simulation.
    for form in pkg
        .forms
        .forms
        .iter()
        .filter(|f| f.periodicity == Periodicity::Monthly)
    {
        let mut period = PeriodKey::month_of(add_days(opts.start, -1));
        while let Some(open) = period.next().and_then(|p| p.start()) {
            if ms_of(open) >= end {
                break;
            }
            let subjects = expected_subjects(&proj.auth, &cfg, form, &period, None);
            for name in &g.names {
                if subjects.contains(&g.w.persons[name]) {
                    let day = add_days(open, g.rng.range(0, 9));
                    let t = at(day, g.rng.range(7 * 60, 22 * 60));
                    push(
                        &mut heap,
                        t,
                        Plan::Report {
                            person: name.clone(),
                            form: form.id.clone(),
                            period,
                        },
                    );
                }
            }
            if form.batching.is_some() {
                for u in &g.units {
                    let t = at(
                        add_days(open, 8 + g.rng.range(0, 3)),
                        g.rng.range(18 * 60, 22 * 60),
                    );
                    push(
                        &mut heap,
                        t,
                        Plan::Batch {
                            unit: *u,
                            form: form.id.clone(),
                            period,
                            kind: 0,
                        },
                    );
                    let t = at(
                        add_days(open, 12 + g.rng.range(0, 3)),
                        g.rng.range(18 * 60, 22 * 60),
                    );
                    push(
                        &mut heap,
                        t,
                        Plan::Batch {
                            unit: *u,
                            form: form.id.clone(),
                            period,
                            kind: 1,
                        },
                    );
                }
            }
            for a in pkg
                .aggregates
                .aggregates
                .iter()
                .filter(|a| a.source.form == form.id)
            {
                push(
                    &mut heap,
                    at(add_days(open, 16), 20 * 60),
                    Plan::Aggregate {
                        aggregate: a.id.clone(),
                        period,
                    },
                );
            }
            match period.next() {
                Some(p) => period = p,
                None => break,
            }
        }
    }
    // Programs: each week (or month), drafted and assigned a week ahead.
    for t in &pkg.programs.program_types {
        let weekdays: Vec<Weekday> = t
            .event_type
            .iter()
            .filter_map(|e| pkg.calendar.event_types.iter().find(|x| &x.id == e))
            .filter_map(|e| e.default_recurrence.as_deref().and_then(weekday_of))
            .collect();
        let weekdays = if weekdays.is_empty() {
            vec![Weekday::Monday]
        } else {
            weekdays
        };
        let mut day = opts.start;
        while ms_of(day) < end {
            let span = match t.period {
                ProgramPeriod::Week => 7,
                ProgramPeriod::Month => 28,
            };
            let dates: Vec<Date> = (7..7 + span)
                .map(|k| add_days(day, k))
                .filter(|d| weekdays.contains(&d.weekday()))
                .collect();
            if !dates.is_empty() {
                push(
                    &mut heap,
                    at(day, g.rng.range(19 * 60, 22 * 60)),
                    Plan::Program {
                        program_type: t.id.clone(),
                        dates,
                    },
                );
            }
            day = add_days(day, span);
        }
    }

    // ----- run ------------------------------------------------------------------------
    while let Some(Reverse((t, _, plan))) = heap.pop() {
        // Only activity inside the simulated window is measured.
        if t >= end {
            continue;
        }
        g.w.set_time(t);
        let date = date_of(t);
        match plan {
            Plan::Report {
                person,
                form,
                period,
            } => {
                let p = g.w.persons[&person];
                let Some(fdef) = pkg.forms.forms.iter().find(|f| f.id == form) else {
                    continue;
                };
                let view = g.w.view(&person, None).map_err(|e| e.to_string())?;
                let variant = core_engine::rules::variant_for(
                    &view.auth,
                    &cfg,
                    fdef,
                    p,
                    period.last_day().unwrap_or(date),
                );
                let values = variant
                    .map(|v| values_for(&v.fields, &mut g.rng))
                    .unwrap_or_default();
                match g
                    .w
                    .act_with(&person, |c| c.fill_document(&form, p, period, values))
                {
                    Ok(doc) => {
                        let wf = pkg
                            .workflows
                            .workflows
                            .iter()
                            .find(|w| w.id == fdef.workflow);
                        let step = wf.and_then(|w| {
                            w.steps.iter().find(|s| {
                                s.kind == StepKind::Transition && s.from.contains(&w.initial)
                            })
                        });
                        if let Some(step) = step {
                            g.w.set_time(t + MINUTE);
                            if g.w
                                .act(&person, |c| c.step_document(doc, &step.id, None))
                                .is_err()
                            {
                                g.refuse("document.step");
                            }
                        }
                    }
                    Err(_) => g.refuse("document.fill"),
                }
            }
            Plan::Batch {
                unit,
                form,
                period,
                kind,
            } => {
                let Some(fdef) = pkg.forms.forms.iter().find(|f| f.id == form) else {
                    continue;
                };
                let Some(wf) = pkg
                    .workflows
                    .workflows
                    .iter()
                    .find(|w| w.id == fdef.workflow)
                else {
                    continue;
                };
                let want = if kind == 0 {
                    StepKind::Forward
                } else {
                    StepKind::Register
                };
                let Some(step) = wf.steps.iter().find(|s| s.kind == want) else {
                    continue;
                };
                let Some(by) = &step.by else { continue };
                let proj = g.admin_view()?;
                let actors = g.actors(&proj, fdef, by, Some(unit), date);
                let Some(actor) = g.rng.pick(&actors) else {
                    g.refuse("batch: nobody can act");
                    continue;
                };
                if g.w
                    .act(&actor, |c| c.batch_step(&form, unit, period, &step.id))
                    .is_err()
                {
                    g.refuse(if kind == 0 {
                        "batch.forward"
                    } else {
                        "batch.register"
                    });
                }
            }
            Plan::Aggregate { aggregate, period } => {
                let proj = g.admin_view()?;
                let obj = core_model::strings::ObjectRef {
                    kind: core_model::strings::ObjectKind::Aggregate,
                    id: aggregate.clone(),
                };
                let Some(actor) = g
                    .holders(&proj, Permission::AggregateGenerate, &obj, date)
                    .first()
                    .cloned()
                else {
                    continue;
                };
                if g.w
                    .act(&actor, |c| c.generate_aggregate(&aggregate, period, None))
                    .is_err()
                {
                    g.refuse("aggregate.generate");
                    continue;
                }
                g.w.set_time(t + MINUTE);
                if g.w
                    .act(&actor, |c| c.distribute_aggregate(&aggregate, period))
                    .is_err()
                {
                    g.refuse("aggregate.distribute");
                }
            }
            Plan::Program {
                program_type,
                dates,
            } => {
                let proj = g.admin_view()?;
                let obj = core_model::strings::ObjectRef {
                    kind: core_model::strings::ObjectKind::Program,
                    id: program_type.clone(),
                };
                let owners = g.holders(&proj, Permission::ProgramDraft, &obj, date);
                let Some(owner) = g.rng.pick(&owners) else {
                    continue;
                };
                let Some(def) = pkg
                    .programs
                    .program_types
                    .iter()
                    .find(|x| x.id == program_type)
                else {
                    continue;
                };
                let Ok(program) =
                    g.w.act_with(&owner, |c| c.draft_program(&program_type, dates.clone()))
                else {
                    g.refuse("program.draft");
                    continue;
                };
                let mut specs = Vec::new();
                let mut assigned: Vec<(String, SlotKey)> = Vec::new();
                for (occ, d) in dates.iter().enumerate() {
                    for slot in &def.slots {
                        let reps = match slot.repeat {
                            Some(Repeat::Count(n)) => n.min(2),
                            _ => 1,
                        };
                        for index in 0..reps * slot.count {
                            let eligible: Vec<String> = g
                                .names
                                .iter()
                                .filter(|n| {
                                    g.w.persons.get(*n).is_some_and(|p| {
                                        proj.auth.satisfies(&cfg, *p, slot.requires.as_ref(), *d)
                                    })
                                })
                                .cloned()
                                .collect();
                            let Some(who) = g.rng.pick(&eligible) else {
                                continue;
                            };
                            let key = SlotKey {
                                occurrence: occ as u16,
                                slot: slot.id.clone(),
                                index: index as u16,
                            };
                            specs.push(AssignmentSpec {
                                key: key.clone(),
                                assignee: Assignee::Person(g.w.persons[&who]),
                                extra: Values::new(),
                            });
                            assigned.push((who, key));
                        }
                    }
                }
                g.w.set_time(t + MINUTE);
                if g.w.act(&owner, |c| c.assign(program, specs)).is_err() {
                    g.refuse("program.assign");
                    continue;
                }
                g.w.set_time(t + 2 * MINUTE);
                if g.w.act(&owner, |c| c.send_assignments(program)).is_err() {
                    g.refuse("program.send");
                    continue;
                }
                for (who, key) in assigned {
                    let rt = t + g.rng.range(60, 2 * DAY_MIN) * MINUTE;
                    push(
                        &mut heap,
                        rt,
                        Plan::Respond {
                            person: who,
                            program,
                            program_type: program_type.clone(),
                            key,
                        },
                    );
                }
                push(&mut heap, t + DAY_MIN * MINUTE, Plan::Publish { program });
            }
            Plan::Respond {
                person,
                program,
                program_type,
                key,
            } => {
                let assignee = Assignee::Person(g.w.persons[&person]);
                if g.w
                    .act(&person, |c| {
                        c.respond(program, &program_type, key, assignee, Response::Accepted)
                    })
                    .is_err()
                {
                    g.refuse("assignment.respond");
                }
            }
            Plan::Publish { program } => {
                let proj = g.admin_view()?;
                let board_posters: Vec<String> = g
                    .names
                    .iter()
                    .filter(|n| {
                        g.w.persons.get(*n).is_some_and(|p| {
                            proj.auth.is_allowed(
                                &cfg,
                                *p,
                                Permission::ProgramPublish,
                                None,
                                None,
                                date,
                            ) || {
                                cfg.package().programs.program_types.iter().any(|t| {
                                    let obj = core_model::strings::ObjectRef {
                                        kind: core_model::strings::ObjectKind::Program,
                                        id: t.id.clone(),
                                    };
                                    proj.auth.is_allowed(
                                        &cfg,
                                        *p,
                                        Permission::ProgramPublish,
                                        Some(&obj),
                                        None,
                                        date,
                                    ) && !proj.auth.is_root(*p)
                                })
                            }
                        })
                    })
                    .cloned()
                    .collect();
                let Some(p) = g.rng.pick(&board_posters) else {
                    continue;
                };
                if g.w.act(&p, |c| c.publish_program(program)).is_err() {
                    g.refuse("program.publish");
                }
            }
        }
    }
    let generation_secs = started.elapsed().as_secs_f64();

    // ----- propagation ----------------------------------------------------------------
    let started = std::time::Instant::now();
    let mut devices: Vec<(core_crypto::DevicePublicKey, bool, bool)> = Vec::new(); // key, archive, ios
    for name in &g.names {
        for (idx, d) in g.w.devices.get(name).into_iter().flatten().enumerate() {
            let archive = archives.contains(&(name.clone(), idx));
            let ios = !archive && g.rng.chance(pop.ios_share);
            devices.push((d.keys.public(), archive, ios));
        }
    }
    let index: BTreeMap<core_crypto::DevicePublicKey, usize> =
        devices.iter().enumerate().map(|(i, d)| (d.0, i)).collect();
    let activity: Vec<&core_events::SignedEvent> = g.w.events[setup_len..].iter().collect();
    let horizon = i64::from(opts.days) * DAY_MIN + 3 * DAY_MIN;
    let ticks = usize::try_from(horizon).unwrap_or(0);
    let nd = devices.len();
    let mut online = vec![vec![false; ticks + 1]; nd];
    let mark = |online: &mut Vec<Vec<bool>>, d: usize, from: i64, len: i64| {
        for t in from.max(0)..(from + len).min(horizon) {
            if let Some(slot) = online.get_mut(d).and_then(|v| v.get_mut(t as usize)) {
                *slot = true;
            }
        }
    };
    for (d, (_, archive, _)) in devices.iter().enumerate() {
        if *archive {
            mark(&mut online, d, 0, horizon);
            continue;
        }
        for day in 0..i64::from(opts.days) + 3 {
            for _ in 0..1 + g.rng.below(3) {
                let s = day * DAY_MIN + g.rng.range(7 * 60, 22 * 60);
                mark(&mut online, d, s, g.rng.range(2, 10));
            }
        }
    }
    // Recipients: devices the channel key of the event's epoch was given to.
    let final_view = g.admin_view()?;
    let recipients: Vec<Vec<usize>> = activity
        .iter()
        .map(|e| {
            let author = index.get(&e.body.author).copied();
            let set: Vec<usize> = match e.body.channel {
                PayloadChannel::Public => (0..nd).collect(),
                PayloadChannel::Channel(c) => final_view
                    .channels
                    .get(&c.id)
                    .and_then(|info| info.epochs.get(&c.epoch))
                    .map(|devs| devs.iter().filter_map(|k| index.get(k).copied()).collect())
                    .unwrap_or_default(),
            };
            set.into_iter().filter(|d| Some(*d) != author).collect()
        })
        .collect();
    let created: Vec<i64> = activity
        .iter()
        .map(|e| (e.body.declared_time - t0) / MINUTE)
        .collect();
    let mut by_tick: BTreeMap<i64, Vec<usize>> = BTreeMap::new();
    for (i, t) in created.iter().enumerate() {
        by_tick.entry(*t).or_default().push(i);
    }
    let words = activity.len().div_ceil(64);
    let mut know = vec![vec![0u64; words]; nd];
    let mut arrival: Vec<Vec<Option<i64>>> = vec![vec![None; nd]; activity.len()];
    for t in 0..horizon {
        if let Some(evs) = by_tick.get(&t) {
            for &i in evs {
                if let Some(a) = index.get(&activity[i].body.author).copied() {
                    mark(&mut online, a, t, 5);
                    know[a][i / 64] |= 1 << (i % 64);
                    arrival[i][a] = Some(t);
                }
                // Push wake-ups for recipients not online now.
                for &r in &recipients[i] {
                    if online[r].get(t as usize).copied().unwrap_or(false) {
                        continue;
                    }
                    let ios = devices[r].2;
                    if ios {
                        if g.rng.chance(0.6) {
                            let delay = (-(g.rng.unit().max(1e-9)).ln() * 45.0) as i64;
                            mark(&mut online, r, t + 1 + delay, 3);
                        }
                    } else if g.rng.chance(0.85) {
                        mark(&mut online, r, t + 1 + g.rng.range(0, 2), 1);
                    }
                }
            }
        }
        let live: Vec<usize> = (0..nd)
            .filter(|d| online[*d].get(t as usize).copied().unwrap_or(false))
            .collect();
        if live.len() < 2 {
            continue;
        }
        let mut union = vec![0u64; words];
        for &d in &live {
            for (u, k) in union.iter_mut().zip(&know[d]) {
                *u |= k;
            }
        }
        for &d in &live {
            for wi in 0..words {
                let new = union[wi] & !know[d][wi];
                if new != 0 {
                    for b in 0..64 {
                        if new & (1 << b) != 0 {
                            arrival[wi * 64 + b][d] = Some(t);
                        }
                    }
                    know[d][wi] |= new;
                }
            }
        }
    }

    // ----- report -------------------------------------------------------------------
    let mut per_kind: BTreeMap<&'static str, (usize, Vec<f64>, usize)> = BTreeMap::new();
    let mut doc_pairs = 0usize;
    let mut doc_ok = 0usize;
    for (i, e) in activity.iter().enumerate() {
        if matches!(
            e.body.kind,
            EventKind::ChannelKeysDistributed | EventKind::ChannelRotated
        ) || recipients[i].is_empty()
        {
            continue;
        }
        let entry = per_kind.entry(e.body.kind.as_str()).or_default();
        entry.0 += 1;
        for &r in &recipients[i] {
            entry.2 += 1;
            let delay = arrival[i][r].map_or(f64::INFINITY, |a| (a - created[i]) as f64);
            entry.1.push(delay);
            if e.body.kind == EventKind::DocumentDistributed {
                doc_pairs += 1;
                if delay <= DAY_MIN as f64 {
                    doc_ok += 1;
                }
            }
        }
    }
    let kinds = per_kind
        .into_iter()
        .map(|(kind, (events, mut delays, pairs))| {
            delays.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let q = |p: f64| {
                delays
                    .get(((delays.len() as f64 - 1.0) * p).round() as usize)
                    .copied()
                    .filter(|v| v.is_finite())
            };
            let within = delays.iter().filter(|d| **d <= DAY_MIN as f64).count() as f64
                / delays.len().max(1) as f64;
            KindStats {
                kind: kind.to_owned(),
                events,
                pairs,
                median_min: q(0.5),
                p95_min: q(0.95),
                within_24h: within,
            }
        })
        .collect();
    let size_of = |e: &core_events::SignedEvent| e.to_bytes().map(|b| b.len() as u64).unwrap_or(0);
    let setup_bytes: u64 = g.w.events[..setup_len].iter().map(size_of).sum();
    let mut total = 0u64;
    for k in &know {
        let mut bytes = setup_bytes;
        for (i, e) in activity.iter().enumerate() {
            if k[i / 64] & (1 << (i % 64)) != 0 {
                bytes += size_of(e);
            }
        }
        total += bytes;
    }
    let storage_avg_bytes = total / nd.max(1) as u64;
    let activity_bytes = storage_avg_bytes.saturating_sub(setup_bytes) as f64;
    let storage_per_year_mb = (setup_bytes as f64
        + activity_bytes * 365.0 / f64::from(opts.days.max(1)))
        / (1024.0 * 1024.0);
    let archive_name = archives.iter().next().map(|(n, _)| n.clone());
    let verdicts = match archive_name {
        Some(n) => g.w.view(&n, Some(1)).map_err(|e| e.to_string())?.verdicts,
        None => final_view.verdicts.clone(),
    };
    let count = |f: &dyn Fn(&Verdict) -> bool| verdicts.values().filter(|v| f(v)).count();
    Ok(Report {
        persons: g.names.len(),
        phones: devices.iter().filter(|d| !d.1).count(),
        archives: devices.iter().filter(|d| d.1).count(),
        days: opts.days,
        setup_events: setup_len,
        activity_events: activity.len(),
        refused_actions: g.refused,
        kinds,
        documents_within_24h: if doc_pairs == 0 {
            0.0
        } else {
            doc_ok as f64 / doc_pairs as f64
        },
        storage_avg_bytes,
        storage_per_year_mb,
        invalid: count(&|v| matches!(v, Verdict::Invalid(_))),
        superseded: count(&|v| *v == Verdict::Superseded),
        quarantined: count(&|v| *v == Verdict::Quarantined),
        generation_secs,
        propagation_secs: started.elapsed().as_secs_f64(),
    })
}
