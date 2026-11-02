//! Keep live JSON number transport out of Serde's untyped tagged-enum buffer.
//!
//! With arbitrary precision, serde_json sends some numbers through a synthetic
//! map. Its key forwards a newtype request to `visit_borrowed_str`; real JSON
//! object keys instead call `visit_newtype_struct`. Check that distinction
//! before buffering, never just the private marker's spelling. Once another
//! deserializer has erased it, an ambiguous map must remain an ordinary map.
//! The dispatch is serde_json implementation behavior, not a general Serde
//! guarantee; keep the independent consumers when upgrading dependencies.

use core::fmt;

use serde::Deserializer;
use serde::de::{self, DeserializeSeed, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor};

pub(crate) struct NumberDeserializer<D>(pub(crate) D);

macro_rules! deserialize_method {
    ($($method:ident $(($($argument:ident: $ty:ty),*))?;)*) => {$ (
        fn $method<V>(self, $($($argument: $ty,)*)? visitor: V) -> Result<V::Value, D::Error>
        where V: Visitor<'de> {
            self.0.$method($($($argument,)*)? NumberVisitor::<_, false>(visitor))
        }
    )*};
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for NumberDeserializer<D> {
    type Error = D::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
        self.0.deserialize_any(NumberVisitor::<_, true>(visitor))
    }

    deserialize_method! {
        deserialize_bool; deserialize_i8; deserialize_i16; deserialize_i32; deserialize_i64;
        deserialize_i128; deserialize_u8; deserialize_u16; deserialize_u32; deserialize_u64;
        deserialize_u128; deserialize_f32; deserialize_f64; deserialize_char; deserialize_str;
        deserialize_string; deserialize_bytes; deserialize_byte_buf; deserialize_option;
        deserialize_unit; deserialize_unit_struct(name: &'static str);
        deserialize_newtype_struct(name: &'static str); deserialize_seq;
        deserialize_tuple(len: usize); deserialize_tuple_struct(name: &'static str, len: usize);
        deserialize_map; deserialize_struct(name: &'static str, fields: &'static [&'static str]);
        deserialize_enum(name: &'static str, variants: &'static [&'static str]);
        deserialize_identifier; deserialize_ignored_any;
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

struct NumberSeed<S>(S);

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for NumberSeed<S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.deserialize(NumberDeserializer(deserializer))
    }
}

struct NumberVisitor<V, const UNTYPED: bool>(V);

macro_rules! visit_scalar {
    ($($method:ident($ty:ty);)*) => {$ (
        fn $method<E: de::Error>(self, value: $ty) -> Result<Self::Value, E> {
            self.0.$method(value)
        }
    )*};
}

impl<'de, V: Visitor<'de>, const UNTYPED: bool> Visitor<'de> for NumberVisitor<V, UNTYPED> {
    type Value = V::Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(formatter)
    }

    visit_scalar! {
        visit_bool(bool); visit_i8(i8); visit_i16(i16); visit_i32(i32); visit_i64(i64);
        visit_u8(u8); visit_u16(u16); visit_u32(u32); visit_u64(u64);
        visit_f32(f32); visit_f64(f64); visit_char(char); visit_str(&str);
        visit_borrowed_str(&'de str); visit_string(String); visit_bytes(&[u8]);
        visit_borrowed_bytes(&'de [u8]); visit_byte_buf(Vec<u8>);
    }

    fn visit_i128<E: de::Error>(self, value: i128) -> Result<Self::Value, E> {
        if UNTYPED {
            if let Ok(value) = i64::try_from(value) {
                return self.0.visit_i64(value);
            }
            if let Ok(value) = u64::try_from(value) {
                return self.0.visit_u64(value);
            }
            return self.0.visit_f64(value as f64);
        }
        self.0.visit_i128(value)
    }

    fn visit_u128<E: de::Error>(self, value: u128) -> Result<Self::Value, E> {
        if UNTYPED {
            if let Ok(value) = u64::try_from(value) {
                return self.0.visit_u64(value);
            }
            return self.0.visit_f64(value as f64);
        }
        self.0.visit_u128(value)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_unit()
    }

    fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
        self.0.visit_none()
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<Self::Value, D::Error> {
        self.0.visit_some(NumberDeserializer(deserializer))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        self.0
            .visit_newtype_struct(NumberDeserializer(deserializer))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, sequence: A) -> Result<Self::Value, A::Error> {
        self.0.visit_seq(NumberSequence(sequence))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let first = if UNTYPED {
            match map.next_key_seed(LiveKeySeed)? {
                Some(MapKey::Number) => {
                    let lexical = map.next_value::<String>()?;
                    if map.next_key::<de::IgnoredAny>()?.is_some() {
                        return Err(de::Error::custom("invalid live number transport"));
                    }
                    let number = lexical.parse::<f64>().map_err(de::Error::custom)?;
                    return self.0.visit_f64(number);
                }
                Some(MapKey::Ordinary(key)) => Some(key),
                None => None,
            }
        } else {
            None
        };
        self.0.visit_map(NumberMap { map, first })
    }

    fn visit_enum<A: EnumAccess<'de>>(self, value: A) -> Result<Self::Value, A::Error> {
        self.0.visit_enum(NumberEnum(value))
    }
}

struct NumberSequence<A>(A);

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for NumberSequence<A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.0.next_element_seed(NumberSeed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

struct NumberMap<A> {
    map: A,
    first: Option<Key>,
}

impl<'de, A: MapAccess<'de>> MapAccess<'de> for NumberMap<A> {
    type Error = A::Error;

    fn next_key_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        match self.first.take() {
            Some(key) => seed.deserialize(key).map(Some).map_err(de::Error::custom),
            None => self.map.next_key_seed(NumberSeed(seed)),
        }
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.map.next_value_seed(NumberSeed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.map
            .size_hint()
            .map(|hint| hint.saturating_add(usize::from(self.first.is_some())))
    }
}

struct NumberEnum<A>(A);
struct NumberVariant<A>(A);

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for NumberEnum<A> {
    type Error = A::Error;
    type Variant = NumberVariant<A::Variant>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Self::Variant), A::Error> {
        self.0
            .variant_seed(NumberSeed(seed))
            .map(|(value, variant)| (value, NumberVariant(variant)))
    }
}

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for NumberVariant<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.0.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.0.newtype_variant_seed(NumberSeed(seed))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.0
            .tuple_variant(len, NumberVisitor::<_, false>(visitor))
    }

    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.0
            .struct_variant(fields, NumberVisitor::<_, false>(visitor))
    }
}

enum MapKey {
    Number,
    Ordinary(Key),
}

// JSON keys are strings. Bytes and integer identifiers also keep the ordinary
// Serde identifier paths usable without introducing a JSON dependency in IR.
enum Key {
    String(String),
    Bytes(Vec<u8>),
    U64(u64),
    I64(i64),
}

struct LiveKeySeed;
struct KeyVisitor {
    live: bool,
}

impl<'de> DeserializeSeed<'de> for LiveKeySeed {
    type Value = MapKey;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<MapKey, D::Error> {
        deserializer
            // Deliberately not serde_json's special RawValue newtype name.
            .deserialize_newtype_struct("FerruleNumberTransportKey", KeyVisitor { live: true })
    }
}

impl<'de> Visitor<'de> for KeyVisitor {
    type Value = MapKey;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a map field name")
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<MapKey, D::Error> {
        deserializer.deserialize_any(KeyVisitor { live: false })
    }

    fn visit_borrowed_str<E: de::Error>(self, key: &'de str) -> Result<MapKey, E> {
        if self.live && key == "$serde_json::private::Number" {
            Ok(MapKey::Number)
        } else {
            self.visit_str(key)
        }
    }

    fn visit_str<E: de::Error>(self, key: &str) -> Result<MapKey, E> {
        self.visit_string(key.to_owned())
    }

    fn visit_string<E: de::Error>(self, key: String) -> Result<MapKey, E> {
        Ok(MapKey::Ordinary(Key::String(key)))
    }

    fn visit_bytes<E: de::Error>(self, key: &[u8]) -> Result<MapKey, E> {
        self.visit_byte_buf(key.to_vec())
    }

    fn visit_byte_buf<E: de::Error>(self, key: Vec<u8>) -> Result<MapKey, E> {
        Ok(MapKey::Ordinary(Key::Bytes(key)))
    }

    fn visit_u64<E: de::Error>(self, key: u64) -> Result<MapKey, E> {
        Ok(MapKey::Ordinary(Key::U64(key)))
    }

    fn visit_i64<E: de::Error>(self, key: i64) -> Result<MapKey, E> {
        Ok(MapKey::Ordinary(Key::I64(key)))
    }
}

impl<'de> Deserializer<'de> for Key {
    type Error = de::value::Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Self::Error> {
        match self {
            Self::String(key) => visitor.visit_string(key),
            Self::Bytes(key) => visitor.visit_byte_buf(key),
            Self::U64(key) => visitor.visit_u64(key),
            Self::I64(key) => visitor.visit_i64(key),
        }
    }

    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error> {
        // Stacked adapters must never mistake a replayed literal marker key
        // for the live synthetic transport. Do not forward this to `any`.
        visitor.visit_newtype_struct(self)
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct seq tuple tuple_struct map struct
        enum identifier ignored_any
    }
}

#[cfg(test)]
mod tests;
