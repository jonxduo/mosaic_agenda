//! Minimal organization state used to evaluate conditions and audiences.
//!
//! The engine (`core-engine`) builds it from events; here it is plain data
//! with date-based history, so every question is "at date D".

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use time::Date;

use crate::id::{PersonId, UnitId};
use crate::scalar::Scalar;
use crate::strings::Slug;

/// Half-open validity interval `[start, end)`; `end = None` means open-ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Period {
    pub start: Date,
    pub end: Option<Date>,
}

impl Period {
    pub fn from(start: Date) -> Self {
        Self { start, end: None }
    }

    pub fn between(start: Date, end: Date) -> Self {
        Self {
            start,
            end: Some(end),
        }
    }

    pub fn contains(&self, date: Date) -> bool {
        date >= self.start && self.end.is_none_or(|end| date < end)
    }

    /// True when the two periods share at least one day.
    pub fn overlaps(&self, other: &Period) -> bool {
        let starts_before_other_ends = other.end.is_none_or(|e| self.start < e);
        let other_starts_before_self_ends = self.end.is_none_or(|e| other.start < e);
        starts_before_other_ends && other_starts_before_self_ends
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PersonRecord {
    pub display_name: String,
    /// Configured attributes (only those this device can read).
    pub attributes: BTreeMap<Slug, Scalar>,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnitRecord {
    pub unit_type: Slug,
    pub name: String,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QualificationGrant {
    pub person: PersonId,
    pub qualification: Slug,
    pub period: Period,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Membership {
    pub person: PersonId,
    pub unit: UnitId,
    pub period: Period,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum GrantScope {
    Organization,
    Unit(UnitId),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RoleGrant {
    pub person: PersonId,
    pub role: Slug,
    pub scope: GrantScope,
    pub period: Period,
}

/// Organization facts relevant to authorization and audiences.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OrgState {
    pub persons: BTreeMap<PersonId, PersonRecord>,
    pub units: BTreeMap<UnitId, UnitRecord>,
    pub qualification_grants: Vec<QualificationGrant>,
    pub memberships: Vec<Membership>,
    pub role_grants: Vec<RoleGrant>,
    /// Root administrators (declared at genesis).
    pub root_admins: BTreeSet<PersonId>,
}

impl OrgState {
    pub fn active_persons(&self) -> impl Iterator<Item = PersonId> + '_ {
        self.persons
            .iter()
            .filter(|(_, p)| p.active)
            .map(|(id, _)| *id)
    }

    /// Qualifications granted directly to the person at `date`.
    pub fn direct_qualifications(
        &self,
        person: PersonId,
        date: Date,
    ) -> impl Iterator<Item = &Slug> + '_ {
        self.qualification_grants
            .iter()
            .filter(move |g| g.person == person && g.period.contains(date))
            .map(|g| &g.qualification)
    }

    /// Units the person belongs to at `date`.
    pub fn units_of(&self, person: PersonId, date: Date) -> impl Iterator<Item = UnitId> + '_ {
        self.memberships
            .iter()
            .filter(move |m| m.person == person && m.period.contains(date))
            .map(|m| m.unit)
    }

    /// Unit of the given type the person belongs to at `date` (first by id).
    pub fn unit_of_type(&self, person: PersonId, unit_type: &Slug, date: Date) -> Option<UnitId> {
        let mut units: Vec<UnitId> = self
            .units_of(person, date)
            .filter(|u| self.units.get(u).is_some_and(|r| &r.unit_type == unit_type))
            .collect();
        units.sort();
        units.into_iter().next()
    }

    pub fn is_member(&self, person: PersonId, unit: UnitId, date: Date) -> bool {
        self.units_of(person, date).any(|u| u == unit)
    }

    /// Role grants active at `date`.
    pub fn active_role_grants(&self, date: Date) -> impl Iterator<Item = &RoleGrant> + '_ {
        self.role_grants
            .iter()
            .filter(move |g| g.period.contains(date))
    }

    pub fn attribute(&self, person: PersonId, name: &Slug) -> Option<&Scalar> {
        self.persons.get(&person)?.attributes.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use time::macros::date;

    #[test]
    fn period_is_half_open() {
        let p = Period::between(date!(2026 - 10 - 01), date!(2026 - 11 - 01));
        assert!(p.contains(date!(2026 - 10 - 01)));
        assert!(p.contains(date!(2026 - 10 - 31)));
        assert!(!p.contains(date!(2026 - 11 - 01)));
        assert!(!p.contains(date!(2026 - 09 - 30)));
        assert!(Period::from(date!(2026 - 01 - 01)).contains(date!(2099 - 01 - 01)));
    }

    #[test]
    fn overlaps() {
        let oct = Period::between(date!(2026 - 10 - 01), date!(2026 - 11 - 01));
        let nov = Period::between(date!(2026 - 11 - 01), date!(2026 - 12 - 01));
        let open = Period::from(date!(2026 - 10 - 15));
        assert!(!oct.overlaps(&nov));
        assert!(oct.overlaps(&open));
        assert!(nov.overlaps(&open));
    }
}
