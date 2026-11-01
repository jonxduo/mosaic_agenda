//! Audiences: sets of persons computed from selectors (PROGETTO.md §5.4.2).

use std::str::FromStr;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::strings::{Slug, TextKey, UnitTarget, UnitWhere};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudienceDef {
    pub id: Slug,
    #[serde(default)]
    pub label: Option<TextKey>,
    pub select: Selector,
}

/// Audience selector. Combinators: `union`, `intersect`, `except`
/// (`except: [base, removed...]` = base minus the union of the others).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Selector {
    Union(Vec<Selector>),
    Intersect(Vec<Selector>),
    Except(Vec<Selector>),
    /// All active persons (`false` = nobody).
    Everyone(bool),
    WithQualification(Slug),
    WithSet(Slug),
    RoleHolders(RoleHolders),
    /// Holders of supervising roles over the unit.
    SupervisorsOf(UnitTarget),
    UnitMembers(UnitTarget),
    /// The subject of the document/object in context.
    Subject(bool),
    /// Persons assigned to the object in context (programs, resources).
    Assigned(bool),
    /// A list chosen when the action is performed.
    Explicit(bool),
    /// Reuse of another audience (configured or system).
    Audience(Slug),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RoleHolders {
    pub role: Slug,
    #[serde(rename = "where", default)]
    pub location: UnitWhere,
}

impl Selector {
    /// Ids of audiences referenced with `audience:` (directly or nested).
    pub fn referenced_audiences(&self, out: &mut Vec<Slug>) {
        match self {
            Self::Union(v) | Self::Intersect(v) | Self::Except(v) => {
                v.iter().for_each(|s| s.referenced_audiences(out));
            }
            Self::Audience(a) => out.push(a.clone()),
            _ => {}
        }
    }

    /// Visits this selector and all nested ones.
    pub fn walk<'a>(&'a self, f: &mut dyn FnMut(&'a Selector)) {
        f(self);
        if let Self::Union(v) | Self::Intersect(v) | Self::Except(v) = self {
            v.iter().for_each(|s| s.walk(f));
        }
    }
}

/// Audiences always available, provided by the core. Their membership comes
/// from the action context rather than from configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SystemAudience {
    /// Only the subject.
    SubjectOnly,
    /// The subject plus the form's `fillers_on_behalf` audience.
    SubjectOrOnBehalf,
    /// Subjects that have not produced the expected document.
    SubjectMissing,
    /// Persons assigned to the program slot or object in context.
    Assignee,
    /// Current holder of the resource in context.
    ResourceHolder,
    /// Owner and assistants of the program (or resource type) in context.
    ObjectTeam,
    /// Root administrators declared at genesis.
    RootAdmins,
}

impl SystemAudience {
    pub const ALL: [SystemAudience; 7] = [
        Self::SubjectOnly,
        Self::SubjectOrOnBehalf,
        Self::SubjectMissing,
        Self::Assignee,
        Self::ResourceHolder,
        Self::ObjectTeam,
        Self::RootAdmins,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Self::SubjectOnly => "subject_only",
            Self::SubjectOrOnBehalf => "subject_or_on_behalf",
            Self::SubjectMissing => "subject_missing",
            Self::Assignee => "assignee",
            Self::ResourceHolder => "resource_holder",
            Self::ObjectTeam => "object_team",
            Self::RootAdmins => "root_admins",
        }
    }
}

impl FromStr for SystemAudience {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, ()> {
        Self::ALL.into_iter().find(|a| a.id() == s).ok_or(())
    }
}
