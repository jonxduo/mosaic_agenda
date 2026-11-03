//! Expected members of channels and key distribution (§8.3, §8.4).

use std::collections::{BTreeMap, BTreeSet};

use core_crypto::distribution::open_channel_key;
use core_crypto::{
    ChannelDescriptor, ChannelId, ChannelKeyring, ChannelScope, DeviceKeys, DevicePublicKey,
};
use core_events::payloads::KeyDistribution;
use core_events::{EventKind, OrgId, SignedEvent};
use core_model::config::NotifyOn;
use core_model::eval::{EvalContext, Evaluator};
use core_model::strings::{ObjectKind, ObjectRef};
use core_model::{Permission, PersonId, Slug};
use serde::Serialize;
use time::Date;

use crate::auth::{AuthState, Denied, deny};
use crate::config::Cfg;

/// Program channel stages.
pub const STAGE_DRAFT: &str = "draft";
pub const STAGE_ASSIGNED: &str = "assigned";

/// Recipients of each epoch of a channel, as distributed so far.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ChannelInfo {
    pub descriptor: ChannelDescriptor,
    pub epochs: BTreeMap<u32, BTreeSet<DevicePublicKey>>,
}

impl ChannelInfo {
    pub fn current_epoch(&self) -> Option<u32> {
        self.epochs.keys().next_back().copied()
    }
}

pub fn program_object(program_type: &Slug) -> ObjectRef {
    ObjectRef {
        kind: ObjectKind::Program,
        id: program_type.clone(),
    }
}

/// Persons expected in a channel at `date`.
pub fn members(
    auth: &AuthState,
    cfg: &Cfg,
    desc: &ChannelDescriptor,
    date: Date,
) -> Result<BTreeSet<PersonId>, Denied> {
    let ev = Evaluator::new(cfg.index(), &auth.org);
    let resolve = |audience: &Slug, ctx: &EvalContext| {
        ev.resolve(audience, ctx).map_err(|e| Denied(e.to_string()))
    };
    let unit_of = |scope: &ChannelScope| match scope {
        ChannelScope::Organization => None,
        ChannelScope::Unit(u) => Some(*u),
    };
    let mut out = match desc {
        ChannelDescriptor::Organization => {
            let mut out: BTreeSet<PersonId> = auth.org.active_persons().collect();
            out.extend(auth.org.root_admins.iter().copied());
            out
        }
        ChannelDescriptor::Admin => auth.org.root_admins.clone(),
        ChannelDescriptor::Audience { audience, scope } => {
            let mut ctx = EvalContext::at(date);
            ctx.subject_unit = unit_of(scope);
            resolve(audience, &ctx)?
        }
        ChannelDescriptor::Subject {
            audience,
            subject,
            scope,
        } => {
            // Personal channels: the read audience plus the subject (§8.2).
            let ctx = EvalContext::at(date).with_subject(*subject, unit_of(scope));
            let mut out = resolve(audience, &ctx)?;
            out.insert(*subject);
            out
        }
        ChannelDescriptor::Program {
            program_type,
            stage,
        } => {
            let mut out = program_team(auth, cfg, program_type, date)?;
            if stage.as_str() == STAGE_ASSIGNED {
                let obj = program_object(program_type);
                out.extend(auth.org.active_persons().filter(|p| {
                    auth.is_allowed(cfg, *p, Permission::ProgramPublish, Some(&obj), None, date)
                }));
            }
            out
        }
        ChannelDescriptor::Assignment {
            program_type,
            person,
        } => {
            let mut out = program_team(auth, cfg, program_type, date)?;
            // Notified at publication: who publishes writes the notice.
            if cfg
                .program_type(program_type)
                .is_some_and(|d| d.notify == NotifyOn::Publish)
            {
                let obj = program_object(program_type);
                out.extend(auth.org.active_persons().filter(|p| {
                    auth.is_allowed(cfg, *p, Permission::ProgramPublish, Some(&obj), None, date)
                }));
            }
            out.insert(*person);
            out
        }
        ChannelDescriptor::Resource {
            resource_type,
            holder,
        } => {
            let def = cfg
                .resource_type(resource_type)
                .ok_or_else(|| Denied("unknown resource type".into()))?;
            let mut roles = vec![&def.manager_role];
            roles.extend(def.assistant_roles.iter());
            let mut out = auth.holders_of(&roles, date);
            out.extend(holder.iter().copied());
            out
        }
        ChannelDescriptor::PermissionHolders { permission, object } => {
            let perm =
                Permission::parse(permission).ok_or_else(|| Denied("unknown permission".into()))?;
            let obj: ObjectRef = object.parse().map_err(|_| Denied("bad object".into()))?;
            auth.org
                .active_persons()
                .filter(|p| auth.is_allowed(cfg, *p, perm, Some(&obj), None, date))
                .collect()
        }
    };
    // The administrators can do everything, so they read every channel:
    // without its keys a permission would be of no use (DECISIONS.md).
    out.extend(auth.org.root_admins.iter().copied());
    Ok(out)
}

fn program_team(
    auth: &AuthState,
    cfg: &Cfg,
    program_type: &Slug,
    date: Date,
) -> Result<BTreeSet<PersonId>, Denied> {
    let def = cfg
        .program_type(program_type)
        .ok_or_else(|| Denied("unknown program type".into()))?;
    let mut roles = vec![&def.owner_role];
    roles.extend(def.assistant_roles.iter());
    Ok(auth.holders_of(&roles, date))
}

/// Non-revoked devices of the expected members.
pub fn member_devices(
    auth: &AuthState,
    cfg: &Cfg,
    desc: &ChannelDescriptor,
    date: Date,
) -> Result<BTreeSet<DevicePublicKey>, Denied> {
    let persons = members(auth, cfg, desc, date)?;
    Ok(auth
        .devices
        .iter()
        .filter(|(_, d)| !d.revoked && persons.contains(&d.person))
        .map(|(k, _)| *k)
        .collect())
}

/// Checks a `channel.keys_distributed` / `channel.rotated` event.
#[allow(clippy::too_many_arguments)] // each input is a distinct fact of the check
pub fn check_distribution(
    org: OrgId,
    auth: &AuthState,
    cfg: &Cfg,
    author: PersonId,
    date: Date,
    kind: EventKind,
    d: &KeyDistribution,
    known: Option<&ChannelInfo>,
) -> Result<(), Denied> {
    let id = ChannelId::derive(&org.0, &d.descriptor).map_err(|e| Denied(e.to_string()))?;
    if id != d.channel.id {
        return deny("channel id does not match its descriptor");
    }
    let persons = members(auth, cfg, &d.descriptor, date)?;
    if !persons.contains(&author) {
        return deny("only members of a channel can distribute its keys");
    }
    let devices = member_devices(auth, cfg, &d.descriptor, date)?;
    if let Some(e) = d.entries.iter().find(|e| !devices.contains(&e.device)) {
        return deny(format!(
            "device {} is not a member of the channel",
            e.device
        ));
    }
    let current = known.and_then(ChannelInfo::current_epoch);
    match kind {
        EventKind::ChannelKeysDistributed => {
            let ok = match current {
                None => d.channel.epoch == 0,
                Some(_) => known.is_some_and(|k| k.epochs.contains_key(&d.channel.epoch)),
            };
            if !ok {
                return deny("keys can only be added to an existing epoch (or epoch 0)");
            }
        }
        EventKind::ChannelRotated => {
            // Concurrent rotations to the same new epoch are allowed.
            let ok = match current {
                None => d.channel.epoch == 0,
                Some(c) => d.channel.epoch == c + 1 || (d.channel.epoch == c && c > 0),
            };
            if !ok {
                return deny("a rotation must open the next epoch");
            }
        }
        _ => return deny("not a key distribution"),
    }
    Ok(())
}

/// Keys a device can open from the distribution events it has.
pub fn derive_keyring(events: &[SignedEvent], keys: &DeviceKeys) -> ChannelKeyring {
    let mut ring = ChannelKeyring::new();
    absorb_keys(&mut ring, events, keys);
    ring
}

/// Adds to `ring` the keys this device can open from `events`.
pub fn absorb_keys(ring: &mut ChannelKeyring, events: &[SignedEvent], keys: &DeviceKeys) {
    let me = keys.public();
    for e in events {
        if !matches!(
            e.body.kind,
            EventKind::ChannelKeysDistributed | EventKind::ChannelRotated
        ) {
            continue;
        }
        let Ok(d) = e.public_content::<KeyDistribution>() else {
            continue;
        };
        for entry in d.entries.iter().filter(|x| x.device == me) {
            if let Ok(k) = open_channel_key(keys, &d.channel, &entry.sealed_key) {
                ring.insert(d.channel, e.id.0, k);
            }
        }
    }
}

pub fn audience_desc(audience: &Slug, scope: ChannelScope) -> ChannelDescriptor {
    ChannelDescriptor::Audience {
        audience: audience.clone(),
        scope,
    }
}

pub fn program_desc(program_type: &Slug, stage: &'static str) -> ChannelDescriptor {
    ChannelDescriptor::Program {
        program_type: program_type.clone(),
        stage: Slug::lit(stage),
    }
}
