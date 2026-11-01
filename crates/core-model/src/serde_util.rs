//! Serde helpers that accept YAML shorthands in human-readable formats while
//! staying compatible with postcard (which cannot handle untagged enums).

use std::borrow::Cow;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A list that may be written as a single value in YAML (`x` or `[x, y]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct OneOrMany<T>(pub Vec<T>);

impl<T> OneOrMany<T> {
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }
}

impl<T: Serialize> Serialize for OneOrMany<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(s)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum OneOrManyRepr<T> {
    Many(Vec<T>),
    One(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for OneOrMany<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            Ok(match OneOrManyRepr::deserialize(d)? {
                OneOrManyRepr::Many(v) => Self(v),
                OneOrManyRepr::One(v) => Self(vec![v]),
            })
        } else {
            Vec::deserialize(d).map(Self)
        }
    }
}

impl<T: JsonSchema> JsonSchema for OneOrMany<T> {
    fn schema_name() -> Cow<'static, str> {
        format!("OneOrMany_{}", T::schema_name()).into()
    }
    fn json_schema(generator: &mut SchemaGenerator) -> Schema {
        let item = generator.subschema_for::<T>();
        json_schema!({
            "anyOf": [ item, { "type": "array", "items": item } ]
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_both_forms() {
        let one: OneOrMany<String> = serde_json::from_str("\"a\"").unwrap_or_default();
        let many: OneOrMany<String> = serde_json::from_str("[\"a\",\"b\"]").unwrap_or_default();
        assert_eq!(one.0, vec!["a"]);
        assert_eq!(many.0, vec!["a", "b"]);
        let bytes = postcard::to_allocvec(&many).unwrap_or_default();
        assert_eq!(
            postcard::from_bytes::<OneOrMany<String>>(&bytes).ok(),
            Some(many)
        );
    }
}
