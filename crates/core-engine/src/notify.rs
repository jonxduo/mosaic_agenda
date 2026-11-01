//! Notifications, reminders and alerts computed on the device (§5.11, §11.1).
//!
//! Event-driven notifications are recorded in [`Projection::triggers`] during
//! the rebuild. Reminders and alerts depend on a date: the caller passes it
//! (the app uses today), so the projection itself never reads the clock.

use std::collections::{BTreeMap, BTreeSet};

use core_model::config::{Periodicity, ReminderKind, TimeAnchor, TimerAction};
use core_model::eval::{EvalContext, Evaluator};
use core_model::state::GrantScope;
use core_model::strings::ScopeDef;
use core_model::{PersonId, Slug, TextKey};
use serde::Serialize;
use time::Date;

use crate::projection::Projection;
use crate::rules::{expected_subjects, present_subjects};
use crate::time::{PeriodKey, add_days};

/// A notification for some persons.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Notice {
    pub text: TextKey,
    pub recipients: BTreeSet<PersonId>,
    pub params: BTreeMap<String, String>,
}

/// Event-driven notifications addressed to `person`.
pub fn notifications_for(
    proj: &Projection,
    person: PersonId,
) -> Vec<&crate::projection::TriggerRecord> {
    proj.triggers
        .iter()
        .filter(|t| t.recipients.contains(&person))
        .collect()
}

fn anchor_period(
    periodicity: Periodicity,
    anchor: TimeAnchor,
    date: Date,
    offset: i32,
) -> Option<PeriodKey> {
    let base = add_days(date, -i64::from(offset));
    let period = match periodicity {
        Periodicity::Monthly => PeriodKey::month_of(base),
        Periodicity::Weekly => PeriodKey::week_of(base),
        Periodicity::None => return None,
    };
    let matches = match anchor {
        TimeAnchor::PeriodEnd => period.last_day() == Some(base),
        TimeAnchor::PeriodStart => period.start() == Some(base),
        TimeAnchor::Opened | TimeAnchor::Due => false,
    };
    matches.then_some(period)
}

/// Reminders due on `date`.
pub fn due_reminders(proj: &Projection, date: Date) -> Vec<Notice> {
    let Some(cfg) = proj.cfg() else {
        return Vec::new();
    };
    let auth = &proj.auth;
    let ev = Evaluator::new(cfg.index(), &auth.org);
    let mut out = Vec::new();
    // Workflow timers: remind when something is missing in a unit.
    for form in cfg.forms() {
        let Some(wf) = cfg.form_workflow(form) else {
            continue;
        };
        let submitted = wf
            .steps
            .iter()
            .find(|s| s.from.contains(&wf.initial))
            .map(|s| s.to.clone())
            .unwrap_or_else(|| wf.initial.clone());
        for timer in &wf.timers {
            let TimerAction::Remind(n) = &timer.action;
            let Some(period) =
                anchor_period(form.periodicity, timer.at.from, date, timer.at.offset_days)
            else {
                continue;
            };
            let units: Vec<Option<core_model::UnitId>> = match &form.batching {
                Some(b) => auth
                    .org
                    .units
                    .iter()
                    .filter(|(_, u)| u.active && u.unit_type == b.unit_type)
                    .map(|(id, _)| Some(*id))
                    .collect(),
                None => vec![None],
            };
            let mut recipients = BTreeSet::new();
            for unit in units {
                let expected = expected_subjects(auth, cfg, form, &period, unit);
                let present = present_subjects(
                    cfg,
                    form,
                    proj.documents.values(),
                    &period,
                    unit,
                    &submitted,
                );
                let missing: BTreeSet<PersonId> = expected.difference(&present).copied().collect();
                if missing.is_empty() {
                    continue;
                }
                let mut ctx = EvalContext::at(date);
                ctx.subject_unit = unit;
                ctx.missing_subjects = missing;
                if let Ok(r) = ev.resolve(&n.audience, &ctx) {
                    recipients.extend(r);
                }
            }
            if !recipients.is_empty() {
                let mut params = BTreeMap::new();
                params.insert("period".to_owned(), period.to_string());
                out.push(Notice {
                    text: n.text.clone(),
                    recipients,
                    params,
                });
            }
        }
    }
    // Configured reminders.
    for r in &cfg.package().notifications.reminders {
        let mut recipients = BTreeSet::new();
        match r.kind {
            ReminderKind::EventAssignmentBefore => {
                for ((_, person), (program_type, items)) in &proj.notices {
                    if r.filter
                        .program_type
                        .as_ref()
                        .is_some_and(|t| t != program_type)
                    {
                        continue;
                    }
                    if items
                        .iter()
                        .any(|i| add_days(i.date, -i64::from(r.days)) == date)
                    {
                        let mut ctx = EvalContext::at(date);
                        ctx.assigned = [*person].into();
                        recipients.extend(ev.resolve(&r.audience, &ctx).unwrap_or_default());
                    }
                }
            }
            ReminderKind::RequestDueAfter => {
                for req in proj.requests.values().filter(|q| q.answer.is_none()) {
                    if add_days(req.due, i64::from(r.days)) == date {
                        let mut ctx = EvalContext::at(date);
                        ctx.holder = Some(req.holder);
                        recipients.extend(ev.resolve(&r.audience, &ctx).unwrap_or_default());
                    }
                }
            }
            ReminderKind::ResourceNoLogAfter => {
                for res in proj.resources.values() {
                    if let Some(h) = &res.holding {
                        let last = h.logs.last().map_or(h.since, |(d, _)| *d);
                        if add_days(last, i64::from(r.days)) == date {
                            let mut ctx = EvalContext::at(date);
                            ctx.holder = Some(h.holder);
                            recipients.extend(ev.resolve(&r.audience, &ctx).unwrap_or_default());
                        }
                    }
                }
            }
            ReminderKind::DocumentMissingAfter
            | ReminderKind::BatchIncompleteAfter
            | ReminderKind::BatchMissingAfter => {
                // Expressed as workflow timers in packages (see above).
            }
        }
        if !recipients.is_empty() {
            out.push(Notice {
                text: r.text.clone(),
                recipients,
                params: BTreeMap::new(),
            });
        }
    }
    // Resource rules.
    for res in proj.resources.values() {
        let Some(def) = cfg.resource_type(&res.resource_type) else {
            continue;
        };
        if let (Some(h), Some(days)) = (&res.holding, def.rules.remind_holder_after_no_log) {
            let last = h.logs.last().map_or(h.since, |(d, _)| *d);
            if add_days(last, i64::from(days.days)) == date {
                // Rules carry no text: the core's own texts provide this one.
                if let Ok(text) = "core.resource_no_log".parse::<TextKey>() {
                    out.push(Notice {
                        text,
                        recipients: [h.holder].into(),
                        params: BTreeMap::new(),
                    });
                }
            }
        }
    }
    out
}

/// Kind of administrative alert.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum CoverageAlert {
    /// A mandatory role has no holder (nor substitute) in a scope.
    Uncovered {
        role: Slug,
        unit: Option<core_model::UnitId>,
    },
    /// A holder no longer meets the role's requirements.
    InvalidGrant { role: Slug, person: PersonId },
}

/// Coverage and validity of role grants at `date` (§5.3).
pub fn coverage_alerts(proj: &Projection, date: Date) -> Vec<CoverageAlert> {
    let Some(cfg) = proj.cfg() else {
        return Vec::new();
    };
    let auth = &proj.auth;
    let ev = Evaluator::new(cfg.index(), &auth.org);
    let mut out = Vec::new();
    let held = |role: &Slug, scope: GrantScope| {
        auth.org
            .active_role_grants(date)
            .any(|g| &g.role == role && g.scope == scope)
    };
    for role in cfg.roles() {
        let Some(mandatory) = &role.mandatory else {
            continue;
        };
        let scopes: Vec<(GrantScope, Option<core_model::UnitId>)> = match &role.scope {
            ScopeDef::Organization => vec![(GrantScope::Organization, None)],
            ScopeDef::UnitType(t) => auth
                .org
                .units
                .iter()
                .filter(|(_, u)| u.active && &u.unit_type == t)
                .map(|(id, _)| (GrantScope::Unit(*id), Some(*id)))
                .collect(),
        };
        for (scope, unit) in scopes {
            let mut ctx = EvalContext::at(date);
            ctx.subject_unit = unit;
            if !ev.eval(mandatory, None, &ctx)
                && !matches!(mandatory, core_model::Cond::Always(true))
            {
                // Conditions about holders are evaluated without a person: try any holder.
                let any = auth
                    .org
                    .active_persons()
                    .any(|p| ev.eval(mandatory, Some(p), &ctx));
                if !any {
                    continue;
                }
            }
            let covered = held(&role.id, scope)
                || cfg
                    .roles()
                    .iter()
                    .any(|r| r.substitute_for.as_ref() == Some(&role.id) && held(&r.id, scope));
            if !covered {
                out.push(CoverageAlert::Uncovered {
                    role: role.id.clone(),
                    unit,
                });
            }
        }
    }
    for g in auth.org.active_role_grants(date) {
        if let Some(role) = cfg.role(&g.role) {
            if !auth.satisfies(cfg, g.person, role.requires.as_ref(), date) {
                out.push(CoverageAlert::InvalidGrant {
                    role: g.role.clone(),
                    person: g.person,
                });
            }
        }
    }
    out
}
