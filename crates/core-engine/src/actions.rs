//! Building events on behalf of the local device.
//!
//! Every action builds the events it needs (key distributions or rotations
//! first, then the content) and checks them with a dry-run [`rebuild`]: an
//! action succeeds only if every device would accept its events.

use std::collections::{BTreeMap, BTreeSet};

use core_crypto::distribution::seal_channel_key;
use core_crypto::{
    ChannelDescriptor, ChannelId, ChannelKey, ChannelKeyring, ChannelRef, DeviceIdentity,
    DeviceKeys, DevicePublicKey,
};
use core_events::payloads::{DeviceAdded, DeviceRevoked, KeyDistribution, SealedKeyEntry};
use core_events::{Draft, EventError, EventHash, EventKind, ObjectHeader, ObjectType, SignedEvent};
use core_model::config::{NotifyOn, StepKind, VariantDef};
use core_model::{Id, PersonId, Slug, UnitId};
use serde::Serialize;
use time::Date;

use crate::auth::Denied;
use crate::channels::{self, ChannelInfo};
use crate::config::Cfg;
use crate::ops::*;
use crate::projection::{Projection, Verdict, rebuild};
use crate::rules::*;
use crate::time::{PeriodKey, date_of};

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("not allowed: {0}")]
    Denied(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error(transparent)]
    Event(#[from] EventError),
    #[error("cryptography: {0}")]
    Crypto(#[from] core_crypto::CryptoError),
    #[error("encoding")]
    Encode,
    #[error("engine: {0}")]
    Engine(String),
}

impl From<Denied> for ActionError {
    fn from(d: Denied) -> Self {
        Self::Denied(d.0)
    }
}

fn nf(what: &str) -> ActionError {
    ActionError::NotFound(what.to_owned())
}

fn new_id() -> Result<Id, ActionError> {
    Ok(Id::from_bytes(core_crypto::random_array::<16>()?))
}

/// The local device acting on a projection.
pub struct Client<'a> {
    pub events: &'a [SignedEvent],
    pub proj: &'a Projection,
    pub keys: &'a DeviceKeys,
    pub keyring: &'a ChannelKeyring,
    /// Declared time of the new events (ms since the Unix epoch).
    pub now: i64,
    /// Incremental engine on the same events, for a fast dry run.
    pub engine: Option<&'a std::sync::Mutex<crate::engine::Engine>>,
}

/// Events being built, chained one after the other.
struct Chain {
    heads: Vec<(EventHash, u64)>,
    events: Vec<SignedEvent>,
    keyring: ChannelKeyring,
    channels: BTreeMap<ChannelId, ChannelInfo>,
}

impl<'a> Client<'a> {
    pub fn date(&self) -> Date {
        date_of(self.now)
    }

    pub fn person(&self) -> Result<PersonId, ActionError> {
        Ok(self.proj.auth.author(&self.keys.public())?)
    }

    fn cfg(&self) -> Result<&Cfg, ActionError> {
        self.proj.cfg().ok_or_else(|| nf("configuration"))
    }

    fn chain(&self) -> Chain {
        Chain {
            heads: self.proj.heads.clone(),
            events: Vec::new(),
            keyring: self.keyring.clone(),
            channels: BTreeMap::new(),
        }
    }

    fn push(chain: &mut Chain, e: SignedEvent) {
        chain.heads = vec![(e.id, e.body.depth)];
        chain.events.push(e);
    }

    fn draft(&self, chain: &Chain, kind: EventKind) -> Draft {
        let mut d = Draft::after(self.proj.org, kind, &chain.heads, self.now);
        d.config_version = self.proj.active_config;
        d
    }

    fn public<T: Serialize>(
        &self,
        chain: &mut Chain,
        kind: EventKind,
        payload: &T,
    ) -> Result<(), ActionError> {
        let e = self.draft(chain, kind).sign_public(payload, self.keys)?;
        Self::push(chain, e);
        Ok(())
    }

    /// Makes sure the channel has a key every expected member has; rotates
    /// when a member left or this device lacks the current key (§8.3, §8.4).
    fn ensure_channel(
        &self,
        chain: &mut Chain,
        desc: &ChannelDescriptor,
    ) -> Result<(ChannelRef, ChannelKey), ActionError> {
        let cfg = self.cfg()?.clone();
        let id = ChannelId::derive(&self.proj.org.0, desc)?;
        let expected = channels::member_devices(&self.proj.auth, &cfg, desc, self.date())?;
        if !expected.contains(&self.keys.public()) {
            return Err(ActionError::Denied(
                "this device is not a member of the channel".into(),
            ));
        }
        let info = chain
            .channels
            .get(&id)
            .or_else(|| self.proj.channels.get(&id))
            .cloned();
        let identities: BTreeMap<DevicePublicKey, DeviceIdentity> = self
            .proj
            .auth
            .devices
            .iter()
            .map(|(k, d)| (*k, d.identity))
            .collect();
        let entries = |r: ChannelRef,
                       key: &ChannelKey,
                       to: &BTreeSet<DevicePublicKey>|
         -> Result<Vec<SealedKeyEntry>, ActionError> {
            to.iter()
                .map(|d| {
                    let ident = identities.get(d).ok_or_else(|| nf("device"))?;
                    Ok(SealedKeyEntry {
                        device: *d,
                        sealed_key: seal_channel_key(&ident.exchange, &r, key)?,
                    })
                })
                .collect()
        };
        let (kind, r, key, to) = match info
            .as_ref()
            .and_then(|i| i.current_epoch().map(|e| (i, e)))
        {
            None => (
                EventKind::ChannelKeysDistributed,
                ChannelRef { id, epoch: 0 },
                ChannelKey::generate()?,
                expected.clone(),
            ),
            Some((info, epoch)) => {
                let r = ChannelRef { id, epoch };
                let recipients = info.epochs.get(&epoch).cloned().unwrap_or_default();
                let someone_left = recipients.iter().any(|d| !expected.contains(d));
                let have = chain
                    .keyring
                    .current(&id)
                    .filter(|(cr, _)| cr.epoch == epoch)
                    .map(|(_, k)| k.clone());
                match have {
                    Some(key) if !someone_left => {
                        let missing: BTreeSet<DevicePublicKey> =
                            expected.difference(&recipients).copied().collect();
                        if missing.is_empty() {
                            return Ok((r, key));
                        }
                        (EventKind::ChannelKeysDistributed, r, key, missing)
                    }
                    _ => (
                        EventKind::ChannelRotated,
                        ChannelRef {
                            id,
                            epoch: epoch + 1,
                        },
                        ChannelKey::generate()?,
                        expected.clone(),
                    ),
                }
            }
        };
        let payload = KeyDistribution {
            descriptor: desc.clone(),
            channel: r,
            entries: entries(r, &key, &to)?,
        };
        self.public(chain, kind, &payload)?;
        let source = chain.events.last().map(|e| e.id.0).unwrap_or([0; 32]);
        chain.keyring.insert(r, source, key.clone());
        let entry = chain.channels.entry(id).or_insert_with(|| {
            info.clone().unwrap_or(ChannelInfo {
                descriptor: desc.clone(),
                epochs: BTreeMap::new(),
            })
        });
        entry.epochs.entry(r.epoch).or_default().extend(to);
        Ok((r, key))
    }

    fn sealed<T: Serialize>(
        &self,
        chain: &mut Chain,
        kind: EventKind,
        desc: &ChannelDescriptor,
        header: Option<ObjectHeader>,
        payload: &T,
    ) -> Result<(), ActionError> {
        self.sealed_with(chain, kind, desc, header, payload, Vec::new())
    }

    /// [`Self::sealed`] naming the blobs the payload refers to, so that
    /// the devices reading it fetch them.
    fn sealed_with<T: Serialize>(
        &self,
        chain: &mut Chain,
        kind: EventKind,
        desc: &ChannelDescriptor,
        header: Option<ObjectHeader>,
        payload: &T,
        attachments: Vec<[u8; 32]>,
    ) -> Result<(), ActionError> {
        let (r, key) = self.ensure_channel(chain, desc)?;
        let bytes = encode(payload).map_err(|_| ActionError::Encode)?;
        let mut d = self.draft(chain, kind);
        d.object = header;
        d.attachments = attachments.into_iter().map(core_events::BlobRef).collect();
        let e = d.sign_sealed(&bytes, r, &key, self.keys)?;
        Self::push(chain, e);
        Ok(())
    }

    /// Dry run: every new event must be valid for this device.
    fn finish(&self, chain: Chain) -> Result<Vec<SignedEvent>, ActionError> {
        let verdicts: Vec<Option<Verdict>> = match self.engine.and_then(|m| m.lock().ok()) {
            Some(mut engine) => engine
                .preview(&chain.events, &chain.keyring)
                .map_err(|e| ActionError::Engine(e.to_string()))?
                .into_iter()
                .map(Some)
                .collect(),
            None => {
                let mut all = self.events.to_vec();
                all.extend(chain.events.iter().cloned());
                let proj = rebuild(&all, &chain.keyring)
                    .map_err(|e| ActionError::Engine(e.to_string()))?;
                chain
                    .events
                    .iter()
                    .map(|e| proj.verdict(&e.id).cloned())
                    .collect()
            }
        };
        for (e, verdict) in chain.events.iter().zip(&verdicts) {
            match verdict {
                Some(Verdict::Valid) => {}
                Some(Verdict::Invalid(m)) => return Err(ActionError::Denied(m.clone())),
                other => {
                    return Err(ActionError::Engine(format!(
                        "{} would be {other:?}",
                        e.body.kind.as_str()
                    )));
                }
            }
        }
        Ok(chain.events)
    }

    fn org_event<T: Serialize>(
        &self,
        kind: EventKind,
        payload: &T,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let mut chain = self.chain();
        if kind.is_public() {
            self.public(&mut chain, kind, payload)?;
        } else {
            self.sealed(
                &mut chain,
                kind,
                &ChannelDescriptor::Organization,
                None,
                payload,
            )?;
        }
        self.finish(chain)
    }

    /// A person's event: in the organization channel, with the person in
    /// the clear header so that an erased copy (§14) still names them.
    fn person_event<T: Serialize>(
        &self,
        kind: EventKind,
        person: PersonId,
        payload: &T,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let mut chain = self.chain();
        let header = ObjectHeader {
            object: ObjectType::Person,
            id: person.0,
            config: None,
        };
        self.sealed(
            &mut chain,
            kind,
            &ChannelDescriptor::Organization,
            Some(header),
            payload,
        )?;
        self.finish(chain)
    }

    /// Background task (§8.3): for every channel whose current key this
    /// device has, distribute it to expected members still missing it (and
    /// rotate when a member left).
    pub fn complete_channels(&self) -> Result<Vec<SignedEvent>, ActionError> {
        let cfg = self.cfg()?.clone();
        let me = self.keys.public();
        let mut chain = self.chain();
        for (id, info) in &self.proj.channels {
            let Some(epoch) = info.current_epoch() else {
                continue;
            };
            if !self
                .keyring
                .current(id)
                .is_some_and(|(r, _)| r.epoch == epoch)
            {
                continue;
            }
            let Ok(expected) =
                channels::member_devices(&self.proj.auth, &cfg, &info.descriptor, self.date())
            else {
                continue;
            };
            let recipients = info.epochs.get(&epoch).cloned().unwrap_or_default();
            if !expected.contains(&me)
                || recipients == expected
                || expected.is_subset(&recipients) && recipients.is_subset(&expected)
            {
                continue;
            }
            self.ensure_channel(&mut chain, &info.descriptor)?;
        }
        if chain.events.is_empty() {
            return Ok(Vec::new());
        }
        self.finish(chain)
    }

    // ----- organization ----------------------------------------------------------

    pub fn add_person(
        &self,
        display_name: &str,
        attributes: Values,
    ) -> Result<(PersonId, Vec<SignedEvent>), ActionError> {
        let person = PersonId(new_id()?);
        let ev = self.person_event(
            EventKind::PersonAdded,
            person,
            &PersonAdded {
                person,
                display_name: display_name.to_owned(),
                attributes,
            },
        )?;
        Ok((person, ev))
    }

    /// Sets `attributes` and removes `cleared` ones.
    pub fn update_person(
        &self,
        person: PersonId,
        attributes: Values,
        cleared: Vec<Slug>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.person_event(
            EventKind::PersonUpdated,
            person,
            &PersonUpdated {
                person,
                display_name: None,
                attributes,
                cleared,
            },
        )
    }

    pub fn add_external(&self, display_name: &str) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let id = new_id()?;
        let ev = self.org_event(
            EventKind::ExternalPersonUpserted,
            &ExternalPersonUpserted {
                id,
                display_name: display_name.to_owned(),
            },
        )?;
        Ok((id, ev))
    }

    pub fn grant_qualification(
        &self,
        person: PersonId,
        q: &Slug,
        start: Date,
        end: Option<Date>,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let grant = new_id()?;
        let ev = self.org_event(
            EventKind::QualificationGranted,
            &QualificationGranted {
                grant,
                person,
                qualification: q.clone(),
                start,
                end,
            },
        )?;
        Ok((grant, ev))
    }

    pub fn end_qualification(&self, grant: Id, end: Date) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(EventKind::QualificationEnded, &GrantEnded { grant, end })
    }

    pub fn create_unit(
        &self,
        unit_type: &Slug,
        name: &str,
    ) -> Result<(UnitId, Vec<SignedEvent>), ActionError> {
        let unit = UnitId(new_id()?);
        let ev = self.org_event(
            EventKind::UnitCreated,
            &UnitCreated {
                unit,
                unit_type: unit_type.clone(),
                name: name.to_owned(),
            },
        )?;
        Ok((unit, ev))
    }

    pub fn add_membership(
        &self,
        person: PersonId,
        unit: UnitId,
        start: Date,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let membership = new_id()?;
        let ev = self.org_event(
            EventKind::MembershipStarted,
            &MembershipStarted {
                membership,
                person,
                unit,
                start,
            },
        )?;
        Ok((membership, ev))
    }

    pub fn end_membership(
        &self,
        membership: Id,
        end: Date,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(
            EventKind::MembershipEnded,
            &GrantEnded {
                grant: membership,
                end,
            },
        )
    }

    pub fn grant_role(
        &self,
        person: PersonId,
        role: &Slug,
        unit: Option<UnitId>,
        start: Date,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let grant = new_id()?;
        let ev = self.org_event(
            EventKind::RoleGranted,
            &RoleGranted {
                grant,
                person,
                role: role.clone(),
                unit,
                start,
                end: None,
            },
        )?;
        Ok((grant, ev))
    }

    pub fn end_role(&self, grant: Id, end: Date) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(EventKind::RoleEnded, &GrantEnded { grant, end })
    }

    pub fn add_device(
        &self,
        person: PersonId,
        device: DeviceIdentity,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(
            EventKind::DeviceAdded,
            &DeviceAdded {
                person,
                device,
                invite: None,
            },
        )
    }

    /// Asks every device to erase the data of a person (§14).
    pub fn request_erasure(&self, person: PersonId) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(
            EventKind::PersonDataErasureRequested,
            &ErasureRequested { person },
        )
    }

    /// Applies the retention periods of the configuration as of `before` (§14).
    pub fn apply_retention(&self, before: Date) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(EventKind::RetentionApplied, &RetentionApplied { before })
    }

    pub fn revoke_device(&self, device: DevicePublicKey) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(EventKind::DeviceRevoked, &DeviceRevoked { device })
    }

    pub fn rename_person(
        &self,
        person: PersonId,
        display_name: &str,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.person_event(
            EventKind::PersonUpdated,
            person,
            &PersonUpdated {
                person,
                display_name: Some(display_name.to_owned()),
                attributes: Values::new(),
                cleared: Vec::new(),
            },
        )
    }

    pub fn invite_device(
        &self,
        person: PersonId,
        invite: Id,
        expires_at: i64,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(
            EventKind::DeviceInvited,
            &core_events::payloads::DeviceInvited {
                invite,
                person,
                expires_at,
            },
        )
    }

    pub fn set_archive(
        &self,
        device: DevicePublicKey,
        archive: bool,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(
            EventKind::DeviceArchiveSet,
            &DeviceArchiveSet { device, archive },
        )
    }

    /// Declares (or removes) the organization's blind backup node.
    pub fn set_backup_node(
        &self,
        node: Option<DevicePublicKey>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(EventKind::OrgBackupNodeSet, &BackupNodeSet { node })
    }

    pub fn publish_config(
        &self,
        version: u32,
        bundle: Vec<u8>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.org_event(
            EventKind::ConfigPublished,
            &ConfigPublished { version, bundle },
        )
    }

    // ----- documents ---------------------------------------------------------------

    fn header(object: ObjectType, id: Id, config: &Slug) -> Option<ObjectHeader> {
        Some(ObjectHeader {
            object,
            id,
            config: Some(config.clone()),
        })
    }

    /// Fills (creates, or edits while in the initial state) a subject's document.
    pub fn fill_document(
        &self,
        form_id: &Slug,
        subject: PersonId,
        period: PeriodKey,
        values: Values,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let cfg = self.cfg()?.clone();
        let form = cfg.form(form_id).ok_or_else(|| nf("form"))?.clone();
        let wf = cfg
            .form_workflow(&form)
            .ok_or_else(|| nf("workflow"))?
            .clone();
        let mut chain = self.chain();
        if let Some((id, doc)) = self.proj.document_by(form_id, subject, period) {
            let desc = document_descriptor(&cfg, &form, &doc.content, &doc.state);
            self.sealed(
                &mut chain,
                EventKind::DocumentUpdated,
                &desc,
                Self::header(ObjectType::Document, *id, form_id),
                &DocumentUpdated {
                    document: *id,
                    values,
                },
            )?;
            return Ok((*id, self.finish(chain)?));
        }
        let at = reference_date(&period, self.date());
        let variant: VariantDef = variant_for(&self.proj.auth, &cfg, &form, subject, at)
            .ok_or_else(|| nf("variant"))?
            .clone();
        let unit = subject_unit(&self.proj.auth, &form, subject, at);
        let document = new_id()?;
        let content = DocumentContent {
            document,
            form: form_id.clone(),
            form_version: cfg.version,
            subject,
            period,
            variant: variant.id.clone(),
            unit,
            values,
            on_behalf: self.person()? != subject,
        };
        let desc = document_descriptor(&cfg, &form, &content, &wf.initial);
        self.sealed(
            &mut chain,
            EventKind::DocumentCreated,
            &desc,
            Self::header(ObjectType::Document, document, form_id),
            &content,
        )?;
        Ok((document, self.finish(chain)?))
    }

    /// State after a step, including an automatic collect.
    fn state_after(wf: &core_model::config::WorkflowDef, to: &Slug) -> Slug {
        wf.steps
            .iter()
            .find(|s| s.kind == StepKind::Collect && s.from.contains(to))
            .map_or_else(|| to.clone(), |c| c.to.clone())
    }

    /// Re-encrypts a document for its new audience when a step changes it (§8.5).
    fn distribute_if_needed(
        &self,
        chain: &mut Chain,
        cfg: &Cfg,
        doc: &crate::projection::Document,
        new_state: &Slug,
    ) -> Result<(), ActionError> {
        let form = cfg.form(&doc.content.form).ok_or_else(|| nf("form"))?;
        let desc = document_descriptor(cfg, form, &doc.content, new_state);
        if document_descriptor(cfg, form, &doc.content, &doc.state) == desc {
            return Ok(());
        }
        let snapshot = DocumentSnapshot {
            content: doc.content.clone(),
            state: new_state.clone(),
            version: doc.version,
        };
        self.sealed(
            chain,
            EventKind::DocumentDistributed,
            &desc,
            Self::header(
                ObjectType::Document,
                doc.content.document,
                &doc.content.form,
            ),
            &Distributed::Document(snapshot),
        )
    }

    pub fn step_document(
        &self,
        document: Id,
        step: &Slug,
        note: Option<String>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let doc = self
            .proj
            .documents
            .get(&document)
            .ok_or_else(|| nf("document"))?
            .clone();
        let cfg = self
            .proj
            .configs
            .get(doc.content.form_version)
            .ok_or_else(|| nf("configuration"))?
            .clone();
        let form = cfg
            .form(&doc.content.form)
            .ok_or_else(|| nf("form"))?
            .clone();
        let wf = cfg
            .form_workflow(&form)
            .ok_or_else(|| nf("workflow"))?
            .clone();
        let def = cfg.step(&wf, step).ok_or_else(|| nf("step"))?.clone();
        let mut chain = self.chain();
        let desc = document_descriptor(&cfg, &form, &doc.content, &doc.state);
        self.sealed(
            &mut chain,
            EventKind::DocumentStep,
            &desc,
            Self::header(ObjectType::Document, document, &form.id),
            &DocumentStep {
                document,
                step: step.clone(),
                note,
            },
        )?;
        self.distribute_if_needed(&mut chain, &cfg, &doc, &Self::state_after(&wf, &def.to))?;
        self.finish(chain)
    }

    /// Forward or register a batch: every known document in a starting state.
    pub fn batch_step(
        &self,
        form_id: &Slug,
        unit: UnitId,
        period: PeriodKey,
        step: &Slug,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let cfg = self.cfg()?.clone();
        let form = cfg.form(form_id).ok_or_else(|| nf("form"))?.clone();
        let wf = cfg
            .form_workflow(&form)
            .ok_or_else(|| nf("workflow"))?
            .clone();
        let def = cfg.step(&wf, step).ok_or_else(|| nf("step"))?.clone();
        let kind = match def.kind {
            StepKind::Forward => EventKind::BatchForwarded,
            StepKind::Register => EventKind::BatchRegistered,
            _ => return Err(ActionError::Denied("not a batch step".into())),
        };
        let docs: Vec<crate::projection::Document> = self
            .proj
            .documents
            .values()
            .filter(|d| {
                &d.content.form == form_id
                    && d.content.unit == Some(unit)
                    && d.content.period == period
                    && def.from.contains(&d.state)
            })
            .cloned()
            .collect();
        let batch = BatchKey {
            form: form_id.clone(),
            unit,
            period,
        };
        let payload = BatchStep {
            batch,
            step: step.clone(),
            documents: docs.iter().map(|d| d.content.document).collect(),
        };
        let mut chain = self.chain();
        let desc = batch_descriptor(&cfg, &form, unit, &def.to);
        self.sealed(
            &mut chain,
            kind,
            &desc,
            Self::header(ObjectType::Batch, unit.0, form_id),
            &payload,
        )?;
        let new_state = Self::state_after(&wf, &def.to);
        for d in &docs {
            self.distribute_if_needed(&mut chain, &cfg, d, &new_state)?;
        }
        self.finish(chain)
    }

    // ----- aggregates -----------------------------------------------------------

    pub fn generate_aggregate(
        &self,
        aggregate: &Slug,
        period: PeriodKey,
        note: Option<String>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let cfg = self.cfg()?.clone();
        let def = cfg
            .aggregate(aggregate)
            .ok_or_else(|| nf("aggregate"))?
            .clone();
        let form = cfg
            .form(&def.source.form)
            .ok_or_else(|| nf("form"))?
            .clone();
        let sources = crate::handle::aggregate_sources(self.proj, &def, &period);
        let result = compute_aggregate(&self.proj.auth, &cfg, &def, &form, &period, &sources, None);
        let documents = if def.include_individual_documents {
            sources
                .iter()
                .map(|(_, d)| DocumentSnapshot {
                    content: d.content.clone(),
                    state: d.state.clone(),
                    version: d.version,
                })
                .collect()
        } else {
            Vec::new()
        };
        let version = self
            .proj
            .aggregates
            .get(&(aggregate.clone(), period))
            .and_then(|a| a.generated.as_ref())
            .map_or(1, |g| g.version + 1);
        let snap = AggregateSnapshot {
            aggregate: aggregate.clone(),
            period,
            version,
            result,
            note,
            documents,
        };
        let mut chain = self.chain();
        let header = Self::header(
            ObjectType::Aggregate,
            Id::from_bytes(
                *blake3::hash(format!("{aggregate}|{period}").as_bytes())
                    .as_bytes()
                    .first_chunk::<16>()
                    .unwrap_or(&[0; 16]),
            ),
            aggregate,
        );
        self.sealed(
            &mut chain,
            EventKind::AggregateGenerated,
            &aggregate_draft_descriptor(aggregate),
            header,
            &snap,
        )?;
        self.finish(chain)
    }

    pub fn distribute_aggregate(
        &self,
        aggregate: &Slug,
        period: PeriodKey,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let cfg = self.cfg()?.clone();
        let def = cfg
            .aggregate(aggregate)
            .ok_or_else(|| nf("aggregate"))?
            .clone();
        let snap = self
            .proj
            .aggregates
            .get(&(aggregate.clone(), period))
            .and_then(|a| a.generated.clone())
            .ok_or_else(|| nf("generated aggregate"))?;
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::DocumentDistributed,
            &aggregate_descriptor(&def),
            None,
            &Distributed::Aggregate(snap),
        )?;
        self.finish(chain)
    }

    // ----- programs ----------------------------------------------------------------

    pub fn draft_program(
        &self,
        program_type: &Slug,
        occurrences: Vec<Date>,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let program = new_id()?;
        let cfg = self.cfg()?;
        let payload = ProgramDrafted {
            program,
            program_type: program_type.clone(),
            config_version: cfg.version,
            occurrences,
        };
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::ProgramDrafted,
            &program_draft_descriptor(program_type),
            Self::header(ObjectType::Program, program, program_type),
            &payload,
        )?;
        Ok((program, self.finish(chain)?))
    }

    pub fn assign(
        &self,
        program: Id,
        assignments: Vec<AssignmentSpec>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.update_program(program, assignments, Vec::new())
    }

    /// Sets some slots and empties others.
    pub fn update_program(
        &self,
        program: Id,
        assignments: Vec<AssignmentSpec>,
        cleared: Vec<SlotKey>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let p = self
            .proj
            .programs
            .get(&program)
            .ok_or_else(|| nf("program"))?;
        let mut chain = self.chain();
        let payload = ProgramUpdated {
            program,
            assignments,
            cleared,
        };
        self.sealed(
            &mut chain,
            EventKind::ProgramUpdated,
            &program_draft_descriptor(&p.program_type),
            Self::header(ObjectType::Program, program, &p.program_type),
            &payload,
        )?;
        self.finish(chain)
    }

    /// Shows or removes parts of a program for single events.
    pub fn show_program_parts(
        &self,
        program: Id,
        parts: Vec<(SlotKey, bool)>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let p = self
            .proj
            .programs
            .get(&program)
            .ok_or_else(|| nf("program"))?;
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::ProgramPartsShown,
            &program_draft_descriptor(&p.program_type),
            Self::header(ObjectType::Program, program, &p.program_type),
            &ProgramPartsShown { program, parts },
        )?;
        self.finish(chain)
    }

    /// Sets the details (extra fields) of slots, assigned or not.
    pub fn set_program_details(
        &self,
        program: Id,
        details: Vec<(SlotKey, Values)>,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let p = self
            .proj
            .programs
            .get(&program)
            .ok_or_else(|| nf("program"))?;
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::ProgramDetailsSet,
            &program_draft_descriptor(&p.program_type),
            Self::header(ObjectType::Program, program, &p.program_type),
            &ProgramDetailsSet { program, details },
        )?;
        self.finish(chain)
    }

    /// Sends the assignments: snapshot for the team and whoever may put it on the board, one notice per assignee.
    pub fn send_assignments(&self, program: Id) -> Result<Vec<SignedEvent>, ActionError> {
        let p = self
            .proj
            .programs
            .get(&program)
            .ok_or_else(|| nf("program"))?
            .clone();
        let snapshot_version = p.sent_version + 1;
        let snapshot = program_snapshot(&self.proj.auth, program, &p, snapshot_version);
        let mut chain = self.chain();
        let header = Self::header(ObjectType::Program, program, &p.program_type);
        self.sealed(
            &mut chain,
            EventKind::ProgramAssignmentsSent,
            &program_assigned_descriptor(&p.program_type),
            header.clone(),
            &AssignmentsSent::Team { program, snapshot },
        )?;
        let by_person = notice_items(&p).ok_or_else(|| nf("occurrence"))?;
        let at_publish = self
            .cfg()?
            .program_type(&p.program_type)
            .is_some_and(|d| d.notify == NotifyOn::Publish);
        if at_publish {
            // The assignees hear of it when the program is published.
            self.sealed(
                &mut chain,
                EventKind::ProgramAssignmentsSent,
                &program_assigned_descriptor(&p.program_type),
                header,
                &AssignmentsSent::Roster {
                    program,
                    version: snapshot_version,
                    persons: by_person.into_iter().collect(),
                },
            )?;
            return self.finish(chain);
        }
        for (person, items) in by_person {
            let desc = assignment_descriptor(&p.program_type, &Assignee::Person(person));
            let notice = AssignmentsSent::Notice {
                program,
                program_type: p.program_type.clone(),
                person,
                items,
            };
            self.sealed(
                &mut chain,
                EventKind::ProgramAssignmentsSent,
                &desc,
                header.clone(),
                &notice,
            )?;
        }
        self.finish(chain)
    }

    pub fn respond(
        &self,
        program: Id,
        program_type: &Slug,
        key: SlotKey,
        assignee: Assignee,
        response: Response,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        self.respond_all(program, program_type, vec![key], assignee, response)
    }

    /// The same answer for several slots (the events of a rotation).
    pub fn respond_all(
        &self,
        program: Id,
        program_type: &Slug,
        keys: Vec<SlotKey>,
        assignee: Assignee,
        response: Response,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let mut chain = self.chain();
        for key in keys {
            let payload = AssignmentResponded {
                program,
                program_type: program_type.clone(),
                key,
                assignee: assignee.clone(),
                response: response.clone(),
            };
            self.sealed(
                &mut chain,
                EventKind::AssignmentResponded,
                &assignment_descriptor(program_type, &assignee),
                Self::header(ObjectType::Assignment, program, program_type),
                &payload,
            )?;
        }
        self.finish(chain)
    }

    /// Publishes (or revises) the assigned version of a program on the board.
    pub fn publish_program(&self, program: Id) -> Result<Vec<SignedEvent>, ActionError> {
        let snap = self
            .proj
            .assigned_snapshots
            .get(&program)
            .ok_or_else(|| nf("assigned program"))?
            .clone();
        let cfg = self.cfg()?;
        let def = cfg
            .program_type(&snap.program_type)
            .ok_or_else(|| nf("program type"))?;
        let kind = if self.proj.published.contains_key(&program) {
            EventKind::ProgramRevised
        } else {
            EventKind::ProgramPublished
        };
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            kind,
            &published_descriptor(&def.publish_audience),
            Self::header(ObjectType::Program, program, &snap.program_type),
            &snap,
        )?;
        if def.notify == NotifyOn::Publish {
            self.publish_notices(&mut chain, program, &snap.program_type)?;
        }
        self.finish(chain)
    }

    /// The notices of a program notified at publication: one for every
    /// assignee whose slots changed since what they were told, an empty one
    /// for who no longer holds any.
    fn publish_notices(
        &self,
        chain: &mut Chain,
        program: Id,
        program_type: &Slug,
    ) -> Result<(), ActionError> {
        let Some((_, roster)) = self.proj.rosters.get(&program) else {
            return Ok(());
        };
        let mut due: BTreeMap<PersonId, Vec<NoticeItem>> = self
            .proj
            .notices
            .keys()
            .filter(|(p, _)| *p == program)
            .map(|(_, person)| (*person, Vec::new()))
            .collect();
        due.extend(roster.iter().cloned());
        let header = Self::header(ObjectType::Program, program, program_type);
        for (person, items) in due {
            let told = self
                .proj
                .notices
                .get(&(program, person))
                .map(|(_, i)| i.as_slice());
            if told == Some(items.as_slice()) || (told.is_none() && items.is_empty()) {
                continue;
            }
            self.sealed(
                chain,
                EventKind::ProgramAssignmentsSent,
                &assignment_descriptor(program_type, &Assignee::Person(person)),
                header.clone(),
                &AssignmentsSent::Notice {
                    program,
                    program_type: program_type.clone(),
                    person,
                    items,
                },
            )?;
        }
        Ok(())
    }

    // ----- resources ----------------------------------------------------------------

    pub fn upsert_resource(
        &self,
        resource: Option<Id>,
        resource_type: &Slug,
        attributes: Values,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let resource = match resource {
            Some(r) => r,
            None => new_id()?,
        };
        let blobs = self.resource_attachments(resource_type, &attributes)?;
        let mut chain = self.chain();
        self.sealed_with(
            &mut chain,
            EventKind::ResourceUpserted,
            &resource_descriptor(resource_type, None),
            Self::header(ObjectType::Resource, resource, resource_type),
            &ResourceUpserted {
                resource,
                resource_type: resource_type.clone(),
                attributes,
            },
            blobs,
        )?;
        Ok((resource, self.finish(chain)?))
    }

    fn resource_attachments(
        &self,
        resource_type: &Slug,
        attributes: &Values,
    ) -> Result<Vec<[u8; 32]>, ActionError> {
        let def = self
            .cfg()?
            .resource_type(resource_type)
            .ok_or_else(|| nf("resource type"))?;
        Ok(crate::values::attachments(&def.attributes, attributes))
    }

    fn resource(&self, id: Id) -> Result<&crate::projection::Resource, ActionError> {
        self.proj.resources.get(&id).ok_or_else(|| nf("resource"))
    }

    pub fn assign_resource(
        &self,
        resource: Id,
        holder: PersonId,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let r = self.resource(resource)?;
        let payload = ResourceAssigned {
            resource,
            resource_type: r.resource_type.clone(),
            holder,
            date: self.date(),
            attributes: r.attributes.clone(),
        };
        // The holder's copy names the pictures too: their devices fetch them.
        let blobs = self.resource_attachments(&r.resource_type, &r.attributes)?;
        let mut chain = self.chain();
        self.sealed_with(
            &mut chain,
            EventKind::ResourceAssigned,
            &resource_descriptor(&r.resource_type, Some(holder)),
            Self::header(ObjectType::Resource, resource, &r.resource_type),
            &payload,
            blobs,
        )?;
        self.finish(chain)
    }

    fn held(&self, resource: Id) -> Result<(Slug, PersonId), ActionError> {
        let r = self.resource(resource)?;
        let h = r.holding.as_ref().ok_or_else(|| nf("holder"))?;
        Ok((r.resource_type.clone(), h.holder))
    }

    pub fn open_request(
        &self,
        resource: Id,
        request_type: &Slug,
        due: Date,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let (resource_type, holder) = self.held(resource)?;
        let request = new_id()?;
        let payload = ResourceRequestOpened {
            request,
            resource,
            resource_type: resource_type.clone(),
            holder,
            request_type: request_type.clone(),
            due,
        };
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::ResourceRequestOpened,
            &resource_descriptor(&resource_type, Some(holder)),
            Self::header(ObjectType::Resource, resource, &resource_type),
            &payload,
        )?;
        Ok((request, self.finish(chain)?))
    }

    pub fn answer_request(
        &self,
        request: Id,
        answer: &Slug,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let req = self
            .proj
            .requests
            .get(&request)
            .ok_or_else(|| nf("request"))?;
        let payload = ResourceRequestAnswered {
            request,
            resource: req.resource,
            resource_type: req.resource_type.clone(),
            holder: req.holder,
            answer: answer.clone(),
        };
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::ResourceRequestAnswered,
            &resource_descriptor(&req.resource_type, Some(req.holder)),
            Self::header(ObjectType::Resource, req.resource, &req.resource_type),
            &payload,
        )?;
        self.finish(chain)
    }

    pub fn log_resource(
        &self,
        resource: Id,
        values: Values,
    ) -> Result<Vec<SignedEvent>, ActionError> {
        let (resource_type, holder) = self.held(resource)?;
        let payload = ResourceLogged {
            resource,
            resource_type: resource_type.clone(),
            holder,
            date: self.date(),
            values,
        };
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::ResourceLogged,
            &resource_descriptor(&resource_type, Some(holder)),
            Self::header(ObjectType::Resource, resource, &resource_type),
            &payload,
        )?;
        self.finish(chain)
    }

    pub fn return_resource(&self, resource: Id) -> Result<Vec<SignedEvent>, ActionError> {
        let (resource_type, holder) = self.held(resource)?;
        let payload = ResourceReturned {
            resource,
            resource_type: resource_type.clone(),
            holder,
            date: self.date(),
        };
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::ResourceReturned,
            &resource_descriptor(&resource_type, Some(holder)),
            Self::header(ObjectType::Resource, resource, &resource_type),
            &payload,
        )?;
        self.finish(chain)
    }

    // ----- board --------------------------------------------------------------------

    pub fn post_notice(
        &self,
        audience: &Slug,
        title: &str,
        body: &str,
        from: Date,
        until: Date,
    ) -> Result<(Id, Vec<SignedEvent>), ActionError> {
        let post = new_id()?;
        let payload = BoardPosted {
            post,
            audience: audience.clone(),
            title: title.to_owned(),
            body: body.to_owned(),
            from,
            until,
        };
        let mut chain = self.chain();
        self.sealed(
            &mut chain,
            EventKind::BoardPosted,
            &published_descriptor(audience),
            Some(ObjectHeader {
                object: ObjectType::BoardPost,
                id: post,
                config: None,
            }),
            &payload,
        )?;
        Ok((post, self.finish(chain)?))
    }
}
