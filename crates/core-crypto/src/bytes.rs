//! Serde for fixed-size byte arrays larger than 32 (serde only covers up to 32).

use std::fmt;

use serde::de::{self, SeqAccess, Visitor};
use serde::{Deserializer, Serializer};

pub fn serialize<S: Serializer, const N: usize>(v: &[u8; N], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bytes(v)
}

struct ArrayVisitor<const N: usize>;

impl<'de, const N: usize> Visitor<'de> for ArrayVisitor<N> {
    type Value = [u8; N];

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{N} bytes")
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<[u8; N], E> {
        v.try_into().map_err(|_| E::invalid_length(v.len(), &self))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<[u8; N], A::Error> {
        let mut out = [0u8; N];
        for (i, slot) in out.iter_mut().enumerate() {
            *slot = seq
                .next_element()?
                .ok_or_else(|| de::Error::invalid_length(i, &self))?;
        }
        Ok(out)
    }
}

pub fn deserialize<'de, D: Deserializer<'de>, const N: usize>(d: D) -> Result<[u8; N], D::Error> {
    d.deserialize_bytes(ArrayVisitor::<N>)
}
