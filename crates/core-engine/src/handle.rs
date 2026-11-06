//! Validation and application of non-organizational events.

use std::collections::{BTreeMap, BTreeSet};

use core_crypto::{ChannelDescriptor, ChannelId};
use core_events::{EventHash, PayloadChannel};
use core_model::config::{
    Effect, NoteRequirement, NotifyOn, Repeat, StepKind, Trigger, TriggerFilter,
};
use core_model::eval::{EvalContext, Evaluator};
use core_model::{Permission, PersonId, Slug, TextKey, UnitId};
use time::Date;

use crate::auth::{AuthState, Denied, deny};
use crate::channels::{self, ChannelInfo, program_object};
use crate::config::Cfg;
use crate::ops::*;
use crate::projection::*;
use crate::rules::*;
use crate::time::{add_days, date_of};
use crate::values::check_values;

fn ensure(c: bool, msg: &str) -> Result<(), Denied> {
    if c { Ok(()) } else { deny(msg) }
}

/// What a trigger is about, matched against rule filters.
#[derive(Debug, Default, Clone)]
pub(crate) struct About {
    pub form: Option<Slug>,
    pub to_state: Option<Slug>,
    pub aggregate: Option<Slug>,
    pub program_type: Option<Slug>,
    pub resource_type: Option<Slug>,
    pub qualification: Option<Slug>,
}

fn filter_matches(f: &TriggerFilter, a: &About) -> bool {
    let m = |want: &Option<Slug>, have: &Option<Slug>| want.is_none() || want == have;
    m(&f.form, &a.form)
        && m(&f.to_state, &a.to_state)
        && m(&f.aggregate, &a.aggregate)
        && m(&f.program_type, &a.program_type)
        && m(&f.resource_type, &a.resource_type)
        && m(&f.qualification, &a.qualification)
}

struct H<'a> {
    auth: &'a AuthState,
    cfg: Cfg,
    author: PersonId,
    date: Date,
    /// The event's declared time (ms): when a program was created.
    time: i64,
    id: EventHash,
    channel: Option<ChannelId>,
}

impl H<'_> {
    fn expect_channel(&self, proj: &Projection, desc: &ChannelDescriptor) -> Result<(), Denied> {
        let want = ChannelId::derive(&proj.org.0, desc).map_err(|e| Denied(e.to_string()))?;
        ensure(
            self.channel == Some(want),
            "encrypted for the wrong channel",
        )
    }

    fn need(
        &self,
        p: Permission,
        obj: Option<&core_model::strings::ObjectRef>,
        unit: Option<UnitId>,
    ) -> Result<(), Denied> {
        self.auth
            .require(&self.cfg, self.author, p, obj, unit, self.date)
    }

    fn allowed(
        &self,
        p: Permission,
        obj: Option<&core_model::strings::ObjectRef>,
        unit: Option<UnitId>,
    ) -> bool {
        self.auth
            .is_allowed(&self.cfg, self.author, p, obj, unit, self.date)
    }

    fn fire(
        &self,
        proj: &mut Projection,
        trigger: Trigger,
        about: &About,
        ctx: &EvalContext,
        params: BTreeMap<String, String>,
    ) {
        let ev = Evaluator::new(self.cfg.index(), &self.auth.org);
        for rule in &self.cfg.package().notifications.notifications {
            if rule.trigger != trigger || !filter_matches(&rule.filter, about) {
                continue;
            }
            if let Ok(recipients) = ev.resolve(&rule.audience, ctx) {
                proj.triggers.push(TriggerRecord {
                    trigger: Some(trigger),
                    text: rule.text.clone(),
                    event: self.id,
                    date: self.date,
                    recipients,
                    params: params.clone(),
                });
            }
        }
    }

    fn notify(
        &self,
        proj: &mut Projection,
        audience: &Slug,
        text: &TextKey,
        ctx: &EvalContext,
        params: BTreeMap<String, String>,
    ) {
        if let Ok(recipients) =
            Evaluator::new(self.cfg.index(), &self.auth.org).resolve(audience, ctx)
        {
            proj.triggers.push(TriggerRecord {
                trigger: None,
                text: text.clone(),
                event: self.id,
                date: self.date,
                recipients,
                params,
            });
        }
    }
}

pub(crate) fn handle(
    proj: &mut Projection,
    auth: &AuthState,
    ctx: &Ctx,
    i: usize,
    op: &Op,
) -> Result<(), Denied> {
    let e = &ctx.graph.events[i];
    let cfg = proj
        .configs
        .get(auth.config_version.max(1))
        .cloned()
        .ok_or_else(|| Denied("unknown configuration version".into()))?;
    let author = auth.author(&e.body.author)?;
    let h = H {
        auth,
        cfg,
        author,
        date: date_of(e.body.declared_time),
        time: e.body.declared_time,
        id: e.id,
        channel: match e.body.channel {
            PayloadChannel::Public => None,
            PayloadChannel::Channel(c) => Some(c.id),
        },
    };
    match op {
        Op::DeviceInvited(_) => h.auth.check(&h.cfg, author, h.date, op),
        Op::KeysDistributed(d) | Op::ChannelRotated(d) => {
            let known = proj.channels.get(&d.channel.id);
            channels::check_distribution(
                proj.org,
                auth,
                &h.cfg,
                author,
                h.date,
                e.body.kind,
                d,
                known,
            )?;
            let info = proj
                .channels
                .entry(d.channel.id)
                .or_insert_with(|| ChannelInfo {
                    descriptor: d.descriptor.clone(),
                    epochs: BTreeMap::new(),
                });
            info.epochs
                .entry(d.channel.epoch)
                .or_default()
                .extend(d.entries.iter().map(|x| x.device));
            Ok(())
        }
        Op::QuarantineResolved(_) => ensure(auth.is_root(author), "root administrators only"),
        Op::ErasureRequested(e) => {
            ensure(auth.is_root(author), "root administrators only")?;
            proj.erasure.persons.insert(e.person);
            Ok(())
        }
        Op::RetentionApplied(r) => {
            ensure(auth.is_root(author), "root administrators only")?;
            proj.erasure.retention = proj.erasure.retention.max(Some(r.before));
            Ok(())
        }
        Op::DocumentCreated(c) => document_created(proj, &h, c),
        Op::DocumentUpdated(u) => document_updated(proj, &h, u),
        Op::DocumentStep(s) => document_step(proj, &h, s),
        Op::DocumentDistributed(d) => match d.as_ref() {
            Distributed::Document(s) => document_distributed(proj, &h, s),
            Distributed::Aggregate(s) => aggregate_distributed(proj, &h, s),
        },
        Op::BatchForwarded(b) => batch_step(proj, &h, b, StepKind::Forward),
        Op::BatchRegistered(b) => batch_step(proj, &h, b, StepKind::Register),
        Op::AggregateGenerated(s) => aggregate_generated(proj, &h, s),
        Op::CalendarEventUpserted(c) => {
            h.need(Permission::CalendarManage, None, None)?;
            h.expect_channel(proj, &ChannelDescriptor::Organization)?;
            ensure(
                h.cfg
                    .package()
                    .calendar
                    .event_types
                    .iter()
                    .any(|t| t.id == c.event_type),
                "unknown event type",
            )?;
            proj.calendar.insert(c.event, c.clone());
            Ok(())
        }
        Op::CalendarEventCancelled(c) => {
            h.need(Permission::CalendarManage, None, None)?;
            h.expect_channel(proj, &ChannelDescriptor::Organization)?;
            ensure(
                proj.calendar.remove(&c.event).is_some(),
                "unknown calendar event",
            )
        }
        Op::CalendarSeriesUpserted(c) => {
            series_allowed(proj, &h, &c.event_type)?;
            ensure((1..=7).contains(&c.weekday), "weekday is 1 (Monday) to 7 (Sunday)")?;
            for u in &c.units {
                ensure(h.auth.org.units.get(u).is_some_and(|x| x.active), "unknown unit")?;
            }
            proj.series.insert(c.series, c.clone());
            Ok(())
        }
        Op::CalendarSeriesRemoved(c) => {
            let ty = proj
                .series
                .get(&c.series)
                .map(|s| s.event_type.clone())
                .ok_or_else(|| Denied("unknown series".into()))?;
            series_allowed(proj, &h, &ty)?;
            proj.series.remove(&c.series);
            Ok(())
        }
        Op::ProgramDrafted(d) => program_drafted(proj, &h, d),
        Op::ProgramUpdated(u) => program_updated(proj, &h, u),
        Op::ProgramDetailsSet(d) => program_details_set(proj, &h, d),
        Op::ProgramPartsShown(d) => program_parts_shown(proj, &h, d),
        Op::ProgramDaysAdded(d) => program_days_added(proj, &h, d),
        Op::ProgramAssignmentsSent(s) => assignments_sent(proj, &h, s),
        Op::AssignmentResponded(r) => assignment_responded(proj, &h, r),
        Op::ProgramPublished(s) => program_published(proj, &h, s, false),
        Op::ProgramRevised(s) => program_published(proj, &h, s, true),
        Op::ResourceUpserted(r) => {
            let def = h
                .cfg
                .resource_type(&r.resource_type)
                .ok_or_else(|| Denied("unknown resource type".into()))?;
            h.need(
                Permission::ResourceManage,
                Some(&resource_object(&r.resource_type)),
                None,
            )?;
            h.expect_channel(proj, &resource_descriptor(&r.resource_type, None))?;
            check_values(&def.attributes, &r.attributes, false)?;
            let entry = proj
                .resources
                .entry(r.resource)
                .or_insert_with(|| Resource {
                    resource_type: r.resource_type.clone(),
                    attributes: BTreeMap::new(),
                    holding: None,
                    history: Vec::new(),
                    retired: false,
                });
            ensure(
                entry.resource_type == r.resource_type,
                "resource type cannot change",
            )?;
            entry.attributes = r.attributes.clone();
            Ok(())
        }
        Op::ResourceRetired(r) => {
            h.need(
                Permission::ResourceManage,
                Some(&resource_object(&r.resource_type)),
                None,
            )?;
            h.expect_channel(proj, &resource_descriptor(&r.resource_type, None))?;
            let res = proj
                .resources
                .get_mut(&r.resource)
                .ok_or_else(|| Denied("unknown resource".into()))?;
            ensure(
                res.holding.is_none(),
                "return the resource before retiring it",
            )?;
            res.retired = true;
            Ok(())
        }
        Op::ResourceAssigned(a) => resource_assigned(proj, &h, a),
        Op::ResourceLogged(l) => {
            let def = h
                .cfg
                .resource_type(&l.resource_type)
                .ok_or_else(|| Denied("unknown resource type".into()))?;
            h.expect_channel(proj, &resource_descriptor(&l.resource_type, Some(l.holder)))?;
            ensure(
                h.author == l.holder
                    || h.allowed(
                        Permission::ResourceLog,
                        Some(&resource_object(&l.resource_type)),
                        None,
                    ),
                "only the holder can log",
            )?;
            check_values(&def.log_fields, &l.values, false)?;
            let res = holder_resource(proj, l.resource, l.holder)?;
            if let Some(hd) = res.holding.as_mut() {
                hd.logs.push((l.date, l.values.clone()));
            }
            Ok(())
        }
        Op::ResourceRequestOpened(r) => {
            let def = h
                .cfg
                .resource_type(&r.resource_type)
                .ok_or_else(|| Denied("unknown resource type".into()))?;
            h.need(
                Permission::ResourceRequest,
                Some(&resource_object(&r.resource_type)),
                None,
            )?;
            h.expect_channel(proj, &resource_descriptor(&r.resource_type, Some(r.holder)))?;
            ensure(
                def.requests.iter().any(|q| q.id == r.request_type),
                "unknown request",
            )?;
            holder_resource(proj, r.resource, r.holder)?;
            ensure(
                !proj.requests.contains_key(&r.request),
                "request id already used",
            )?;
            proj.requests.insert(
                r.request,
                Request {
                    resource: r.resource,
                    resource_type: r.resource_type.clone(),
                    holder: r.holder,
                    request_type: r.request_type.clone(),
                    due: r.due,
                    answer: None,
                },
            );
            let mut ctx = EvalContext::at(h.date);
            ctx.holder = Some(r.holder);
            let about = About {
                resource_type: Some(r.resource_type.clone()),
                ..About::default()
            };
            h.fire(
                proj,
                Trigger::ResourceRequestOpened,
                &about,
                &ctx,
                BTreeMap::new(),
            );
            Ok(())
        }
        Op::ResourceRequestAnswered(a) => {
            let def = h
                .cfg
                .resource_type(&a.resource_type)
                .ok_or_else(|| Denied("unknown resource type".into()))?;
            h.expect_channel(proj, &resource_descriptor(&a.resource_type, Some(a.holder)))?;
            ensure(h.author == a.holder, "only the holder answers")?;
            let req = proj
                .requests
                .get_mut(&a.request)
                .ok_or_else(|| Denied("unknown request".into()))?;
            ensure(
                req.holder == a.holder && req.answer.is_none(),
                "request already answered",
            )?;
            let answers = def
                .requests
                .iter()
                .find(|q| q.id == req.request_type)
                .map(|q| q.answers.clone())
                .unwrap_or_default();
            ensure(answers.iter().any(|x| x.id == a.answer), "unknown answer")?;
            req.answer = Some(a.answer.clone());
            Ok(())
        }
        Op::ResourceReturned(r) => {
            h.expect_channel(proj, &resource_descriptor(&r.resource_type, Some(r.holder)))?;
            ensure(
                h.author == r.holder
                    || h.allowed(
                        Permission::ResourceAssign,
                        Some(&resource_object(&r.resource_type)),
                        None,
                    ),
                "only the holder or the managers can return it",
            )?;
            let res = holder_resource(proj, r.resource, r.holder)?;
            if let Some(mut hd) = res.holding.take() {
                hd.until = Some(r.date);
                res.history.push(hd);
            }
            Ok(())
        }
        Op::BoardPosted(b) => {
            h.need(Permission::BoardPublish, None, None)?;
            h.expect_channel(proj, &published_descriptor(&b.audience))?;
            let free = h
                .cfg
                .package()
                .board
                .board
                .as_ref()
                .map(|x| x.free_notices.clone())
                .unwrap_or_default();
            ensure(
                free.allowed && free.audiences.contains(&b.audience),
                "notices are not allowed for this audience",
            )?;
            ensure(b.until >= b.from, "a notice must end after it starts")?;
            proj.board.insert(
                b.post,
                BoardPost {
                    audience: b.audience.clone(),
                    item: BoardItem::Notice {
                        title: b.title.clone(),
                        body: b.body.clone(),
                    },
                    from: b.from,
                    until: b.until,
                    withdrawn: false,
                },
            );
            Ok(())
        }
        Op::BoardWithdrawn(w) => {
            h.need(Permission::BoardWithdraw, None, None)?;
            h.expect_channel(proj, &published_descriptor(&w.audience))?;
            let post = proj
                .board
                .get_mut(&w.post)
                .ok_or_else(|| Denied("unknown post".into()))?;
            post.withdrawn = true;
            Ok(())
        }
        _ => deny("unexpected operation"),
    }
}

// ----- documents ---------------------------------------------------------------

fn document_created(proj: &mut Projection, h: &H<'_>, c: &DocumentContent) -> Result<(), Denied> {
    let cfg = proj
        .configs
        .get(c.form_version)
        .cloned()
        .ok_or_else(|| Denied("unknown form version".into()))?;
    let form = cfg
        .form(&c.form)
        .ok_or_else(|| Denied("unknown form".into()))?
        .clone();
    let wf = cfg
        .form_workflow(&form)
        .ok_or_else(|| Denied("unknown workflow".into()))?
        .clone();
    ensure(
        period_matches(&form, &c.period),
        "the period does not match the form",
    )?;
    ensure(
        !proj.documents.contains_key(&c.document),
        "document id already used",
    )?;
    ensure(h.auth.person_exists(c.subject), "unknown subject")?;
    let at = reference_date(&c.period, h.date);
    let ev = Evaluator::new(cfg.index(), &h.auth.org);
    ensure(
        ev.eval(&form.subject, Some(c.subject), &EvalContext::at(at)),
        "the subject cannot have this document",
    )?;
    let variant = variant_for(h.auth, &cfg, &form, c.subject, at)
        .ok_or_else(|| Denied("no variant applies".into()))?;
    ensure(
        variant.id == c.variant,
        "wrong variant for the subject and period",
    )?;
    ensure(
        subject_unit(h.auth, &form, c.subject, at) == c.unit,
        "wrong unit for the subject at the end of the period",
    )?;
    check_values(&variant.fields, &c.values, false)?;
    ensure(
        may_fill(h.auth, &cfg, h.author, &form, c.subject, c.unit, h.date),
        "not allowed to fill this document",
    )?;
    if form.periodicity != core_model::config::Periodicity::None {
        ensure(
            proj.document_by(&c.form, c.subject, c.period).is_none(),
            "a document for this period already exists",
        )?;
    }
    ensure(
        c.on_behalf == (h.author != c.subject),
        "wrong on-behalf flag",
    )?;
    h.expect_channel(proj, &document_descriptor(&cfg, &form, c, &wf.initial))?;
    proj.documents.insert(
        c.document,
        Document {
            content: c.clone(),
            state: wf.initial.clone(),
            version: 1,
            created_by: Some(h.author),
            history: Vec::new(),
        },
    );
    Ok(())
}

fn doc_cfg(
    proj: &Projection,
    doc: &Document,
) -> Result<
    (
        Cfg,
        core_model::config::FormDef,
        core_model::config::WorkflowDef,
    ),
    Denied,
> {
    let cfg = proj
        .configs
        .get(doc.content.form_version)
        .cloned()
        .ok_or_else(|| Denied("unknown form version".into()))?;
    let form = cfg
        .form(&doc.content.form)
        .ok_or_else(|| Denied("unknown form".into()))?
        .clone();
    let wf = cfg
        .form_workflow(&form)
        .ok_or_else(|| Denied("unknown workflow".into()))?
        .clone();
    Ok((cfg, form, wf))
}

fn document_updated(proj: &mut Projection, h: &H<'_>, u: &DocumentUpdated) -> Result<(), Denied> {
    let doc = proj
        .documents
        .get(&u.document)
        .ok_or_else(|| Denied("unknown document".into()))?;
    let (cfg, form, wf) = doc_cfg(proj, doc)?;
    ensure(
        doc.state == wf.initial,
        "the document can only be edited before it is submitted",
    )?;
    ensure(
        may_fill(
            h.auth,
            &cfg,
            h.author,
            &form,
            doc.content.subject,
            doc.content.unit,
            h.date,
        ),
        "not allowed to edit this document",
    )?;
    let variant = form
        .variants
        .iter()
        .find(|v| v.id == doc.content.variant)
        .ok_or_else(|| Denied("unknown variant".into()))?;
    check_values(&variant.fields, &u.values, false)?;
    h.expect_channel(
        proj,
        &document_descriptor(&cfg, &form, &doc.content, &doc.state),
    )?;
    if let Some(doc) = proj.documents.get_mut(&u.document) {
        for (k, v) in &u.values {
            doc.content.values.insert(k.clone(), v.clone());
        }
        doc.version += 1;
    }
    Ok(())
}

/// Moves a document to `to`, then through any automatic `collect` step.
fn move_document(
    proj: &mut Projection,
    h: &H<'_>,
    id: core_model::Id,
    step: &Slug,
    to: &Slug,
    note: Option<String>,
) {
    let Some(doc) = proj.documents.get(&id) else {
        return;
    };
    let Ok((cfg, form, wf)) = doc_cfg(proj, doc) else {
        return;
    };
    let subject = doc.content.subject;
    let unit = doc.content.unit;
    let mut records = vec![StepRecord {
        step: step.clone(),
        from: doc.state.clone(),
        to: to.clone(),
        by: Some(h.author),
        date: h.date,
        note,
        event: h.id,
    }];
    let mut state = to.clone();
    if let Some(collect) = wf
        .steps
        .iter()
        .find(|s| s.kind == StepKind::Collect && s.from.contains(&state))
    {
        records.push(StepRecord {
            step: collect.id.clone(),
            from: state.clone(),
            to: collect.to.clone(),
            by: None,
            date: h.date,
            note: None,
            event: h.id,
        });
        state = collect.to.clone();
    }
    if let Some(doc) = proj.documents.get_mut(&id) {
        doc.state = state.clone();
        doc.history.extend(records.iter().cloned());
    }
    let mut ctx = EvalContext::at(h.date).with_subject(subject, unit);
    ctx.on_behalf = form.fillers_on_behalf.clone();
    let mut params = BTreeMap::new();
    params.insert("form".to_owned(), form.id.to_string());
    for r in &records {
        let about = About {
            form: Some(form.id.clone()),
            to_state: Some(r.to.clone()),
            ..About::default()
        };
        h.fire(
            proj,
            Trigger::DocumentStateChanged,
            &about,
            &ctx,
            params.clone(),
        );
    }
    let _ = cfg;
}

fn step_effects(proj: &mut Projection, h: &H<'_>, effects: &[Effect], ctx: &EvalContext) {
    for e in effects {
        if let Effect::Notify(n) = e {
            h.notify(proj, &n.audience, &n.text, ctx, BTreeMap::new());
        }
    }
}

fn document_step(proj: &mut Projection, h: &H<'_>, s: &DocumentStep) -> Result<(), Denied> {
    let doc = proj
        .documents
        .get(&s.document)
        .ok_or_else(|| Denied("unknown document".into()))?
        .clone();
    let (cfg, form, wf) = doc_cfg(proj, &doc)?;
    let step = cfg
        .step(&wf, &s.step)
        .ok_or_else(|| Denied("unknown step".into()))?
        .clone();
    ensure(
        step.kind == StepKind::Transition,
        "this step applies to batches",
    )?;
    ensure(
        step.from.contains(&doc.state),
        "the document is not in a state this step starts from",
    )?;
    let by = step
        .by
        .clone()
        .ok_or_else(|| Denied("step without actors".into()))?;
    let actors = step_actors(
        h.auth,
        &cfg,
        &form,
        &by,
        Some(doc.content.subject),
        doc.content.unit,
        h.date,
    )?;
    ensure(
        actors.contains(&h.author),
        "not allowed to perform this step",
    )?;
    if step.note == NoteRequirement::Required {
        ensure(
            s.note.as_ref().is_some_and(|n| !n.trim().is_empty()),
            "a note is required",
        )?;
    }
    if let Some(when) = &step.when {
        let mut ctx = EvalContext::at(h.date).with_subject(doc.content.subject, doc.content.unit);
        ctx.fields = doc.content.values.clone();
        ensure(
            Evaluator::new(cfg.index(), &h.auth.org).eval(when, Some(h.author), &ctx),
            "the step conditions are not met",
        )?;
    }
    if doc.state == wf.initial {
        let variant = form
            .variants
            .iter()
            .find(|v| v.id == doc.content.variant)
            .ok_or_else(|| Denied("unknown variant".into()))?;
        check_values(&variant.fields, &doc.content.values, true)?;
    }
    h.expect_channel(
        proj,
        &document_descriptor(&cfg, &form, &doc.content, &doc.state),
    )?;
    move_document(proj, h, s.document, &step.id, &step.to, s.note.clone());
    let mut ctx = EvalContext::at(h.date).with_subject(doc.content.subject, doc.content.unit);
    ctx.on_behalf = form.fillers_on_behalf.clone();
    step_effects(proj, h, &step.effects, &ctx);
    Ok(())
}

fn document_distributed(
    proj: &mut Projection,
    h: &H<'_>,
    s: &DocumentSnapshot,
) -> Result<(), Denied> {
    let cfg = proj
        .configs
        .get(s.content.form_version)
        .cloned()
        .ok_or_else(|| Denied("unknown form version".into()))?;
    let form = cfg
        .form(&s.content.form)
        .ok_or_else(|| Denied("unknown form".into()))?
        .clone();
    let desc = document_descriptor(&cfg, &form, &s.content, &s.state);
    h.expect_channel(proj, &desc)?;
    let members = channels::members(h.auth, &cfg, &desc, h.date)?;
    ensure(
        members.contains(&h.author),
        "only readers can distribute a document",
    )?;
    match proj.documents.get(&s.content.document) {
        Some(doc) => {
            ensure(
                doc.content == s.content && doc.state == s.state,
                "the snapshot does not match the document",
            )?;
        }
        None => {
            proj.documents.insert(
                s.content.document,
                Document {
                    content: s.content.clone(),
                    state: s.state.clone(),
                    version: s.version,
                    created_by: None,
                    history: Vec::new(),
                },
            );
        }
    }
    Ok(())
}

fn batch_step(
    proj: &mut Projection,
    h: &H<'_>,
    b: &BatchStep,
    kind: StepKind,
) -> Result<(), Denied> {
    let cfg = h.cfg.clone();
    let form = cfg
        .form(&b.batch.form)
        .ok_or_else(|| Denied("unknown form".into()))?
        .clone();
    let wf = cfg
        .form_workflow(&form)
        .ok_or_else(|| Denied("unknown workflow".into()))?
        .clone();
    let step = cfg
        .step(&wf, &b.step)
        .ok_or_else(|| Denied("unknown step".into()))?
        .clone();
    ensure(step.kind == kind, "wrong kind of step")?;
    ensure(form.batching.is_some(), "this form has no batches")?;
    ensure(!b.documents.is_empty(), "a batch step needs documents")?;
    let unit = h
        .auth
        .org
        .units
        .get(&b.batch.unit)
        .ok_or_else(|| Denied("unknown unit".into()))?;
    ensure(
        form.batching
            .as_ref()
            .is_some_and(|x| x.unit_type == unit.unit_type),
        "wrong unit type",
    )?;
    let by = step
        .by
        .clone()
        .ok_or_else(|| Denied("step without actors".into()))?;
    let actors = step_actors(h.auth, &cfg, &form, &by, None, Some(b.batch.unit), h.date)?;
    ensure(
        actors.contains(&h.author),
        "not allowed to perform this step",
    )?;
    // Known documents must belong to the batch and be in a starting state;
    // unknown ones (not readable here) cannot be checked.
    for d in &b.documents {
        if let Some(doc) = proj.documents.get(d) {
            ensure(
                doc.content.form == b.batch.form
                    && doc.content.unit == Some(b.batch.unit)
                    && doc.content.period == b.batch.period,
                "a document does not belong to the batch",
            )?;
            ensure(
                step.from.contains(&doc.state),
                "a document is not in a state this step starts from",
            )?;
        }
    }
    if kind == StepKind::Forward {
        let min_state = step
            .from
            .first()
            .cloned()
            .unwrap_or_else(|| wf.initial.clone());
        let expected = expected_subjects(h.auth, &cfg, &form, &b.batch.period, Some(b.batch.unit));
        let present = present_subjects(
            &cfg,
            &form,
            proj.documents.values(),
            &b.batch.period,
            Some(b.batch.unit),
            &min_state,
        );
        let missing: BTreeSet<PersonId> = expected.difference(&present).copied().collect();
        if !missing.is_empty() {
            let last = b
                .batch
                .period
                .last_day()
                .ok_or_else(|| Denied("batch without period".into()))?;
            let allowed_from = step
                .allow_incomplete_after
                .map(|o| add_days(last, i64::from(o.offset_days)));
            ensure(
                allowed_from.is_some_and(|d| h.date >= d),
                "documents are missing: the batch can be forwarded incomplete only later",
            )?;
        }
    }
    h.expect_channel(proj, &batch_descriptor(&cfg, &form, b.batch.unit, &step.to))?;
    for d in &b.documents {
        if proj.documents.contains_key(d) {
            move_document(proj, h, *d, &step.id, &step.to, None);
        }
    }
    proj.batches
        .entry(b.batch.clone())
        .or_default()
        .records
        .push(BatchRecord {
            step: step.id.clone(),
            documents: b.documents.clone(),
            by: h.author,
            date: h.date,
            event: h.id,
        });
    let mut ctx = EvalContext::at(h.date);
    ctx.subject_unit = Some(b.batch.unit);
    if kind == StepKind::Forward {
        let about = About {
            form: Some(form.id.clone()),
            ..About::default()
        };
        h.fire(proj, Trigger::BatchForwarded, &about, &ctx, BTreeMap::new());
    }
    step_effects(proj, h, &step.effects, &ctx);
    Ok(())
}

// ----- aggregates ------------------------------------------------------------------

/// Registered documents of the period, for an aggregation.
pub(crate) fn aggregate_sources<'p>(
    proj: &'p Projection,
    def: &core_model::config::AggregateDef,
    period: &crate::time::PeriodKey,
) -> Vec<(&'p core_model::Id, &'p Document)> {
    proj.documents
        .iter()
        .filter(|(_, d)| {
            d.content.form == def.source.form
                && &d.content.period == period
                && d.state == def.source.state
        })
        .collect()
}

fn aggregate_generated(
    proj: &mut Projection,
    h: &H<'_>,
    s: &AggregateSnapshot,
) -> Result<(), Denied> {
    let cfg = h.cfg.clone();
    let def = cfg
        .aggregate(&s.aggregate)
        .ok_or_else(|| Denied("unknown aggregate".into()))?
        .clone();
    let form = cfg
        .form(&def.source.form)
        .ok_or_else(|| Denied("unknown form".into()))?
        .clone();
    h.need(
        Permission::AggregateGenerate,
        Some(&aggregate_object(&def.id)),
        None,
    )?;
    h.expect_channel(proj, &aggregate_draft_descriptor(&def.id))?;
    let key = (s.aggregate.clone(), s.period);
    let prev = proj
        .aggregates
        .get(&key)
        .and_then(|a| a.generated.as_ref())
        .map_or(0, |g| g.version);
    ensure(
        s.version == prev + 1,
        "aggregate versions must follow each other",
    )?;
    let sources = aggregate_sources(proj, &def, &s.period);
    let known: BTreeSet<core_model::Id> = sources.iter().map(|(id, _)| **id).collect();
    let claimed: BTreeSet<core_model::Id> = s.result.documents.iter().copied().collect();
    if known == claimed {
        let result = compute_aggregate(h.auth, &cfg, &def, &form, &s.period, &sources, None);
        ensure(
            result == s.result,
            "the aggregate does not match the registered documents",
        )?;
    }
    proj.aggregates.entry(key).or_default().generated = Some(s.clone());
    Ok(())
}

fn aggregate_distributed(
    proj: &mut Projection,
    h: &H<'_>,
    s: &AggregateSnapshot,
) -> Result<(), Denied> {
    let def = h
        .cfg
        .aggregate(&s.aggregate)
        .ok_or_else(|| Denied("unknown aggregate".into()))?
        .clone();
    h.need(
        Permission::DocumentDistribute,
        Some(&aggregate_object(&def.id)),
        None,
    )?;
    h.expect_channel(proj, &aggregate_descriptor(&def))?;
    let key = (s.aggregate.clone(), s.period);
    if let Some(g) = proj.aggregates.get(&key).and_then(|a| a.generated.as_ref()) {
        if g.version == s.version {
            ensure(
                g == s,
                "the distributed aggregate differs from the generated one",
            )?;
        }
    }
    if let Some(d) = proj
        .aggregates
        .get(&key)
        .and_then(|a| a.distributed.as_ref())
    {
        ensure(
            s.version > d.version,
            "this version was already distributed",
        )?;
    }
    proj.aggregates.entry(key).or_default().distributed = Some(s.clone());
    let about = About {
        aggregate: Some(def.id.clone()),
        ..About::default()
    };
    let mut params = BTreeMap::new();
    params.insert("period".to_owned(), s.period.to_string());
    h.fire(
        proj,
        Trigger::AggregateDistributed,
        &about,
        &EvalContext::at(h.date),
        params,
    );
    Ok(())
}

// ----- programs -------------------------------------------------------------------

fn program_drafted(proj: &mut Projection, h: &H<'_>, d: &ProgramDrafted) -> Result<(), Denied> {
    ensure(
        h.cfg.program_type(&d.program_type).is_some(),
        "unknown program type",
    )?;
    h.need(
        Permission::ProgramDraft,
        Some(&program_object(&d.program_type)),
        None,
    )?;
    h.expect_channel(proj, &program_draft_descriptor(&d.program_type))?;
    ensure(
        !proj.programs.contains_key(&d.program),
        "program id already used",
    )?;
    ensure(
        // Equal dates: several series on the same day.
        !d.occurrences.is_empty() && d.occurrences.windows(2).all(|w| w[0] <= w[1]),
        "occurrences must be dates in order",
    )?;
    proj.programs.insert(
        d.program,
        Program {
            program_type: d.program_type.clone(),
            config_version: d.config_version,
            occurrences: d.occurrences.clone(),
            assignments: BTreeMap::new(),
            status: ProgramStatus::Draft,
            sent_version: 0,
            created: h.time,
            dedicated_days: Vec::new(),
            details: BTreeMap::new(),
            shown: BTreeMap::new(),
        },
    );
    Ok(())
}

/// Weekly series of an event type: who manages the calendar, or who drafts
/// a program held at that event type (e.g. its manager and assistant).
fn series_allowed(proj: &Projection, h: &H<'_>, event_type: &Slug) -> Result<(), Denied> {
    h.expect_channel(proj, &ChannelDescriptor::Organization)?;
    ensure(
        h.cfg.package().calendar.event_types.iter().any(|t| &t.id == event_type),
        "unknown event type",
    )?;
    let drafts = h
        .cfg
        .package()
        .programs
        .program_types
        .iter()
        .filter(|p| p.event_type.iter().any(|e| e == event_type))
        .any(|p| h.allowed(Permission::ProgramDraft, Some(&program_object(&p.id)), None));
    ensure(
        drafts || h.allowed(Permission::CalendarManage, None, None),
        "permission calendar.manage or program.draft required",
    )
}

fn program_days_added(proj: &mut Projection, h: &H<'_>, d: &ProgramDaysAdded) -> Result<(), Denied> {
    let p = proj
        .programs
        .get(&d.program)
        .ok_or_else(|| Denied("unknown program".into()))?;
    let obj = program_object(&p.program_type);
    ensure(
        h.allowed(Permission::ProgramDraft, Some(&obj), None)
            || h.allowed(Permission::ProgramAssign, Some(&obj), None),
        "permission program.draft or program.assign required",
    )?;
    h.expect_channel(proj, &program_draft_descriptor(&p.program_type))?;
    let p = proj
        .programs
        .get_mut(&d.program)
        .ok_or_else(|| Denied("unknown program".into()))?;
    for day in &d.days {
        if !p.dedicated_days.contains(day) {
            p.dedicated_days.push(*day);
        }
    }
    ensure(
        p.occurrences.len() + p.dedicated_days.len() <= usize::from(u16::MAX),
        "too many days",
    )
}

fn program_parts_shown(
    proj: &mut Projection,
    h: &H<'_>,
    d: &ProgramPartsShown,
) -> Result<(), Denied> {
    let p = proj
        .programs
        .get(&d.program)
        .ok_or_else(|| Denied("unknown program".into()))?;
    let def = h
        .cfg
        .program_type(&p.program_type)
        .ok_or_else(|| Denied("unknown program type".into()))?;
    let obj = program_object(&p.program_type);
    ensure(
        h.allowed(Permission::ProgramDraft, Some(&obj), None)
            || h.allowed(Permission::ProgramAssign, Some(&obj), None),
        "permission program.draft or program.assign required",
    )?;
    h.expect_channel(proj, &program_draft_descriptor(&p.program_type))?;
    for (key, _) in &d.parts {
        let slot = def
            .slots
            .iter()
            .find(|s| s.id == key.slot)
            .ok_or_else(|| Denied(format!("unknown slot `{}`", key.slot)))?;
        ensure(
            usize::from(key.occurrence) < p.occurrences.len(),
            "unknown occurrence",
        )?;
        let reps = match &slot.repeat {
            None => 1,
            Some(Repeat::Count(n)) => *n,
            Some(Repeat::Field(_)) => 50,
        };
        ensure(
            u32::from(key.index) < slot.count.saturating_mul(reps),
            "slot position out of range",
        )?;
    }
    if let Some(p) = proj.programs.get_mut(&d.program) {
        for (key, on) in &d.parts {
            p.shown.insert(key.clone(), *on);
        }
    }
    Ok(())
}

fn program_details_set(
    proj: &mut Projection,
    h: &H<'_>,
    d: &ProgramDetailsSet,
) -> Result<(), Denied> {
    let p = proj
        .programs
        .get(&d.program)
        .ok_or_else(|| Denied("unknown program".into()))?;
    let def = h
        .cfg
        .program_type(&p.program_type)
        .ok_or_else(|| Denied("unknown program type".into()))?;
    let obj = program_object(&p.program_type);
    ensure(
        h.allowed(Permission::ProgramDraft, Some(&obj), None)
            || h.allowed(Permission::ProgramAssign, Some(&obj), None),
        "permission program.draft or program.assign required",
    )?;
    h.expect_channel(proj, &program_draft_descriptor(&p.program_type))?;
    for (key, values) in &d.details {
        let slot = def
            .slots
            .iter()
            .find(|s| s.id == key.slot)
            .ok_or_else(|| Denied(format!("unknown slot `{}`", key.slot)))?;
        ensure(
            usize::from(key.occurrence) < p.occurrences.len(),
            "unknown occurrence",
        )?;
        let reps = match &slot.repeat {
            None => 1,
            Some(Repeat::Count(n)) => *n,
            Some(Repeat::Field(_)) => 50,
        };
        ensure(
            u32::from(key.index) < slot.count.saturating_mul(reps),
            "slot position out of range",
        )?;
        check_values(&slot.extra_fields, values, false)?;
    }
    if let Some(p) = proj.programs.get_mut(&d.program) {
        for (key, values) in &d.details {
            if values.is_empty() {
                p.details.remove(key);
            } else {
                p.details.insert(key.clone(), values.clone());
            }
        }
    }
    Ok(())
}

fn program_updated(proj: &mut Projection, h: &H<'_>, u: &ProgramUpdated) -> Result<(), Denied> {
    let p = proj
        .programs
        .get(&u.program)
        .ok_or_else(|| Denied("unknown program".into()))?
        .clone();
    let def = h
        .cfg
        .program_type(&p.program_type)
        .ok_or_else(|| Denied("unknown program type".into()))?
        .clone();
    let obj = program_object(&p.program_type);
    ensure(
        h.allowed(Permission::ProgramDraft, Some(&obj), None)
            || h.allowed(Permission::ProgramAssign, Some(&obj), None),
        "permission program.draft or program.assign required",
    )?;
    h.expect_channel(proj, &program_draft_descriptor(&p.program_type))?;
    let mut warnings = Vec::new();
    for a in &u.assignments {
        let slot = def
            .slots
            .iter()
            .find(|s| s.id == a.key.slot)
            .ok_or_else(|| Denied(format!("unknown slot `{}`", a.key.slot)))?;
        let date = p
            .date_of(a.key.occurrence)
            .ok_or_else(|| Denied("unknown occurrence".into()))?;
        let reps = match &slot.repeat {
            None => 1,
            Some(Repeat::Count(n)) => *n,
            Some(Repeat::Field(_)) => 50,
        };
        ensure(
            u32::from(a.key.index) < slot.count.saturating_mul(reps),
            "slot position out of range",
        )?;
        check_values(&slot.extra_fields, &a.extra, false)?;
        ensure(slot.assignable, "this slot takes no person")?;
        match &a.assignee {
            Assignee::Person(person) => {
                ensure(h.auth.person_exists(*person), "unknown person")?;
                // On the event day, or on the day of the assignment when the
                // event is already past (a qualification granted today counts
                // for the rest of a program that has started).
                ensure(
                    h.auth
                        .satisfies(&h.cfg, *person, slot.requires.as_ref(), date.max(h.date)),
                    &format!("the person does not meet the requirements of `{}`", slot.id),
                )?;
                let doubled = p.assignments.iter().any(|(k, (x, _))| {
                    k.occurrence == a.key.occurrence && k != &a.key && x == &a.assignee
                });
                if doubled {
                    warnings.push(format!("person assigned twice on {date}"));
                }
            }
            Assignee::External(id) => {
                ensure(
                    slot.allow_external,
                    "this slot does not accept external persons",
                )?;
                ensure(h.auth.externals.contains_key(id), "unknown external person")?;
            }
            Assignee::Unit(u) => {
                let unit = h.auth.org.units.get(u).filter(|x| x.active);
                ensure(
                    unit.is_some_and(|x| {
                        def.assign_units.as_ref() == Some(&x.unit_type)
                            || slot.allow_units.as_ref() == Some(&x.unit_type)
                    }),
                    "this slot does not go to units of that type",
                )?;
            }
        }
    }
    if let Some(p) = proj.programs.get_mut(&u.program) {
        for k in &u.cleared {
            p.assignments.remove(k);
        }
        for a in &u.assignments {
            p.assignments
                .insert(a.key.clone(), (a.assignee.clone(), a.extra.clone()));
        }
    }
    for w in warnings {
        proj.alerts.push(Alert {
            kind: AlertKind::DoubleAssignment,
            event: Some(h.id),
            message: w,
        });
    }
    Ok(())
}

fn assignments_sent(proj: &mut Projection, h: &H<'_>, s: &AssignmentsSent) -> Result<(), Denied> {
    match s {
        AssignmentsSent::Team { program, snapshot } => {
            h.need(
                Permission::ProgramAssign,
                Some(&program_object(&snapshot.program_type)),
                None,
            )?;
            h.expect_channel(proj, &program_assigned_descriptor(&snapshot.program_type))?;
            ensure(&snapshot.program == program, "snapshot of another program")?;
            if let Some(p) = proj.programs.get(program) {
                ensure(
                    p.program_type == snapshot.program_type,
                    "wrong program type",
                )?;
                ensure(
                    snapshot.version == p.sent_version + 1,
                    "versions must follow each other",
                )?;
                let expected = program_snapshot(h.auth, *program, p, snapshot.version);
                ensure(
                    &expected == snapshot,
                    "the snapshot does not match the program",
                )?;
            }
            if let Some(p) = proj.programs.get_mut(program) {
                p.status = ProgramStatus::Assigned;
                p.sent_version = snapshot.version;
            }
            proj.assigned_snapshots.insert(*program, snapshot.clone());
            Ok(())
        }
        AssignmentsSent::Roster {
            program,
            version,
            persons,
        } => {
            let p = proj
                .programs
                .get(program)
                .map(|p| p.program_type.clone())
                .or_else(|| {
                    proj.assigned_snapshots
                        .get(program)
                        .map(|s| s.program_type.clone())
                })
                .ok_or_else(|| Denied("unknown program".into()))?;
            h.need(Permission::ProgramAssign, Some(&program_object(&p)), None)?;
            h.expect_channel(proj, &program_assigned_descriptor(&p))?;
            ensure(
                proj.assigned_snapshots
                    .get(program)
                    .is_some_and(|s| s.version == *version),
                "the roster follows the version it lists",
            )?;
            if let Some(prog) = proj.programs.get(program) {
                let expected: Vec<_> = notice_items(h.auth, prog)
                    .ok_or_else(|| Denied("slot past the events".into()))?
                    .into_iter()
                    .collect();
                ensure(
                    &expected == persons,
                    "the roster does not match the program",
                )?;
            }
            proj.rosters.insert(*program, (*version, persons.clone()));
            Ok(())
        }
        AssignmentsSent::Notice {
            program,
            program_type,
            person,
            items,
        } => {
            let obj = program_object(program_type);
            let at_publish = h
                .cfg
                .program_type(program_type)
                .is_some_and(|d| d.notify == NotifyOn::Publish);
            // Notified at publication: who publishes writes the notices,
            // from the roster of the version on the board.
            if !(at_publish && h.allowed(Permission::ProgramPublish, Some(&obj), None)) {
                h.need(Permission::ProgramAssign, Some(&obj), None)?;
            }
            h.expect_channel(
                proj,
                &assignment_descriptor(program_type, &Assignee::Person(*person)),
            )?;
            if let Some((_, roster)) = proj.rosters.get(program).filter(|_| at_publish) {
                let listed = roster
                    .iter()
                    .find(|(p, _)| p == person)
                    .map(|(_, i)| i.as_slice())
                    .unwrap_or_default();
                ensure(
                    listed == items.as_slice(),
                    "the notice does not match the roster",
                )?;
            } else if let Some(p) = proj.programs.get(program) {
                for it in items {
                    let ok = p
                        .assignments
                        .get(&it.key)
                        .is_some_and(|(a, _)| {
                            p.date_of(it.key.occurrence).is_some_and(|d| {
                                crate::rules::assignee_persons(h.auth, a, d).contains(person)
                            })
                        })
                        && p.date_of(it.key.occurrence) == Some(it.date);
                    ensure(ok, "the notice does not match the program")?;
                }
            }
            if items.is_empty() {
                // No slot left for this person: nothing to tell or remind.
                proj.notices.remove(&(*program, *person));
                return Ok(());
            }
            proj.notices
                .insert((*program, *person), (program_type.clone(), items.clone()));
            let mut ctx = EvalContext::at(h.date);
            ctx.assigned = [*person].into();
            let about = About {
                program_type: Some(program_type.clone()),
                ..About::default()
            };
            h.fire(
                proj,
                Trigger::AssignmentCreated,
                &about,
                &ctx,
                BTreeMap::new(),
            );
            Ok(())
        }
    }
}

fn assignment_responded(
    proj: &mut Projection,
    h: &H<'_>,
    r: &AssignmentResponded,
) -> Result<(), Denied> {
    let def = h
        .cfg
        .program_type(&r.program_type)
        .ok_or_else(|| Denied("unknown program type".into()))?
        .clone();
    h.expect_channel(proj, &assignment_descriptor(&r.program_type, &r.assignee))?;
    let on_behalf = h.allowed(
        Permission::AssignmentRespondOnBehalf,
        Some(&program_object(&r.program_type)),
        None,
    );
    match &r.assignee {
        Assignee::Person(p) => {
            ensure(*p == h.author || on_behalf, "only the assignee can respond")?
        }
        Assignee::External(_) => ensure(
            on_behalf,
            "permission assignment.respond_on_behalf required",
        )?,
        // A unit answers through any of its members.
        Assignee::Unit(u) => ensure(
            h.auth.org.is_member(h.author, *u, h.date) || on_behalf,
            "only a member of the unit can respond",
        )?,
    }
    if let Some(p) = proj.programs.get(&r.program) {
        ensure(
            p.assignments.get(&r.key).is_some_and(|(a, _)| {
                a == &r.assignee
                    // A member answers for the unit holding the slot.
                    || matches!((a, &r.assignee), (Assignee::Unit(u), Assignee::Person(m))
                        if h.auth.org.is_member(*m, *u, h.date))
            }),
            "not assigned to this slot",
        )?;
    }
    if let Response::Substituted { substitute, .. } = &r.response {
        ensure(h.auth.person_exists(*substitute), "unknown person")?;
        // Who can take the slot on that day, when the program is known here.
        if let Some(date) = proj.programs.get(&r.program).and_then(|p| p.date_of(r.key.occurrence)) {
            let slot = def.slots.iter().find(|s| s.id == r.key.slot);
            ensure(
                h.auth.satisfies(&h.cfg, *substitute, slot.and_then(|s| s.requires.as_ref()), date.max(h.date)),
                "the substitute does not meet the requirements",
            )?;
        }
    }
    proj.responses
        .insert((r.program, r.key.clone()), r.response.clone());
    if r.response.is_declined() {
        let mut roles = vec![&def.owner_role];
        roles.extend(def.assistant_roles.iter());
        let mut ctx = EvalContext::at(h.date);
        ctx.object_team = h.auth.holders_of(&roles, h.date);
        let about = About {
            program_type: Some(r.program_type.clone()),
            ..About::default()
        };
        // Who, which slot (its text key), when, and the substitute if any.
        let date = proj
            .programs
            .get(&r.program)
            .and_then(|p| p.date_of(r.key.occurrence))
            .or_else(|| {
                proj.notices.values().flat_map(|(_, items)| items).find(|i| i.key == r.key).map(|i| i.date)
            });
        let mut params = BTreeMap::from([
            ("person".to_owned(), crate::rules::display_name(h.auth, &r.assignee)),
            (
                "slot".to_owned(),
                def.slots
                    .iter()
                    .find(|s| s.id == r.key.slot)
                    .map_or_else(|| r.key.slot.to_string(), |s| s.label.to_string()),
            ),
            ("date".to_owned(), date.map(|d| d.to_string()).unwrap_or_default()),
            ("substitute".to_owned(), String::new()),
        ]);
        if let Response::Substituted { substitute, .. } = &r.response {
            params.insert(
                "substitute".to_owned(),
                crate::rules::display_name(h.auth, &Assignee::Person(*substitute)),
            );
        }
        h.fire(proj, Trigger::AssignmentDeclined, &about, &ctx, params);
    }
    Ok(())
}

fn program_published(
    proj: &mut Projection,
    h: &H<'_>,
    s: &ProgramSnapshot,
    revision: bool,
) -> Result<(), Denied> {
    let def = h
        .cfg
        .program_type(&s.program_type)
        .ok_or_else(|| Denied("unknown program type".into()))?
        .clone();
    h.need(
        Permission::ProgramPublish,
        Some(&program_object(&s.program_type)),
        None,
    )?;
    h.expect_channel(proj, &published_descriptor(&def.publish_audience))?;
    if let Some(a) = proj.assigned_snapshots.get(&s.program) {
        ensure(a == s, "only the assigned version can be published")?;
    }
    match (proj.published.get(&s.program), revision) {
        (None, false) => {}
        (Some(prev), true) => ensure(s.version > prev.version, "a revision needs a newer version")?,
        (Some(_), false) => return deny("already published: publish a revision"),
        (None, true) => return deny("nothing to revise"),
    }
    let (from, until) = visibility(h, s);
    let changed: BTreeSet<String> = match proj.published.get(&s.program) {
        Some(prev) => s
            .assignments
            .iter()
            .filter(|a| !prev.assignments.contains(a))
            .map(|a| a.display_name.clone())
            .collect(),
        None => BTreeSet::new(),
    };
    proj.published.insert(s.program, s.clone());
    proj.board.insert(
        s.program,
        BoardPost {
            audience: def.publish_audience.clone(),
            item: BoardItem::Program(s.clone()),
            from,
            until,
            withdrawn: false,
        },
    );
    let about = About {
        program_type: Some(s.program_type.clone()),
        ..About::default()
    };
    let mut params = BTreeMap::new();
    params.insert("program".to_owned(), s.program_type.to_string());
    if !changed.is_empty() {
        params.insert(
            "changed".to_owned(),
            changed.into_iter().collect::<Vec<_>>().join(", "),
        );
    }
    h.fire(
        proj,
        Trigger::ProgramPublished,
        &about,
        &EvalContext::at(h.date),
        params,
    );
    Ok(())
}

fn visibility(h: &H<'_>, s: &ProgramSnapshot) -> (Date, Date) {
    use core_model::strings::VisibilityBound as V;
    let start = s.occurrences.first().copied().unwrap_or(h.date);
    let end = s.occurrences.last().copied().unwrap_or(h.date);
    let Some(board) = h.cfg.package().board.board.as_ref() else {
        return (h.date, end);
    };
    let bound = |b: V| match b {
        V::PeriodStart { minus_days } => add_days(start, -i64::from(minus_days)),
        V::PeriodEnd { plus_days } => add_days(end, i64::from(plus_days)),
    };
    (
        bound(board.default_visibility.from),
        bound(board.default_visibility.until),
    )
}

// ----- resources ------------------------------------------------------------------

fn holder_resource(
    proj: &mut Projection,
    id: core_model::Id,
    holder: PersonId,
) -> Result<&mut Resource, Denied> {
    let res = proj
        .resources
        .get_mut(&id)
        .ok_or_else(|| Denied("unknown resource".into()))?;
    ensure(
        res.holding.as_ref().is_some_and(|x| x.holder == holder),
        "the resource is not held by this person",
    )?;
    Ok(res)
}

fn resource_assigned(proj: &mut Projection, h: &H<'_>, a: &ResourceAssigned) -> Result<(), Denied> {
    let def = h
        .cfg
        .resource_type(&a.resource_type)
        .ok_or_else(|| Denied("unknown resource type".into()))?
        .clone();
    h.need(
        Permission::ResourceAssign,
        Some(&resource_object(&a.resource_type)),
        None,
    )?;
    h.expect_channel(proj, &resource_descriptor(&a.resource_type, Some(a.holder)))?;
    ensure(h.auth.person_exists(a.holder), "unknown holder")?;
    ensure(
        h.auth
            .satisfies(&h.cfg, a.holder, def.holder_requires.as_ref(), a.date),
        "the person cannot hold this resource",
    )?;
    if let Some(max) = def.rules.max_per_holder {
        let held = proj
            .resources
            .values()
            .filter(|r| {
                r.resource_type == a.resource_type
                    && r.holding.as_ref().is_some_and(|x| x.holder == a.holder)
            })
            .count();
        ensure(
            held < max as usize,
            "the person already holds the maximum number of these resources",
        )?;
    }
    let entry = proj
        .resources
        .entry(a.resource)
        .or_insert_with(|| Resource {
            resource_type: a.resource_type.clone(),
            attributes: a.attributes.clone(),
            holding: None,
            history: Vec::new(),
            retired: false,
        });
    ensure(!entry.retired, "retired resource")?;
    ensure(entry.holding.is_none(), "the resource is already assigned")?;
    entry.holding = Some(Holding {
        holder: a.holder,
        since: a.date,
        until: None,
        logs: Vec::new(),
    });
    let mut ctx = EvalContext::at(h.date);
    ctx.holder = Some(a.holder);
    let about = About {
        resource_type: Some(a.resource_type.clone()),
        ..About::default()
    };
    h.fire(
        proj,
        Trigger::ResourceAssigned,
        &about,
        &ctx,
        BTreeMap::new(),
    );
    Ok(())
}
