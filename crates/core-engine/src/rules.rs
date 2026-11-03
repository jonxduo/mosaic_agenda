//! Rules shared by validation (engine) and event building (actions), so
//! that what a client builds is exactly what every device accepts.

use std::collections::{BTreeMap, BTreeSet};

use core_crypto::{ChannelDescriptor, ChannelScope};
use core_model::config::{AggregateDef, FormDef, MeasureOp, Periodicity, VariantDef};
use core_model::eval::{EvalContext, Evaluator};
use core_model::strings::{GroupBy, ObjectKind, ObjectRef};
use core_model::{Id, Permission, PersonId, Scalar, Slug, UnitId};
use time::Date;

use crate::auth::{AuthState, Denied};
use crate::channels::{STAGE_ASSIGNED, STAGE_DRAFT, audience_desc, program_desc};
use crate::config::Cfg;
use crate::ops::{AggregateResult, Assignee, NoticeItem, ProgramSnapshot, PublishedAssignment};
use crate::projection::{Document, Program};
use crate::time::PeriodKey;
use crate::values::with_computed;

pub fn scope(unit: Option<UnitId>) -> ChannelScope {
    unit.map_or(ChannelScope::Organization, ChannelScope::Unit)
}

pub fn form_object(form: &Slug) -> ObjectRef {
    ObjectRef {
        kind: ObjectKind::Form,
        id: form.clone(),
    }
}

pub fn aggregate_object(id: &Slug) -> ObjectRef {
    ObjectRef {
        kind: ObjectKind::Aggregate,
        id: id.clone(),
    }
}

pub fn resource_object(id: &Slug) -> ObjectRef {
    ObjectRef {
        kind: ObjectKind::Resource,
        id: id.clone(),
    }
}

/// Channel of a document in a state. A draft filled on behalf of the
/// subject is also readable by those who may fill it on their behalf.
pub fn document_descriptor(
    cfg: &Cfg,
    form: &FormDef,
    content: &crate::ops::DocumentContent,
    state: &Slug,
) -> ChannelDescriptor {
    let initial = cfg.form_workflow(form).is_some_and(|w| &w.initial == state);
    let audience = match (&form.fillers_on_behalf, initial && content.on_behalf) {
        (Some(fillers), true) => fillers.clone(),
        _ => cfg.read_audience(form, state),
    };
    ChannelDescriptor::Subject {
        audience,
        subject: content.subject,
        scope: scope(content.unit),
    }
}

/// Channel of a batch step leading to `to_state` (its read audience without a subject).
pub fn batch_descriptor(
    cfg: &Cfg,
    form: &FormDef,
    unit: UnitId,
    to_state: &Slug,
) -> ChannelDescriptor {
    audience_desc(&cfg.read_audience(form, to_state), ChannelScope::Unit(unit))
}

/// Drafts of an aggregation: those allowed to generate it.
pub fn aggregate_draft_descriptor(id: &Slug) -> ChannelDescriptor {
    ChannelDescriptor::PermissionHolders {
        permission: Permission::AggregateGenerate.as_str().to_owned(),
        object: aggregate_object(id).to_string(),
    }
}

pub fn aggregate_descriptor(def: &AggregateDef) -> ChannelDescriptor {
    audience_desc(&def.read_audience, ChannelScope::Organization)
}

pub fn program_draft_descriptor(program_type: &Slug) -> ChannelDescriptor {
    program_desc(program_type, STAGE_DRAFT)
}

pub fn program_assigned_descriptor(program_type: &Slug) -> ChannelDescriptor {
    program_desc(program_type, STAGE_ASSIGNED)
}

pub fn assignment_descriptor(program_type: &Slug, assignee: &Assignee) -> ChannelDescriptor {
    match assignee {
        Assignee::Person(p) => ChannelDescriptor::Assignment {
            program_type: program_type.clone(),
            person: *p,
        },
        Assignee::External(_) => program_draft_descriptor(program_type),
    }
}

pub fn published_descriptor(publish_audience: &Slug) -> ChannelDescriptor {
    audience_desc(publish_audience, ChannelScope::Organization)
}

pub fn resource_descriptor(resource_type: &Slug, holder: Option<PersonId>) -> ChannelDescriptor {
    ChannelDescriptor::Resource {
        resource_type: resource_type.clone(),
        holder,
    }
}

/// The period a document of this form covers, from its periodicity.
pub fn period_matches(form: &FormDef, period: &PeriodKey) -> bool {
    matches!(
        (form.periodicity, period),
        (Periodicity::Monthly, PeriodKey::Month { .. })
            | (Periodicity::Weekly, PeriodKey::Week { .. })
            | (Periodicity::None, PeriodKey::Once)
    )
}

/// Reference date of a period: its last day (§17: unit at the end of the period).
pub fn reference_date(period: &PeriodKey, fallback: Date) -> Date {
    period.last_day().unwrap_or(fallback)
}

pub fn subject_unit(
    auth: &AuthState,
    form: &FormDef,
    subject: PersonId,
    at: Date,
) -> Option<UnitId> {
    form.batching
        .as_ref()
        .and_then(|b| auth.org.unit_of_type(subject, &b.unit_type, at))
}

/// First variant whose condition holds for the subject at `at`.
pub fn variant_for<'a>(
    auth: &AuthState,
    cfg: &Cfg,
    form: &'a FormDef,
    subject: PersonId,
    at: Date,
) -> Option<&'a VariantDef> {
    let ev = Evaluator::new(cfg.index(), &auth.org);
    let ctx = EvalContext::at(at).with_subject(subject, None);
    form.variants
        .iter()
        .find(|v| ev.eval(&v.when, Some(subject), &ctx))
}

/// May `author` create or edit the subject's document?
pub fn may_fill(
    auth: &AuthState,
    cfg: &Cfg,
    author: PersonId,
    form: &FormDef,
    subject: PersonId,
    unit: Option<UnitId>,
    date: Date,
) -> bool {
    if author == subject {
        return true;
    }
    if auth.is_allowed(
        cfg,
        author,
        Permission::DocumentFillOnBehalf,
        Some(&form_object(&form.id)),
        unit,
        date,
    ) {
        return true;
    }
    form.fillers_on_behalf.as_ref().is_some_and(|a| {
        let ctx = EvalContext::at(date).with_subject(subject, unit);
        Evaluator::new(cfg.index(), &auth.org)
            .resolve(a, &ctx)
            .is_ok_and(|s| s.contains(&author))
    })
}

/// Persons allowed to perform a workflow step on a subject's document (or a batch).
pub fn step_actors(
    auth: &AuthState,
    cfg: &Cfg,
    form: &FormDef,
    by: &Slug,
    subject: Option<PersonId>,
    unit: Option<UnitId>,
    date: Date,
) -> Result<BTreeSet<PersonId>, Denied> {
    let mut ctx = EvalContext::at(date);
    ctx.subject = subject;
    ctx.subject_unit = unit;
    ctx.on_behalf = form.fillers_on_behalf.clone();
    let mut out = Evaluator::new(cfg.index(), &auth.org)
        .resolve(by, &ctx)
        .map_err(|e| Denied(e.to_string()))?;
    out.extend(auth.org.root_admins.iter().copied());
    Ok(out)
}

/// Subjects expected to have a document for the period (in the unit, if given).
pub fn expected_subjects(
    auth: &AuthState,
    cfg: &Cfg,
    form: &FormDef,
    period: &PeriodKey,
    unit: Option<UnitId>,
) -> BTreeSet<PersonId> {
    let Some(at) = period.last_day() else {
        return BTreeSet::new();
    };
    let ev = Evaluator::new(cfg.index(), &auth.org);
    let ctx = EvalContext::at(at);
    auth.org
        .active_persons()
        .filter(|p| ev.eval(&form.subject, Some(*p), &ctx))
        .filter(|p| unit.is_none() || subject_unit(auth, form, *p, at) == unit)
        .collect()
}

/// Subjects whose document reached at least `min_state`.
pub fn present_subjects<'a>(
    cfg: &Cfg,
    form: &FormDef,
    docs: impl Iterator<Item = &'a Document>,
    period: &PeriodKey,
    unit: Option<UnitId>,
    min_state: &Slug,
) -> BTreeSet<PersonId> {
    let min = cfg.state_rank(form, min_state).unwrap_or(usize::MAX);
    docs.filter(|d| d.content.form == form.id && &d.content.period == period)
        .filter(|d| unit.is_none() || d.content.unit == unit)
        .filter(|d| cfg.state_rank(form, &d.state).is_some_and(|r| r >= min))
        .map(|d| d.content.subject)
        .collect()
}

/// Running sum, count, min and max of a measure.
type Accumulator = (f64, f64, Option<f64>, Option<f64>);

/// Computes an aggregation over documents (already filtered to the source state and period).
pub fn compute_aggregate(
    auth: &AuthState,
    cfg: &Cfg,
    def: &AggregateDef,
    form: &FormDef,
    period: &PeriodKey,
    docs: &[(&Id, &Document)],
) -> AggregateResult {
    let at = period.last_day().unwrap_or(Date::MIN);
    let ev = Evaluator::new(cfg.index(), &auth.org);
    let mut acc: BTreeMap<String, BTreeMap<Slug, Accumulator>> = BTreeMap::new();
    for (_, d) in docs {
        let fields = form
            .variants
            .iter()
            .find(|v| v.id == d.content.variant)
            .map(|v| with_computed(&v.fields, &d.content.values))
            .unwrap_or_else(|| d.content.values.clone());
        let mut ctx = EvalContext::at(at).with_subject(d.content.subject, d.content.unit);
        ctx.fields = fields.clone();
        let mut keys = vec![String::new()];
        if !def.group_by.is_empty() {
            let parts: Vec<String> = def
                .group_by
                .iter()
                .map(|g| match g {
                    GroupBy::Variant => format!("variant={}", d.content.variant),
                    GroupBy::UnitType(t) => {
                        let u = auth.org.unit_of_type(d.content.subject, t, at);
                        format!("unit={}", u.map(|u| u.to_string()).unwrap_or_default())
                    }
                    GroupBy::QualificationSet(s) => {
                        format!("set={s}:{}", ev.in_set(d.content.subject, s, at))
                    }
                })
                .collect();
            keys.push(parts.join(","));
        }
        for m in &def.measures {
            if let Some(f) = &m.filter {
                if !ev.eval(f, Some(d.content.subject), &ctx) {
                    continue;
                }
            }
            let value = m.field.as_ref().and_then(|f| fields.get(f));
            let contribution: Option<f64> = match m.op {
                MeasureOp::Count => Some(1.0),
                MeasureOp::CountTrue => Some(if matches!(value, Some(Scalar::Bool(true))) {
                    1.0
                } else {
                    0.0
                }),
                _ => value.and_then(Scalar::as_f64),
            };
            let Some(x) = contribution else {
                continue;
            };
            for k in &keys {
                let slot = acc
                    .entry(k.clone())
                    .or_default()
                    .entry(m.id.clone())
                    .or_insert((0.0, 0.0, None, None));
                slot.0 += x;
                slot.1 += 1.0;
                slot.2 = Some(slot.2.map_or(x, |v: f64| v.min(x)));
                slot.3 = Some(slot.3.map_or(x, |v: f64| v.max(x)));
            }
        }
    }
    let mut groups: BTreeMap<String, BTreeMap<Slug, f64>> = BTreeMap::new();
    for (k, measures) in acc {
        let mut out = BTreeMap::new();
        for m in &def.measures {
            let v = measures
                .get(&m.id)
                .map_or(0.0, |(sum, n, min, max)| match m.op {
                    MeasureOp::Count | MeasureOp::CountTrue | MeasureOp::Sum => *sum,
                    MeasureOp::Avg => {
                        if *n > 0.0 {
                            sum / n
                        } else {
                            0.0
                        }
                    }
                    MeasureOp::Min => min.unwrap_or(0.0),
                    MeasureOp::Max => max.unwrap_or(0.0),
                });
            out.insert(m.id.clone(), v);
        }
        groups.insert(k, out);
    }
    let present: BTreeSet<PersonId> = docs.iter().map(|(_, d)| d.content.subject).collect();
    let missing = if def.list_missing {
        expected_subjects(auth, cfg, form, period, None)
            .difference(&present)
            .copied()
            .collect()
    } else {
        Vec::new()
    };
    let mut documents: Vec<Id> = docs.iter().map(|(id, _)| **id).collect();
    documents.sort();
    AggregateResult {
        groups,
        missing,
        documents,
    }
}

pub fn display_name(auth: &AuthState, a: &Assignee) -> String {
    match a {
        Assignee::Person(p) => auth
            .org
            .persons
            .get(p)
            .map(|r| r.display_name.clone())
            .unwrap_or_default(),
        Assignee::External(id) => auth.externals.get(id).cloned().unwrap_or_default(),
    }
}

/// What gets published for a program.
/// What each assignee is told: their slots in the program, with the dates.
/// `None` when a slot points past the program's events.
pub fn notice_items(p: &Program) -> Option<BTreeMap<PersonId, Vec<NoticeItem>>> {
    let mut by_person: BTreeMap<PersonId, Vec<NoticeItem>> = BTreeMap::new();
    for (k, (a, _)) in &p.assignments {
        if let Assignee::Person(person) = a {
            let date = p.occurrences.get(usize::from(k.occurrence)).copied()?;
            by_person.entry(*person).or_default().push(NoticeItem {
                key: k.clone(),
                date,
            });
        }
    }
    Some(by_person)
}

pub fn program_snapshot(auth: &AuthState, id: Id, p: &Program, version: u32) -> ProgramSnapshot {
    ProgramSnapshot {
        program: id,
        program_type: p.program_type.clone(),
        version,
        occurrences: p.occurrences.clone(),
        assignments: p
            .assignments
            .iter()
            .map(|(k, (a, extra))| PublishedAssignment {
                key: k.clone(),
                date: p
                    .occurrences
                    .get(usize::from(k.occurrence))
                    .copied()
                    .unwrap_or(Date::MIN),
                display_name: display_name(auth, a),
                extra: extra.clone(),
            })
            .collect(),
    }
}
