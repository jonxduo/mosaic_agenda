//! Read-only views (§12.2), built from the projection of this device.

use std::collections::{BTreeMap, BTreeSet};

use core_engine::config::Cfg;
use core_engine::ops::{Assignee, Response};
use core_engine::projection::{BoardItem as PBoardItem, Document, ProgramStatus};
use core_engine::rules::{expected_subjects, may_fill, present_subjects, step_actors, variant_for};
use core_engine::time::{PeriodKey, add_days, date_of};
use core_model::config::{FieldDef, FieldType, Periodicity, Rotation, StepKind, ViewKind};
use core_model::eval::{EvalContext, Evaluator};
use core_model::strings::{ObjectKind, ObjectRef};
use core_model::{Permission, PersonId, Scalar, Slug, UnitId};
use time::Date;

use crate::dto::*;
use crate::texts::Texts;
use crate::{ApiError, ApiResult, Core, View};

pub(crate) fn cfg(v: &View) -> ApiResult<&Cfg> {
    v.proj
        .cfg()
        .ok_or_else(|| ApiError::internal("no configuration"))
}

pub(crate) fn name_of(v: &View, p: PersonId) -> String {
    v.proj
        .auth
        .org
        .persons
        .get(&p)
        .map(|r| r.display_name.clone())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "—".into())
}

pub(crate) fn scalar_json(s: &Scalar) -> serde_json::Value {
    serde_json::to_value(s).unwrap_or(serde_json::Value::Null)
}

fn kind_name(t: FieldType) -> String {
    serde_json::to_value(t)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

pub(crate) fn field_dto(t: &Texts, f: &FieldDef, value: Option<&Scalar>) -> Field {
    let label = f.label.as_ref().map_or_else(
        || t.get(&core_model::config::default_label::field(f.id.as_str())),
        |l| t.get(l.as_str()),
    );
    Field {
        id: f.id.to_string(),
        label,
        kind: kind_name(f.ty),
        required: f.required,
        min: f.min,
        max: f.max,
        max_length: f.max_length,
        options: f
            .options
            .iter()
            .flatten()
            .map(|o| Choice {
                id: o.id.to_string(),
                label: o.label.as_ref().map_or_else(
                    || t.get(&core_model::config::default_label::option(o.id.as_str())),
                    |l| t.get(l.as_str()),
                ),
            })
            .collect(),
        value: value.map_or(serde_json::Value::Null, scalar_json),
    }
}

pub(crate) fn state_label(t: &Texts, state: &Slug) -> String {
    let key = format!("state.{state}");
    let s = t.get(&key);
    if s == key { state.to_string() } else { s }
}

pub(crate) fn today(core: &Core) -> Date {
    date_of(core.now())
}

/// The last complete period of a periodicity, relative to `today`.
pub(crate) fn last_period(p: Periodicity, today: Date) -> PeriodKey {
    match p {
        Periodicity::Monthly => PeriodKey::month_of(add_days(
            PeriodKey::month_of(today).start().unwrap_or(today),
            -1,
        )),
        Periodicity::Weekly => PeriodKey::week_of(add_days(today, -7)),
        Periodicity::None => PeriodKey::Once,
    }
}

fn allowed_in(v: &View, me: PersonId, audience: &Slug, date: Date) -> bool {
    let Ok(cfg) = cfg(v) else { return false };
    if v.proj.auth.is_root(me) {
        return true;
    }
    Evaluator::new(cfg.index(), &v.proj.auth.org)
        .resolve(audience, &EvalContext::at(date))
        .is_ok_and(|s| s.contains(&me))
}

fn params_map(p: &core_model::config::ViewParams) -> BTreeMap<String, String> {
    let mut m = BTreeMap::new();
    if let Some(x) = &p.workflow {
        m.insert("workflow".into(), x.to_string());
    }
    if let Some(x) = &p.form {
        m.insert("form".into(), x.to_string());
    }
    if let Some(x) = &p.aggregate {
        m.insert("aggregate".into(), x.to_string());
    }
    if let Some(x) = &p.resource_type {
        m.insert("resource_type".into(), x.to_string());
    }
    if let Some(x) = &p.unit_type {
        m.insert("unit_type".into(), x.to_string());
    }
    if !p.types.is_empty() {
        m.insert(
            "types".into(),
            p.types
                .iter()
                .map(Slug::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
    }
    m
}

fn view_name(k: ViewKind) -> String {
    serde_json::to_value(k)
        .ok()
        .and_then(|v| v.as_str().map(str::to_owned))
        .unwrap_or_default()
}

impl Core {
    pub fn status(&self) -> ApiResult<AppStatus> {
        let t = self.texts();
        let pkg = &self.package.package;
        let branding = Branding {
            app_name: t.get(pkg.manifest.branding.app_name.as_str()),
            primary_color: pkg.manifest.branding.primary_color.clone(),
            locale: t.locale.clone(),
        };
        let local = self.local()?;
        let privacy = (!local.privacy_accepted)
            .then(|| {
                pkg.privacy_notices
                    .get(&pkg.manifest.default_locale)
                    .cloned()
            })
            .flatten();
        if self
            .restart_needed
            .load(std::sync::atomic::Ordering::SeqCst)
        {
            return Ok(AppStatus {
                state: AppState::Restart,
                branding,
                org_name: None,
                me: None,
                unsynced: 0,
                unsynced_label: String::new(),
                has_pin: false,
                biometric: false,
                privacy_notice: None,
            });
        }
        if !self.has_org() {
            return Ok(AppStatus {
                state: AppState::Empty,
                branding,
                org_name: None,
                me: None,
                unsynced: 0,
                unsynced_label: String::new(),
                has_pin: self.has_pin(),
                biometric: self.local().is_ok_and(|l| l.biometric),
                privacy_notice: privacy,
            });
        }
        // Locked: the database is closed, nothing else can be shown.
        if self.is_locked() {
            return Ok(AppStatus {
                state: if self.is_wiped() {
                    AppState::Wiped
                } else {
                    AppState::Locked
                },
                branding,
                org_name: None,
                me: None,
                unsynced: 0,
                unsynced_label: String::new(),
                has_pin: true,
                biometric: self.local().is_ok_and(|l| l.biometric),
                privacy_notice: None,
            });
        }
        let outbox = self
            .state
            .lock()
            .map_err(|_| ApiError::internal("poisoned"))?
            .store
            .store()
            .and_then(|s| s.outbox().ok())
            .map_or(0, |o| o.len()) as u32;
        let unsynced_label = if outbox == 0 {
            String::new()
        } else {
            t.fmt("core.unsynced", &[("n", outbox.to_string())])
        };
        let v = match self.view() {
            Ok(v) => v,
            Err(_) => {
                return Ok(AppStatus {
                    state: AppState::Joining,
                    branding,
                    org_name: None,
                    me: None,
                    unsynced: outbox,
                    unsynced_label,
                    has_pin: self.has_pin(),
                    biometric: self.local().is_ok_and(|l| l.biometric),
                    privacy_notice: privacy,
                });
            }
        };
        let org_name = v
            .events
            .first()
            .and_then(|g| {
                g.public_content::<core_events::payloads::GenesisPayload>()
                    .ok()
            })
            .map(|g| g.name);
        let me = v.proj.auth.author(&self.keys.public()).ok().map(|p| Me {
            person_id: p.to_string(),
            display_name: name_of(&v, p),
            device_id: self.keys.public().to_string(),
            is_admin: v.proj.auth.is_root(p)
                || cfg(&v).is_ok_and(|c| {
                    v.proj.auth.is_allowed(
                        c,
                        p,
                        Permission::PeopleManage,
                        None,
                        None,
                        date_of(self.now()),
                    )
                }),
        });
        let state = if me.is_none() {
            AppState::Joining
        } else {
            AppState::Ready
        };
        Ok(AppStatus {
            state,
            branding,
            org_name,
            me,
            unsynced: outbox,
            unsynced_label,
            has_pin: self.has_pin(),
            biometric: self.local().is_ok_and(|l| l.biometric),
            privacy_notice: privacy,
        })
    }

    pub fn texts_map(&self) -> BTreeMap<String, String> {
        self.texts().all().clone()
    }

    pub fn menu(&self) -> ApiResult<Vec<MenuItem>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let mut items: Vec<MenuItem> = cfg
            .package()
            .ui
            .menu
            .iter()
            .filter(|m| {
                m.visible_to
                    .as_ref()
                    .is_none_or(|a| allowed_in(&v, me, a, today(self)))
            })
            .map(|m| MenuItem {
                label: t.get(m.label.as_str()),
                view: view_name(m.view),
                params: params_map(&m.params),
            })
            .collect();
        // Core view: the configuration editor, for who may publish (§12.4).
        let may_configure = v.proj.auth.is_root(me)
            || v.proj
                .auth
                .is_allowed(cfg, me, Permission::ConfigPublish, None, None, today(self));
        if may_configure {
            items.push(MenuItem {
                label: t.get("core.menu.schedule"),
                view: "schedule".into(),
                params: Default::default(),
            });
            items.push(MenuItem {
                label: t.get("core.menu.config"),
                view: "config".into(),
                params: Default::default(),
            });
        }
        Ok(items)
    }

    // ----- home -------------------------------------------------------------------

    pub fn home(&self) -> ApiResult<Home> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let mut sections = Vec::new();
        for item in &cfg.package().ui.home {
            match item.view {
                ViewKind::MyDocumentsDue => sections.push(HomeSection::DocumentsDue {
                    title: String::new(),
                    items: self.due_documents(&v, me, &t)?,
                }),
                ViewKind::MyAssignments => sections.push(HomeSection::Assignments {
                    title: String::new(),
                    items: self.assignments_of(&v, me, &t)?,
                }),
                ViewKind::MyResources => sections.push(HomeSection::Resources {
                    title: String::new(),
                    items: self.held_resources(&v, me, &t, item.params.resource_type.as_ref())?,
                }),
                ViewKind::Board => sections.push(HomeSection::Board {
                    title: String::new(),
                    items: self.board_items(&v, &t)?,
                }),
                _ => {}
            }
        }
        let work = self.work_items(&t)?;
        sections.push(HomeSection::Work {
            title: String::new(),
            items: work,
        });
        Ok(Home {
            date: capitalize(&t.long_date(today(self))),
            greeting: t.fmt(
                "core.greeting",
                &[(
                    "name",
                    name_of(&v, me)
                        .split_whitespace()
                        .next()
                        .unwrap_or_default()
                        .to_owned(),
                )],
            ),
            todo_title: String::new(),
            sections,
        })
    }

    /// Work for others reachable from this user's menu: the home shows it
    /// without having to visit every view.
    fn work_items(&self, t: &Texts) -> ApiResult<Vec<WorkItem>> {
        let mut out = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for m in self.menu()? {
            let param = |k: &str| m.params.get(k).cloned();
            match m.view.as_str() {
                "work_queue" => {
                    let Some(wf) = param("workflow") else {
                        continue;
                    };
                    let Ok(q) = self.work_queue(wf, param("unit_type")) else {
                        continue;
                    };
                    for b in q.batches {
                        let ready: Vec<&Action> =
                            b.actions.iter().filter(|a| !a.disabled).collect();
                        let missing = !b.missing.is_empty() && b.can_fill_on_behalf;
                        if (ready.is_empty() && !missing)
                            || !seen.insert((b.form.clone(), b.unit_id.clone(), b.period.clone()))
                        {
                            continue;
                        }
                        let subtitle = match ready.first() {
                            Some(a) => format!("{} · {}", b.progress, a.label),
                            None => {
                                let names: Vec<&str> =
                                    b.missing.iter().map(|p| p.name.as_str()).take(3).collect();
                                let more = b.missing.len().saturating_sub(names.len());
                                format!(
                                    "{}: {}{}",
                                    t.fmt(
                                        "core.missing_count",
                                        &[("n", b.missing.len().to_string())]
                                    ),
                                    names.join(", "),
                                    if more > 0 {
                                        format!(" +{more}")
                                    } else {
                                        String::new()
                                    }
                                )
                            }
                        };
                        out.push(WorkItem {
                            title: format!("{} · {} · {}", q.title, b.unit_name, b.period_label),
                            subtitle,
                            view: m.view.clone(),
                            params: m.params.clone(),
                            action: ready
                                .first()
                                .map_or_else(|| t.get("core.work.open"), |a| a.label.clone()),
                            urgent: !ready.is_empty(),
                        });
                    }
                }
                "aggregate" => {
                    let Some(agg) = param("aggregate") else {
                        continue;
                    };
                    let Ok(a) = self.aggregate(agg, None) else {
                        continue;
                    };
                    let todo = if a.can_generate && a.version.is_none() && a.registered_count > 0 {
                        Some("core.work.aggregate")
                    } else if a.can_distribute && a.version.is_some() && !a.distributed {
                        Some("core.work.aggregate_send")
                    } else {
                        None
                    };
                    if let Some(key) = todo {
                        out.push(WorkItem {
                            title: format!("{} · {}", a.title, a.period_label),
                            subtitle: t.get(key),
                            view: m.view.clone(),
                            params: m.params.clone(),
                            action: t.get("core.work.open"),
                            urgent: false,
                        });
                    }
                }
                "admin_alerts" => {
                    let n = self.alerts().map(|a| a.len()).unwrap_or(0);
                    if n > 0 {
                        out.push(WorkItem {
                            title: t.fmt("core.work.alerts", &[("n", n.to_string())]),
                            subtitle: t.get("core.work.alerts_hint"),
                            view: m.view.clone(),
                            params: m.params.clone(),
                            action: t.get("core.work.open"),
                            urgent: false,
                        });
                    }
                }
                "sync_status" => {
                    for p in self.pairings().unwrap_or_default() {
                        out.push(WorkItem {
                            title: t.get("core.work.pairing"),
                            subtitle: t.fmt("core.work.pairing_hint", &[("name", p.person)]),
                            view: m.view.clone(),
                            params: m.params.clone(),
                            action: t.get("core.work.open"),
                            urgent: true,
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(out)
    }

    fn due_documents(&self, v: &View, me: PersonId, t: &Texts) -> ApiResult<Vec<DueDocument>> {
        let cfg = cfg(v)?;
        let now = today(self);
        let mut out = Vec::new();
        for form in cfg
            .forms()
            .iter()
            .filter(|f| f.periodicity != Periodicity::None)
        {
            let period = last_period(form.periodicity, now);
            if !expected_subjects(&v.proj.auth, cfg, form, &period, None).contains(&me) {
                continue;
            }
            let initial = cfg.form_workflow(form).map(|w| w.initial.clone());
            let doc = v.proj.document_by(&form.id, me, period);
            let pending = match doc {
                None => true,
                Some((_, d)) => Some(&d.state) == initial.as_ref(),
            };
            if !pending {
                continue;
            }
            let due = due_date(&period, form.due, now);
            out.push(DueDocument {
                form: form.id.to_string(),
                subject_id: me.to_string(),
                period: period.to_string(),
                title: format!("{} · {}", t.get(form.label.as_str()), t.period(&period)),
                subtitle: t.fmt("core.due_by", &[("date", t.day_month(due))]),
                urgent: now >= add_days(due, -2),
            });
        }
        Ok(out)
    }

    fn assignments_of(&self, v: &View, me: PersonId, t: &Texts) -> ApiResult<Vec<AssignmentItem>> {
        let cfg = cfg(v)?;
        let now = today(self);
        let mut out = Vec::new();
        for ((program, person), (program_type, items)) in &v.proj.notices {
            if *person != me {
                continue;
            }
            let def = cfg.program_type(program_type);
            let rotation = def.map(|d| d.rotation).unwrap_or_default();
            let answer =
                |key: &core_engine::ops::SlotKey| v.proj.responses.get(&(*program, key.clone()));
            // Items of a rotation are shown once per group (week or month).
            let mut groups: Vec<(Option<PeriodKey>, &core_engine::ops::NoticeItem)> = Vec::new();
            let mut members: BTreeMap<usize, Vec<&core_engine::ops::NoticeItem>> = BTreeMap::new();
            for it in items.iter().filter(|it| it.date >= now) {
                let g = crate::schedule::rotation_key(rotation, it.date);
                let pos = groups.iter().position(|(k, first)| {
                    g.is_some()
                        && *k == g
                        && first.key.slot == it.key.slot
                        && first.key.index == it.key.index
                });
                let i = pos.unwrap_or_else(|| {
                    groups.push((g, it));
                    groups.len() - 1
                });
                members.entry(i).or_default().push(it);
            }
            for (i, (g, it)) in groups.iter().enumerate() {
                let all = members.get(&i).map(Vec::as_slice).unwrap_or_default();
                let slot_label = def
                    .and_then(|d| d.slots.iter().find(|s| s.id == it.key.slot))
                    .map_or_else(|| it.key.slot.to_string(), |s| t.get(s.label.as_str()));
                let response = if all
                    .iter()
                    .any(|x| matches!(answer(&x.key), Some(Response::Declined { .. })))
                {
                    "declined"
                } else if all
                    .iter()
                    .all(|x| matches!(answer(&x.key), Some(Response::Accepted)))
                {
                    "accepted"
                } else {
                    "pending"
                };
                let when = match (g, all.len()) {
                    (Some(k), n) if n > 1 => format!(
                        "{} · {}",
                        capitalize(&t.period(k)),
                        all.iter()
                            .map(|x| t.day_month(x.date))
                            .collect::<Vec<_>>()
                            .join(", ")
                    ),
                    _ => capitalize(&t.long_date(it.date)),
                };
                out.push(AssignmentItem {
                    program_id: program.to_string(),
                    program_type: program_type.to_string(),
                    slot: it.key.slot.to_string(),
                    occurrence: u32::from(it.key.occurrence),
                    index: u32::from(it.key.index),
                    title: slot_label,
                    subtitle: format!(
                        "{} · {}",
                        def.map_or_else(String::new, |d| t.get(d.label.as_str())),
                        when
                    ),
                    response: response.into(),
                    can_respond: def.is_some_and(|d| d.acceptance) && response == "pending",
                });
            }
        }
        Ok(out)
    }

    fn held_resources(
        &self,
        v: &View,
        me: PersonId,
        t: &Texts,
        only: Option<&Slug>,
    ) -> ApiResult<Vec<HeldResource>> {
        let cfg = cfg(v)?;
        let mut out = Vec::new();
        for (id, r) in &v.proj.resources {
            if only.is_some_and(|o| o != &r.resource_type)
                || r.holding.as_ref().is_none_or(|h| h.holder != me)
            {
                continue;
            }
            let def = cfg.resource_type(&r.resource_type);
            let request = v
                .proj
                .requests
                .iter()
                .find(|(_, q)| q.resource == *id && q.answer.is_none() && q.holder == me);
            out.push(HeldResource {
                resource_id: id.to_string(),
                title: resource_title(t, def.map(|d| d.label.as_str()), &r.attributes),
                subtitle: r
                    .holding
                    .as_ref()
                    .map(|h| t.day_month(h.since))
                    .unwrap_or_default(),
                open_request: request.and_then(|(rid, q)| {
                    let rdef = def?.requests.iter().find(|x| x.id == q.request_type)?;
                    Some(OpenRequest {
                        request_id: rid.to_string(),
                        label: t.get(rdef.label.as_str()),
                        due: t.day_month(q.due),
                        answers: rdef
                            .answers
                            .iter()
                            .map(|a| Choice {
                                id: a.id.to_string(),
                                label: a.label.as_ref().map_or_else(
                                    || {
                                        t.get(&core_model::config::default_label::option(
                                            a.id.as_str(),
                                        ))
                                    },
                                    |l| t.get(l.as_str()),
                                ),
                            })
                            .collect(),
                    })
                }),
            });
        }
        Ok(out)
    }

    fn board_items(&self, v: &View, t: &Texts) -> ApiResult<Vec<BoardItem>> {
        let cfg = cfg(v)?;
        let now = today(self);
        let mut out = Vec::new();
        for (id, post) in &v.proj.board {
            if post.withdrawn || now < post.from || now > post.until {
                continue;
            }
            match &post.item {
                PBoardItem::Notice { title, body } => out.push(BoardItem {
                    id: id.to_string(),
                    title: title.clone(),
                    subtitle: String::new(),
                    body: vec![BoardLine {
                        label: String::new(),
                        value: body.clone(),
                    }],
                }),
                PBoardItem::Program(s) => {
                    let def = cfg.program_type(&s.program_type);
                    let first = s.occurrences.first().copied().unwrap_or(now);
                    let rotation = def.map(|d| d.rotation).unwrap_or_default();
                    let mut seen = BTreeSet::new();
                    let lines = s
                        .assignments
                        .iter()
                        .filter(|a| {
                            // One line per person and group of a rotation.
                            let group = crate::schedule::rotation_key(rotation, a.date);
                            group.is_none()
                                || seen.insert((
                                    group,
                                    a.key.slot.clone(),
                                    a.key.index,
                                    a.display_name.clone(),
                                ))
                        })
                        .map(|a| {
                            let slot = def
                                .and_then(|d| d.slots.iter().find(|x| x.id == a.key.slot))
                                .map_or_else(
                                    || a.key.slot.to_string(),
                                    |x| t.get(x.label.as_str()),
                                );
                            let when = match rotation {
                                _ if s.occurrences.len() < 2 => String::new(),
                                Rotation::Occurrence => format!(" ({})", t.day_month(a.date)),
                                Rotation::Week => {
                                    format!(" ({})", t.period(&PeriodKey::week_of(a.date)))
                                }
                                Rotation::Month => String::new(),
                            };
                            BoardLine {
                                label: format!("{slot}{when}"),
                                value: a.display_name.clone(),
                            }
                        })
                        .collect();
                    out.push(BoardItem {
                        id: id.to_string(),
                        title: def.map_or_else(String::new, |d| t.get(d.label.as_str())),
                        subtitle: def.map_or_else(
                            || t.period(&PeriodKey::week_of(first)),
                            |d| t.period(&crate::schedule::period_of(d, first)),
                        ),
                        body: lines,
                    });
                }
            }
        }
        Ok(out)
    }

    pub fn board(&self) -> ApiResult<Vec<BoardItem>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        self.board_items(&v, &self.texts())
    }

    pub fn my_assignments(&self) -> ApiResult<Vec<AssignmentItem>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        self.assignments_of(&v, me, &self.texts())
    }

    pub fn my_resources(&self, resource_type: Option<String>) -> ApiResult<Vec<HeldResource>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let only = resource_type
            .map(|r| Slug::new(&r))
            .transpose()
            .map_err(ApiError::invalid)?;
        self.held_resources(&v, me, &self.texts(), only.as_ref())
    }

    // ----- documents ---------------------------------------------------------------

    fn doc_delivery(&self, v: &View, doc_id: &core_model::Id) -> Option<String> {
        let me = self.keys.public();
        let last = v.events.iter().rev().find(|e| {
            e.body.author == me
                && e.payload()
                    .ok()
                    .and_then(|p| p.object)
                    .is_some_and(|o| o.id == *doc_id)
        })?;
        let st = self.state.lock().ok()?;
        let d = core_sync::delivery::delivery(st.store.store()?, &v.proj, last).ok()?;
        let t = self.texts();
        Some(t.get(match d {
            core_sync::delivery::Delivery::Pending => "core.delivery.pending",
            core_sync::delivery::Delivery::Sent => "core.delivery.sent",
            core_sync::delivery::Delivery::Delivered => "core.delivery.delivered",
        }))
    }

    pub(crate) fn document_row(
        &self,
        v: &View,
        t: &Texts,
        id: &core_model::Id,
        d: &Document,
    ) -> DocumentRow {
        let variant_key = format!("variant.{}", d.content.variant);
        let variant_label = t.get(&variant_key);
        DocumentRow {
            document_id: id.to_string(),
            form: d.content.form.to_string(),
            subject_id: d.content.subject.to_string(),
            subject_name: name_of(v, d.content.subject),
            period: d.content.period.to_string(),
            period_label: t.period(&d.content.period),
            state_label: state_label(t, &d.state),
            variant_label: if variant_label == variant_key {
                String::new()
            } else {
                variant_label
            },
            delivery: self.doc_delivery(v, id),
        }
    }

    /// The form of a subject for a period: existing document or a blank one.
    pub fn form(
        &self,
        form: String,
        subject: Option<String>,
        period: String,
    ) -> ApiResult<FormView> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let form_id = Slug::new(&form).map_err(ApiError::invalid)?;
        let fdef = cfg
            .form(&form_id)
            .ok_or_else(|| ApiError::not_found("form"))?;
        let wf = cfg
            .form_workflow(fdef)
            .ok_or_else(|| ApiError::not_found("workflow"))?;
        let subject = match subject {
            Some(s) => crate::parse_person(&s)?,
            None => me,
        };
        let period: PeriodKey = period.parse().map_err(ApiError::invalid)?;
        let at = core_engine::rules::reference_date(&period, today(self));
        let doc = v.proj.document_by(&form_id, subject, period);
        let (variant, values, state) = match doc {
            Some((_, d)) => (
                fdef.variants.iter().find(|x| x.id == d.content.variant),
                Some(&d.content.values),
                Some(d.state.clone()),
            ),
            None => (
                variant_for(&v.proj.auth, cfg, fdef, subject, at),
                None,
                None,
            ),
        };
        let variant =
            variant.ok_or_else(|| ApiError::not_found("no variant applies to this person"))?;
        let current = state.clone().unwrap_or_else(|| wf.initial.clone());
        let unit = doc
            .map(|(_, d)| d.content.unit)
            .unwrap_or_else(|| core_engine::rules::subject_unit(&v.proj.auth, fdef, subject, at));
        let editable = current == wf.initial
            && may_fill(&v.proj.auth, cfg, me, fdef, subject, unit, today(self));
        let actions = wf
            .steps
            .iter()
            .filter(|s| s.kind == StepKind::Transition && s.from.contains(&current))
            .filter(|s| {
                s.by.as_ref().is_some_and(|by| {
                    step_actors(
                        &v.proj.auth,
                        cfg,
                        fdef,
                        by,
                        Some(subject),
                        unit,
                        today(self),
                    )
                    .is_ok_and(|a| a.contains(&me))
                })
            })
            .map(|s| Action {
                id: s.id.to_string(),
                label: s.label.as_ref().map_or_else(
                    || t.get(&core_model::config::default_label::step(s.id.as_str())),
                    |l| t.get(l.as_str()),
                ),
                note: serde_json::to_value(s.note)
                    .ok()
                    .and_then(|x| x.as_str().map(str::to_owned))
                    .unwrap_or_default(),
                primary: s.to != wf.initial,
                disabled: false,
            })
            .collect();
        // Who will read it once submitted (the state after the first step out of the initial one).
        let next = wf
            .steps
            .iter()
            .find(|s| s.from.contains(&wf.initial) && s.kind == StepKind::Transition)
            .map(|s| s.to.clone())
            .unwrap_or_else(|| current.clone());
        let audience = cfg.read_audience(fdef, &next);
        let readers: BTreeSet<PersonId> = Evaluator::new(cfg.index(), &v.proj.auth.org)
            .resolve(
                &audience,
                &EvalContext::at(today(self)).with_subject(subject, unit),
            )
            .unwrap_or_default();
        let names: Vec<String> = readers
            .iter()
            .filter(|p| **p != subject && **p != me)
            .map(|p| name_of(&v, *p))
            .collect();
        let history = doc
            .map(|(_, d)| {
                d.history
                    .iter()
                    .filter(|h| h.by.is_some())
                    .map(|h| HistoryLine {
                        date: t.day_month(h.date),
                        label: wf
                            .steps
                            .iter()
                            .find(|s| s.id == h.step)
                            .and_then(|s| s.label.as_ref())
                            .map_or_else(|| state_label(&t, &h.to), |l| t.get(l.as_str())),
                        by: h.by.map(|p| name_of(&v, p)).unwrap_or_default(),
                        note: h.note.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let due = due_date(&period, fdef.due, today(self));
        Ok(FormView {
            document_id: doc.map(|(id, _)| id.to_string()),
            form,
            subject_id: subject.to_string(),
            subject_name: name_of(&v, subject),
            period: period.to_string(),
            title: format!("{} · {}", t.get(fdef.label.as_str()), t.period(&period)),
            subtitle: if state.is_none() || state.as_ref() == Some(&wf.initial) {
                t.fmt("core.due_by", &[("date", t.day_month(due))])
            } else {
                String::new()
            },
            state_label: state.as_ref().map(|s| state_label(&t, s)),
            state: state.map(|s| s.to_string()),
            delivery: doc.and_then(|(id, _)| self.doc_delivery(&v, id)),
            editable,
            fields: variant
                .fields
                .iter()
                .filter(|f| f.ty != FieldType::Computed)
                .map(|f| field_dto(&t, f, values.and_then(|vals| vals.get(&f.id))))
                .collect(),
            actions,
            readers: if names.is_empty() {
                String::new()
            } else {
                t.fmt("core.readers", &[("readers", names.join(", "))])
            },
            history,
        })
    }

    pub fn my_documents(&self, form: Option<String>) -> ApiResult<Vec<DocumentRow>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let only = form
            .map(|f| Slug::new(&f))
            .transpose()
            .map_err(ApiError::invalid)?;
        let mut rows: Vec<DocumentRow> = v
            .proj
            .documents
            .iter()
            .filter(|(_, d)| {
                d.content.subject == me && only.as_ref().is_none_or(|f| &d.content.form == f)
            })
            .map(|(id, d)| self.document_row(&v, &t, id, d))
            .collect();
        rows.sort_by(|a, b| b.period.cmp(&a.period));
        Ok(rows)
    }

    /// Batches and documents of a workflow this user can act on (§12.2 `work_queue`, `batch`).
    pub fn work_queue(&self, workflow: String, unit_type: Option<String>) -> ApiResult<WorkQueue> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let wf_id = Slug::new(&workflow).map_err(ApiError::invalid)?;
        let wf = cfg
            .workflow(&wf_id)
            .ok_or_else(|| ApiError::not_found("workflow"))?;
        let now = today(self);
        let mut batches = Vec::new();
        let mut title = String::new();
        for form in cfg.forms().iter().filter(|f| f.workflow == wf_id) {
            title = t.get(form.label.as_str());
            let Some(batching) = &form.batching else {
                continue;
            };
            if unit_type
                .as_ref()
                .is_some_and(|u| u != batching.unit_type.as_str())
            {
                continue;
            }
            let batch_steps: Vec<_> = wf
                .steps
                .iter()
                .filter(|s| matches!(s.kind, StepKind::Forward | StepKind::Register))
                .collect();
            let collect_state = wf
                .steps
                .iter()
                .find(|s| s.kind == StepKind::Collect)
                .map(|s| s.to.clone())
                .unwrap_or_else(|| wf.initial.clone());
            for (uid, unit) in v
                .proj
                .auth
                .org
                .units
                .iter()
                .filter(|(_, u)| u.active && u.unit_type == batching.unit_type)
            {
                let my_steps: Vec<_> = batch_steps
                    .iter()
                    .filter(|s| {
                        s.by.as_ref().is_some_and(|by| {
                            step_actors(&v.proj.auth, cfg, form, by, None, Some(*uid), now)
                                .is_ok_and(|a| a.contains(&me))
                        })
                    })
                    .collect();
                if my_steps.is_empty() {
                    continue;
                }
                // Periods: the last complete one plus any with documents still in the flow.
                let mut periods: BTreeSet<PeriodKey> = [last_period(form.periodicity, now)].into();
                for d in v
                    .proj
                    .documents
                    .values()
                    .filter(|d| d.content.form == form.id && d.content.unit == Some(*uid))
                {
                    if my_steps.iter().any(|s| s.from.contains(&d.state)) {
                        periods.insert(d.content.period);
                    }
                }
                for period in periods.into_iter().rev() {
                    batches.push(self.batch_view(
                        &v,
                        &t,
                        cfg,
                        form,
                        wf,
                        *uid,
                        &unit.name,
                        period,
                        &my_steps,
                        &collect_state,
                    )?);
                }
            }
        }
        let documents = v
            .proj
            .documents
            .iter()
            .filter(|(_, d)| {
                cfg.form(&d.content.form)
                    .is_some_and(|f| f.workflow == wf_id)
                    && d.content.subject != me
                    && wf.steps.iter().any(|s| {
                        s.kind == StepKind::Transition
                            && s.from.contains(&d.state)
                            && s.by.as_ref().is_some_and(|by| {
                                cfg.form(&d.content.form).is_some_and(|f| {
                                    step_actors(
                                        &v.proj.auth,
                                        cfg,
                                        f,
                                        by,
                                        Some(d.content.subject),
                                        d.content.unit,
                                        now,
                                    )
                                    .is_ok_and(|a| a.contains(&me))
                                })
                            })
                    })
            })
            .map(|(id, d)| self.document_row(&v, &t, id, d))
            .collect();
        Ok(WorkQueue {
            title,
            batches,
            documents,
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn batch_view(
        &self,
        v: &View,
        t: &Texts,
        cfg: &Cfg,
        form: &core_model::config::FormDef,
        wf: &core_model::config::WorkflowDef,
        unit: UnitId,
        unit_name: &str,
        period: PeriodKey,
        my_steps: &[&&core_model::config::StepDef],
        collect_state: &Slug,
    ) -> ApiResult<BatchView> {
        let now = today(self);
        let expected = expected_subjects(&v.proj.auth, cfg, form, &period, Some(unit));
        let present = present_subjects(
            cfg,
            form,
            v.proj.documents.values(),
            &period,
            Some(unit),
            collect_state,
        );
        let docs: Vec<(&core_model::Id, &Document)> = v
            .proj
            .documents
            .iter()
            .filter(|(_, d)| {
                d.content.form == form.id
                    && d.content.unit == Some(unit)
                    && d.content.period == period
            })
            .collect();
        let kind_label = |p: PersonId| -> String {
            docs.iter()
                .find(|(_, d)| d.content.subject == p)
                .map(|(_, d)| {
                    let k = format!("variant.{}", d.content.variant);
                    let l = t.get(&k);
                    if l == k { String::new() } else { l }
                })
                .unwrap_or_default()
        };
        let received: Vec<PersonRow> = docs
            .iter()
            .filter(|(_, d)| d.state == *collect_state)
            .map(|(id, d)| PersonRow {
                person_id: d.content.subject.to_string(),
                name: name_of(v, d.content.subject),
                detail: kind_label(d.content.subject),
                document_id: Some(id.to_string()),
            })
            .collect();
        let missing: Vec<PersonRow> = expected
            .iter()
            .filter(|p| !present.contains(p))
            .map(|p| PersonRow {
                person_id: p.to_string(),
                name: name_of(v, *p),
                detail: String::new(),
                document_id: docs
                    .iter()
                    .find(|(_, d)| d.content.subject == *p)
                    .map(|(id, _)| id.to_string()),
            })
            .collect();
        let done: Vec<DocumentRow> = docs
            .iter()
            .filter(|(_, d)| cfg.state_rank(form, &d.state) > cfg.state_rank(form, collect_state))
            .map(|(id, d)| self.document_row(v, t, id, d))
            .collect();
        let forward = wf.steps.iter().find(|s| s.kind == StepKind::Forward);
        let hint = (!missing.is_empty())
            .then(|| {
                let last = period.last_day()?;
                let offset = forward?.allow_incomplete_after?;
                let d = add_days(last, i64::from(offset.offset_days));
                (now < d).then(|| t.fmt("core.incomplete_from", &[("date", t.day_month(d))]))
            })
            .flatten();
        let actions = my_steps
            .iter()
            .filter(|s| docs.iter().any(|(_, d)| s.from.contains(&d.state)))
            .map(|s| Action {
                id: s.id.to_string(),
                label: s.label.as_ref().map_or_else(
                    || t.get(&core_model::config::default_label::step(s.id.as_str())),
                    |l| t.get(l.as_str()),
                ),
                note: "none".into(),
                primary: true,
                disabled: s.kind == StepKind::Forward && hint.is_some(),
            })
            .collect();
        let me = self.me()?;
        Ok(BatchView {
            form: form.id.to_string(),
            unit_id: unit.to_string(),
            unit_name: unit_name.to_owned(),
            period: period.to_string(),
            period_label: t.period(&period),
            progress: t.fmt(
                "core.received_count",
                &[
                    ("n", (expected.len() - missing.len()).to_string()),
                    ("total", expected.len().to_string()),
                ],
            ),
            received,
            missing,
            done,
            actions,
            hint,
            can_fill_on_behalf: may_fill(
                &v.proj.auth,
                cfg,
                me,
                form,
                PersonId(core_model::Id::from_bytes([0; 16])),
                Some(unit),
                now,
            ),
        })
    }

    // ----- aggregates ------------------------------------------------------------

    pub fn aggregate(&self, aggregate: String, period: Option<String>) -> ApiResult<AggregateView> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let id = Slug::new(&aggregate).map_err(ApiError::invalid)?;
        let def = cfg
            .aggregate(&id)
            .ok_or_else(|| ApiError::not_found("aggregate"))?;
        let form = cfg
            .form(&def.source.form)
            .ok_or_else(|| ApiError::not_found("form"))?;
        let period: PeriodKey = match period {
            Some(p) => p.parse().map_err(ApiError::invalid)?,
            None => last_period(form.periodicity, today(self)),
        };
        let agg = v.proj.aggregates.get(&(id.clone(), period));
        let snap = agg.and_then(|a| match (&a.generated, &a.distributed) {
            (Some(g), Some(d)) => Some(if g.version > d.version { g } else { d }),
            (g, d) => g.as_ref().or(d.as_ref()),
        });
        let obj = ObjectRef {
            kind: ObjectKind::Aggregate,
            id: id.clone(),
        };
        let now = today(self);
        let registered = v
            .proj
            .documents
            .values()
            .filter(|d| {
                d.content.form == def.source.form
                    && d.content.period == period
                    && d.state == def.source.state
            })
            .count() as u32;
        let unit_names: BTreeMap<String, String> = v
            .proj
            .auth
            .org
            .units
            .iter()
            .map(|(id, u)| (id.to_string(), u.name.clone()))
            .collect();
        let group_label = |key: &str| -> String {
            if key.is_empty() {
                return t.get("core.total");
            }
            key.split(',')
                .map(|part| match part.split_once('=') {
                    Some(("variant", x)) => {
                        let k = format!("variant.{x}");
                        let l = t.get(&k);
                        if l == k { x.to_owned() } else { l }
                    }
                    Some(("unit", x)) => unit_names.get(x).cloned().unwrap_or_else(|| x.to_owned()),
                    Some((_, x)) => x.to_owned(),
                    None => part.to_owned(),
                })
                .collect::<Vec<_>>()
                .join(" · ")
        };
        let groups = snap
            .map(|s| {
                s.result
                    .groups
                    .iter()
                    .map(|(k, ms)| MeasureGroup {
                        label: group_label(k),
                        measures: def
                            .measures
                            .iter()
                            .map(|m| Measure {
                                label: m.label.as_ref().map_or_else(
                                    || {
                                        t.get(&core_model::config::default_label::measure(
                                            m.id.as_str(),
                                        ))
                                    },
                                    |l| t.get(l.as_str()),
                                ),
                                value: ms.get(&m.id).copied().unwrap_or(0.0),
                            })
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let documents = snap
            .map(|s| {
                s.documents
                    .iter()
                    .map(|d| {
                        let doc = Document {
                            content: d.content.clone(),
                            state: d.state.clone(),
                            version: d.version,
                            created_by: None,
                            history: Vec::new(),
                        };
                        self.document_row(&v, &t, &d.content.document, &doc)
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(AggregateView {
            aggregate,
            title: t.get(def.label.as_str()),
            period: period.to_string(),
            period_label: t.period(&period),
            version: snap.map(|s| s.version),
            distributed: agg
                .and_then(|a| a.distributed.as_ref())
                .is_some_and(|d| snap.is_some_and(|s| s.version == d.version)),
            groups,
            missing: snap
                .map(|s| s.result.missing.iter().map(|p| name_of(&v, *p)).collect())
                .unwrap_or_default(),
            documents,
            can_generate: v.proj.auth.is_allowed(
                cfg,
                me,
                Permission::AggregateGenerate,
                Some(&obj),
                None,
                now,
            ),
            can_distribute: agg.is_some_and(|a| a.generated.is_some())
                && v.proj.auth.is_allowed(
                    cfg,
                    me,
                    Permission::DocumentDistribute,
                    Some(&obj),
                    None,
                    now,
                ),
            registered_count: registered,
        })
    }

    // ----- programs ----------------------------------------------------------------

    pub fn programs(&self, types: Option<String>) -> ApiResult<ProgramList> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let wanted: Option<BTreeSet<String>> =
            types.map(|s| s.split(',').map(str::to_owned).collect());
        let defs: Vec<_> = cfg
            .package()
            .programs
            .program_types
            .iter()
            .filter(|d| wanted.as_ref().is_none_or(|w| w.contains(d.id.as_str())))
            .collect();
        let now = today(self);
        let mut programs = Vec::new();
        let mut seen = BTreeSet::new();
        for (id, p) in &v.proj.programs {
            if defs.iter().any(|d| d.id == p.program_type) {
                seen.insert(*id);
                let first = p.occurrences.first().copied().unwrap_or(now);
                programs.push(ProgramRow {
                    program_id: id.to_string(),
                    program_type: p.program_type.to_string(),
                    title: defs
                        .iter()
                        .find(|d| d.id == p.program_type)
                        .map_or_else(String::new, |d| t.get(d.label.as_str())),
                    subtitle: defs.iter().find(|d| d.id == p.program_type).map_or_else(
                        || t.period(&PeriodKey::week_of(first)),
                        |d| t.period(&crate::schedule::period_of(d, first)),
                    ),
                    status: match (p.status, v.proj.published.contains_key(id)) {
                        (_, true) => "published",
                        (ProgramStatus::Assigned, false) => "assigned",
                        (ProgramStatus::Draft, false) => "draft",
                    }
                    .into(),
                });
            }
        }
        for (id, s) in v
            .proj
            .assigned_snapshots
            .iter()
            .chain(v.proj.published.iter())
        {
            if seen.insert(*id) && defs.iter().any(|d| d.id == s.program_type) {
                let first = s.occurrences.first().copied().unwrap_or(now);
                programs.push(ProgramRow {
                    program_id: id.to_string(),
                    program_type: s.program_type.to_string(),
                    title: defs
                        .iter()
                        .find(|d| d.id == s.program_type)
                        .map_or_else(String::new, |d| t.get(d.label.as_str())),
                    subtitle: defs.iter().find(|d| d.id == s.program_type).map_or_else(
                        || t.period(&PeriodKey::week_of(first)),
                        |d| t.period(&crate::schedule::period_of(d, first)),
                    ),
                    status: if v.proj.published.contains_key(id) {
                        "published"
                    } else {
                        "assigned"
                    }
                    .into(),
                });
            }
        }
        programs.sort_by(|a, b| b.subtitle.cmp(&a.subtitle));
        Ok(ProgramList {
            types: defs
                .iter()
                .map(|d| Choice {
                    id: d.id.to_string(),
                    label: t.get(d.label.as_str()),
                })
                .collect(),
            can_draft: defs
                .iter()
                .filter(|d| {
                    v.proj.auth.is_allowed(
                        cfg,
                        me,
                        Permission::ProgramDraft,
                        Some(&ObjectRef {
                            kind: ObjectKind::Program,
                            id: d.id.clone(),
                        }),
                        None,
                        now,
                    )
                })
                .map(|d| d.id.to_string())
                .collect(),
            programs,
        })
    }

    pub fn program(&self, program: String) -> ApiResult<ProgramEditor> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let id = crate::parse_id(&program)?;
        let now = today(self);
        let (program_type, occurrences, assignments, status): ProgramParts<'_> =
            match v.proj.programs.get(&id) {
                Some(p) => (
                    p.program_type.clone(),
                    p.occurrences.clone(),
                    p.assignments
                        .iter()
                        .map(|(k, (a, _))| {
                            (
                                k.clone(),
                                (a.clone(), core_engine::rules::display_name(&v.proj.auth, a)),
                            )
                        })
                        .collect(),
                    if v.proj.published.contains_key(&id) {
                        "published"
                    } else if p.status == ProgramStatus::Assigned {
                        "assigned"
                    } else {
                        "draft"
                    },
                ),
                None => {
                    let s = v
                        .proj
                        .assigned_snapshots
                        .get(&id)
                        .or_else(|| v.proj.published.get(&id))
                        .ok_or_else(|| ApiError::not_found("program"))?;
                    (
                        s.program_type.clone(),
                        s.occurrences.clone(),
                        s.assignments
                            .iter()
                            .map(|a| {
                                (
                                    a.key.clone(),
                                    (
                                        Assignee::External(core_model::Id::from_bytes([0; 16])),
                                        a.display_name.clone(),
                                    ),
                                )
                            })
                            .collect(),
                        if v.proj.published.contains_key(&id) {
                            "published"
                        } else {
                            "assigned"
                        },
                    )
                }
            };
        let def = cfg
            .program_type(&program_type)
            .ok_or_else(|| ApiError::not_found("program type"))?;
        let obj = ObjectRef {
            kind: ObjectKind::Program,
            id: program_type.clone(),
        };
        let can_edit = v.proj.programs.contains_key(&id)
            && (v
                .proj
                .auth
                .is_allowed(cfg, me, Permission::ProgramAssign, Some(&obj), None, now)
                || v.proj.auth.is_allowed(
                    cfg,
                    me,
                    Permission::ProgramDraft,
                    Some(&obj),
                    None,
                    now,
                ));
        let groups = crate::schedule::groups(def, &occurrences);
        let mut cells = Vec::new();
        for group in &groups {
            let occ = group.first().copied().unwrap_or_default();
            let Some(date) = occurrences.get(occ) else {
                continue;
            };
            for slot in def.slots.iter().filter(|s| s.enabled) {
                let reps = match slot.repeat {
                    Some(core_model::config::Repeat::Count(n)) => n,
                    _ => 1,
                };
                for index in 0..slot.count * reps {
                    let key = core_engine::ops::SlotKey {
                        occurrence: occ as u16,
                        slot: slot.id.clone(),
                        index: index as u16,
                    };
                    let current = assignments.get(&key);
                    let candidates = if can_edit {
                        let mut c: Vec<Choice> = v
                            .proj
                            .auth
                            .org
                            .persons
                            .iter()
                            .filter(|(p, r)| {
                                r.active
                                    && !v.proj.auth.is_root(**p)
                                    && v.proj.auth.satisfies(
                                        cfg,
                                        **p,
                                        slot.requires.as_ref(),
                                        *date,
                                    )
                            })
                            .map(|(p, r)| Choice {
                                id: p.to_string(),
                                label: r.display_name.clone(),
                            })
                            .collect();
                        if slot.allow_external {
                            c.extend(v.proj.auth.externals.iter().map(|(id, n)| Choice {
                                id: format!("ext:{id}"),
                                label: n.clone(),
                            }));
                        }
                        c.sort_by(|a, b| a.label.cmp(&b.label));
                        c
                    } else {
                        Vec::new()
                    };
                    // The group's answer: declined if one is, accepted if all are.
                    let answers: Vec<_> = group
                        .iter()
                        .map(|o| {
                            v.proj.responses.get(&(
                                id,
                                core_engine::ops::SlotKey {
                                    occurrence: *o as u16,
                                    ..key.clone()
                                },
                            ))
                        })
                        .collect();
                    let response = if answers
                        .iter()
                        .any(|r| matches!(r, Some(Response::Declined { .. })))
                    {
                        "declined"
                    } else if answers
                        .iter()
                        .all(|r| matches!(r, Some(Response::Accepted)))
                    {
                        "accepted"
                    } else {
                        "pending"
                    };
                    let label = if slot.count * reps > 1 {
                        format!("{} {}", t.get(slot.label.as_str()), index + 1)
                    } else {
                        t.get(slot.label.as_str())
                    };
                    cells.push(SlotCell {
                        occurrence: occ as u32,
                        slot: slot.id.to_string(),
                        index,
                        label,
                        assignee: current.map(|(a, _)| match a {
                            Assignee::Person(p) => p.to_string(),
                            Assignee::External(e) => format!("ext:{e}"),
                        }),
                        assignee_name: current.map(|(_, n)| n.clone()),
                        response: response.into(),
                        candidates,
                    });
                }
            }
        }
        let can_publish = v.proj.assigned_snapshots.contains_key(&id)
            && v.proj
                .auth
                .is_allowed(cfg, me, Permission::ProgramPublish, Some(&obj), None, now);
        let warnings = v
            .proj
            .alerts
            .iter()
            .filter(|a| a.kind == core_engine::projection::AlertKind::DoubleAssignment)
            .map(|a| a.message.clone())
            .collect();
        Ok(ProgramEditor {
            program_id: program,
            program_type: program_type.to_string(),
            title: format!(
                "{} · {}",
                t.get(def.label.as_str()),
                t.period(&crate::schedule::period_of(
                    def,
                    occurrences.first().copied().unwrap_or(now)
                ))
            ),
            status: status.into(),
            occurrences: occurrences
                .iter()
                .map(|d| capitalize(&t.long_date(*d)))
                .collect(),
            groups: groups
                .iter()
                .filter_map(|g| {
                    let first = *g.first()?;
                    let d = *occurrences.get(first)?;
                    let label = match def.rotation {
                        Rotation::Occurrence => t.long_date(d),
                        Rotation::Week => t.period(&PeriodKey::week_of(d)),
                        Rotation::Month => t.period(&PeriodKey::month_of(d)),
                    };
                    let detail = (def.rotation != Rotation::Occurrence).then(|| {
                        g.iter()
                            .filter_map(|o| occurrences.get(*o))
                            .map(|x| t.long_date(*x))
                            .collect::<Vec<_>>()
                            .join(", ")
                    });
                    Some(ProgramGroup {
                        occurrence: first as u32,
                        label: capitalize(&label),
                        detail,
                    })
                })
                .collect(),
            cells,
            can_edit,
            can_send: can_edit
                && v.proj.auth.is_allowed(
                    cfg,
                    me,
                    Permission::ProgramAssign,
                    Some(&obj),
                    None,
                    now,
                ),
            can_publish,
            warnings,
        })
    }

    // ----- resources ---------------------------------------------------------------

    pub fn resources(&self, resource_type: String) -> ApiResult<ResourceList> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let rt = Slug::new(&resource_type).map_err(ApiError::invalid)?;
        let def = cfg
            .resource_type(&rt)
            .ok_or_else(|| ApiError::not_found("resource type"))?;
        let obj = ObjectRef {
            kind: ObjectKind::Resource,
            id: rt.clone(),
        };
        let now = today(self);
        let mut resources: Vec<ResourceRow> = v
            .proj
            .resources
            .iter()
            .filter(|(_, r)| r.resource_type == rt && !r.retired)
            .map(|(id, r)| ResourceRow {
                resource_id: id.to_string(),
                title: resource_title(&t, Some(def.label.as_str()), &r.attributes),
                holder: r.holding.as_ref().map(|h| name_of(&v, h.holder)),
                status: if r.holding.is_some() {
                    "assigned"
                } else {
                    "available"
                }
                .into(),
            })
            .collect();
        resources.sort_by(|a, b| a.title.cmp(&b.title));
        let candidates = v
            .proj
            .auth
            .org
            .persons
            .iter()
            .filter(|(p, r)| {
                r.active
                    && !v.proj.auth.is_root(**p)
                    && v.proj
                        .auth
                        .satisfies(cfg, **p, def.holder_requires.as_ref(), now)
            })
            .map(|(p, r)| Choice {
                id: p.to_string(),
                label: r.display_name.clone(),
            })
            .collect();
        Ok(ResourceList {
            resource_type,
            title: t.get(def.label.as_str()),
            can_manage: v.proj.auth.is_allowed(
                cfg,
                me,
                Permission::ResourceManage,
                Some(&obj),
                None,
                now,
            ),
            can_assign: v.proj.auth.is_allowed(
                cfg,
                me,
                Permission::ResourceAssign,
                Some(&obj),
                None,
                now,
            ),
            resources,
            attributes: def
                .attributes
                .iter()
                .filter(|f| f.ty != FieldType::Photos)
                .map(|f| field_dto(&t, f, None))
                .collect(),
            candidates,
            requests: def
                .requests
                .iter()
                .map(|r| Choice {
                    id: r.id.to_string(),
                    label: t.get(r.label.as_str()),
                })
                .collect(),
        })
    }

    pub fn resource(&self, resource: String) -> ApiResult<ResourceDetail> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let id = crate::parse_id(&resource)?;
        let r = v
            .proj
            .resources
            .get(&id)
            .ok_or_else(|| ApiError::not_found("resource"))?;
        let def = cfg
            .resource_type(&r.resource_type)
            .ok_or_else(|| ApiError::not_found("resource type"))?;
        let obj = ObjectRef {
            kind: ObjectKind::Resource,
            id: r.resource_type.clone(),
        };
        let now = today(self);
        let attributes = def
            .attributes
            .iter()
            .filter_map(|f| {
                let value = r.attributes.get(&f.id)?;
                let dto = field_dto(&t, f, Some(value));
                let shown = dto
                    .options
                    .iter()
                    .find(|o| serde_json::Value::String(o.id.clone()) == dto.value)
                    .map_or_else(|| display_value(value), |o| o.label.clone());
                Some(BoardLine {
                    label: dto.label,
                    value: shown,
                })
            })
            .collect();
        let mut history: Vec<HistoryLine> = r
            .history
            .iter()
            .chain(r.holding.iter())
            .map(|h| HistoryLine {
                date: format!(
                    "{} – {}",
                    t.day_month(h.since),
                    h.until.map(|u| t.day_month(u)).unwrap_or_default()
                ),
                label: name_of(&v, h.holder),
                by: format!("{}", h.logs.len()),
                note: h.logs.last().map(|(d, _)| t.day_month(*d)),
            })
            .collect();
        history.reverse();
        let holder = r.holding.as_ref().map(|h| h.holder);
        let held = self.held_resources(&v, me, &t, Some(&r.resource_type))?;
        Ok(ResourceDetail {
            resource_id: resource,
            title: resource_title(&t, Some(def.label.as_str()), &r.attributes),
            attributes,
            holder: holder.map(|h| name_of(&v, h)),
            history,
            log_fields: def
                .log_fields
                .iter()
                .map(|f| field_dto(&t, f, None))
                .collect(),
            is_holder: holder == Some(me),
            can_assign: v.proj.auth.is_allowed(
                cfg,
                me,
                Permission::ResourceAssign,
                Some(&obj),
                None,
                now,
            ),
            can_request: holder.is_some()
                && v.proj.auth.is_allowed(
                    cfg,
                    me,
                    Permission::ResourceRequest,
                    Some(&obj),
                    None,
                    now,
                ),
            open_request: held
                .into_iter()
                .find(|h| h.resource_id == id.to_string())
                .and_then(|h| h.open_request),
        })
    }

    // ----- organization -------------------------------------------------------------

    pub fn people(&self) -> ApiResult<People> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let now = today(self);
        let auth = &v.proj.auth;
        let persons = auth
            .org
            .persons
            .iter()
            .filter(|(_, r)| r.active)
            .map(|(p, r)| {
                let attrs = cfg
                    .package()
                    .manifest
                    .person_attributes
                    .iter()
                    .filter_map(|a| {
                        let val = r.attributes.get(&a.id)?;
                        let label = a.label.as_ref().map_or_else(
                            || t.get(&core_model::config::default_label::field(a.id.as_str())),
                            |l| t.get(l.as_str()),
                        );
                        let shown = match val {
                            Scalar::Str(s) if a.options.is_some() => {
                                t.get(&core_model::config::default_label::option(s))
                            }
                            other => display_value(other),
                        };
                        Some(BoardLine {
                            label,
                            value: shown,
                        })
                    })
                    .collect();
                let quals = auth
                    .qualification_ids
                    .iter()
                    .filter_map(|(gid, i)| auth.org.qualification_grants.get(*i).map(|g| (gid, g)))
                    .filter(|(_, g)| g.person == *p && g.period.end.is_none_or(|e| e > now))
                    .map(|(gid, g)| Grant {
                        grant_id: gid.to_string(),
                        label: cfg.qualification(&g.qualification).map_or_else(
                            || g.qualification.to_string(),
                            |q| t.get(q.label.as_str()),
                        ),
                        period: g
                            .period
                            .end
                            .map(|e| {
                                format!("{} – {}", t.day_month(g.period.start), t.day_month(e))
                            })
                            .unwrap_or_default(),
                    })
                    .collect();
                let roles = auth
                    .role_ids
                    .iter()
                    .filter_map(|(gid, (i, _))| auth.org.role_grants.get(*i).map(|g| (gid, g)))
                    .filter(|(_, g)| g.person == *p && g.period.end.is_none_or(|e| e > now))
                    .map(|(gid, g)| Grant {
                        grant_id: gid.to_string(),
                        label: {
                            let base = cfg
                                .role(&g.role)
                                .map_or_else(|| g.role.to_string(), |r| t.get(r.label.as_str()));
                            match g.scope {
                                core_model::state::GrantScope::Unit(u) => format!(
                                    "{base} · {}",
                                    auth.org
                                        .units
                                        .get(&u)
                                        .map(|x| x.name.clone())
                                        .unwrap_or_default()
                                ),
                                core_model::state::GrantScope::Organization => base,
                            }
                        },
                        period: String::new(),
                    })
                    .collect();
                let ui = &cfg.package().ui.people;
                let defs = &cfg.package().manifest.person_attributes;
                let attr_label = |a: &core_model::config::PersonAttributeDef| {
                    a.label.as_ref().map_or_else(
                        || t.get(&core_model::config::default_label::field(a.id.as_str())),
                        |l| t.get(l.as_str()),
                    )
                };
                let fact = |id: &Slug| {
                    let a = defs.iter().find(|a| &a.id == id)?;
                    let val = r.attributes.get(id)?;
                    Some(match (a.ty, val) {
                        (FieldType::Date, Scalar::Str(s)) => BoardLine {
                            label: attr_label(a),
                            value: core_engine::time::parse_date(s)
                                .map_or_else(|| s.clone(), |d| t.short_date(d)),
                        },
                        (_, Scalar::Str(s)) if a.options.is_some() => BoardLine {
                            label: String::new(),
                            value: t.get(&core_model::config::default_label::option(s)),
                        },
                        (_, other) => BoardLine {
                            label: attr_label(a),
                            value: display_value(other),
                        },
                    })
                };
                let held = |q: &Slug| {
                    auth.org
                        .qualification_grants
                        .iter()
                        .filter(|g| {
                            g.person == *p
                                && &g.qualification == q
                                && g.period.start <= now
                                && g.period.end.is_none_or(|e| e > now)
                        })
                        .map(|g| g.period.start)
                        .min()
                };
                let label_of = |q: &Slug| {
                    cfg.qualification(q)
                        .map_or_else(|| q.to_string(), |x| t.get(x.label.as_str()))
                };
                let is_date =
                    |id: &Slug| defs.iter().any(|a| &a.id == id && a.ty == FieldType::Date);
                let mut facts: Vec<BoardLine> = ui
                    .attributes
                    .iter()
                    .filter(|a| is_date(a))
                    .filter_map(&fact)
                    .collect();
                facts.extend(ui.dated_qualifications.iter().filter_map(|q| {
                    held(q).map(|d| BoardLine {
                        label: label_of(q),
                        value: t.short_date(d),
                    })
                }));
                facts.extend(
                    ui.attributes
                        .iter()
                        .filter(|a| !is_date(a))
                        .filter_map(fact),
                );
                let marks = ui
                    .qualifications
                    .iter()
                    .filter(|q| held(q).is_some())
                    .map(label_of)
                    .collect();
                let values = JsonMap(
                    r.attributes
                        .iter()
                        .map(|(k, v)| (k.to_string(), scalar_json(v)))
                        .collect(),
                );
                PersonView {
                    facts,
                    marks,
                    values,
                    erased: v.proj.erasure.persons.contains(p),
                    person_id: p.to_string(),
                    name: if r.display_name.is_empty() {
                        "—".into()
                    } else {
                        r.display_name.clone()
                    },
                    attributes: attrs,
                    qualifications: quals,
                    unit: auth.units_of_name(*p, now),
                    roles,
                    devices: auth
                        .devices_of(*p)
                        .map(|d| Choice {
                            id: d.identity.signing.to_string(),
                            label: if d.archive {
                                "archive".into()
                            } else {
                                String::new()
                            },
                        })
                        .collect(),
                }
            })
            .collect();
        Ok(People {
            can_manage: auth.is_allowed(cfg, me, Permission::PeopleManage, None, None, now),
            persons,
            qualifications: cfg
                .package()
                .qualifications
                .qualifications
                .iter()
                .map(|q| Choice {
                    id: q.id.to_string(),
                    label: t.get(q.label.as_str()),
                })
                .collect(),
            roles: cfg
                .roles()
                .iter()
                .map(|r| Choice {
                    id: r.id.to_string(),
                    label: t.get(r.label.as_str()),
                })
                .collect(),
            units: auth
                .org
                .units
                .iter()
                .filter(|(_, u)| u.active)
                .map(|(id, u)| Choice {
                    id: id.to_string(),
                    label: u.name.clone(),
                })
                .collect(),
            unit_types: cfg
                .package()
                .units
                .unit_types
                .iter()
                .map(|u| Choice {
                    id: u.id.to_string(),
                    label: t.get(u.label.as_str()),
                })
                .collect(),
            attributes: cfg
                .package()
                .manifest
                .person_attributes
                .iter()
                .map(|a| {
                    let f = FieldDef {
                        id: a.id.clone(),
                        ty: a.ty,
                        label: a.label.clone(),
                        required: false,
                        min: None,
                        max: None,
                        step: None,
                        unit: None,
                        max_length: a.max_length,
                        options: a.options.clone(),
                        filter: None,
                        max_count: None,
                        keep_exif: false,
                        expr: None,
                    };
                    field_dto(&t, &f, None)
                })
                .collect(),
        })
    }

    pub fn units(&self) -> ApiResult<Vec<UnitView>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let now = today(self);
        let auth = &v.proj.auth;
        Ok(auth
            .org
            .units
            .iter()
            .filter(|(_, u)| u.active)
            .map(|(id, u)| UnitView {
                unit_id: id.to_string(),
                name: u.name.clone(),
                unit_type: cfg
                    .unit_type(&u.unit_type)
                    .map_or_else(|| u.unit_type.to_string(), |x| t.get(x.label.as_str())),
                members: auth
                    .org
                    .persons
                    .keys()
                    .filter(|p| auth.org.is_member(**p, *id, now))
                    .map(|p| name_of(&v, *p))
                    .collect(),
                roles: auth
                    .org
                    .active_role_grants(now)
                    .filter(|g| g.scope == core_model::state::GrantScope::Unit(*id))
                    .map(|g| BoardLine {
                        label: cfg
                            .role(&g.role)
                            .map_or_else(|| g.role.to_string(), |r| t.get(r.label.as_str())),
                        value: name_of(&v, g.person),
                    })
                    .collect(),
            })
            .collect())
    }

    pub fn alerts(&self) -> ApiResult<Vec<Alert>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        self.alerts_of(&v)
    }

    /// Alerts of a view (also used on a simulated one, see `config`).
    pub(crate) fn alerts_of(&self, v: &View) -> ApiResult<Vec<Alert>> {
        let t = self.texts();
        let cfg = cfg(v)?;
        let mut out: Vec<Alert> = v
            .proj
            .alerts
            .iter()
            .map(|a| Alert {
                kind: format!("{:?}", a.kind).to_lowercase(),
                message: a.message.clone(),
            })
            .collect();
        for a in core_engine::notify::coverage_alerts(&v.proj, today(self)) {
            match a {
                core_engine::notify::CoverageAlert::Uncovered { role, unit } => out.push(Alert {
                    kind: "uncovered".into(),
                    message: format!(
                        "{}{}",
                        cfg.role(&role)
                            .map_or_else(|| role.to_string(), |r| t.get(r.label.as_str())),
                        unit.and_then(|u| v.proj.auth.org.units.get(&u))
                            .map(|u| format!(" · {}", u.name))
                            .unwrap_or_default()
                    ),
                }),
                core_engine::notify::CoverageAlert::InvalidGrant { role, person } => {
                    out.push(Alert {
                        kind: "invalid_grant".into(),
                        message: format!(
                            "{} · {}",
                            cfg.role(&role)
                                .map_or_else(|| role.to_string(), |r| t.get(r.label.as_str())),
                            name_of(v, person)
                        ),
                    })
                }
            }
        }
        Ok(out)
    }

    pub async fn sync_status(&self) -> ApiResult<SyncStatus> {
        let node = self.node().await;
        let (events, outbox) = {
            let st = self
                .state
                .lock()
                .map_err(|_| ApiError::internal("poisoned"))?;
            (
                st.store.store().and_then(|s| s.count().ok()).unwrap_or(0) as u32,
                st.store
                    .store()
                    .and_then(|s| s.outbox().ok())
                    .map_or(0, |o| o.len()) as u32,
            )
        };
        let t = self.texts();
        let peers = self
            .view()
            .map(|v| {
                v.proj
                    .auth
                    .devices
                    .iter()
                    .filter(|(k, d)| **k != self.keys.public() && !d.revoked)
                    .map(|(k, d)| PeerInfo {
                        device_id: k.to_string(),
                        person: name_of(&v, d.person),
                        archive: d.archive,
                        last_contact: None,
                    })
                    .collect()
            })
            .unwrap_or_default();
        let last = self.local()?.last_sync.map(|ms| {
            t.day_month(date_of(ms))
                + &format!(" {:02}:{:02}", (ms / 3_600_000) % 24, (ms / 60_000) % 60)
        });
        Ok(SyncStatus {
            online: node.is_some(),
            relay: match self.effective_network() {
                None => "offline".into(),
                Some(core_sync::net::Network::Local) => "local".into(),
                Some(core_sync::net::Network::PublicRelays) => "public relays".into(),
                Some(core_sync::net::Network::Server { relays, .. }) => relays.join(", "),
            },
            events,
            outbox,
            last_sync: last,
            peers,
        })
    }

    /// New notifications for this person since the last call (§11.1).
    pub fn take_notifications(&self) -> ApiResult<Vec<Notification>> {
        let Ok(v) = self.view() else {
            return Ok(Vec::new());
        };
        let Ok(me) = self.me() else {
            return Ok(Vec::new());
        };
        let t = self.texts();
        let local = self.local()?;
        let seen: BTreeSet<&String> = local.notified.iter().collect();
        let mut out = Vec::new();
        let mut ids = Vec::new();
        for tr in core_engine::notify::notifications_for(&v.proj, me) {
            let id = format!("{}:{}", tr.event, tr.text);
            if seen.contains(&id) {
                continue;
            }
            let params: Vec<(&str, String)> = tr
                .params
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            out.push(Notification {
                id: id.clone(),
                text: t.fmt(tr.text.as_str(), &params),
                date: t.day_month(tr.date),
            });
            ids.push(id);
        }
        for n in core_engine::notify::due_reminders(&v.proj, today(self)) {
            if !n.recipients.contains(&me) {
                continue;
            }
            let id = format!("{}:{}", today(self), n.text);
            if seen.contains(&id) {
                continue;
            }
            let params: Vec<(&str, String)> = n
                .params
                .iter()
                .map(|(k, v)| (k.as_str(), v.clone()))
                .collect();
            out.push(Notification {
                id: id.clone(),
                text: t.fmt(n.text.as_str(), &params),
                date: t.day_month(today(self)),
            });
            ids.push(id);
        }
        if !ids.is_empty() {
            self.update_local(|l| {
                l.notified.extend(ids);
                let n = l.notified.len();
                if n > 2000 {
                    l.notified.drain(..n - 2000);
                }
            })?;
        }
        Ok(out)
    }

    pub fn settings(&self) -> ApiResult<Settings> {
        let local = self.local()?;
        let t = self.texts();
        let cats: BTreeSet<String> = self
            .package
            .package
            .notifications
            .notifications
            .iter()
            .filter(|n| !n.mandatory)
            .filter_map(|n| n.category.as_ref().map(Slug::to_string))
            .collect();
        Ok(Settings {
            auto_lock_minutes: local.auto_lock_minutes.unwrap_or(5),
            mobile_data_photos: local.mobile_data_photos,
            muted_categories: local.muted_categories,
            categories: cats
                .into_iter()
                .map(|c| Choice {
                    label: t.get_or(&format!("category.{c}"), &c),
                    id: c,
                })
                .collect(),
        })
    }

    pub fn pairings(&self) -> ApiResult<Vec<Pairing>> {
        let v = self.view()?;
        Ok(self
            .pending_pairings()?
            .into_iter()
            .map(|(p, code)| Pairing {
                person: name_of(&v, p),
                code,
            })
            .collect())
    }
}

/// Due date: `offset_days` after the last day of the period (as the
/// reminders count), so `due: 5` on a monthly form means the 5th.
/// Type, dates, assignments (with their status) and status of a program.
type ProgramParts<'a> = (
    Slug,
    Vec<Date>,
    BTreeMap<core_engine::ops::SlotKey, (Assignee, String)>,
    &'a str,
);

pub(crate) fn due_date(
    period: &PeriodKey,
    due: Option<core_model::config::Offset>,
    fallback: Date,
) -> Date {
    add_days(
        period.last_day().unwrap_or(fallback),
        i64::from(due.map_or(0, |d| d.offset_days)),
    )
}

pub(crate) fn display_value(v: &Scalar) -> String {
    match v {
        Scalar::Null => String::new(),
        Scalar::Bool(b) => {
            if *b {
                "✓".into()
            } else {
                "✗".into()
            }
        }
        Scalar::Int(i) => i.to_string(),
        Scalar::Num(n) => n.to_string(),
        Scalar::Str(s) => s.clone(),
        Scalar::List(l) => l.iter().map(display_value).collect::<Vec<_>>().join(", "),
    }
}

/// "<type> <number> · <name>" from the first text attributes.
pub(crate) fn resource_title(
    t: &Texts,
    type_label: Option<&str>,
    attrs: &core_engine::ops::Values,
) -> String {
    let texts: Vec<String> = attrs
        .values()
        .filter_map(|v| {
            if let Scalar::Str(s) = v {
                Some(s.clone())
            } else {
                None
            }
        })
        .take(2)
        .collect();
    let base = type_label.map(|l| t.get(l)).unwrap_or_default();
    match texts.as_slice() {
        [] => base,
        [a] => format!("{base} {a}"),
        [a, b, ..] => format!("{base} {a} · {b}"),
    }
}

pub(crate) fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

trait UnitsOf {
    fn units_of_name(&self, p: PersonId, at: Date) -> Option<String>;
}

impl UnitsOf for core_engine::AuthState {
    fn units_of_name(&self, p: PersonId, at: Date) -> Option<String> {
        let names: Vec<String> = self
            .org
            .units_of(p, at)
            .filter_map(|u| self.org.units.get(&u).map(|x| x.name.clone()))
            .collect();
        (!names.is_empty()).then(|| names.join(", "))
    }
}
