//! Instance identifiers: 16 random bytes, shown as lowercase base32 without padding.

use std::borrow::Cow;
use std::fmt;
use std::str::FromStr;

use data_encoding::BASE32_NOPAD;
use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::strings::ParseError;

/// 16-byte instance identifier.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id([u8; 16]);

impl Id {
    pub const fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// A fresh random id from the operating system's generator.
    pub fn random() -> Result<Self, getrandom::Error> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes)?;
        Ok(Self(bytes))
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&BASE32_NOPAD.encode(&self.0).to_ascii_lowercase())
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({self})")
    }
}

impl FromStr for Id {
    type Err = ParseError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || ParseError {
            kind: "id",
            value: s.to_owned(),
            reason: "expected 26 lowercase base32 characters",
        };
        let bytes = BASE32_NOPAD
            .decode(s.to_ascii_uppercase().as_bytes())
            .map_err(|_| err())?;
        let arr: [u8; 16] = bytes.try_into().map_err(|_| err())?;
        Ok(Self(arr))
    }
}

impl Serialize for Id {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(self)
        } else {
            self.0.serialize(s)
        }
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            let raw = String::deserialize(d)?;
            raw.parse().map_err(serde::de::Error::custom)
        } else {
            <[u8; 16]>::deserialize(d).map(Self)
        }
    }
}

impl JsonSchema for Id {
    fn schema_name() -> Cow<'static, str> {
        "Id".into()
    }
    fn json_schema(_: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "string", "pattern": "^[a-z2-7]{26}$" })
    }
}

macro_rules! typed_id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Serialize, Deserialize, JsonSchema)]
        #[serde(transparent)]
        pub struct $name(pub Id);

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }
    };
}

typed_id!(
    /// Identifier of a person.
    PersonId
);
typed_id!(
    /// Identifier of a unit instance.
    UnitId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_parse_round_trip() {
        let id = Id::from_bytes([7; 16]);
        let s = id.to_string();
        assert_eq!(s.len(), 26);
        assert!(
            s.chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
        );
        assert_eq!(s.parse::<Id>(), Ok(id));
        assert!("short".parse::<Id>().is_err());
    }

    #[test]
    fn random_ids_differ() {
        let a = Id::random();
        let b = Id::random();
        assert!(matches!((a, b), (Ok(a), Ok(b)) if a != b));
    }

    #[test]
    fn serde_human_and_binary() {
        let id = Id::from_bytes([1; 16]);
        let json = serde_json::to_string(&id).unwrap_or_default();
        assert_eq!(json, format!("\"{id}\""));
        let bin = postcard::to_allocvec(&id).unwrap_or_default();
        assert_eq!(bin.len(), 16);
        assert_eq!(postcard::from_bytes::<Id>(&bin).ok(), Some(id));
    }
}
