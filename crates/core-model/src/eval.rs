//! Evaluation of conditions and resolution of audiences (PROGETTO.md §5.4).
//!
//! Everything is a pure function of (configuration index, organization
//! state, context): results are deterministic and ordered.

use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use time::Date;

use crate::audience::{Selector, SystemAudience};
use crate::cond::Cond;
use crate::config::{AudiencesFile, QualificationsFile, RoleDef, RolesFile};
use crate::id::{PersonId, UnitId};
use crate::scalar::Scalar;
use crate::state::{GrantScope, OrgState};
use crate::strings::{MemberOf, Slug, UnitInstanceRef, UnitTarget, UnitWhere};

/// Lookup tables derived from the configuration, needed for evaluation.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct ConfigIndex {
    /// Qualification → itself plus everything it implies (transitively).
    pub implies_closure: BTreeMap<Slug, BTreeSet<Slug>>,
    /// Set → qualifications belonging to it.
    pub set_members: BTreeMap<Slug, BTreeSet<Slug>>,
    pub audiences: BTreeMap<Slug, Selector>,
    pub roles: BTreeMap<Slug, RoleDef>,
}

impl ConfigIndex {
    pub fn build(q: &QualificationsFile, r: &RolesFile, a: &AudiencesFile) -> Self {
        let direct: BTreeMap<&Slug, &Vec<Slug>> = q
            .qualifications
            .iter()
            .map(|d| (&d.id, &d.implies))
            .collect();
        let mut implies_closure = BTreeMap::new();
        for def in &q.qualifications {
            let mut seen = BTreeSet::new();
            let mut stack = vec![def.id.clone()];
            while let Some(cur) = stack.pop() {
                if seen.insert(cur.clone()) {
                    if let Some(next) = direct.get(&cur) {
                        stack.extend(next.iter().cloned());
                    }
                }
            }
            implies_closure.insert(def.id.clone(), seen);
        }
        let mut set_members: BTreeMap<Slug, BTreeSet<Slug>> = q
            .sets
            .iter()
            .map(|s| (s.clone(), BTreeSet::new()))
            .collect();
        for def in &q.qualifications {
            for s in &def.sets {
                set_members
                    .entry(s.clone())
                    .or_default()
                    .insert(def.id.clone());
            }
        }
        Self {
            implies_closure,
            set_members,
            audiences: a
                .audiences
                .iter()
                .map(|d| (d.id.clone(), d.select.clone()))
                .collect(),
            roles: r.roles.iter().map(|d| (d.id.clone(), d.clone())).collect(),
        }
    }
}

/// Context of an evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct EvalContext {
    /// Reference date.
    pub date: Date,
    /// Subject of the document or object.
    pub subject: Option<PersonId>,
    /// Unit of the subject (for `subject_unit`).
    pub subject_unit: Option<UnitId>,
    /// Document field values (for `field` conditions).
    pub fields: BTreeMap<Slug, Scalar>,
    /// Persons assigned to the object.
    pub assigned: BTreeSet<PersonId>,
    /// Persons chosen explicitly for the action.
    pub explicit: BTreeSet<PersonId>,
    /// Current holder of the resource.
    pub holder: Option<PersonId>,
    /// Audience of those who may act on behalf of the subject.
    pub on_behalf: Option<Slug>,
    /// Subjects without the expected document.
    pub missing_subjects: BTreeSet<PersonId>,
    /// Owner and assistants of the object.
    pub object_team: BTreeSet<PersonId>,
}

impl EvalContext {
    pub fn at(date: Date) -> Self {
        Self {
            date,
            subject: None,
            subject_unit: None,
            fields: BTreeMap::new(),
            assigned: BTreeSet::new(),
            explicit: BTreeSet::new(),
            holder: None,
            on_behalf: None,
            missing_subjects: BTreeSet::new(),
            object_team: BTreeSet::new(),
        }
    }

    pub fn with_subject(mut self, subject: PersonId, unit: Option<UnitId>) -> Self {
        self.subject = Some(subject);
        self.subject_unit = unit;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EvalError {
    #[error("unknown audience `{0}`")]
    UnknownAudience(Slug),
    #[error("audience `{0}` is defined recursively")]
    AudienceCycle(Slug),
}

/// Evaluator over a configuration index and an organization state.
#[derive(Debug, Clone, Copy)]
pub struct Evaluator<'a> {
    pub cfg: &'a ConfigIndex,
    pub state: &'a OrgState,
}

impl<'a> Evaluator<'a> {
    pub fn new(cfg: &'a ConfigIndex, state: &'a OrgState) -> Self {
        Self { cfg, state }
    }

    /// Qualifications held at `date`, including implied ones.
    pub fn qualifications(&self, person: PersonId, date: Date) -> BTreeSet<Slug> {
        let mut out = BTreeSet::new();
        for q in self.state.direct_qualifications(person, date) {
            match self.cfg.implies_closure.get(q) {
                Some(closure) => out.extend(closure.iter().cloned()),
                None => {
                    out.insert(q.clone());
                }
            }
        }
        out
    }

    pub fn has_qualification(&self, person: PersonId, q: &Slug, date: Date) -> bool {
        self.qualifications(person, date).contains(q)
    }

    pub fn in_set(&self, person: PersonId, set: &Slug, date: Date) -> bool {
        let Some(members) = self.cfg.set_members.get(set) else {
            return false;
        };
        self.qualifications(person, date)
            .iter()
            .any(|q| members.contains(q))
    }

    fn find_unit(&self, r: &UnitInstanceRef) -> Option<UnitId> {
        self.state
            .units
            .keys()
            .find(|u| u.to_string() == r.0)
            .copied()
    }

    fn target_unit(&self, t: &UnitTarget, ctx: &EvalContext) -> Option<UnitId> {
        match t {
            UnitTarget::SubjectUnit => ctx.subject_unit,
            UnitTarget::Unit(r) => self.find_unit(r),
        }
    }

    pub fn holds_role(
        &self,
        person: PersonId,
        role: &Slug,
        location: &UnitWhere,
        ctx: &EvalContext,
    ) -> bool {
        self.state
            .active_role_grants(ctx.date)
            .filter(|g| g.person == person && &g.role == role)
            .any(|g| match (location, g.scope) {
                (UnitWhere::Anywhere, _) => true,
                (UnitWhere::SubjectUnit, GrantScope::Unit(u)) => ctx.subject_unit == Some(u),
                (UnitWhere::OwnUnit, GrantScope::Unit(u)) => {
                    self.state.is_member(person, u, ctx.date)
                }
                (UnitWhere::Unit(r), GrantScope::Unit(u)) => self.find_unit(r) == Some(u),
                (_, GrantScope::Organization) => false,
            })
    }

    pub fn member_of(&self, person: PersonId, target: &MemberOf, ctx: &EvalContext) -> bool {
        match target {
            MemberOf::SubjectUnit => ctx
                .subject_unit
                .is_some_and(|u| self.state.is_member(person, u, ctx.date)),
            MemberOf::Unit(r) => self
                .find_unit(r)
                .is_some_and(|u| self.state.is_member(person, u, ctx.date)),
            MemberOf::UnitType(t) => self.state.unit_of_type(person, t, ctx.date).is_some(),
        }
    }

    /// Evaluates a condition for `person` (None = no evaluated person, e.g.
    /// pure field conditions; person-based tests are then false).
    pub fn eval(&self, cond: &Cond, person: Option<PersonId>, ctx: &EvalContext) -> bool {
        match cond {
            Cond::All(v) => v.iter().all(|c| self.eval(c, person, ctx)),
            Cond::Any(v) => v.iter().any(|c| self.eval(c, person, ctx)),
            Cond::Not(c) => !self.eval(c, person, ctx),
            Cond::Always(b) => *b,
            Cond::Field(cmp) => cmp.op.apply(ctx.fields.get(&cmp.name), &cmp.value),
            Cond::IsSubject(b) => person.is_some() && (ctx.subject == person) == *b,
            Cond::HasQualification(q) => {
                person.is_some_and(|p| self.has_qualification(p, q, ctx.date))
            }
            Cond::InSet(s) => person.is_some_and(|p| self.in_set(p, s, ctx.date)),
            Cond::HoldsRole(h) => {
                person.is_some_and(|p| self.holds_role(p, &h.role, &h.location, ctx))
            }
            Cond::MemberOf(m) => person.is_some_and(|p| self.member_of(p, m, ctx)),
            Cond::Attribute(cmp) => {
                person.is_some_and(|p| cmp.op.apply(self.state.attribute(p, &cmp.name), &cmp.value))
            }
        }
    }

    /// Resolves an audience id (configured or system) to a set of persons.
    pub fn resolve(
        &self,
        audience: &Slug,
        ctx: &EvalContext,
    ) -> Result<BTreeSet<PersonId>, EvalError> {
        self.resolve_inner(audience, ctx, &mut Vec::new())
    }

    pub fn resolve_selector(
        &self,
        sel: &Selector,
        ctx: &EvalContext,
    ) -> Result<BTreeSet<PersonId>, EvalError> {
        self.selector_inner(sel, ctx, &mut Vec::new())
    }

    fn resolve_inner(
        &self,
        audience: &Slug,
        ctx: &EvalContext,
        stack: &mut Vec<Slug>,
    ) -> Result<BTreeSet<PersonId>, EvalError> {
        if let Ok(sys) = SystemAudience::from_str(audience.as_str()) {
            return self.system(sys, ctx, stack);
        }
        if stack.contains(audience) {
            return Err(EvalError::AudienceCycle(audience.clone()));
        }
        let sel = self
            .cfg
            .audiences
            .get(audience)
            .ok_or_else(|| EvalError::UnknownAudience(audience.clone()))?;
        stack.push(audience.clone());
        let out = self.selector_inner(sel, ctx, stack);
        stack.pop();
        out
    }

    fn system(
        &self,
        sys: SystemAudience,
        ctx: &EvalContext,
        stack: &mut Vec<Slug>,
    ) -> Result<BTreeSet<PersonId>, EvalError> {
        Ok(match sys {
            SystemAudience::SubjectOnly => ctx.subject.into_iter().collect(),
            SystemAudience::SubjectOrOnBehalf => {
                let mut out: BTreeSet<PersonId> = ctx.subject.into_iter().collect();
                if let Some(a) = &ctx.on_behalf {
                    out.extend(self.resolve_inner(a, ctx, stack)?);
                }
                out
            }
            SystemAudience::SubjectMissing => ctx.missing_subjects.clone(),
            SystemAudience::Assignee => ctx.assigned.clone(),
            SystemAudience::ResourceHolder => ctx.holder.into_iter().collect(),
            SystemAudience::ObjectTeam => ctx.object_team.clone(),
            SystemAudience::RootAdmins => self.state.root_admins.clone(),
        })
    }

    fn active_where(&self, f: impl Fn(PersonId) -> bool) -> BTreeSet<PersonId> {
        self.state.active_persons().filter(|p| f(*p)).collect()
    }

    fn selector_inner(
        &self,
        sel: &Selector,
        ctx: &EvalContext,
        stack: &mut Vec<Slug>,
    ) -> Result<BTreeSet<PersonId>, EvalError> {
        Ok(match sel {
            Selector::Union(v) => {
                let mut out = BTreeSet::new();
                for s in v {
                    out.extend(self.selector_inner(s, ctx, stack)?);
                }
                out
            }
            Selector::Intersect(v) => {
                let mut iter = v.iter();
                let Some(first) = iter.next() else {
                    return Ok(BTreeSet::new());
                };
                let mut out = self.selector_inner(first, ctx, stack)?;
                for s in iter {
                    let other = self.selector_inner(s, ctx, stack)?;
                    out.retain(|p| other.contains(p));
                }
                out
            }
            Selector::Except(v) => {
                let mut iter = v.iter();
                let Some(first) = iter.next() else {
                    return Ok(BTreeSet::new());
                };
                let mut out = self.selector_inner(first, ctx, stack)?;
                for s in iter {
                    for p in self.selector_inner(s, ctx, stack)? {
                        out.remove(&p);
                    }
                }
                out
            }
            Selector::Everyone(b) => {
                if *b {
                    self.state.active_persons().collect()
                } else {
                    BTreeSet::new()
                }
            }
            Selector::WithQualification(q) => {
                self.active_where(|p| self.has_qualification(p, q, ctx.date))
            }
            Selector::WithSet(s) => self.active_where(|p| self.in_set(p, s, ctx.date)),
            Selector::RoleHolders(rh) => {
                self.active_where(|p| self.holds_role(p, &rh.role, &rh.location, ctx))
            }
            Selector::SupervisorsOf(t) => match self.target_unit(t, ctx) {
                Some(unit) => self.supervisors_of(unit, ctx.date),
                None => BTreeSet::new(),
            },
            Selector::UnitMembers(t) => match self.target_unit(t, ctx) {
                Some(unit) => self.active_where(|p| self.state.is_member(p, unit, ctx.date)),
                None => BTreeSet::new(),
            },
            Selector::Subject(b) => {
                if *b {
                    ctx.subject.into_iter().collect()
                } else {
                    BTreeSet::new()
                }
            }
            Selector::Assigned(b) => {
                if *b {
                    ctx.assigned.clone()
                } else {
                    BTreeSet::new()
                }
            }
            Selector::Explicit(b) => {
                if *b {
                    ctx.explicit.clone()
                } else {
                    BTreeSet::new()
                }
            }
            Selector::Audience(a) => self.resolve_inner(a, ctx, stack)?,
        })
    }

    /// Holders, in `unit`, of a role that supervises another role currently
    /// held in the same unit.
    pub fn supervisors_of(&self, unit: UnitId, date: Date) -> BTreeSet<PersonId> {
        let held_here: BTreeSet<&Slug> = self
            .state
            .active_role_grants(date)
            .filter(|g| g.scope == GrantScope::Unit(unit))
            .map(|g| &g.role)
            .collect();
        self.state
            .active_role_grants(date)
            .filter(|g| g.scope == GrantScope::Unit(unit))
            .filter(|g| {
                self.cfg
                    .roles
                    .get(&g.role)
                    .and_then(|r| r.supervises.as_ref())
                    .is_some_and(|sup| held_here.contains(sup))
            })
            .map(|g| g.person)
            .filter(|p| self.state.persons.get(p).is_some_and(|r| r.active))
            .collect()
    }
}
