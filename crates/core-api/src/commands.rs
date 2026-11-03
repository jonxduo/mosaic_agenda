//! Commands that change something: they build events through the engine's
//! client (which checks them against the rules) and commit them.

use std::collections::BTreeMap;
use std::sync::Arc;

use core_engine::ops::{Assignee, AssignmentSpec, Response, SlotKey, Values};
use core_engine::time::{PeriodKey, date_of, parse_date};
use core_events::genesis::{NewOrganization, create_genesis};
use core_events::payloads::{NetworkParams, RootAdmin};
use core_model::{Id, PersonId, Scalar, Slug, UnitId};

use crate::dto::*;
use crate::views::cfg;
use crate::{ApiError, ApiResult, Core, parse_id, parse_person};

fn slug(s: &str) -> ApiResult<Slug> {
    Slug::new(s).map_err(ApiError::invalid)
}

fn date(s: &str) -> ApiResult<time::Date> {
    parse_date(s).ok_or_else(|| ApiError::invalid(format!("invalid date `{s}`")))
}

fn values(map: BTreeMap<String, serde_json::Value>) -> ApiResult<Values> {
    let mut out = Values::new();
    for (k, v) in map {
        if v.is_null() || v == serde_json::Value::String(String::new()) {
            continue;
        }
        let s: Scalar = serde_json::from_value(v).map_err(ApiError::invalid)?;
        out.insert(slug(&k)?, s);
    }
    Ok(out)
}

fn assignee(s: &str) -> ApiResult<Assignee> {
    match s.strip_prefix("ext:") {
        Some(id) => Ok(Assignee::External(parse_id(id)?)),
        None => Ok(Assignee::Person(parse_person(s)?)),
    }
}

fn qr_svg(text: &str) -> String {
    qrcode::QrCode::new(text.as_bytes())
        .map(|c| {
            c.render::<qrcode::render::svg::Color<'_>>()
                .min_dimensions(240, 240)
                .build()
        })
        .unwrap_or_default()
}

impl Core {
    pub fn accept_privacy(&self) -> ApiResult<()> {
        self.update_local(|l| l.privacy_accepted = true)
    }

    /// Creates a new organization with this device as its first root administrator.
    pub async fn create_org(
        self: &Arc<Self>,
        name: String,
        my_name: String,
    ) -> ApiResult<AppStatus> {
        if self.has_org() {
            return Err(ApiError::invalid(
                "this device already belongs to an organization",
            ));
        }
        let person = PersonId(Id::from_bytes(
            core_crypto::random_array::<16>().map_err(ApiError::internal)?,
        ));
        let org = NewOrganization {
            name,
            root_admins: vec![RootAdmin {
                person,
                devices: vec![self.keys.identity()],
            }],
            config_bundle: self.bundle.clone(),
            network: self.server.clone().unwrap_or(NetworkParams {
                relay_urls: Vec::new(),
                dns_url: None,
                push_url: None,
            }),
            security: self.security.clone().unwrap_or_default(),
        };
        let (_, genesis) =
            create_genesis(org, &self.keys, self.now()).map_err(ApiError::internal)?;
        self.state
            .lock()
            .map_err(|_| ApiError::internal("poisoned"))?
            .store
            .store_mut()
            .ok_or_else(crate::locked_error)?
            .insert(&genesis, self.now())
            .map_err(ApiError::internal)?;
        self.invalidate();
        self.act_events(|c| c.rename_person(person, &my_name))?;
        self.start_network().await?;
        self.status()
    }

    pub async fn join_org(self: &Arc<Self>, ticket: String) -> ApiResult<String> {
        self.join(&ticket).await
    }

    /// Invitation for a new device of `person` (ticket + QR).
    pub async fn invite_device(self: &Arc<Self>, person: String) -> ApiResult<Invitation> {
        let person = parse_person(&person)?;
        self.start_network().await?;
        let v = self.view()?;
        let org = v.proj.org;
        let (relay, addrs) = match self.node().await {
            Some(n) => {
                let a = n.addr();
                (
                    a.relay_urls().next().map(|u| u.to_string()),
                    a.ip_addrs().map(|s| s.to_string()).collect(),
                )
            }
            None => (None, Vec::new()),
        };
        let ticket = self
            .invitations()
            .lock()
            .map_err(|_| ApiError::internal("poisoned"))?
            .invitations
            .create(org, self.keys.public(), person, self.now(), relay, addrs)
            .map_err(ApiError::internal)?;
        self.act_events(|c| c.invite_device(person, ticket.invite, ticket.expires_at))?;
        let text = ticket.encode();
        let t = self.texts();
        Ok(Invitation {
            qr_svg: qr_svg(&text),
            ticket: text,
            expires_at: format!(
                "{} {:02}:{:02}",
                t.day_month(date_of(ticket.expires_at)),
                (ticket.expires_at / 3_600_000) % 24,
                (ticket.expires_at / 60_000) % 60
            ),
        })
    }

    /// Confirms (or refuses) a pairing whose code matches the new device's.
    pub async fn confirm_pairing(self: &Arc<Self>, code: String, accept: bool) -> ApiResult<()> {
        let Some((person, identity)) = self.take_pairing_decision(&code, accept)? else {
            return Ok(());
        };
        // Give the pairing handler a moment to answer the new device.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        self.act_events(|c| c.add_device(person, identity))?;
        self.act_events(|c| c.complete_channels())?;
        Ok(())
    }

    pub fn set_pin_cmd(&self, pin: Option<String>) -> ApiResult<()> {
        self.set_pin(pin)
    }

    // ----- documents -------------------------------------------------------------

    pub fn save_document(
        self: &Arc<Self>,
        form: String,
        subject: Option<String>,
        period: String,
        values_in: BTreeMap<String, serde_json::Value>,
    ) -> ApiResult<FormView> {
        let me = self.me()?;
        let subject_id = match &subject {
            Some(s) => parse_person(s)?,
            None => me,
        };
        let p: PeriodKey = period.parse().map_err(ApiError::invalid)?;
        let f = slug(&form)?;
        let vals = values(values_in)?;
        self.act(|c| c.fill_document(&f, subject_id, p, vals))?;
        self.form(form, Some(subject_id.to_string()), period)
    }

    pub fn step_document(
        self: &Arc<Self>,
        document: String,
        step: String,
        note: Option<String>,
    ) -> ApiResult<FormView> {
        let id = parse_id(&document)?;
        let st = slug(&step)?;
        let note = note.filter(|n| !n.trim().is_empty());
        self.act_events(|c| c.step_document(id, &st, note))?;
        let v = self.view()?;
        let d = v
            .proj
            .documents
            .get(&id)
            .ok_or_else(|| ApiError::not_found("document"))?;
        self.form(
            d.content.form.to_string(),
            Some(d.content.subject.to_string()),
            d.content.period.to_string(),
        )
    }

    pub fn batch_step(
        self: &Arc<Self>,
        form: String,
        unit: String,
        period: String,
        step: String,
    ) -> ApiResult<()> {
        let f = slug(&form)?;
        let u = UnitId(parse_id(&unit)?);
        let p: PeriodKey = period.parse().map_err(ApiError::invalid)?;
        let s = slug(&step)?;
        self.act_events(|c| c.batch_step(&f, u, p, &s))
    }

    pub fn generate_aggregate(
        self: &Arc<Self>,
        aggregate: String,
        period: String,
        note: Option<String>,
    ) -> ApiResult<AggregateView> {
        let a = slug(&aggregate)?;
        let p: PeriodKey = period.parse().map_err(ApiError::invalid)?;
        self.act_events(|c| c.generate_aggregate(&a, p, note))?;
        self.aggregate(aggregate, Some(period))
    }

    pub fn distribute_aggregate(
        self: &Arc<Self>,
        aggregate: String,
        period: String,
    ) -> ApiResult<AggregateView> {
        let a = slug(&aggregate)?;
        let p: PeriodKey = period.parse().map_err(ApiError::invalid)?;
        self.act_events(|c| c.distribute_aggregate(&a, p))?;
        self.aggregate(aggregate, Some(period))
    }

    // ----- programs ----------------------------------------------------------------

    pub fn draft_program(
        self: &Arc<Self>,
        program_type: String,
        dates: Vec<String>,
    ) -> ApiResult<String> {
        let t = slug(&program_type)?;
        let mut d: Vec<time::Date> = dates.iter().map(|x| date(x)).collect::<ApiResult<_>>()?;
        d.sort();
        d.dedup();
        let id = self.act(|c| c.draft_program(&t, d))?;
        Ok(id.to_string())
    }

    /// Assigns a slot (an empty `person` empties it). In a program that
    /// rotates by week or month, every event of the group gets the same person.
    pub fn assign_slot(
        self: &Arc<Self>,
        program: String,
        occurrence: u32,
        slot: String,
        index: u32,
        person: String,
    ) -> ApiResult<ProgramEditor> {
        self.assign_slots(
            program,
            vec![SlotChange {
                occurrence,
                slot,
                index,
                person,
            }],
            Vec::new(),
        )
    }

    /// Saves the draft: several slot choices in one change of the program.
    pub fn assign_slots(
        self: &Arc<Self>,
        program: String,
        changes: Vec<SlotChange>,
        details: Vec<DetailChange>,
    ) -> ApiResult<ProgramEditor> {
        let id = parse_id(&program)?;
        if !details.is_empty() {
            let set = self.detail_values(id, details)?;
            self.act_events(|c| c.set_program_details(id, set))?;
        }
        let mut specs = Vec::new();
        let mut cleared = Vec::new();
        for ch in changes {
            let slot = slug(&ch.slot)?;
            let index = u16::try_from(ch.index).map_err(ApiError::invalid)?;
            let keys = self.group_keys(id, ch.occurrence, &slot, index)?;
            if ch.person.is_empty() {
                cleared.extend(keys);
            } else {
                let a = assignee(&ch.person)?;
                specs.extend(keys.into_iter().map(|key| AssignmentSpec {
                    key,
                    assignee: a.clone(),
                    extra: Values::new(),
                }));
            }
        }
        if !specs.is_empty() || !cleared.is_empty() {
            self.act_events(|c| c.update_program(id, specs, cleared))?;
        }
        // Notified at publication: saving hands the program to who publishes
        // it; the assignees hear of it only from the board.
        let hand_over = {
            let v = self.view()?;
            let c = cfg(&v)?;
            let me = self.me()?;
            v.proj.programs.get(&id).is_some_and(|p| {
                crate::views::notified_at_publish(c, &p.program_type)
                    && crate::views::unsent(&v, &id)
                    && v.proj.auth.is_allowed(
                        c,
                        me,
                        core_model::Permission::ProgramAssign,
                        Some(&core_engine::channels::program_object(&p.program_type)),
                        None,
                        crate::views::today(self),
                    )
            })
        };
        if hand_over {
            self.act_events(|c| c.send_assignments(id))?;
        }
        self.program(program)
    }

    /// Editor texts as typed values of the slots' extra fields.
    fn detail_values(
        &self,
        program: Id,
        details: Vec<DetailChange>,
    ) -> ApiResult<Vec<(SlotKey, Values)>> {
        let v = self.view()?;
        let p = v
            .proj
            .programs
            .get(&program)
            .ok_or_else(|| ApiError::not_found("program"))?;
        let def = cfg(&v)?
            .program_type(&p.program_type)
            .ok_or_else(|| ApiError::not_found("program type"))?;
        let mut out = Vec::new();
        for d in details {
            let slot = slug(&d.slot)?;
            let sdef = def
                .slots
                .iter()
                .find(|s| s.id == slot)
                .ok_or_else(|| ApiError::not_found("slot"))?;
            let key = SlotKey {
                occurrence: u16::try_from(d.occurrence).map_err(ApiError::invalid)?,
                slot,
                index: u16::try_from(d.index).map_err(ApiError::invalid)?,
            };
            let mut values = p.details.get(&key).cloned().unwrap_or_default();
            for (field, text) in d.values {
                let f = slug(&field)?;
                let ty = sdef.extra_fields.iter().find(|x| x.id == f).map(|x| x.ty);
                match crate::prefill::scalar(ty, text.trim()) {
                    Some(s) => values.insert(f, s),
                    None => values.remove(&f),
                };
            }
            out.push((key, values));
        }
        Ok(out)
    }

    /// The slot keys of the group of `occurrence` (see `rotation`).
    fn group_keys(
        &self,
        program: Id,
        occurrence: u32,
        slot: &Slug,
        index: u16,
    ) -> ApiResult<Vec<SlotKey>> {
        let occ = usize::try_from(occurrence).map_err(ApiError::invalid)?;
        let v = self.view()?;
        let (program_type, occurrences) = match v.proj.programs.get(&program) {
            Some(p) => (p.program_type.clone(), p.occurrences.clone()),
            None => {
                let s = v
                    .proj
                    .assigned_snapshots
                    .get(&program)
                    .or_else(|| v.proj.published.get(&program))
                    .ok_or_else(|| ApiError::not_found("program"))?;
                (s.program_type.clone(), s.occurrences.clone())
            }
        };
        let group = cfg(&v)?
            .program_type(&program_type)
            .map(|d| {
                crate::schedule::group_of(
                    crate::schedule::slot_rotation(d, slot),
                    &occurrences,
                    occ,
                )
            })
            .filter(|g| !g.is_empty())
            .unwrap_or_else(|| vec![occ]);
        group
            .into_iter()
            .map(|o| {
                Ok(SlotKey {
                    occurrence: u16::try_from(o).map_err(ApiError::invalid)?,
                    slot: slot.clone(),
                    index,
                })
            })
            .collect()
    }

    pub fn send_program(self: &Arc<Self>, program: String) -> ApiResult<ProgramEditor> {
        let id = parse_id(&program)?;
        self.act_events(|c| c.send_assignments(id))?;
        self.program(program)
    }

    pub fn publish_program(self: &Arc<Self>, program: String) -> ApiResult<ProgramEditor> {
        let id = parse_id(&program)?;
        self.act_events(|c| c.publish_program(id))?;
        self.program(program)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn respond(
        self: &Arc<Self>,
        program: String,
        program_type: String,
        occurrence: u32,
        slot: String,
        index: u32,
        accept: bool,
        note: Option<String>,
        assignee_id: Option<String>,
    ) -> ApiResult<()> {
        let id = parse_id(&program)?;
        let t = slug(&program_type)?;
        let slot = slug(&slot)?;
        let index = u16::try_from(index).map_err(ApiError::invalid)?;
        let a = match assignee_id {
            Some(a) => assignee(&a)?,
            None => Assignee::Person(self.me()?),
        };
        // The answer covers the events of the rotation held by the same person.
        let keys: Vec<SlotKey> = {
            let v = self.view()?;
            let own = SlotKey {
                occurrence: u16::try_from(occurrence).map_err(ApiError::invalid)?,
                slot: slot.clone(),
                index,
            };
            match &a {
                Assignee::Person(p) => {
                    let rotation = cfg(&v)?
                        .program_type(&t)
                        .map(|d| crate::schedule::slot_rotation(d, &slot))
                        .unwrap_or_default();
                    let items = v
                        .proj
                        .notices
                        .get(&(id, *p))
                        .map(|(_, items)| items.as_slice())
                        .unwrap_or_default();
                    let group = items
                        .iter()
                        .find(|i| i.key == own)
                        .and_then(|i| crate::schedule::rotation_key(rotation, i.date));
                    let mut keys: Vec<SlotKey> = items
                        .iter()
                        .filter(|i| {
                            i.key == own
                                || (group.is_some()
                                    && i.key.slot == slot
                                    && i.key.index == index
                                    && crate::schedule::rotation_key(rotation, i.date) == group)
                        })
                        .map(|i| i.key.clone())
                        .collect();
                    if keys.is_empty() {
                        keys.push(own);
                    }
                    keys
                }
                Assignee::External(_) => {
                    let held = |k: &SlotKey| {
                        v.proj
                            .programs
                            .get(&id)
                            .is_some_and(|p| p.assignments.get(k).is_some_and(|(x, _)| *x == a))
                    };
                    let keys: Vec<SlotKey> = self
                        .group_keys(id, occurrence, &slot, index)
                        .unwrap_or_default()
                        .into_iter()
                        .filter(|k| *k == own || held(k))
                        .collect();
                    if keys.is_empty() { vec![own] } else { keys }
                }
            }
        };
        let response = if accept {
            Response::Accepted
        } else {
            Response::Declined { note }
        };
        self.act_events(|c| c.respond_all(id, &t, keys, a, response))
    }

    pub fn add_external(self: &Arc<Self>, name: String) -> ApiResult<String> {
        let id = self.act(|c| c.add_external(&name))?;
        Ok(format!("ext:{id}"))
    }

    // ----- resources ---------------------------------------------------------------

    pub fn create_resource(
        self: &Arc<Self>,
        resource_type: String,
        attributes: BTreeMap<String, serde_json::Value>,
    ) -> ApiResult<String> {
        let t = slug(&resource_type)?;
        let vals = values(attributes)?;
        let id = self.act(|c| c.upsert_resource(None, &t, vals))?;
        Ok(id.to_string())
    }

    pub fn assign_resource(self: &Arc<Self>, resource: String, holder: String) -> ApiResult<()> {
        let r = parse_id(&resource)?;
        let h = parse_person(&holder)?;
        self.act_events(|c| c.assign_resource(r, h))
    }

    pub fn open_request(self: &Arc<Self>, resource: String, request: String) -> ApiResult<()> {
        let r = parse_id(&resource)?;
        let q = slug(&request)?;
        let v = self.view()?;
        let cfg = cfg(&v)?;
        let res = v
            .proj
            .resources
            .get(&r)
            .ok_or_else(|| ApiError::not_found("resource"))?;
        let days = cfg
            .resource_type(&res.resource_type)
            .and_then(|d| d.requests.iter().find(|x| x.id == q))
            .and_then(|x| x.due)
            .map_or(14, |d| d.days);
        let due = core_engine::time::add_days(date_of(self.now()), i64::from(days));
        self.act(|c| c.open_request(r, &q, due)).map(|_| ())
    }

    pub fn answer_request(self: &Arc<Self>, request: String, answer: String) -> ApiResult<()> {
        let r = parse_id(&request)?;
        let a = slug(&answer)?;
        self.act_events(|c| c.answer_request(r, &a))
    }

    pub fn log_resource(
        self: &Arc<Self>,
        resource: String,
        values_in: BTreeMap<String, serde_json::Value>,
    ) -> ApiResult<()> {
        let r = parse_id(&resource)?;
        let vals = values(values_in)?;
        self.act_events(|c| c.log_resource(r, vals))
    }

    pub fn return_resource(self: &Arc<Self>, resource: String) -> ApiResult<()> {
        let r = parse_id(&resource)?;
        self.act_events(|c| c.return_resource(r))
    }

    // ----- organization -------------------------------------------------------------

    pub fn add_person(
        self: &Arc<Self>,
        name: String,
        attributes: BTreeMap<String, serde_json::Value>,
    ) -> ApiResult<String> {
        let vals = values(attributes)?;
        let p = self.act(|c| c.add_person(&name, vals))?;
        Ok(p.to_string())
    }

    pub fn update_person(
        self: &Arc<Self>,
        person: String,
        name: Option<String>,
        attributes: BTreeMap<String, serde_json::Value>,
    ) -> ApiResult<()> {
        let p = parse_person(&person)?;
        if let Some(n) = name.filter(|n| !n.trim().is_empty()) {
            self.act_events(|c| c.rename_person(p, &n))?;
        }
        // An empty value clears an attribute the person has.
        let cleared: Vec<Slug> = {
            let v = self.view()?;
            let had = v
                .proj
                .auth
                .org
                .persons
                .get(&p)
                .map(|r| r.attributes.clone())
                .unwrap_or_default();
            attributes
                .iter()
                .filter(|(_, x)| x.is_null() || **x == serde_json::Value::String(String::new()))
                .filter_map(|(k, _)| Slug::new(k).ok())
                .filter(|k| had.contains_key(k))
                .collect()
        };
        let vals = values(attributes)?;
        if !vals.is_empty() || !cleared.is_empty() {
            self.act_events(|c| c.update_person(p, vals, cleared))?;
        }
        Ok(())
    }

    pub fn grant_qualification(
        self: &Arc<Self>,
        person: String,
        qualification: String,
        from: Option<String>,
        until: Option<String>,
    ) -> ApiResult<()> {
        let p = parse_person(&person)?;
        let q = slug(&qualification)?;
        let v = self.view()?;
        let cfg = cfg(&v)?;
        let def = cfg
            .qualification(&q)
            .ok_or_else(|| ApiError::not_found("qualification"))?;
        let start = match from {
            Some(f) => date(&f)?,
            None => date_of(self.now()),
        };
        let end = match (until, def.duration) {
            (Some(u), _) => Some(date(&u)?),
            (None, core_model::config::QualificationDuration::FixedPeriod) => {
                core_engine::auth::fixed_end(start, def.fixed_period_unit)
            }
            (None, _) => None,
        };
        // A qualification it excludes, held by the person on `start` (even if
        // it ended later), is closed on `start`: the new one replaces it from
        // that day, so they never overlap. One that starts later still clashes.
        let clashing: Vec<Id> = {
            let auth = &v.proj.auth;
            let excluded = |x: &Slug| {
                def.excludes.contains(x)
                    || cfg
                        .qualification(x)
                        .is_some_and(|d| d.excludes.contains(&q))
            };
            auth.qualification_ids
                .iter()
                .filter_map(|(gid, i)| auth.org.qualification_grants.get(*i).map(|g| (gid, g)))
                .filter(|(_, g)| {
                    g.person == p
                        && excluded(&g.qualification)
                        && g.period.start < start
                        && g.period.end.is_none_or(|e| e > start)
                })
                .map(|(gid, _)| *gid)
                .collect()
        };
        drop(v);
        for g in clashing {
            self.act_events(|c| c.end_qualification(g, start))?;
        }
        self.act(|c| c.grant_qualification(p, &q, start, end))
            .map(|_| ())
    }

    /// Ends a qualification, membership or role grant (`kind`) at `end` (default today).
    pub fn end_grant(
        self: &Arc<Self>,
        kind: String,
        grant: String,
        end: Option<String>,
    ) -> ApiResult<()> {
        let g = parse_id(&grant)?;
        let end = match end {
            Some(e) => date(&e)?,
            None => date_of(self.now()),
        };
        match kind.as_str() {
            "qualification" => self.act_events(|c| c.end_qualification(g, end)),
            "membership" => self.act_events(|c| c.end_membership(g, end)),
            "role" => self.act_events(|c| c.end_role(g, end)),
            _ => Err(ApiError::invalid(
                "kind must be qualification, membership or role",
            )),
        }
    }

    pub fn create_unit(self: &Arc<Self>, unit_type: String, name: String) -> ApiResult<String> {
        let t = slug(&unit_type)?;
        let u = self.act(|c| c.create_unit(&t, &name))?;
        Ok(u.to_string())
    }

    /// Moves a person to a unit (ending the current membership of the same type).
    pub fn set_membership(
        self: &Arc<Self>,
        person: String,
        unit: String,
        from: Option<String>,
    ) -> ApiResult<()> {
        let p = parse_person(&person)?;
        let u = UnitId(parse_id(&unit)?);
        let today = match from {
            Some(f) => date(&f)?,
            None => date_of(self.now()),
        };
        let v = self.view()?;
        let ut = v
            .proj
            .auth
            .org
            .units
            .get(&u)
            .map(|x| x.unit_type.clone())
            .ok_or_else(|| ApiError::not_found("unit"))?;
        let current: Vec<Id> = v
            .proj
            .auth
            .membership_ids
            .iter()
            .filter_map(|(gid, i)| v.proj.auth.org.memberships.get(*i).map(|m| (gid, m)))
            .filter(|(_, m)| {
                m.person == p
                    && m.period.contains(today)
                    && v.proj
                        .auth
                        .org
                        .units
                        .get(&m.unit)
                        .is_some_and(|x| x.unit_type == ut)
            })
            .map(|(gid, _)| *gid)
            .collect();
        drop(v);
        for g in current {
            self.act_events(|c| c.end_membership(g, today))?;
        }
        self.act(|c| c.add_membership(p, u, today)).map(|_| ())
    }

    pub fn grant_role(
        self: &Arc<Self>,
        person: String,
        role: String,
        unit: Option<String>,
    ) -> ApiResult<()> {
        let p = parse_person(&person)?;
        let r = slug(&role)?;
        let u = unit
            .filter(|u| !u.is_empty())
            .map(|u| parse_id(&u).map(UnitId))
            .transpose()?;
        let today = date_of(self.now());
        self.act(|c| c.grant_role(p, &r, u, today)).map(|_| ())
    }

    pub fn revoke_device(self: &Arc<Self>, device: String) -> ApiResult<()> {
        let key = device_key(&device)?;
        self.act_events(|c| c.revoke_device(key))
    }

    /// Marks a device as an archive (§9.4): always-on desktop that keeps
    /// the whole history and every attachment, contacted first.
    pub fn set_archive(self: &Arc<Self>, device: String, archive: bool) -> ApiResult<()> {
        let key = device_key(&device)?;
        self.act_events(|c| c.set_archive(key, archive))
    }

    pub fn update_settings(
        &self,
        auto_lock_minutes: u32,
        mobile_data_photos: bool,
        muted_categories: Vec<String>,
    ) -> ApiResult<Settings> {
        if !(1..=30).contains(&auto_lock_minutes) {
            return Err(ApiError::invalid(
                "auto-lock must be between 1 and 30 minutes",
            ));
        }
        self.update_local(|l| {
            l.auto_lock_minutes = Some(auto_lock_minutes);
            l.mobile_data_photos = mobile_data_photos;
            l.muted_categories = muted_categories;
        })?;
        self.settings()
    }

    pub async fn sync_now(self: &Arc<Self>) -> ApiResult<SyncStatus> {
        self.start_network().await?;
        self.sync_round().await?;
        self.sync_status().await
    }
}

fn device_key(device: &str) -> ApiResult<core_crypto::DevicePublicKey> {
    let bytes = data_encoding::HEXLOWER
        .decode(device.as_bytes())
        .map_err(ApiError::invalid)?;
    Ok(core_crypto::DevicePublicKey(
        bytes
            .try_into()
            .map_err(|_| ApiError::invalid("device id"))?,
    ))
}
