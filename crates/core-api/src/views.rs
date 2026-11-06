//! Read-only views (§12.2), built from the projection of this device.

use std::collections::{BTreeMap, BTreeSet};

use core_engine::config::Cfg;
use core_engine::ops::{Assignee, Response};
use core_engine::projection::{BoardItem as PBoardItem, Document, ProgramStatus};
use core_engine::rules::{expected_subjects, may_fill, present_subjects, step_actors, variant_for};
use core_engine::time::{PeriodKey, add_days, date_of};
use core_model::config::{
    FieldDef, FieldType, NotifyOn, Periodicity, Rotation, StepKind, ViewKind,
};
use core_model::eval::{EvalContext, Evaluator};
use core_model::strings::{ObjectKind, ObjectRef};
use core_model::{Permission, PersonId, Scalar, Slug, UnitId};
use time::Date;

use crate::access::{holds_any, view_permissions};
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

/// A person attribute as a form field, with its value.
pub(crate) fn attribute_field(
    t: &Texts,
    a: &core_model::config::PersonAttributeDef,
    value: Option<&Scalar>,
) -> Field {
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
    field_dto(t, &f, value)
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
/// A publish group's programs as one table: a column per assignment
/// (program types in the group's order, slots in their order, "Name 2" for
/// the second of a slot), a row per event, or per week when every program
/// keeps the same persons for the week (both dates in the row).
fn board_grid(t: &Texts, cfg: &Cfg, snaps: &[&core_engine::ops::ProgramSnapshot]) -> BoardGrid {
    use core_model::config::Rotation;
    let group = snaps.first().and_then(|s| {
        cfg.package()
            .programs
            .publish_groups
            .iter()
            .find(|g| g.program_types.contains(&s.program_type))
    });
    let mut ordered: Vec<&&core_engine::ops::ProgramSnapshot> = snaps.iter().collect();
    ordered.sort_by_key(|s| {
        group
            .and_then(|g| g.program_types.iter().position(|x| *x == s.program_type))
            .unwrap_or(usize::MAX)
    });
    // Columns: (type, slot) with at least one person; a slot for several
    // persons is one column, its persons one per line.
    let mut columns: Vec<(Slug, Slug, String)> = Vec::new();
    for s in &ordered {
        let Some(def) = cfg.program_type(&s.program_type) else {
            continue;
        };
        for slot in &def.slots {
            if s.assignments.iter().any(|a| a.key.slot == slot.id && !a.display_name.is_empty()) {
                columns.push((s.program_type.clone(), slot.id.clone(), t.get(slot.label.as_str())));
            }
        }
    }
    // Rows: a week when every program keeps the persons for the week.
    let weekly = ordered.iter().all(|s| {
        cfg.program_type(&s.program_type)
            .is_some_and(|d| d.rotation == Rotation::Week)
    });
    let mut dates: Vec<Date> = ordered.iter().flat_map(|s| s.occurrences.iter().copied()).collect();
    dates.sort_unstable();
    dates.dedup();
    let mut rows: Vec<Vec<Date>> = Vec::new();
    for d in dates {
        let same = |x: &Date| weekly && PeriodKey::week_of(*x) == PeriodKey::week_of(d);
        match rows.last_mut() {
            Some(r) if r.first().is_some_and(same) => r.push(d),
            _ => rows.push(vec![d]),
        }
    }
    BoardGrid {
        columns: columns.iter().map(|c| c.2.clone()).collect(),
        colors: columns
            .iter()
            .map(|c| cfg.program_type(&c.0).and_then(|d| d.color.clone()))
            .collect(),
        rows: rows
            .into_iter()
            .map(|ds| BoardGridRow {
                dates: ds.iter().map(|d| t.long_date(*d)).collect(),
                cells: columns
                    .iter()
                    .map(|(pt, slot, _)| {
                        // In the slot's order (first, second…), each person once.
                        let mut found: Vec<(u16, &str)> = ordered
                            .iter()
                            .filter(|s| &s.program_type == pt)
                            .flat_map(|s| s.assignments.iter())
                            .filter(|a| &a.key.slot == slot && ds.contains(&a.date))
                            .filter(|a| !a.display_name.is_empty())
                            .map(|a| (a.key.index, a.display_name.as_str()))
                            .collect();
                        found.sort_by_key(|(i, _)| *i);
                        let mut names: Vec<String> = Vec::new();
                        for (_, n) in found {
                            if !names.iter().any(|x| x == n) {
                                names.push(n.to_owned());
                            }
                        }
                        names
                    })
                    .collect(),
            })
            .collect(),
    }
}

/// A publish group's period while listing: newest creation, statuses of its
/// programs, program types present.
type GroupRow = (i64, Vec<&'static str>, BTreeSet<Slug>);

/// One measure of an aggregate as shown: label, value (durations in hours),
/// section and emphasis.
fn measure_row(
    t: &Texts,
    def: &core_model::config::AggregateDef,
    form: &core_model::config::FormDef,
    m: &core_model::config::MeasureDef,
    values: &BTreeMap<Slug, f64>,
) -> Measure {
    use core_model::config::MeasureOp;
    let field_is_duration = |id: &Slug| {
        form.variants
            .iter()
            .flat_map(|v| v.fields.iter())
            .any(|f| &f.id == id && f.ty == FieldType::Duration)
    };
    fn duration(
        def: &core_model::config::AggregateDef,
        m: &core_model::config::MeasureDef,
        field: &dyn Fn(&Slug) -> bool,
        depth: u8,
    ) -> bool {
        match m.op {
            MeasureOp::Sum | MeasureOp::Avg | MeasureOp::Min | MeasureOp::Max => {
                m.field.as_ref().is_some_and(field)
            }
            MeasureOp::Total if depth < 8 => m.of.iter().all(|o| {
                def.measures
                    .iter()
                    .find(|x| &x.id == o)
                    .is_some_and(|x| duration(def, x, field, depth + 1))
            }),
            _ => false,
        }
    }
    let value = values.get(&m.id).copied().unwrap_or(0.0) + 0.0;
    let display = if duration(def, m, &field_is_duration, 0) {
        let h = value / 60.0;
        if h.fract() == 0.0 {
            format!("{h} h")
        } else {
            format!("{h:.1} h").replace('.', ",")
        }
    } else if value.fract() == 0.0 {
        format!("{value}")
    } else {
        format!("{value:.1}").replace('.', ",")
    };
    Measure {
        label: m.label.as_ref().map_or_else(
            || t.get(&core_model::config::default_label::measure(m.id.as_str())),
            |l| t.get(l.as_str()),
        ),
        value,
        display,
        section: m.section.as_ref().map(|s| t.get(s.as_str())),
        strong: m.strong,
        row: m.row.as_ref().map(|s| t.get(s.as_str())),
        column: m.column.as_ref().map(|s| t.get(s.as_str())),
        tile: m.tile,
        note: m.note,
    }
}

/// Totals of a set of documents: sum of each numeric field (durations in
/// hours), count of the yes answers, in the order of the form's fields.
fn form_totals(t: &Texts, form: &core_model::config::FormDef, docs: &[&Document]) -> Vec<Measure> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for f in form.variants.iter().flat_map(|v| v.fields.iter()) {
        if !seen.insert(f.id.clone()) {
            continue;
        }
        let values = docs.iter().filter_map(|d| d.content.values.get(&f.id));
        let total: Option<f64> = match f.ty {
            FieldType::Integer => Some(values.filter_map(|x| match x {
                Scalar::Int(n) => Some(*n as f64),
                _ => None,
            }).sum()),
            FieldType::Decimal => Some(values.filter_map(|x| match x {
                Scalar::Int(n) => Some(*n as f64),
                Scalar::Num(n) => Some(*n),
                _ => None,
            }).sum()),
            FieldType::Duration => Some(values.filter_map(|x| match x {
                Scalar::Int(m) => Some(*m as f64 / 60.0),
                _ => None,
            }).sum()),
            FieldType::Boolean => Some(values.filter(|x| matches!(x, Scalar::Bool(true))).count() as f64),
            _ => None,
        };
        if let Some(value) = total {
            let value = value + 0.0; // an empty float sum is -0.0
            out.push(Measure {
                label: field_dto(t, f, None).label,
                display: if value.fract() == 0.0 { format!("{value}") } else { format!("{value:.1}").replace('.', ",") },
                value,
                section: None,
                strong: false,
                row: None,
                column: None,
                tile: false,
                note: false,
            });
        }
    }
    out
}

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
    if !p.qualifications.is_empty() {
        m.insert(
            "qualifications".into(),
            p.qualifications
                .iter()
                .map(Slug::to_string)
                .collect::<Vec<_>>()
                .join(","),
        );
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

/// The slots held in the other programs sent to who publishes them (their
/// last roster: who holds what, published or not), as far as this device
/// can read them.
pub(crate) fn other_slots(v: &View, cfg: &Cfg, t: &Texts, this: &core_model::Id) -> Vec<OtherSlot> {
    let mut out = Vec::new();
    for (id, (_, persons)) in &v.proj.rosters {
        if id == this {
            continue;
        }
        let Some(ty) = v
            .proj
            .assigned_snapshots
            .get(id)
            .map(|s| s.program_type.clone())
            .or_else(|| v.proj.programs.get(id).map(|p| p.program_type.clone()))
        else {
            continue;
        };
        let Some(def) = cfg.program_type(&ty) else {
            continue;
        };
        for (person, items) in persons {
            for it in items {
                let Some(slot) = def.slots.iter().find(|s| s.id == it.key.slot) else {
                    continue;
                };
                let reps = match slot.repeat {
                    Some(core_model::config::Repeat::Count(n)) => n,
                    _ => 1,
                };
                let slot_label = if slot.count * reps > 1 && slot.numbered {
                    format!("{} {}", t.get(slot.label.as_str()), it.key.index + 1)
                } else {
                    t.get(slot.label.as_str())
                };
                // Named like a slot of another program: add the short name.
                let shared = cfg
                    .package()
                    .programs
                    .program_types
                    .iter()
                    .any(|o| o.id != def.id && o.slots.iter().any(|x| x.id == slot.id));
                let slot_name = match (&def.short, shared) {
                    (Some(k), true) => format!("{slot_label} ({})", t.get(k.as_str())),
                    _ => slot_label.clone(),
                };
                out.push(OtherSlot {
                    person: person.to_string(),
                    date: it.date.to_string(),
                    label: format!("{} · {}", t.get(def.label.as_str()), slot_label),
                    slot: slot_name,
                });
            }
        }
    }
    out.sort_by(|a, b| a.date.cmp(&b.date).then_with(|| a.label.cmp(&b.label)));
    out
}

/// Active persons by how long since their last slot in another program of
/// `program_type` before `from`: never first, then the longest, then by name.
pub(crate) fn people_load(
    v: &View,
    program_type: &Slug,
    this: &core_model::Id,
    from: Date,
) -> Vec<PersonLoad> {
    let mut last: BTreeMap<PersonId, Date> = BTreeMap::new();
    for (pid, p) in &v.proj.programs {
        if pid == this || &p.program_type != program_type {
            continue;
        }
        for (k, (a, _)) in &p.assignments {
            let (Assignee::Person(person), Some(d)) = (a, p.date_of(k.occurrence)) else {
                continue;
            };
            if d < from {
                let e = last.entry(*person).or_insert(d);
                *e = (*e).max(d);
            }
        }
    }
    let mut out: Vec<PersonLoad> = v
        .proj
        .auth
        .org
        .persons
        .iter()
        .filter(|(p, r)| {
            r.active && !v.proj.erasure.persons.contains(p) && !r.display_name.is_empty()
        })
        .map(|(p, r)| PersonLoad {
            id: p.to_string(),
            name: r.display_name.clone(),
            weeks: last
                .get(p)
                .map(|d| u32::try_from((from - *d).whole_weeks()).unwrap_or(0)),
            last: last.get(p).map(ToString::to_string),
        })
        .collect();
    out.sort_by(|a, b| {
        b.weeks
            .unwrap_or(u32::MAX)
            .cmp(&a.weeks.unwrap_or(u32::MAX))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// The row (slot) and column (option) that show a slot in the editor: its
/// own row, or the row of the slot it sits `beside` (in its option's column),
/// `with`, or is paired to (`pair_with` of another slot).
pub(crate) fn placement<'a>(
    def: &'a core_model::config::ProgramTypeDef,
    slot: &'a core_model::config::SlotDef,
) -> (&'a Slug, Option<&'a Slug>) {
    let mut current = slot;
    let mut column = None;
    // Bounded walk: a broken chain cannot loop.
    for _ in 0..def.slots.len() {
        let host = if let Some(b) = &current.beside {
            column = column.or(current.option.as_ref());
            Some(b)
        } else if let Some(w) = &current.with {
            Some(w)
        } else {
            def.slots
                .iter()
                .find(|s| s.pair_with.as_ref() == Some(&current.id))
                .map(|s| &s.id)
        };
        match host.and_then(|h| def.slots.iter().find(|s| &s.id == h)) {
            Some(next) => current = next,
            None => break,
        }
    }
    (&current.id, column)
}

/// Whether the assignees of this program type hear of it only when it is published.
pub(crate) fn notified_at_publish(cfg: &Cfg, program_type: &Slug) -> bool {
    cfg.program_type(program_type)
        .is_some_and(|d| d.notify == NotifyOn::Publish)
}

/// `draft`, `assigned` (sent to the assignees), `to_publish` (handed to who
/// publishes, waiting) or `published`.
pub(crate) fn program_status(
    v: &View,
    cfg: &Cfg,
    id: &core_model::Id,
    program_type: &Slug,
) -> &'static str {
    let sent = v.proj.assigned_snapshots.get(id).map(|s| s.version);
    let published = v.proj.published.get(id).map(|s| s.version);
    let at_publish = notified_at_publish(cfg, program_type);
    match (sent, published) {
        (Some(s), Some(p)) if at_publish && s > p => "to_publish",
        (_, Some(_)) => "published",
        (Some(_), None) if at_publish => "to_publish",
        (Some(_), None) => "assigned",
        (None, None) => "draft",
    }
}

/// The draft holds changes not yet sent (for types notified at publication,
/// "Salva" sends them to who publishes).
pub(crate) fn unsent(v: &View, id: &core_model::Id) -> bool {
    let Some(p) = v.proj.programs.get(id) else {
        return false;
    };
    match v.proj.assigned_snapshots.get(id) {
        Some(s) => core_engine::rules::program_snapshot(&v.proj.auth, *id, p, s.version) != *s,
        None => !p.assignments.is_empty(),
    }
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
                // The administrator sees every screen (it may do everything).
                m.visible_to.as_ref().is_none_or(|a| {
                    v.proj.auth.is_root(me) || allowed_in(&v, me, a, today(self))
                })
                    && holds_any(&v, me, &view_permissions(m.view, &m.params), self.now())
            })
            .map(|m| MenuItem {
                label: t.get(m.label.as_str()),
                view: view_name(m.view),
                params: params_map(&m.params),
            })
            .collect();
        // Core view: everyone's own data, when the package has a profile.
        let pr = &cfg.package().ui.profile;
        if !pr.fields.is_empty() || pr.given_name.is_some() || !pr.dated_qualifications.is_empty() {
            items.push(MenuItem {
                label: t.get("core.menu.profile"),
                view: "profile".into(),
                params: Default::default(),
            });
        }
        // Core view: the configuration editor, for who may publish (§12.4).
        let may_configure = holds_any(
            &v,
            me,
            &view_permissions(ViewKind::ConfigEditor, &Default::default()),
            self.now(),
        );
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
                    items: self.board_items(&v, &t, None)?,
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
                "program_list" => {
                    let Ok(list) = self.programs(param("types")) else {
                        continue;
                    };
                    let v = self.view()?;
                    let c = cfg(&v)?;
                    let me = self.me()?;
                    for r in list.programs.iter().filter(|r| r.status == "to_publish") {
                        // A publish group: one item, opening the group.
                        if let Some(g) = &r.group {
                            let may = self
                                .program_group(g.clone(), r.period.clone())
                                .is_ok_and(|x| x.can_publish);
                            if may && seen.insert((g.clone(), r.period.clone(), "group".to_owned())) {
                                out.push(WorkItem {
                                    title: format!("{} · {}", r.title, r.subtitle),
                                    subtitle: t.get("core.work.program_publish"),
                                    view: "program_group".into(),
                                    params: [
                                        ("group".to_owned(), g.clone()),
                                        ("period".to_owned(), r.period.clone()),
                                    ]
                                    .into(),
                                    action: t.get("core.work.open"),
                                    urgent: true,
                                });
                            }
                            continue;
                        }
                        let may = Slug::new(&r.program_type).is_ok_and(|pt| {
                            v.proj.auth.is_allowed(
                                c,
                                me,
                                Permission::ProgramPublish,
                                Some(&ObjectRef {
                                    kind: ObjectKind::Program,
                                    id: pt,
                                }),
                                None,
                                today(self),
                            )
                        });
                        if may && seen.insert((r.program_id.clone(), String::new(), String::new()))
                        {
                            out.push(WorkItem {
                                title: format!("{} · {}", r.title, r.subtitle),
                                subtitle: t.get("core.work.program_publish"),
                                view: "program_editor".into(),
                                params: [("program".to_owned(), r.program_id.clone())].into(),
                                action: t.get("core.work.open"),
                                urgent: true,
                            });
                        }
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
            let rotation_of = |slot: &Slug| {
                def.map(|d| crate::schedule::slot_rotation(d, slot))
                    .unwrap_or_default()
            };
            let answer =
                |key: &core_engine::ops::SlotKey| v.proj.responses.get(&(*program, key.clone()));
            // Items of a rotation are shown once per group (week or month).
            let mut groups: Vec<(Option<PeriodKey>, &core_engine::ops::NoticeItem)> = Vec::new();
            let mut members: BTreeMap<usize, Vec<&core_engine::ops::NoticeItem>> = BTreeMap::new();
            for it in items.iter().filter(|it| it.date >= now) {
                let g = crate::schedule::rotation_key(rotation_of(&it.key.slot), it.date);
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
                // Accepted unless the person said they cannot.
                let response = if all
                    .iter()
                    .any(|x| matches!(answer(&x.key), Some(Response::Declined { .. })))
                {
                    "declined"
                } else {
                    "accepted"
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
                    can_respond: def.is_some_and(|d| d.acceptance) && response != "declined",
                    date: all.iter().map(|x| x.date).min().unwrap_or(it.date).to_string(),
                    monthly: matches!(g, Some(PeriodKey::Month { .. })),
                });
            }
        }
        // The nearest first; two places of the same slot on the same day are
        // one activity.
        out.sort_by(|a, b| a.date.cmp(&b.date).then(a.title.cmp(&b.title)));
        out.dedup_by(|b, a| a.program_id == b.program_id && a.slot == b.slot && a.date == b.date);
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
                title: resource_title(t, def, &r.attributes),
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

    /// The board today, or (`month`) the programs of a month, old ones
    /// included, with today's notices when it is the current month.
    fn board_items(
        &self,
        v: &View,
        t: &Texts,
        month: Option<&PeriodKey>,
    ) -> ApiResult<Vec<BoardItem>> {
        let cfg = cfg(v)?;
        let now = today(self);
        let mut out = Vec::new();
        // Programs of each publish group's post, for its table.
        let mut snaps: BTreeMap<String, Vec<&core_engine::ops::ProgramSnapshot>> = BTreeMap::new();
        for (id, post) in &v.proj.board {
            let on_now = !(now < post.from || now > post.until);
            let shown = match (month, &post.item) {
                _ if post.withdrawn => false,
                (None, _) => on_now,
                // A monthly program in the month it belongs to (whole weeks
                // may spill over), a weekly one wherever it has an event.
                (Some(m), PBoardItem::Program(s)) => match cfg.program_type(&s.program_type) {
                    Some(d) if d.period == core_model::config::ProgramPeriod::Month => s
                        .occurrences
                        .first()
                        .is_some_and(|f| crate::schedule::period_of(d, *f) == *m),
                    _ => s.occurrences.iter().any(|d| m.contains(*d)),
                },
                (Some(m), PBoardItem::Notice { .. }) => on_now && *m == PeriodKey::month_of(now),
            };
            if !shown {
                continue;
            }
            match &post.item {
                PBoardItem::Notice { title, body } => out.push(BoardItem {
                    id: id.to_string(),
                    icon: None,
                    title: title.clone(),
                    subtitle: String::new(),
                    body: vec![BoardLine {
                        label: String::new(),
                        value: body.clone(),
                    }],
                    programs: Vec::new(),
                    grid: None,
                }),
                PBoardItem::Program(s) => {
                    let def = cfg.program_type(&s.program_type);
                    let first = s.occurrences.first().copied().unwrap_or(now);
                    let rotation_of = |slot: &Slug| {
                        def.map(|d| crate::schedule::slot_rotation(d, slot))
                            .unwrap_or_default()
                    };
                    let mut seen = BTreeSet::new();
                    let lines = s
                        .assignments
                        .iter()
                        // Parts with details only (a song) have nobody to list.
                        .filter(|a| !a.display_name.is_empty())
                        .filter(|a| {
                            // One line per person and group of a rotation.
                            let group =
                                crate::schedule::rotation_key(rotation_of(&a.key.slot), a.date);
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
                            let when = match rotation_of(&a.key.slot) {
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
                    let period = def.map_or_else(
                        || PeriodKey::week_of(first),
                        |d| crate::schedule::period_of(d, first),
                    );
                    // A program of a publish group joins the others of its
                    // period in one post.
                    let group = cfg
                        .package()
                        .programs
                        .publish_groups
                        .iter()
                        .find(|g| g.program_types.contains(&s.program_type));
                    if let Some(g) = group {
                        let gid = format!("{}-{}", g.id, period);
                        snaps.entry(gid.clone()).or_default().push(s);
                        if let Some(item) = out.iter_mut().find(|x| x.id == gid) {
                            item.body.extend(lines);
                            item.programs.push(id.to_string());
                            continue;
                        }
                        out.push(BoardItem {
                            id: gid,
                            icon: g.icon.as_ref().map(ToString::to_string),
                            title: t.get(g.label.as_str()),
                            subtitle: t.period(&period),
                            body: lines,
                            programs: vec![id.to_string()],
                            grid: None,
                        });
                        continue;
                    }
                    out.push(BoardItem {
                        id: id.to_string(),
                        icon: def.and_then(|d| d.icon.as_ref()).map(ToString::to_string),
                        title: def.map_or_else(String::new, |d| t.get(d.label.as_str())),
                        subtitle: t.period(&period),
                        body: lines,
                        programs: vec![id.to_string()],
                        grid: None,
                    });
                }
            }
        }
        for item in &mut out {
            if let Some(list) = snaps.get(&item.id) {
                item.grid = Some(board_grid(t, cfg, list));
            }
        }
        // The parts of a group in the group's order.
        for item in &mut out {
            if item.programs.len() > 1 {
                let rank = |pid: &String| {
                    v.proj
                        .published
                        .iter()
                        .find(|(id, _)| id.to_string() == *pid)
                        .and_then(|(_, s)| {
                            cfg.package()
                                .programs
                                .publish_groups
                                .iter()
                                .find_map(|g| g.program_types.iter().position(|x| *x == s.program_type))
                        })
                        .unwrap_or(usize::MAX)
                };
                item.programs.sort_by_key(rank);
            }
        }
        Ok(out)
    }

    /// `month` (YYYY-MM): the programs of that month, old ones included.
    pub fn board(&self, month: Option<String>) -> ApiResult<Vec<BoardItem>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let month: Option<PeriodKey> = month
            .map(|m| m.parse())
            .transpose()
            .map_err(ApiError::invalid)?;
        let mut items = self.board_items(&v, &self.texts(), month.as_ref())?;
        if month.is_some() {
            items.sort_by(|a, b| a.title.cmp(&b.title));
        }
        Ok(items)
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
            core_sync::delivery::Delivery::Stored => "core.delivery.stored",
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
            values: cfg(v)
                .ok()
                .and_then(|c| c.form(&d.content.form))
                .and_then(|f| f.variants.iter().find(|x| x.id == d.content.variant))
                .map(|var| {
                    var.fields
                        .iter()
                        .filter_map(|f| {
                            let val = d.content.values.get(&f.id)?;
                            let value = match (f.ty, val) {
                                (FieldType::Duration, Scalar::Int(m)) => {
                                    if m % 60 == 0 {
                                        format!("{} h", m / 60)
                                    } else {
                                        format!("{} h {} min", m / 60, m % 60)
                                    }
                                }
                                (_, Scalar::Str(s)) if f.options.is_some() => {
                                    t.get(&core_model::config::default_label::option(s))
                                }
                                (_, other) => display_value(other),
                            };
                            Some(BoardLine {
                                label: field_dto(t, f, None).label,
                                value,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            editable: cfg(v)
                .ok()
                .and_then(|c| c.form(&d.content.form).and_then(|f| c.form_workflow(f)))
                .is_some_and(|w| w.initial == d.state),
        }
    }

    /// The period of `form` this user fills next: the one due if not handed
    /// in yet, else the current one (none for forms without periods).
    pub fn next_period(&self, form: String) -> ApiResult<Option<String>> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let c = cfg(&v)?;
        let f = c
            .form(&Slug::new(&form).map_err(ApiError::invalid)?)
            .ok_or_else(|| ApiError::not_found("form"))?;
        if f.periodicity == Periodicity::None {
            return Ok(None);
        }
        let initial = c.form_workflow(f).map(|w| w.initial.clone());
        let handed_in = |p: &PeriodKey| {
            v.proj.documents.values().any(|d| {
                d.content.subject == me
                    && d.content.form == f.id
                    && &d.content.period == p
                    && Some(&d.state) != initial.as_ref()
            })
        };
        let now = today(self);
        let due = last_period(f.periodicity, now);
        let current = match f.periodicity {
            Periodicity::Weekly => PeriodKey::week_of(now),
            _ => PeriodKey::month_of(now),
        };
        Ok([due, current]
            .into_iter()
            .find(|p| !handed_in(p))
            .map(|p| p.to_string()))
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
            label: t.get(fdef.label.as_str()),
            period_label: t.period(&period),
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

    /// The units of `unit_type` whose documents of `workflow` this user may
    /// read: members, collections in progress, totals of the period being
    /// collected and the documents month by month.
    pub fn unit_reports(&self, workflow: String, unit_type: String) -> ApiResult<UnitReports> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let now = today(self);
        let wf_id = Slug::new(&workflow).map_err(ApiError::invalid)?;
        let ut = Slug::new(&unit_type).map_err(ApiError::invalid)?;
        let Some(form) = cfg.forms().iter().find(|f| {
            f.workflow == wf_id && f.batching.as_ref().is_some_and(|b| b.unit_type == ut)
        }) else {
            return Err(ApiError::not_found("form"));
        };
        let wf = cfg
            .workflow(&wf_id)
            .ok_or_else(|| ApiError::not_found("workflow"))?;
        let object = ObjectRef {
            kind: ObjectKind::Form,
            id: form.id.clone(),
        };
        let queue = self.work_queue(workflow.clone(), Some(unit_type.clone()))?;
        let people = self.people_unchecked()?;
        let current = last_period(form.periodicity, now);
        let mut units = Vec::new();
        for (uid, unit) in v
            .proj
            .auth
            .org
            .units
            .iter()
            .filter(|(_, u)| u.active && u.unit_type == ut)
        {
            if !v.proj.auth.is_allowed(
                cfg,
                me,
                Permission::DocumentRead,
                Some(&object),
                Some(*uid),
                now,
            ) {
                continue;
            }
            let id = uid.to_string();
            let mut members: Vec<PersonView> = people
                .persons
                .iter()
                .filter(|p| p.unit_ids.iter().any(|u| u.unit_id == id))
                .cloned()
                .collect();
            members.sort_by(|a, b| a.name.cmp(&b.name));
            let batches: Vec<BatchView> = queue
                .batches
                .iter()
                .filter(|b| b.unit_id == id)
                .cloned()
                .collect();
            // Documents of this unit handed in (past the initial state).
            let docs: Vec<(&core_model::Id, &Document)> = v
                .proj
                .documents
                .iter()
                .filter(|(_, d)| {
                    d.content.form == form.id && d.content.unit == Some(*uid) && d.state != wf.initial
                })
                .collect();
            let mut periods: BTreeMap<PeriodKey, Vec<(&core_model::Id, &Document)>> =
                BTreeMap::from([(current, Vec::new())]);
            for (did, d) in &docs {
                periods.entry(d.content.period).or_default().push((did, d));
            }
            let months = periods
                .into_iter()
                .rev()
                .take(24)
                .map(|(p, ds)| {
                    let expected = expected_subjects(&v.proj.auth, cfg, form, &p, Some(*uid)).len();
                    let plain: Vec<&Document> = ds.iter().map(|(_, d)| *d).collect();
                    // The aggregate defined on this form, narrowed to the unit;
                    // otherwise plain sums of the fields.
                    let totals = match cfg.aggregates().iter().find(|a| a.source.form == form.id) {
                        Some(def) => {
                            let r = core_engine::rules::compute_aggregate(
                                &v.proj.auth,
                                cfg,
                                def,
                                form,
                                &p,
                                &ds,
                                Some(*uid),
                            );
                            let whole = r.groups.get("").cloned().unwrap_or_default();
                            def.measures.iter().map(|m| measure_row(&t, def, form, m, &whole)).collect()
                        }
                        None => form_totals(&t, form, &plain),
                    };
                    let mut rows: Vec<DocumentRow> = ds
                        .iter()
                        .map(|(did, d)| self.document_row(&v, &t, did, d))
                        .collect();
                    rows.sort_by(|a, b| a.subject_name.cmp(&b.subject_name));
                    MonthReports {
                        period: p.to_string(),
                        period_label: t.period(&p),
                        received: u32::try_from(ds.len()).unwrap_or(u32::MAX),
                        expected: u32::try_from(expected.max(ds.len())).unwrap_or(u32::MAX),
                        totals,
                        rows,
                    }
                })
                .collect();
            units.push(UnitPanel {
                unit_id: id,
                unit_name: unit.name.clone(),
                members,
                batches,
                months,
            });
        }
        units.sort_by(|a, b| a.unit_name.cmp(&b.unit_name));
        Ok(UnitReports {
            title: t.get(form.label.as_str()),
            units,
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
                            .filter(|m| k.is_empty() || !m.op.is_scope_wide())
                            .map(|m| measure_row(&t, def, form, m, ms))
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
        let now = today(self);
        let may = |perm: Permission, id: &Slug| {
            v.proj.auth.is_allowed(
                cfg,
                me,
                perm,
                Some(&ObjectRef {
                    kind: ObjectKind::Program,
                    id: id.clone(),
                }),
                None,
                now,
            )
        };
        let listed: Vec<_> = cfg
            .package()
            .programs
            .program_types
            .iter()
            .filter(|d| wanted.as_ref().is_none_or(|w| w.contains(d.id.as_str())))
            .collect();
        let can_draft: Vec<String> = listed
            .iter()
            .filter(|d| may(Permission::ProgramDraft, &d.id))
            .map(|d| d.id.to_string())
            .collect();
        // The programs of one's competence: the types one drafts or publishes.
        let defs: Vec<_> = listed
            .into_iter()
            .filter(|d| {
                can_draft.iter().any(|c| c == d.id.as_str()) || may(Permission::ProgramPublish, &d.id)
            })
            .collect();
        // Who only publishes a type of a publish group sees the group as one
        // program per period.
        let group_of = |pt: &Slug| {
            if can_draft.iter().any(|c| c == pt.as_str()) {
                return None;
            }
            cfg.package()
                .programs
                .publish_groups
                .iter()
                .find(|g| g.program_types.contains(pt))
        };
        let mut programs: Vec<(i64, ProgramRow)> = Vec::new();
        // (group, period) → newest creation, statuses, types present.
        let mut grouped: BTreeMap<(Slug, PeriodKey), GroupRow> = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let drafts = v.proj.programs.iter().map(|(id, p)| {
            (id, &p.program_type, p.occurrences.first().copied(), p.created)
        });
        // Without its draft the creation time is unknown: listed last.
        let sent = v
            .proj
            .assigned_snapshots
            .iter()
            .chain(v.proj.published.iter())
            .map(|(id, s)| (id, &s.program_type, s.occurrences.first().copied(), i64::MIN));
        for (id, program_type, first, created) in drafts.chain(sent) {
            let Some(def) = defs.iter().find(|d| &d.id == program_type) else {
                continue;
            };
            if !seen.insert(*id) {
                continue;
            }
            let first = first.unwrap_or(now);
            let period = crate::schedule::period_of(def, first);
            let status = program_status(&v, cfg, id, program_type);
            if let Some(g) = group_of(program_type) {
                if status != "draft" {
                    let e = grouped
                        .entry((g.id.clone(), period))
                        .or_insert((i64::MIN, Vec::new(), BTreeSet::new()));
                    e.0 = e.0.max(created);
                    e.1.push(status);
                    e.2.insert(program_type.clone());
                }
                continue;
            }
            programs.push((
                created,
                ProgramRow {
                    program_id: id.to_string(),
                    program_type: program_type.to_string(),
                    icon: def.icon.as_ref().map(ToString::to_string),
                    title: t.get(def.label.as_str()),
                    subtitle: t.period(&period),
                    status: status.into(),
                    group: None,
                    period: period.to_string(),
                    incomplete: false,
                },
            ));
        }
        for ((gid, period), (created, statuses, present)) in grouped {
            let Some(g) = cfg.package().programs.publish_groups.iter().find(|g| g.id == gid) else {
                continue;
            };
            let status = if statuses.contains(&"to_publish") {
                "to_publish"
            } else if statuses.iter().all(|s| *s == "published") {
                "published"
            } else {
                "assigned"
            };
            programs.push((
                created,
                ProgramRow {
                    program_id: String::new(),
                    program_type: String::new(),
                    icon: g.icon.as_ref().map(ToString::to_string),
                    title: t.get(g.label.as_str()),
                    subtitle: t.period(&period),
                    status: status.into(),
                    group: Some(gid.to_string()),
                    period: period.to_string(),
                    incomplete: g.program_types.iter().any(|x| !present.contains(x)),
                },
            ));
        }
        // Newest first.
        programs.sort_by_key(|p| std::cmp::Reverse(p.0));
        let programs = programs.into_iter().map(|(_, row)| row).collect();
        Ok(ProgramList {
            types: defs
                .iter()
                .map(|d| Choice {
                    id: d.id.to_string(),
                    label: t.get(d.label.as_str()),
                })
                .collect(),
            can_draft,
            programs,
        })
    }

    pub fn program(&self, program: String) -> ApiResult<ProgramEditor> {
        self.program_view(program, false, false)
    }

    /// The programs of a publish group handed in for `period` (one per type,
    /// the newest), and the types still missing.
    pub(crate) fn group_parts(
        &self,
        v: &View,
        group: &core_model::config::PublishGroupDef,
        period: PeriodKey,
    ) -> ApiResult<Vec<(core_model::Id, Slug)>> {
        let cfg = cfg(v)?;
        let mut out: BTreeMap<Slug, (core_model::Id, u32)> = BTreeMap::new();
        for (id, s) in v.proj.assigned_snapshots.iter().chain(v.proj.published.iter()) {
            if !group.program_types.contains(&s.program_type) {
                continue;
            }
            let Some(def) = cfg.program_type(&s.program_type) else {
                continue;
            };
            let first = s.occurrences.first().copied().unwrap_or(Date::MIN);
            if crate::schedule::period_of(def, first) != period {
                continue;
            }
            let e = out.entry(s.program_type.clone()).or_insert((*id, s.version));
            if s.version > e.1 && e.0 != *id {
                *e = (*id, s.version);
            }
        }
        Ok(group
            .program_types
            .iter()
            .filter_map(|pt| out.get(pt).map(|(id, _)| (*id, pt.clone())))
            .collect())
    }

    pub fn program_group(&self, group: String, period: String) -> ApiResult<ProgramGroupView> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let gid = Slug::new(&group).map_err(ApiError::invalid)?;
        let g = cfg
            .package()
            .programs
            .publish_groups
            .iter()
            .find(|g| g.id == gid)
            .ok_or_else(|| ApiError::not_found("group"))?;
        let period: PeriodKey = period.parse().map_err(ApiError::invalid)?;
        let now = today(self);
        let parts: Vec<ProgramGroupPart> = self
            .group_parts(&v, g, period)?
            .into_iter()
            .map(|(id, pt)| ProgramGroupPart {
                program_id: id.to_string(),
                program_type: pt.to_string(),
                label: cfg
                    .program_type(&pt)
                    .map_or_else(|| pt.to_string(), |d| t.get(d.label.as_str())),
                status: program_status(&v, cfg, &id, &pt).into(),
            })
            .collect();
        let missing = g
            .program_types
            .iter()
            .filter(|pt| !parts.iter().any(|p| p.program_type == pt.as_str()))
            .map(|pt| {
                cfg.program_type(pt)
                    .map_or_else(|| pt.to_string(), |d| t.get(d.label.as_str()))
            })
            .collect();
        let can_publish = parts.iter().any(|p| {
            p.status == "to_publish"
                && Slug::new(&p.program_type).is_ok_and(|pt| {
                    v.proj.auth.is_allowed(
                        cfg,
                        me,
                        Permission::ProgramPublish,
                        Some(&ObjectRef {
                            kind: ObjectKind::Program,
                            id: pt,
                        }),
                        None,
                        now,
                    )
                })
        });
        let status = if parts.iter().any(|p| p.status == "to_publish") {
            "to_publish"
        } else if !parts.is_empty() && parts.iter().all(|p| p.status == "published") {
            "published"
        } else {
            "assigned"
        };
        Ok(ProgramGroupView {
            group,
            title: t.get(g.label.as_str()),
            period: period.to_string(),
            period_label: t.period(&period),
            status: status.into(),
            parts,
            missing,
            can_publish,
        })
    }

    /// The version on the board, read-only (also for the team, who would
    /// otherwise see their draft).
    pub fn published_program(&self, program: String) -> ApiResult<ProgramEditor> {
        self.program_view(program, true, true)
    }

    /// The version handed in for publication (or the published one), never
    /// the draft: what who publishes is about to put on the board.
    pub fn sent_program(&self, program: String) -> ApiResult<ProgramEditor> {
        self.program_view(program, false, true)
    }

    fn program_view(
        &self,
        program: String,
        published_only: bool,
        skip_draft: bool,
    ) -> ApiResult<ProgramEditor> {
        self.ensure_unlocked()?;
        let v = self.view()?;
        let me = self.me()?;
        let t = self.texts();
        let cfg = cfg(&v)?;
        let id = crate::parse_id(&program)?;
        let now = today(self);
        let draft = v.proj.programs.get(&id).filter(|_| !skip_draft);
        let (program_type, occurrences, assignments, _): ProgramParts<'_> = match draft {
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
                let s = if published_only {
                    v.proj.published.get(&id)
                } else {
                    v.proj
                        .assigned_snapshots
                        .get(&id)
                        .or_else(|| v.proj.published.get(&id))
                }
                .ok_or_else(|| ApiError::not_found("program"))?;
                (
                    s.program_type.clone(),
                    s.occurrences.clone(),
                    s.assignments
                        .iter()
                        // A part with details only has nobody.
                        .filter(|a| !a.display_name.is_empty())
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
        let status = program_status(&v, cfg, &id, &program_type);
        let at_publish = def.notify == NotifyOn::Publish;
        let obj = ObjectRef {
            kind: ObjectKind::Program,
            id: program_type.clone(),
        };
        let can_edit = draft.is_some()
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
        // Details of each slot: the draft's, else those of the version sent.
        let details: BTreeMap<core_engine::ops::SlotKey, core_engine::ops::Values> = match draft {
            Some(p) => {
                let mut d = p.details.clone();
                for (k, (_, extra)) in &p.assignments {
                    d.entry(k.clone()).or_default().extend(extra.clone());
                }
                d
            }
            None => if published_only {
                v.proj.published.get(&id)
            } else {
                v.proj
                    .assigned_snapshots
                    .get(&id)
                    .or_else(|| v.proj.published.get(&id))
            }
            .map(|s| {
                s.assignments
                    .iter()
                    .map(|a| (a.key.clone(), a.extra.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        };
        let shown_of = draft.map(|p| p.shown.clone()).unwrap_or_default();
        let groups = crate::schedule::groups(def.rotation, &occurrences);
        let mut cells = Vec::new();
        // Slots that change at every event get one cell per event.
        let dedicated_mode = |s: &core_model::config::SlotDef| s.mode == core_model::config::SlotMode::Dedicated;
        // A program assigned to units: the active units of that type.
        let unit_choices: Vec<Choice> = def
            .assign_units
            .as_ref()
            .map(|ut| {
                let mut c: Vec<Choice> = v
                    .proj
                    .auth
                    .org
                    .units
                    .iter()
                    .filter(|(_, u)| u.active && &u.unit_type == ut)
                    .map(|(id, u)| Choice {
                        id: format!("unit:{}", id.0),
                        label: u.name.clone(),
                    })
                    .collect();
                c.sort_by(|a, b| a.label.cmp(&b.label));
                c
            })
            .unwrap_or_default();
        for slot in def.slots.iter().filter(|s| def.slot_active(s) && !dedicated_mode(s)) {
            let rotation = crate::schedule::slot_rotation(def, &slot.id);
            let per_event = rotation != def.rotation;
            for group in &crate::schedule::groups(rotation, &occurrences) {
                let occ = group.first().copied().unwrap_or_default();
                let Some(date) = occurrences.get(occ) else {
                    continue;
                };
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
                    let candidates = if can_edit && slot.assignable && def.assign_units.is_some() {
                        unit_choices.clone()
                    } else if can_edit && slot.assignable {
                        let mut c: Vec<Choice> = v
                            .proj
                            .auth
                            .org
                            .persons
                            .iter()
                            .filter(|(p, r)| {
                                // Same rule as the engine: requirements on the
                                // event day, or today if the event is past.
                                r.active
                                    && !v.proj.erasure.persons.contains(p)
                                    && v.proj.auth.satisfies(
                                        cfg,
                                        **p,
                                        slot.requires.as_ref(),
                                        (*date).max(now),
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
                    // Accepted unless someone said they cannot.
                    let response = if answers
                        .iter()
                        .any(|r| matches!(r, Some(Response::Declined { .. })))
                    {
                        "declined"
                    } else {
                        "accepted"
                    };
                    let label = if slot.count * reps > 1 && slot.numbered {
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
                            Assignee::Unit(u) => format!("unit:{}", u.0),
                        }),
                        assignee_name: current.map(|(_, n)| n.clone()),
                        response: response.into(),
                        candidates,
                        per_event,
                        details: slot
                            .extra_fields
                            .iter()
                            .map(|f| field_dto(&t, f, details.get(&key).and_then(|d| d.get(&f.id))))
                            .collect(),
                        row: placement(def, slot).0.to_string(),
                        column: placement(def, slot).1.map(ToString::to_string),
                        section: slot.section.as_ref().map(ToString::to_string),
                        assignable: slot.assignable,
                        shown: shown_of
                            .get(&key)
                            .copied()
                            .unwrap_or_else(|| slot.shown_by_default(index)),
                        row_name: slot.row_name,
                        prefix: slot.prefix.as_ref().map(|k| t.get(k.as_str())),
                    });
                }
            }
        }
        // Nothing new to publish: the board already shows the version sent.
        let can_publish = v.proj.assigned_snapshots.contains_key(&id)
            && (!at_publish || status == "to_publish")
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
        // Slots on dedicated days: rows of date, slot and person, added by hand.
        let dedicated_slots_def: Vec<&core_model::config::SlotDef> =
            def.slots.iter().filter(|s| def.slot_active(s) && dedicated_mode(s)).collect();
        let is_dedicated = |slot: &Slug| dedicated_slots_def.iter().any(|s| &s.id == slot);
        let rows: Vec<(core_engine::ops::SlotKey, Date, String)> = match draft {
            Some(p) => p
                .assignments
                .iter()
                .filter(|(k, _)| is_dedicated(&k.slot))
                .filter_map(|(k, (a, _))| {
                    Some((k.clone(), p.date_of(k.occurrence)?, core_engine::rules::display_name(&v.proj.auth, a)))
                })
                .collect(),
            None => {
                let s = if published_only {
                    v.proj.published.get(&id)
                } else {
                    v.proj.assigned_snapshots.get(&id).or_else(|| v.proj.published.get(&id))
                };
                s.map(|s| {
                    s.assignments
                        .iter()
                        .filter(|a| is_dedicated(&a.key.slot) && !a.display_name.is_empty())
                        .map(|a| (a.key.clone(), a.date, a.display_name.clone()))
                        .collect()
                })
                .unwrap_or_default()
            }
        };
        let slot_rank = |slot: &Slug| def.slots.iter().position(|s| &s.id == slot).unwrap_or(usize::MAX);
        let mut rows = rows;
        rows.sort_by_key(|(k, d, _)| (*d, slot_rank(&k.slot), k.index));
        let dedicated: Vec<DedicatedRow> = rows
            .into_iter()
            .map(|(k, d, name)| DedicatedRow {
                occurrence: u32::from(k.occurrence),
                label: dedicated_slots_def
                    .iter()
                    .find(|s| s.id == k.slot)
                    .map_or_else(|| k.slot.to_string(), |s| t.get(s.label.as_str())),
                slot: k.slot.to_string(),
                index: u32::from(k.index),
                date: d.to_string(),
                date_label: capitalize(&t.long_date(d)),
                assignee: name,
            })
            .collect();
        let dedicated_slots: Vec<DedicatedSlot> = if can_edit {
            dedicated_slots_def
                .iter()
                .map(|slot| {
                    let mut candidates: Vec<Choice> = if def.assign_units.is_some() {
                        unit_choices.clone()
                    } else {
                        v.proj
                            .auth
                            .org
                            .persons
                            .iter()
                            .filter(|(p, r)| {
                                r.active
                                    && !v.proj.erasure.persons.contains(p)
                                    && v.proj.auth.satisfies(cfg, **p, slot.requires.as_ref(), now)
                            })
                            .map(|(p, r)| Choice {
                                id: p.to_string(),
                                label: r.display_name.clone(),
                            })
                            .collect()
                    };
                    candidates.sort_by(|a, b| a.label.cmp(&b.label));
                    DedicatedSlot {
                        id: slot.id.to_string(),
                        label: t.get(slot.label.as_str()),
                        candidates,
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        Ok(ProgramEditor {
            dedicated,
            dedicated_slots,
            assign_units: def.assign_units.is_some(),
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
            dates: occurrences.iter().map(ToString::to_string).collect(),
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
                        days: g
                            .iter()
                            .filter_map(|o| {
                                Some(ProgramDay {
                                    occurrence: *o as u32,
                                    label: capitalize(&t.long_date(*occurrences.get(*o)?)),
                                })
                            })
                            .collect(),
                    })
                })
                .collect(),
            sections: def
                .sections
                .iter()
                .map(|x| SectionView {
                    id: x.id.to_string(),
                    label: t.get(x.label.as_str()),
                    color: x.color.clone(),
                })
                .collect(),
            columns: def
                .options
                .iter()
                .filter(|o| {
                    o.enabled
                        && def.slots.iter().any(|s| {
                            def.slot_active(s)
                                && s.beside.is_some()
                                && s.option.as_ref() == Some(&o.id)
                        })
                })
                .map(|o| Choice {
                    id: o.id.to_string(),
                    label: t.get(o.label.as_str()),
                })
                .collect(),
            others: if can_edit {
                other_slots(&v, cfg, &t, &id)
            } else {
                Vec::new()
            },
            people: if can_edit {
                people_load(
                    &v,
                    &program_type,
                    &id,
                    occurrences.first().copied().unwrap_or(now),
                )
            } else {
                Vec::new()
            },
            cells,
            can_edit,
            unsent: can_edit && unsent(&v, &id),
            can_prefill: can_edit && def.prefill.is_some(),
            needs_prefill: can_edit
                && def.prefill.is_some()
                && v.proj
                    .programs
                    .get(&id)
                    .is_some_and(|p| p.details.is_empty()),
            // "Invia": to the assignees, or, notified at publication, to who publishes.
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
                title: resource_title(&t, Some(def), &r.attributes),
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
        let can_manage =
            v.proj
                .auth
                .is_allowed(cfg, me, Permission::ResourceManage, Some(&obj), None, now);
        Ok(ResourceList {
            resource_type,
            title: t.get(def.label.as_str()),
            can_manage,
            can_scan: can_manage && def.scan.is_some(),
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
            .filter(|f| !matches!(f.ty, FieldType::Photos | FieldType::Geo))
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
        let can_manage =
            v.proj
                .auth
                .is_allowed(cfg, me, Permission::ResourceManage, Some(&obj), None, now);
        let mut pictures = Vec::new();
        let mut places = Vec::new();
        for f in &def.attributes {
            let Some(value) = r.attributes.get(&f.id) else {
                continue;
            };
            let label = field_dto(&t, f, None).label;
            match (f.ty, value) {
                (FieldType::Photos, Scalar::List(items)) => {
                    pictures.extend(items.iter().filter_map(|x| match x {
                        Scalar::Str(reference) => Some(Picture {
                            label: label.clone(),
                            reference: reference.clone(),
                        }),
                        _ => None,
                    }));
                }
                (FieldType::Geo, v) => {
                    if let Some((lat, lng, link)) = crate::scan::geo_value(v) {
                        places.push(Place {
                            label,
                            lat,
                            lng,
                            link,
                        });
                    }
                }
                _ => {}
            }
        }
        Ok(ResourceDetail {
            fields: if can_manage {
                def.attributes
                    .iter()
                    .map(|f| field_dto(&t, f, r.attributes.get(&f.id)))
                    .collect()
            } else {
                Vec::new()
            },
            can_manage,
            can_scan: can_manage && def.scan.is_some(),
            pictures,
            places,
            resource_id: resource,
            title: resource_title(&t, Some(def), &r.attributes),
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
        self.ensure_view(ViewKind::People, &Default::default())?;
        self.people_unchecked()
    }

    /// The people view without the view check: a person exporting their own
    /// data reads their row from it.
    pub(crate) fn people_unchecked(&self) -> ApiResult<People> {
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
                    held_roles: auth
                        .role_ids
                        .iter()
                        .filter_map(|(gid, (i, _))| auth.org.role_grants.get(*i).map(|g| (gid, g)))
                        .filter(|(_, g)| g.person == *p && g.period.end.is_none_or(|e| e > now))
                        .map(|(gid, g)| HeldRole {
                            role: g.role.to_string(),
                            grant_id: gid.to_string(),
                            unit: match g.scope {
                                core_model::state::GrantScope::Unit(u) => Some(u.to_string()),
                                core_model::state::GrantScope::Organization => None,
                            },
                        })
                        .collect(),
                    unit_ids: auth
                        .membership_ids
                        .iter()
                        .filter_map(|(gid, i)| auth.org.memberships.get(*i).map(|m| (gid, m)))
                        .filter(|(_, m)| m.person == *p && m.period.contains(now))
                        .filter_map(|(gid, m)| {
                            auth.org.units.get(&m.unit).map(|x| UnitRef {
                                unit_id: m.unit.to_string(),
                                unit_type: x.unit_type.to_string(),
                                membership_id: gid.to_string(),
                            })
                        })
                        .collect(),
                    held: auth
                        .qualification_ids
                        .iter()
                        .filter_map(|(gid, i)| {
                            auth.org.qualification_grants.get(*i).map(|g| (gid, g))
                        })
                        .filter(|(_, g)| {
                            g.person == *p
                                && g.period.end.is_none_or(|e| e > now && e > g.period.start)
                        })
                        .map(|(gid, g)| HeldQualification {
                            qualification: g.qualification.to_string(),
                            grant_id: gid.to_string(),
                            from: g.period.start.to_string(),
                        })
                        .collect(),
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
        let may = |p: Permission| auth.is_allowed(cfg, me, p, None, None, now);
        let choices_of = |ids: &[Slug]| -> Vec<Choice> {
            ids.iter()
                .filter_map(|q| cfg.qualification(q))
                .map(|q| Choice {
                    id: q.id.to_string(),
                    label: t.get(q.label.as_str()),
                })
                .collect()
        };
        Ok(People {
            can_manage: may(Permission::PeopleManage),
            can_qualify: may(Permission::QualificationsManage),
            can_grant_roles: may(Permission::RolesGrant),
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
            role_options: cfg
                .roles()
                .iter()
                .map(|r| {
                    let unit_type = match &r.scope {
                        core_model::strings::ScopeDef::UnitType(ty) => Some(ty.clone()),
                        core_model::strings::ScopeDef::Organization => None,
                    };
                    let choose_unit = unit_type.is_some() && r.supervises.is_some();
                    RoleOption {
                        id: r.id.to_string(),
                        label: t.get(r.label.as_str()),
                        units: if choose_unit {
                            auth.org
                                .units
                                .iter()
                                .filter(|(_, u)| {
                                    u.active && Some(&u.unit_type) == unit_type.as_ref()
                                })
                                .map(|(id, u)| Choice {
                                    id: id.to_string(),
                                    label: u.name.clone(),
                                })
                                .collect()
                        } else {
                            Vec::new()
                        },
                        unit_type: unit_type.map(|x| x.to_string()),
                        choose_unit,
                    }
                })
                .collect(),
            dated: choices_of(&cfg.package().ui.people.dated_qualifications),
            appointments: cfg
                .package()
                .ui
                .people
                .qualifications
                .iter()
                .filter_map(|q| cfg.qualification(q))
                .map(|q| QualificationOption {
                    id: q.id.to_string(),
                    label: t.get(q.label.as_str()),
                    excludes: q.excludes.iter().map(ToString::to_string).collect(),
                })
                .collect(),
            with_devices: cfg
                .package()
                .ui
                .people
                .with_devices
                .iter()
                .map(ToString::to_string)
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
                .map(|a| attribute_field(&t, a, None))
                .collect(),
        })
    }

    pub fn units(&self) -> ApiResult<Vec<UnitView>> {
        self.ensure_unlocked()?;
        self.ensure_view(ViewKind::Units, &Default::default())?;
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
        let (org_id, backup_node, can_set_backup_node) = self
            .view()
            .ok()
            .map(|v| {
                let root = self.me().is_ok_and(|me| v.proj.auth.is_root(me));
                (
                    Some(v.proj.org.0.iter().map(|b| format!("{b:02x}")).collect::<String>()),
                    v.proj.auth.backup_node.map(|k| k.to_string()),
                    root,
                )
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
            org_id,
            backup_node,
            can_set_backup_node,
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

/// "<type> <number> · <name>" from the first two text attributes, in the
/// order the type declares them.
pub(crate) fn resource_title(
    t: &Texts,
    def: Option<&core_model::config::ResourceTypeDef>,
    attrs: &core_engine::ops::Values,
) -> String {
    let texts: Vec<String> = def
        .map(|d| d.attributes.as_slice())
        .unwrap_or_default()
        .iter()
        .filter(|f| f.ty == FieldType::Text)
        .filter_map(|f| match attrs.get(&f.id) {
            Some(Scalar::Str(s)) if !s.is_empty() => Some(s.clone()),
            _ => None,
        })
        .take(2)
        .collect();
    let base = def.map(|d| t.get(d.label.as_str())).unwrap_or_default();
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

pub(crate) trait UnitsOf {
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

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod layout_tests {
    use super::*;
    use core_model::config::ProgramTypeDef;

    fn def(aux_on: bool) -> ProgramTypeDef {
        serde_json::from_value(serde_json::json!({
            "id": "weekly", "label": "p.weekly", "event_type": "session", "period": "month",
            "owner_role": "lead", "publish_audience": "all",
            "options": [ { "id": "second_room", "label": "o.second", "enabled": aux_on } ],
            "slots": [
                { "id": "part", "label": "s.part", "repeat": { "count": 2 }, "pair_with": "helper" },
                { "id": "helper", "label": "s.helper", "repeat": { "count": 2 } },
                { "id": "part_2", "label": "s.part_2", "option": "second_room", "beside": "part",
                  "repeat": { "count": 2 }, "pair_with": "helper_2" },
                { "id": "helper_2", "label": "s.helper_2", "option": "second_room", "repeat": { "count": 2 } },
                { "id": "lead", "label": "s.lead" },
                { "id": "reader", "label": "s.reader", "with": "lead" },
                { "id": "number", "label": "s.number", "assignable": false }
            ]
        }))
        .unwrap()
    }

    #[test]
    fn slots_are_placed_in_rows_and_columns() {
        let d = def(true);
        let at = |id: &str| {
            let s = d.slots.iter().find(|s| s.id.as_str() == id).unwrap();
            let (row, col) = placement(&d, s);
            (row.to_string(), col.map(ToString::to_string))
        };
        assert_eq!(at("part"), ("part".into(), None));
        assert_eq!(at("helper"), ("part".into(), None));
        assert_eq!(at("part_2"), ("part".into(), Some("second_room".into())));
        assert_eq!(at("helper_2"), ("part".into(), Some("second_room".into())));
        assert_eq!(at("reader"), ("lead".into(), None));
        assert_eq!(at("number"), ("number".into(), None));
    }

    #[test]
    fn an_option_off_hides_its_slots() {
        let off = def(false);
        let active: Vec<_> = off
            .slots
            .iter()
            .filter(|s| off.slot_active(s))
            .map(|s| s.id.as_str())
            .collect();
        assert!(
            !active.contains(&"part_2") && !active.contains(&"helper_2"),
            "{active:?}"
        );
        let on = def(true);
        assert!(
            on.slots
                .iter()
                .filter(|s| on.slot_active(s))
                .any(|s| s.id.as_str() == "part_2")
        );
    }
}
