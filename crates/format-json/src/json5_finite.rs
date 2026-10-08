//! Keep the existing JSON5-to-JSON visitor while refusing nonfinite numbers.
//!
//! Wrapping every array element and object value checks numbers before
//! `serde_json::Value` can turn them into null or discard an overwritten value.
//! Map keys are delegated unchanged, including serde_json's private key handling.

use std::fmt;

use serde::de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};
use serde_json::Value;

pub(crate) fn from_str(source: &str) -> Result<Value, json5::Error> {
    json5::from_str::<FiniteValue>(source).map(|value| value.0)
}

struct FiniteValue(Value);

impl<'de> Deserialize<'de> for FiniteValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        Value::deserialize(FiniteDeserializer(deserializer)).map(Self)
    }
}

struct FiniteDeserializer<D>(D);

macro_rules! delegate_deserializer {
    ($($method:ident($($argument:ident: $argument_type:ty),*)),* $(,)?) => {
        $(
            fn $method<V>(self, $($argument: $argument_type,)* visitor: V)
                -> Result<V::Value, Self::Error>
            where
                V: Visitor<'de>,
            {
                self.0.$method($($argument,)* FiniteVisitor(visitor))
            }
        )*
    };
}

impl<'de, D> Deserializer<'de> for FiniteDeserializer<D>
where
    D: Deserializer<'de>,
{
    type Error = D::Error;

    // Preserve each requested method rather than redirecting everything to any.
    delegate_deserializer! {
        deserialize_any(),
        deserialize_bool(),
        deserialize_i8(),
        deserialize_i16(),
        deserialize_i32(),
        deserialize_i64(),
        deserialize_i128(),
        deserialize_u8(),
        deserialize_u16(),
        deserialize_u32(),
        deserialize_u64(),
        deserialize_u128(),
        deserialize_f32(),
        deserialize_f64(),
        deserialize_char(),
        deserialize_str(),
        deserialize_string(),
        deserialize_bytes(),
        deserialize_byte_buf(),
        deserialize_option(),
        deserialize_unit(),
        deserialize_unit_struct(name: &'static str),
        deserialize_newtype_struct(name: &'static str),
        deserialize_seq(),
        deserialize_tuple(len: usize),
        deserialize_tuple_struct(name: &'static str, len: usize),
        deserialize_map(),
        deserialize_struct(name: &'static str, fields: &'static [&'static str]),
        deserialize_enum(name: &'static str, variants: &'static [&'static str]),
        deserialize_identifier(),
        deserialize_ignored_any(),
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

struct FiniteVisitor<V>(V);

macro_rules! delegate_scalar {
    ($($method:ident($value_type:ty)),* $(,)?) => {
        $(
            fn $method<E>(self, value: $value_type) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.0.$method(value)
            }
        )*
    };
}

impl<'de, V> Visitor<'de> for FiniteVisitor<V>
where
    V: Visitor<'de>,
{
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        self.0.expecting(formatter)
    }

    delegate_scalar! {
        visit_bool(bool),
        visit_i8(i8),
        visit_i16(i16),
        visit_i32(i32),
        visit_i64(i64),
        visit_i128(i128),
        visit_u8(u8),
        visit_u16(u16),
        visit_u32(u32),
        visit_u64(u64),
        visit_u128(u128),
        visit_char(char),
        visit_str(&str),
        visit_borrowed_str(&'de str),
        visit_string(String),
        visit_bytes(&[u8]),
        visit_borrowed_bytes(&'de [u8]),
        visit_byte_buf(Vec<u8>),
    }

    fn visit_f32<E>(self, value: f32) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.is_finite() {
            self.0.visit_f32(value)
        } else {
            Err(E::custom("JSON5 number must be finite"))
        }
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        if value.is_finite() {
            self.0.visit_f64(value)
        } else {
            Err(E::custom("JSON5 number must be finite"))
        }
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.0.visit_none()
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.0.visit_unit()
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.0.visit_some(FiniteDeserializer(deserializer))
    }

    fn visit_newtype_struct<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.0
            .visit_newtype_struct(FiniteDeserializer(deserializer))
    }

    fn visit_seq<A>(self, access: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        self.0.visit_seq(FiniteSeqAccess(access))
    }

    fn visit_map<A>(self, access: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        self.0.visit_map(FiniteMapAccess(access))
    }

    // This private adapter is used only for Value. Its visitor does not request
    // enums; delegating preserves its existing unsupported-type error.
    fn visit_enum<A>(self, access: A) -> Result<Self::Value, A::Error>
    where
        A: EnumAccess<'de>,
    {
        self.0.visit_enum(access)
    }
}

struct FiniteSeed<T>(T);

impl<'de, T> DeserializeSeed<'de> for FiniteSeed<T>
where
    T: DeserializeSeed<'de>,
{
    type Value = T::Value;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        self.0.deserialize(FiniteDeserializer(deserializer))
    }
}

struct FiniteSeqAccess<A>(A);

impl<'de, A> SeqAccess<'de> for FiniteSeqAccess<A>
where
    A: SeqAccess<'de>,
{
    type Error = A::Error;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.0.next_element_seed(FiniteSeed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct FiniteMapAccess<A>(A);

impl<'de, A> MapAccess<'de> for FiniteMapAccess<A>
where
    A: MapAccess<'de>,
{
    type Error = A::Error;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        self.0.next_key_seed(seed)
    }

    fn next_value_seed<T>(&mut self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        self.0.next_value_seed(FiniteSeed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}
