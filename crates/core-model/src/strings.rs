//! String-backed identifiers and prefixed references (`unit:<id>`, `form:<id>`, ...).
//!
//! All of them serialize as plain strings, so they work the same in YAML,
//! JSON and postcard.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Error produced when parsing a string-backed value.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid {kind} `{value}`: {reason}")]
pub struct ParseError {
    pub kind: &'static str,
    pub value: String,
    pub reason: &'static str,
}

impl ParseError {
    fn new(kind: &'static str, value: &str, reason: &'static str) -> Self {
        Self {
            kind,
            value: value.to_owned(),
            reason,
        }
    }
}

/// Implements `Serialize`/`Deserialize` (through `Display`/`FromStr`) and
/// `JsonSchema` (a string with a pattern) for a type.
macro_rules! string_backed {
    ($ty:ty, $name:literal, $pattern:literal) => {
        impl Serialize for $ty {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.collect_str(self)
            }
        }
        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let raw = String::deserialize(d)?;
                raw.parse().map_err(serde::de::Error::custom)
            }
        }
        impl JsonSchema for $ty {
            fn schema_name() -> Cow<'static, str> {
                $name.into()
            }
            fn json_schema(_: &mut SchemaGenerator) -> Schema {
                json_schema!({ "type": "string", "pattern": $pattern })
            }
        }
    };
}

fn is_slug(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some('a'..='z'))
        && chars.all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
}

/// Configuration identifier: `[a-z][a-z0-9_]*`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slug(String);

impl Slug {
    pub fn as_str(&self) -> &str {
        &self.0
    }
    /// Builds a slug, failing on invalid input.
    pub fn new(s: &str) -> Result<Self, ParseError> {
        s.parse()
    }
}

impl FromStr for Slug {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if is_slug(s) {
            Ok(Self(s.to_owned()))
        } else {
            Err(ParseError::new(
                "identifier",
                s,
                "must match [a-z][a-z0-9_]*",
            ))
        }
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl AsRef<str> for Slug {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

string_backed!(Slug, "Slug", "^[a-z][a-z0-9_]*$");

/// Key of a text in `texts/<locale>.yaml`: dot-separated slugs, e.g. `menu.home`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TextKey(String);

impl TextKey {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for TextKey {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let ok = !s.is_empty()
            && s.split('.').enumerate().all(|(i, part)| {
                if i == 0 {
                    is_slug(part)
                } else {
                    !part.is_empty()
                        && part
                            .chars()
                            .all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_'))
                }
            });
        if ok {
            Ok(Self(s.to_owned()))
        } else {
            Err(ParseError::new(
                "text key",
                s,
                "must be dot-separated lowercase identifiers, e.g. `menu.home`",
            ))
        }
    }
}

impl fmt::Display for TextKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

string_backed!(TextKey, "TextKey", "^[a-z][a-z0-9_]*(\\.[a-z0-9_]+)*$");

/// Reference to a unit instance by its id (as written after `unit:`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UnitInstanceRef(pub String);

fn parse_instance(kind: &'static str, s: &str) -> Result<UnitInstanceRef, ParseError> {
    if !s.is_empty() && s.chars().all(|c| matches!(c, 'a'..='z' | '0'..='9' | '_')) {
        Ok(UnitInstanceRef(s.to_owned()))
    } else {
        Err(ParseError::new(kind, s, "unit id must be lowercase base32"))
    }
}

/// Where a role is looked for: `anywhere | subject_unit | own_unit | unit:<id>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum UnitWhere {
    #[default]
    Anywhere,
    SubjectUnit,
    OwnUnit,
    Unit(UnitInstanceRef),
}

impl FromStr for UnitWhere {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "anywhere" => Ok(Self::Anywhere),
            "subject_unit" => Ok(Self::SubjectUnit),
            "own_unit" => Ok(Self::OwnUnit),
            _ => match s.strip_prefix("unit:") {
                Some(rest) => Ok(Self::Unit(parse_instance("unit location", rest)?)),
                None => Err(ParseError::new(
                    "unit location",
                    s,
                    "expected anywhere, subject_unit, own_unit or unit:<id>",
                )),
            },
        }
    }
}

impl fmt::Display for UnitWhere {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Anywhere => f.write_str("anywhere"),
            Self::SubjectUnit => f.write_str("subject_unit"),
            Self::OwnUnit => f.write_str("own_unit"),
            Self::Unit(u) => write!(f, "unit:{}", u.0),
        }
    }
}

string_backed!(
    UnitWhere,
    "UnitWhere",
    "^(anywhere|subject_unit|own_unit|unit:[a-z0-9_]+)$"
);

/// A concrete unit: `subject_unit | unit:<id>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum UnitTarget {
    SubjectUnit,
    Unit(UnitInstanceRef),
}

impl FromStr for UnitTarget {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "subject_unit" {
            return Ok(Self::SubjectUnit);
        }
        match s.strip_prefix("unit:") {
            Some(rest) => Ok(Self::Unit(parse_instance("unit", rest)?)),
            None => Err(ParseError::new(
                "unit",
                s,
                "expected subject_unit or unit:<id>",
            )),
        }
    }
}

impl fmt::Display for UnitTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectUnit => f.write_str("subject_unit"),
            Self::Unit(u) => write!(f, "unit:{}", u.0),
        }
    }
}

string_backed!(UnitTarget, "UnitTarget", "^(subject_unit|unit:[a-z0-9_]+)$");

/// Membership test target: `subject_unit | unit:<id> | unit_type:<slug>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberOf {
    SubjectUnit,
    Unit(UnitInstanceRef),
    UnitType(Slug),
}

impl FromStr for MemberOf {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "subject_unit" {
            return Ok(Self::SubjectUnit);
        }
        if let Some(rest) = s.strip_prefix("unit_type:") {
            return Ok(Self::UnitType(rest.parse()?));
        }
        match s.strip_prefix("unit:") {
            Some(rest) => Ok(Self::Unit(parse_instance("membership target", rest)?)),
            None => Err(ParseError::new(
                "membership target",
                s,
                "expected subject_unit, unit:<id> or unit_type:<type>",
            )),
        }
    }
}

impl fmt::Display for MemberOf {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SubjectUnit => f.write_str("subject_unit"),
            Self::Unit(u) => write!(f, "unit:{}", u.0),
            Self::UnitType(t) => write!(f, "unit_type:{t}"),
        }
    }
}

string_backed!(
    MemberOf,
    "MemberOf",
    "^(subject_unit|unit:[a-z0-9_]+|unit_type:[a-z][a-z0-9_]*)$"
);

/// Scope of a role or calendar event type: `organization | unit:<unit_type>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum ScopeDef {
    #[default]
    Organization,
    UnitType(Slug),
}

impl FromStr for ScopeDef {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "organization" {
            return Ok(Self::Organization);
        }
        match s.strip_prefix("unit:") {
            Some(rest) => Ok(Self::UnitType(rest.parse()?)),
            None => Err(ParseError::new(
                "scope",
                s,
                "expected organization or unit:<unit_type>",
            )),
        }
    }
}

impl fmt::Display for ScopeDef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Organization => f.write_str("organization"),
            Self::UnitType(t) => write!(f, "unit:{t}"),
        }
    }
}

string_backed!(
    ScopeDef,
    "ScopeDef",
    "^(organization|unit:[a-z][a-z0-9_]*)$"
);

/// Kind of configured object a permission applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ObjectKind {
    Form,
    Program,
    Resource,
    Aggregate,
    Workflow,
}

impl ObjectKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Form => "form",
            Self::Program => "program",
            Self::Resource => "resource",
            Self::Aggregate => "aggregate",
            Self::Workflow => "workflow",
        }
    }
}

/// Typed reference to a configured object: `form:<id>`, `program:<id>`, ...
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectRef {
    pub kind: ObjectKind,
    pub id: Slug,
}

impl FromStr for ObjectRef {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (kind, id) = s
            .split_once(':')
            .ok_or_else(|| ParseError::new("object reference", s, "expected <kind>:<id>"))?;
        let kind = match kind {
            "form" => ObjectKind::Form,
            "program" => ObjectKind::Program,
            "resource" => ObjectKind::Resource,
            "aggregate" => ObjectKind::Aggregate,
            "workflow" => ObjectKind::Workflow,
            _ => {
                return Err(ParseError::new(
                    "object reference",
                    s,
                    "kind must be form, program, resource, aggregate or workflow",
                ));
            }
        };
        Ok(Self {
            kind,
            id: id.parse()?,
        })
    }
}

impl fmt::Display for ObjectRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.kind.as_str(), self.id)
    }
}

string_backed!(
    ObjectRef,
    "ObjectRef",
    "^(form|program|resource|aggregate|workflow):[a-z][a-z0-9_]*$"
);

/// Grouping key of an aggregation: `variant | qualification_set:<slug> | unit:<unit_type>`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum GroupBy {
    Variant,
    QualificationSet(Slug),
    UnitType(Slug),
}

impl FromStr for GroupBy {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "variant" {
            return Ok(Self::Variant);
        }
        if let Some(rest) = s.strip_prefix("qualification_set:") {
            return Ok(Self::QualificationSet(rest.parse()?));
        }
        match s.strip_prefix("unit:") {
            Some(rest) => Ok(Self::UnitType(rest.parse()?)),
            None => Err(ParseError::new(
                "grouping",
                s,
                "expected variant, qualification_set:<set> or unit:<unit_type>",
            )),
        }
    }
}

impl fmt::Display for GroupBy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Variant => f.write_str("variant"),
            Self::QualificationSet(s) => write!(f, "qualification_set:{s}"),
            Self::UnitType(t) => write!(f, "unit:{t}"),
        }
    }
}

string_backed!(
    GroupBy,
    "GroupBy",
    "^(variant|qualification_set:[a-z][a-z0-9_]*|unit:[a-z][a-z0-9_]*)$"
);

/// Bound of a visibility window: `period_start`, `period_end`,
/// `period_start_minus_days_<n>`, `period_end_plus_days_<n>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum VisibilityBound {
    PeriodStart { minus_days: u32 },
    PeriodEnd { plus_days: u32 },
}

impl FromStr for VisibilityBound {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let bad = || {
            ParseError::new(
                "visibility bound",
                s,
                "expected period_start[_minus_days_<n>] or period_end[_plus_days_<n>]",
            )
        };
        match s {
            "period_start" => return Ok(Self::PeriodStart { minus_days: 0 }),
            "period_end" => return Ok(Self::PeriodEnd { plus_days: 0 }),
            _ => {}
        }
        if let Some(n) = s.strip_prefix("period_start_minus_days_") {
            return n
                .parse()
                .map(|minus_days| Self::PeriodStart { minus_days })
                .map_err(|_| bad());
        }
        if let Some(n) = s.strip_prefix("period_end_plus_days_") {
            return n
                .parse()
                .map(|plus_days| Self::PeriodEnd { plus_days })
                .map_err(|_| bad());
        }
        Err(bad())
    }
}

impl fmt::Display for VisibilityBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PeriodStart { minus_days: 0 } => f.write_str("period_start"),
            Self::PeriodStart { minus_days } => write!(f, "period_start_minus_days_{minus_days}"),
            Self::PeriodEnd { plus_days: 0 } => f.write_str("period_end"),
            Self::PeriodEnd { plus_days } => write!(f, "period_end_plus_days_{plus_days}"),
        }
    }
}

string_backed!(
    VisibilityBound,
    "VisibilityBound",
    "^(period_start(_minus_days_[0-9]+)?|period_end(_plus_days_[0-9]+)?)$"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slug_rules() {
        assert!(Slug::new("a_b1").is_ok());
        assert!(Slug::new("1ab").is_err());
        assert!(Slug::new("Ab").is_err());
        assert!(Slug::new("").is_err());
        assert!(Slug::new("a-b").is_err());
    }

    #[test]
    fn text_key_rules() {
        assert!("menu.home".parse::<TextKey>().is_ok());
        assert!("a.1".parse::<TextKey>().is_ok());
        assert!("a..b".parse::<TextKey>().is_err());
        assert!(".a".parse::<TextKey>().is_err());
    }

    #[test]
    fn prefixed_round_trip() {
        for s in ["anywhere", "subject_unit", "own_unit", "unit:abc123"] {
            assert_eq!(
                s.parse::<UnitWhere>().map(|v| v.to_string()).as_deref(),
                Ok(s)
            );
        }
        for s in ["subject_unit", "unit:x1", "unit_type:team"] {
            assert_eq!(
                s.parse::<MemberOf>().map(|v| v.to_string()).as_deref(),
                Ok(s)
            );
        }
        for s in ["form:log", "program:week_plan", "aggregate:sum"] {
            assert_eq!(
                s.parse::<ObjectRef>().map(|v| v.to_string()).as_deref(),
                Ok(s)
            );
        }
        for s in [
            "period_start",
            "period_end",
            "period_start_minus_days_30",
            "period_end_plus_days_2",
        ] {
            assert_eq!(
                s.parse::<VisibilityBound>()
                    .map(|v| v.to_string())
                    .as_deref(),
                Ok(s)
            );
        }
        assert!("thing:x".parse::<ObjectRef>().is_err());
        assert!("unit:".parse::<UnitWhere>().is_err());
        assert!(
            "period_start_minus_days_x"
                .parse::<VisibilityBound>()
                .is_err()
        );
    }
}
