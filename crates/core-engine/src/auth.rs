//! Authorization state and the checks of organizational events (§5.3, §5.5, §7.6).
//!
//! An [`AuthState`] is the fold of the valid organizational events of some
//! set (the causal past of an event, or everything known). Each
//! organizational event is checked against the state of its own causal past.

use std::collections::{BTreeMap, BTreeSet};

use core_crypto::{DeviceIdentity, DevicePublicKey};
use core_events::EventHash;
use core_model::config::{MembershipKind, PeriodUnit, QualificationDuration};
use core_model::eval::{EvalContext, Evaluator};
use core_model::state::{
    GrantScope, Membership, OrgState, Period, PersonRecord, QualificationGrant, RoleGrant,
    UnitRecord,
};
use core_model::strings::{ObjectRef, ScopeDef};
use core_model::{Id, Permission, PermissionTarget, PersonId, Slug, UnitId};
use serde::Serialize;
use time::Date;

use crate::config::Cfg;
use crate::ops::Op;
use crate::time::add_days;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DeviceRecord {
    pub person: PersonId,
    pub identity: DeviceIdentity,
    pub revoked: bool,
    pub archive: bool,
    /// Event that revoked the device.
    pub revoked_by: Option<EventHash>,
}

/// Who may do what, as of some set of events.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct AuthState {
    pub org: OrgState,
    pub devices: BTreeMap<DevicePublicKey, DeviceRecord>,
    pub externals: BTreeMap<Id, String>,
    pub config_version: u32,
    /// Grant id → index in `org.qualification_grants` / `memberships` / `role_grants`.
    pub qualification_ids: BTreeMap<Id, usize>,
    pub membership_ids: BTreeMap<Id, usize>,
    pub role_ids: BTreeMap<Id, (usize, EventHash)>,
    /// Role grants dropped because concurrent grants exceeded `holders.max`.
    pub superseded_roles: BTreeSet<EventHash>,
}

/// Why an action is not allowed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct Denied(pub String);

pub fn deny<T>(msg: impl Into<String>) -> Result<T, Denied> {
    Err(Denied(msg.into()))
}

fn ensure(cond: bool, msg: &str) -> Result<(), Denied> {
    if cond { Ok(()) } else { deny(msg) }
}

impl AuthState {
    pub fn is_root(&self, person: PersonId) -> bool {
        self.org.root_admins.contains(&person)
    }

    /// Person owning a non-revoked device.
    pub fn author(&self, device: &DevicePublicKey) -> Result<PersonId, Denied> {
        match self.devices.get(device) {
            None => deny("unknown device"),
            Some(d) if d.revoked => deny("revoked device"),
            Some(d) => Ok(d.person),
        }
    }

    pub fn person_exists(&self, p: PersonId) -> bool {
        self.org.persons.get(&p).is_some_and(|r| r.active)
    }

    /// Non-revoked devices of a person.
    pub fn devices_of(&self, person: PersonId) -> impl Iterator<Item = &DeviceRecord> + '_ {
        self.devices
            .values()
            .filter(move |d| d.person == person && !d.revoked)
    }

    /// The core permission check (§5.5).
    pub fn is_allowed(
        &self,
        cfg: &Cfg,
        person: PersonId,
        permission: Permission,
        object: Option<&ObjectRef>,
        unit: Option<UnitId>,
        date: Date,
    ) -> bool {
        if self.is_root(person) {
            return true;
        }
        self.org
            .active_role_grants(date)
            .filter(|g| g.person == person)
            .any(|g| {
                let Some(role) = cfg.role(&g.role) else {
                    return false;
                };
                role.permissions.iter().any(|pg| {
                    if pg.permission != permission {
                        return false;
                    }
                    if let (Some(on), Some(obj)) = (&pg.on, object) {
                        if on != obj {
                            return false;
                        }
                    }
                    if pg.on.is_some() && object.is_none() {
                        return false;
                    }
                    match (pg.target.unwrap_or(PermissionTarget::OwnScope), g.scope) {
                        (PermissionTarget::Organization, _) => true,
                        (PermissionTarget::OwnScope, GrantScope::Organization) => true,
                        (
                            PermissionTarget::OwnScope | PermissionTarget::SubjectUnit,
                            GrantScope::Unit(u),
                        ) => unit == Some(u),
                        (PermissionTarget::SubjectUnit, GrantScope::Organization) => false,
                    }
                })
            })
    }

    pub fn require(
        &self,
        cfg: &Cfg,
        person: PersonId,
        permission: Permission,
        object: Option<&ObjectRef>,
        unit: Option<UnitId>,
        date: Date,
    ) -> Result<(), Denied> {
        if self.is_allowed(cfg, person, permission, object, unit, date) {
            Ok(())
        } else {
            deny(format!("permission {} required", permission.as_str()))
        }
    }

    /// Highest role priority of a person at a date (conflict resolution).
    pub fn priority(&self, cfg: &Cfg, person: PersonId, date: Date) -> i32 {
        self.org
            .active_role_grants(date)
            .filter(|g| g.person == person)
            .filter_map(|g| cfg.role(&g.role).map(|r| r.priority))
            .max()
            .unwrap_or(0)
    }

    fn evaluator<'a>(&'a self, cfg: &'a Cfg) -> Evaluator<'a> {
        Evaluator::new(cfg.index(), &self.org)
    }

    /// True if `person` satisfies `cond` at `date` (unit context optional).
    pub fn satisfies(
        &self,
        cfg: &Cfg,
        person: PersonId,
        cond: Option<&core_model::Cond>,
        date: Date,
    ) -> bool {
        match cond {
            None => true,
            Some(c) => self
                .evaluator(cfg)
                .eval(c, Some(person), &EvalContext::at(date)),
        }
    }

    /// Persons holding any of the roles (anywhere) at a date.
    pub fn holders_of(&self, roles: &[&Slug], date: Date) -> BTreeSet<PersonId> {
        self.org
            .active_role_grants(date)
            .filter(|g| roles.contains(&&g.role))
            .map(|g| g.person)
            .filter(|p| self.person_exists(*p))
            .collect()
    }

    // ----- checks -----------------------------------------------------------

    /// Checks an organizational operation by `author` at `date`.
    pub fn check(&self, cfg: &Cfg, author: PersonId, date: Date, op: &Op) -> Result<(), Denied> {
        use Permission as P;
        let req = |p: Permission| self.require(cfg, author, p, None, None, date);
        match op {
            Op::Genesis(_) => deny("only one genesis"),
            Op::OrgSettingsChanged(_) => ensure(self.is_root(author), "root administrators only"),
            Op::ConfigPublished(c) => {
                req(P::ConfigPublish)?;
                ensure(
                    c.version > self.config_version,
                    "configuration version must increase",
                )?;
                let next = core_config::decode(&c.bundle)
                    .map_err(|e| Denied(format!("configuration bundle: {e}")))?;
                // Removed elements must be declared as renamed or retired (§6.6).
                match core_config::check_migrations(&cfg.compiled.package, &next.package)
                    .into_iter()
                    .next()
                {
                    Some(issue) => deny(format!("configuration: {}", issue.message)),
                    None => Ok(()),
                }
            }
            Op::DeviceInvited(d) => {
                if d.person != author {
                    req(P::DevicesManage)?;
                }
                Ok(())
            }
            Op::DeviceAdded(d) => {
                if d.person != author {
                    req(P::DevicesManage)?;
                }
                ensure(
                    self.person_exists(d.person) || self.is_root(d.person),
                    "unknown person",
                )?;
                ensure(
                    !self.devices.contains_key(&d.device.signing),
                    "device already added",
                )
            }
            Op::DeviceRevoked(d) => {
                let rec = self
                    .devices
                    .get(&d.device)
                    .ok_or_else(|| Denied("unknown device".into()))?;
                if rec.person != author {
                    req(P::DevicesManage)?;
                }
                ensure(!rec.revoked, "device already revoked")
            }
            Op::DeviceArchiveSet(d) => {
                req(P::DevicesManage)?;
                ensure(self.devices.contains_key(&d.device), "unknown device")
            }
            Op::PersonAdded(p) => {
                req(P::PeopleManage)?;
                ensure(
                    !self.org.persons.contains_key(&p.person),
                    "person already exists",
                )?;
                self.check_attributes(cfg, &p.attributes)
            }
            Op::PersonUpdated(p) => {
                // Their own name and the attributes marked `self_edit`: anyone.
                let own = author == p.person
                    && p.attributes.keys().chain(p.cleared.iter()).all(|k| {
                        cfg.package()
                            .manifest
                            .person_attributes
                            .iter()
                            .any(|a| &a.id == k && a.self_edit)
                    });
                if !own {
                    req(P::PeopleManage)?;
                }
                ensure(self.org.persons.contains_key(&p.person), "unknown person")?;
                self.check_attributes(cfg, &p.attributes)
            }
            Op::PersonRemoved(p) => {
                req(P::PeopleManage)?;
                ensure(self.person_exists(p.person), "unknown person")
            }
            Op::ExternalPersonUpserted(_) => {
                let any_program = cfg.package().programs.program_types.iter().any(|t| {
                    let obj = ObjectRef {
                        kind: core_model::strings::ObjectKind::Program,
                        id: t.id.clone(),
                    };
                    self.is_allowed(cfg, author, P::ProgramAssign, Some(&obj), None, date)
                });
                ensure(
                    any_program || self.is_allowed(cfg, author, P::PeopleManage, None, None, date),
                    "permission people.manage required",
                )
            }
            Op::QualificationGranted(g) => {
                let own = author == g.person
                    && cfg
                        .qualification(&g.qualification)
                        .is_some_and(|d| d.self_service);
                if !own {
                    req(P::QualificationsManage)?;
                }
                self.check_qualification_grant(cfg, g)
            }
            Op::QualificationEnded(e) => {
                let i = *self
                    .qualification_ids
                    .get(&e.grant)
                    .ok_or_else(|| Denied("unknown grant".into()))?;
                let own = self.org.qualification_grants.get(i).is_some_and(|g| {
                    g.person == author
                        && cfg
                            .qualification(&g.qualification)
                            .is_some_and(|d| d.self_service)
                });
                if !own {
                    req(P::QualificationsManage)?;
                }
                ensure(
                    self.org
                        .qualification_grants
                        .get(i)
                        .is_some_and(|g| e.end >= g.period.start),
                    "end before start",
                )
            }
            Op::UnitCreated(u) => {
                req(P::UnitsManage)?;
                ensure(cfg.unit_type(&u.unit_type).is_some(), "unknown unit type")?;
                ensure(!self.org.units.contains_key(&u.unit), "unit already exists")
            }
            Op::UnitUpdated(u) => {
                req(P::UnitsManage)?;
                ensure(self.org.units.contains_key(&u.unit), "unknown unit")
            }
            Op::UnitClosed(u) => {
                req(P::UnitsManage)?;
                ensure(
                    self.org.units.get(&u.unit).is_some_and(|r| r.active),
                    "unknown unit",
                )
            }
            Op::MembershipStarted(m) => {
                req(P::UnitsManage)?;
                self.check_membership(cfg, m)
            }
            Op::MembershipEnded(e) => {
                req(P::UnitsManage)?;
                ensure(
                    self.membership_ids.contains_key(&e.grant),
                    "unknown membership",
                )
            }
            Op::RoleGranted(g) => {
                req(P::RolesGrant)?;
                self.check_role_grant(cfg, g)
            }
            Op::RoleEnded(e) => {
                req(P::RolesGrant)?;
                ensure(self.role_ids.contains_key(&e.grant), "unknown role grant")
            }
            _ => deny("not an organizational operation"),
        }
    }

    fn check_attributes(&self, cfg: &Cfg, attrs: &crate::ops::Values) -> Result<(), Denied> {
        let declared: BTreeSet<&Slug> = cfg
            .package()
            .manifest
            .person_attributes
            .iter()
            .map(|a| &a.id)
            .collect();
        for k in attrs.keys() {
            ensure(
                declared.contains(k),
                &format!("unknown person attribute `{k}`"),
            )?;
        }
        Ok(())
    }

    fn check_qualification_grant(
        &self,
        cfg: &Cfg,
        g: &crate::ops::QualificationGranted,
    ) -> Result<(), Denied> {
        let def = cfg
            .qualification(&g.qualification)
            .ok_or_else(|| Denied("unknown qualification".into()))?;
        ensure(self.person_exists(g.person), "unknown person")?;
        ensure(
            !self.qualification_ids.contains_key(&g.grant),
            "grant id already used",
        )?;
        if let Some(end) = g.end {
            ensure(end > g.start, "a grant must end after it starts")?;
        }
        if def.duration == QualificationDuration::FixedPeriod {
            let expected_end = fixed_end(g.start, def.fixed_period_unit);
            ensure(
                g.end.is_some() && g.end == expected_end,
                "fixed-period qualifications last exactly one period",
            )?;
        }
        ensure(
            self.satisfies(cfg, g.person, def.requires.as_ref(), g.start),
            "the person does not meet the requirements of this qualification",
        )?;
        let period = Period {
            start: g.start,
            end: g.end,
        };
        let new_closure = cfg
            .index()
            .implies_closure
            .get(&g.qualification)
            .cloned()
            .unwrap_or_default();
        let excludes_of = |q: &Slug| -> BTreeSet<Slug> {
            let mut out: BTreeSet<Slug> = cfg
                .qualification(q)
                .map(|d| d.excludes.iter().cloned().collect())
                .unwrap_or_default();
            for d in &cfg.package().qualifications.qualifications {
                if d.excludes.contains(q) {
                    out.insert(d.id.clone());
                }
            }
            out
        };
        for other in self
            .org
            .qualification_grants
            .iter()
            .filter(|x| x.person == g.person && x.period.overlaps(&period))
        {
            let other_closure = cfg
                .index()
                .implies_closure
                .get(&other.qualification)
                .cloned()
                .unwrap_or_default();
            for a in &new_closure {
                let ex = excludes_of(a);
                if other_closure.iter().any(|b| ex.contains(b)) {
                    return deny(format!(
                        "`{}` is incompatible with `{}`",
                        g.qualification, other.qualification
                    ));
                }
            }
        }
        Ok(())
    }

    fn check_membership(&self, cfg: &Cfg, m: &crate::ops::MembershipStarted) -> Result<(), Denied> {
        ensure(self.person_exists(m.person), "unknown person")?;
        let unit = self
            .org
            .units
            .get(&m.unit)
            .filter(|u| u.active)
            .ok_or_else(|| Denied("unknown unit".into()))?;
        let ut = cfg
            .unit_type(&unit.unit_type)
            .ok_or_else(|| Denied("unknown unit type".into()))?;
        ensure(
            !self.membership_ids.contains_key(&m.membership),
            "membership id already used",
        )?;
        ensure(
            self.satisfies(cfg, m.person, ut.member_requires.as_ref(), m.start),
            "the person cannot be a member of this unit",
        )?;
        if ut.membership == MembershipKind::Exclusive {
            let period = Period::from(m.start);
            let clash = self.org.memberships.iter().any(|x| {
                x.person == m.person
                    && x.period.overlaps(&period)
                    && self
                        .org
                        .units
                        .get(&x.unit)
                        .is_some_and(|u| u.unit_type == unit.unit_type)
            });
            ensure(!clash, "the person already belongs to a unit of this type")?;
        }
        Ok(())
    }

    fn check_role_grant(&self, cfg: &Cfg, g: &crate::ops::RoleGranted) -> Result<(), Denied> {
        let role = cfg
            .role(&g.role)
            .ok_or_else(|| Denied("unknown role".into()))?;
        ensure(self.person_exists(g.person), "unknown person")?;
        ensure(
            !self.role_ids.contains_key(&g.grant),
            "grant id already used",
        )?;
        let scope = match (&role.scope, g.unit) {
            (ScopeDef::Organization, None) => GrantScope::Organization,
            (ScopeDef::UnitType(t), Some(u)) => {
                let unit = self
                    .org
                    .units
                    .get(&u)
                    .filter(|x| x.active)
                    .ok_or_else(|| Denied("unknown unit".into()))?;
                ensure(&unit.unit_type == t, "the unit is not of the role's type")?;
                GrantScope::Unit(u)
            }
            _ => return deny("the grant scope does not match the role"),
        };
        if let Some(end) = g.end {
            ensure(end > g.start, "a grant must end after it starts")?;
        }
        ensure(
            self.satisfies(cfg, g.person, role.requires.as_ref(), g.start),
            "the person does not meet the requirements of this role",
        )?;
        let period = Period {
            start: g.start,
            end: g.end,
        };
        let overlapping = |r: &Slug| {
            self.org
                .role_grants
                .iter()
                .filter(|x| &x.role == r && x.scope == scope && x.period.overlaps(&period))
                .count()
        };
        if let Some(max) = role.holders.max {
            ensure(
                overlapping(&g.role) < max as usize,
                "the role already has its maximum number of holders",
            )?;
        }
        for x in &role.exclusive_with {
            ensure(
                overlapping(x) == 0,
                &format!("the role cannot coexist with `{x}` in the same scope"),
            )?;
        }
        // Declared on the other side too.
        for other in cfg.roles() {
            if other.exclusive_with.contains(&g.role) {
                ensure(
                    overlapping(&other.id) == 0,
                    &format!(
                        "the role cannot coexist with `{}` in the same scope",
                        other.id
                    ),
                )?;
            }
        }
        Ok(())
    }

    // ----- apply ------------------------------------------------------------

    /// Applies a valid organizational operation from event `id`.
    fn rename(&mut self, renamed: &BTreeMap<Slug, Slug>) {
        let map = |s: &mut Slug| {
            if let Some(to) = renamed.get(s) {
                *s = to.clone();
            }
        };
        for g in &mut self.org.qualification_grants {
            map(&mut g.qualification);
        }
        for g in &mut self.org.role_grants {
            map(&mut g.role);
        }
        for u in self.org.units.values_mut() {
            map(&mut u.unit_type);
        }
    }

    pub fn apply(&mut self, cfg: &Cfg, id: EventHash, op: &Op) {
        match op {
            Op::Genesis(g) => {
                self.config_version = 1;
                for a in &g.root_admins {
                    self.org.root_admins.insert(a.person);
                    self.org
                        .persons
                        .entry(a.person)
                        .or_insert_with(|| PersonRecord {
                            display_name: String::new(),
                            attributes: BTreeMap::new(),
                            active: true,
                        });
                    for d in &a.devices {
                        self.devices.insert(
                            d.signing,
                            DeviceRecord {
                                person: a.person,
                                identity: *d,
                                revoked: false,
                                archive: false,
                                revoked_by: None,
                            },
                        );
                    }
                }
            }
            Op::OrgSettingsChanged(s) => {
                for p in &s.add_root_admins {
                    self.org.root_admins.insert(*p);
                }
                for p in &s.remove_root_admins {
                    if self.org.root_admins.len() > 1 {
                        self.org.root_admins.remove(p);
                    }
                }
            }
            Op::ConfigPublished(c) => {
                self.config_version = c.version;
                // Grants keep working under renamed qualifications, roles and
                // unit types.
                if let Ok(next) = core_config::decode(&c.bundle) {
                    let renamed = core_config::renames(&next.package);
                    if !renamed.is_empty() {
                        self.rename(&renamed);
                    }
                }
            }
            Op::DeviceAdded(d) => {
                self.devices.insert(
                    d.device.signing,
                    DeviceRecord {
                        person: d.person,
                        identity: d.device,
                        revoked: false,
                        archive: false,
                        revoked_by: None,
                    },
                );
            }
            Op::DeviceRevoked(d) => {
                if let Some(r) = self.devices.get_mut(&d.device) {
                    r.revoked = true;
                    r.revoked_by = Some(id);
                }
            }
            Op::DeviceArchiveSet(d) => {
                if let Some(r) = self.devices.get_mut(&d.device) {
                    r.archive = d.archive;
                }
            }
            Op::PersonAdded(p) => {
                self.org.persons.insert(
                    p.person,
                    PersonRecord {
                        display_name: p.display_name.clone(),
                        attributes: p.attributes.clone(),
                        active: true,
                    },
                );
            }
            Op::PersonUpdated(p) => {
                if let Some(r) = self.org.persons.get_mut(&p.person) {
                    if let Some(n) = &p.display_name {
                        r.display_name = n.clone();
                    }
                    for (k, v) in &p.attributes {
                        r.attributes.insert(k.clone(), v.clone());
                    }
                    for k in &p.cleared {
                        r.attributes.remove(k);
                    }
                }
            }
            Op::PersonRemoved(p) => {
                if let Some(r) = self.org.persons.get_mut(&p.person) {
                    r.active = false;
                }
            }
            Op::ExternalPersonUpserted(e) => {
                self.externals.insert(e.id, e.display_name.clone());
            }
            Op::QualificationGranted(g) => {
                self.qualification_ids
                    .insert(g.grant, self.org.qualification_grants.len());
                self.org.qualification_grants.push(QualificationGrant {
                    person: g.person,
                    qualification: g.qualification.clone(),
                    period: Period {
                        start: g.start,
                        end: g.end,
                    },
                });
            }
            Op::QualificationEnded(e) => {
                if let Some(g) = self
                    .qualification_ids
                    .get(&e.grant)
                    .and_then(|i| self.org.qualification_grants.get_mut(*i))
                {
                    g.period.end = Some(g.period.end.map_or(e.end, |x| x.min(e.end)));
                }
            }
            Op::UnitCreated(u) => {
                self.org.units.insert(
                    u.unit,
                    UnitRecord {
                        unit_type: u.unit_type.clone(),
                        name: u.name.clone(),
                        active: true,
                    },
                );
            }
            Op::UnitUpdated(u) => {
                if let Some(r) = self.org.units.get_mut(&u.unit) {
                    r.name = u.name.clone();
                }
            }
            Op::UnitClosed(u) => {
                if let Some(r) = self.org.units.get_mut(&u.unit) {
                    r.active = false;
                }
            }
            Op::MembershipStarted(m) => {
                self.membership_ids
                    .insert(m.membership, self.org.memberships.len());
                self.org.memberships.push(Membership {
                    person: m.person,
                    unit: m.unit,
                    period: Period::from(m.start),
                });
            }
            Op::MembershipEnded(e) => {
                if let Some(g) = self
                    .membership_ids
                    .get(&e.grant)
                    .and_then(|i| self.org.memberships.get_mut(*i))
                {
                    g.period.end = Some(
                        g.period
                            .end
                            .map_or(e.end, |x| x.min(e.end))
                            .max(g.period.start),
                    );
                }
            }
            Op::RoleGranted(g) => self.apply_role_grant(cfg, id, g),
            Op::RoleEnded(e) => {
                if let Some(g) = self
                    .role_ids
                    .get(&e.grant)
                    .and_then(|(i, _)| self.org.role_grants.get_mut(*i))
                {
                    g.period.end = Some(
                        g.period
                            .end
                            .map_or(e.end, |x| x.min(e.end))
                            .max(g.period.start),
                    );
                }
            }
            _ => {}
        }
    }

    /// Applies a role grant; concurrent grants beyond `holders.max` keep the
    /// smallest event ids (§7.6).
    fn apply_role_grant(&mut self, cfg: &Cfg, id: EventHash, g: &crate::ops::RoleGranted) {
        let scope = g.unit.map_or(GrantScope::Organization, GrantScope::Unit);
        let period = Period {
            start: g.start,
            end: g.end,
        };
        if let Some(max) = cfg.role(&g.role).and_then(|r| r.holders.max) {
            let mut clashing: Vec<(EventHash, Id)> = self
                .role_ids
                .iter()
                .filter_map(|(gid, (i, ev))| {
                    let x = self.org.role_grants.get(*i)?;
                    (x.role == g.role
                        && x.scope == scope
                        && x.period.overlaps(&period)
                        && x.period.start != x.period.end.unwrap_or(Date::MAX))
                    .then_some((*ev, *gid))
                })
                .collect();
            if clashing.len() >= max as usize {
                clashing.sort();
                match clashing.last() {
                    Some((worst_ev, worst_id)) if id < *worst_ev => {
                        let (worst_ev, worst_id) = (*worst_ev, *worst_id);
                        if let Some(x) = self
                            .role_ids
                            .get(&worst_id)
                            .and_then(|(i, _)| self.org.role_grants.get_mut(*i))
                        {
                            x.period.end = Some(x.period.start);
                        }
                        self.superseded_roles.insert(worst_ev);
                    }
                    _ => {
                        self.superseded_roles.insert(id);
                        return;
                    }
                }
            }
        }
        self.role_ids
            .insert(g.grant, (self.org.role_grants.len(), id));
        self.org.role_grants.push(RoleGrant {
            person: g.person,
            role: g.role.clone(),
            scope,
            period,
        });
    }
}

/// End (exclusive) of a fixed-period qualification starting at `start`.
pub fn fixed_end(start: Date, unit: Option<PeriodUnit>) -> Option<Date> {
    match unit? {
        PeriodUnit::Week => Some(add_days(start, 7)),
        PeriodUnit::Month => crate::time::PeriodKey::month_of(start).next()?.start(),
        PeriodUnit::Year => {
            Date::from_calendar_date(start.year() + 1, start.month(), start.day()).ok()
        }
    }
}
