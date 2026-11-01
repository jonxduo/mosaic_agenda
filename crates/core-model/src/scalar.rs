//! A small dynamic value used for attribute and field values in conditions.
//!
//! In human-readable formats (YAML, JSON) it is written naturally (`true`,
//! `3`, `"m"`, `[a, b]`); in binary formats (postcard) it is tagged, because
//! postcard is not self-describing.

use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Scalar {
    #[default]
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    List(Vec<Scalar>),
}

impl Scalar {
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }

    /// Numeric view, if the value is a number.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Self::Int(i) => Some(*i as f64),
            Self::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// Partial order between comparable values (numbers with numbers,
    /// strings with strings, booleans with booleans).
    pub fn compare(&self, other: &Self) -> Option<Ordering> {
        match (self, other) {
            (Self::Str(a), Self::Str(b)) => Some(a.cmp(b)),
            (Self::Bool(a), Self::Bool(b)) => Some(a.cmp(b)),
            _ => {
                let (a, b) = (self.as_f64()?, other.as_f64()?);
                a.partial_cmp(&b)
            }
        }
    }

    /// Equality across numeric representations (`1` == `1.0`).
    pub fn loose_eq(&self, other: &Self) -> bool {
        match (self.as_f64(), other.as_f64()) {
            (Some(a), Some(b)) => a == b,
            _ => self == other,
        }
    }
}

impl From<bool> for Scalar {
    fn from(v: bool) -> Self {
        Self::Bool(v)
    }
}
impl From<i64> for Scalar {
    fn from(v: i64) -> Self {
        Self::Int(v)
    }
}
impl From<&str> for Scalar {
    fn from(v: &str) -> Self {
        Self::Str(v.to_owned())
    }
}

/// Binary (tagged) mirror of [`Scalar`].
#[derive(Serialize, Deserialize)]
enum Tagged {
    Null,
    Bool(bool),
    Int(i64),
    Num(f64),
    Str(String),
    List(Vec<Scalar>),
}

impl Serialize for Scalar {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            match self {
                Self::Null => s.serialize_unit(),
                Self::Bool(b) => s.serialize_bool(*b),
                Self::Int(i) => s.serialize_i64(*i),
                Self::Num(n) => s.serialize_f64(*n),
                Self::Str(v) => s.serialize_str(v),
                Self::List(l) => l.serialize(s),
            }
        } else {
            let tagged = match self.clone() {
                Self::Null => Tagged::Null,
                Self::Bool(b) => Tagged::Bool(b),
                Self::Int(i) => Tagged::Int(i),
                Self::Num(n) => Tagged::Num(n),
                Self::Str(v) => Tagged::Str(v),
                Self::List(l) => Tagged::List(l),
            };
            tagged.serialize(s)
        }
    }
}

struct HumanVisitor;

impl<'de> Visitor<'de> for HumanVisitor {
    type Value = Scalar;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a boolean, number, string, null or list of them")
    }
    fn visit_unit<E: de::Error>(self) -> Result<Scalar, E> {
        Ok(Scalar::Null)
    }
    fn visit_none<E: de::Error>(self) -> Result<Scalar, E> {
        Ok(Scalar::Null)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> Result<Scalar, E> {
        Ok(Scalar::Bool(v))
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Scalar, E> {
        Ok(Scalar::Int(v))
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Scalar, E> {
        i64::try_from(v)
            .map(Scalar::Int)
            .map_err(|_| E::custom("integer too large"))
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> Result<Scalar, E> {
        Ok(Scalar::Num(v))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> Result<Scalar, E> {
        Ok(Scalar::Str(v.to_owned()))
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Scalar, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = seq.next_element::<Scalar>()? {
            out.push(v);
        }
        Ok(Scalar::List(out))
    }
}

impl<'de> Deserialize<'de> for Scalar {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            d.deserialize_any(HumanVisitor)
        } else {
            Ok(match Tagged::deserialize(d)? {
                Tagged::Null => Self::Null,
                Tagged::Bool(b) => Self::Bool(b),
                Tagged::Int(i) => Self::Int(i),
                Tagged::Num(n) => Self::Num(n),
                Tagged::Str(v) => Self::Str(v),
                Tagged::List(l) => Self::List(l),
            })
        }
    }
}

impl JsonSchema for Scalar {
    fn schema_name() -> Cow<'static, str> {
        "Scalar".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "type": ["null", "boolean", "integer", "number", "string", "array"]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_round_trip() {
        let v: Scalar = serde_json::from_str("[1, 2.5, \"a\", true, null]").unwrap_or_default();
        assert_eq!(
            v,
            Scalar::List(vec![
                Scalar::Int(1),
                Scalar::Num(2.5),
                Scalar::Str("a".into()),
                Scalar::Bool(true),
                Scalar::Null
            ])
        );
        assert_eq!(
            serde_json::to_string(&v).unwrap_or_default(),
            "[1,2.5,\"a\",true,null]"
        );
    }

    #[test]
    fn binary_round_trip() {
        let v = Scalar::List(vec![Scalar::Int(-3), Scalar::Str("x".into())]);
        let bytes = postcard::to_allocvec(&v).unwrap_or_default();
        assert_eq!(postcard::from_bytes::<Scalar>(&bytes).ok(), Some(v));
    }

    #[test]
    fn comparisons() {
        assert!(Scalar::Int(1).loose_eq(&Scalar::Num(1.0)));
        assert_eq!(
            Scalar::Int(2).compare(&Scalar::Num(1.5)),
            Some(Ordering::Greater)
        );
        assert_eq!(Scalar::Str("a".into()).compare(&Scalar::Int(1)), None);
    }
}
