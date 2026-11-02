//! JSON ignores Serde container names. Probe the public Serde interface directly
//! so helper type names cannot silently change a name-sensitive format's wire.

use ir::{
    GroupAlternativeConstraintValue, IntegerRange, JsonAllowedValue, JsonSchemaPredicate,
    NumericRange, ScalarType, SchemaKind,
};
use serde::de::{self, Expected, Visitor};
use serde::ser::{self, Impossible, SerializeStruct};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

type Error = de::value::Error;

struct ContainerName;
struct NamedStruct(&'static str);

macro_rules! refuse_serialization {
    ($($method:ident($($argument:ident: $ty:ty),*) -> $output:ty;)*) => {$ (
        fn $method(self, $($argument: $ty),*) -> Result<$output, Error> {
            $(let _ = $argument;)*
            Err(ser::Error::custom("expected a named struct token"))
        }
    )*};
}

impl Serializer for ContainerName {
    type Ok = &'static str;
    type Error = Error;
    type SerializeSeq = Impossible<Self::Ok, Error>;
    type SerializeTuple = Impossible<Self::Ok, Error>;
    type SerializeTupleStruct = Impossible<Self::Ok, Error>;
    type SerializeTupleVariant = Impossible<Self::Ok, Error>;
    type SerializeMap = Impossible<Self::Ok, Error>;
    type SerializeStruct = NamedStruct;
    type SerializeStructVariant = Impossible<Self::Ok, Error>;

    fn serialize_struct(self, name: &'static str, _len: usize) -> Result<NamedStruct, Error> {
        Ok(NamedStruct(name))
    }

    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<Self::Ok, Error> {
        Err(ser::Error::custom("expected a named struct token"))
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Error> {
        Err(ser::Error::custom("expected a named struct token"))
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<Self::Ok, Error> {
        Err(ser::Error::custom("expected a named struct token"))
    }

    refuse_serialization! {
        serialize_bool(value: bool) -> Self::Ok;
        serialize_i8(value: i8) -> Self::Ok;
        serialize_i16(value: i16) -> Self::Ok;
        serialize_i32(value: i32) -> Self::Ok;
        serialize_i64(value: i64) -> Self::Ok;
        serialize_i128(value: i128) -> Self::Ok;
        serialize_u8(value: u8) -> Self::Ok;
        serialize_u16(value: u16) -> Self::Ok;
        serialize_u32(value: u32) -> Self::Ok;
        serialize_u64(value: u64) -> Self::Ok;
        serialize_u128(value: u128) -> Self::Ok;
        serialize_f32(value: f32) -> Self::Ok;
        serialize_f64(value: f64) -> Self::Ok;
        serialize_char(value: char) -> Self::Ok;
        serialize_str(value: &str) -> Self::Ok;
        serialize_bytes(value: &[u8]) -> Self::Ok;
        serialize_none() -> Self::Ok;
        serialize_unit() -> Self::Ok;
        serialize_unit_struct(name: &'static str) -> Self::Ok;
        serialize_unit_variant(name: &'static str, index: u32, variant: &'static str) -> Self::Ok;
        serialize_seq(len: Option<usize>) -> Self::SerializeSeq;
        serialize_tuple(len: usize) -> Self::SerializeTuple;
        serialize_tuple_struct(name: &'static str, len: usize) -> Self::SerializeTupleStruct;
        serialize_tuple_variant(name: &'static str, index: u32, variant: &'static str, len: usize) -> Self::SerializeTupleVariant;
        serialize_map(len: Option<usize>) -> Self::SerializeMap;
        serialize_struct_variant(name: &'static str, index: u32, variant: &'static str, len: usize) -> Self::SerializeStructVariant;
    }
}

impl SerializeStruct for NamedStruct {
    type Ok = &'static str;
    type Error = Error;

    fn serialize_field<T: ?Sized + Serialize>(
        &mut self,
        _key: &'static str,
        _value: &T,
    ) -> Result<(), Error> {
        Ok(())
    }

    fn end(self) -> Result<Self::Ok, Error> {
        Ok(self.0)
    }
}

enum ContainerProbe {
    Struct(&'static str),
    InternalTag(&'static str),
}

impl<'de> Deserializer<'de> for ContainerProbe {
    type Error = Error;

    fn deserialize_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        _fields: &'static [&'static str],
        _visitor: V,
    ) -> Result<V::Value, Error> {
        let Self::Struct(expected) = self else {
            return Err(de::Error::custom(
                "expected the internal-tag untyped request",
            ));
        };
        if name != expected {
            return Err(de::Error::custom(format!(
                "container {name}, expected {expected}"
            )));
        }
        Err(de::Error::custom(
            "observed the original named struct request",
        ))
    }

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        let Self::InternalTag(expected) = self else {
            return Err(de::Error::custom("expected a named struct request"));
        };
        // Internally tagged derives request deserialize_any, not a named
        // deserialize_struct. Its visitor must still describe the public type,
        // as it did before the remote helper was introduced.
        let description = (&visitor as &dyn Expected).to_string();
        let expected = format!("internally tagged enum {expected}");
        if description != expected {
            return Err(de::Error::custom(format!(
                "visitor {description}, expected {expected}"
            )));
        }
        Err(de::Error::custom(
            "observed the original internal-tag visitor",
        ))
    }

    serde::forward_to_deserialize_any! {
        bool i8 i16 i32 i64 i128 u8 u16 u32 u64 u128 f32 f64 char str string
        bytes byte_buf option unit unit_struct newtype_struct seq tuple tuple_struct
        map enum identifier ignored_any
    }
}

fn assert_name<T: Serialize + for<'de> Deserialize<'de>>(
    value: T,
    expected: &'static str,
    tagged: bool,
) {
    assert_eq!(value.serialize(ContainerName).unwrap(), expected);
    let (probe, stop) = if tagged {
        (
            ContainerProbe::InternalTag(expected),
            "observed the original internal-tag visitor",
        )
    } else {
        (
            ContainerProbe::Struct(expected),
            "observed the original named struct request",
        )
    };
    // Intentional stop after observing the requested public Serde boundary;
    // this is a name/token probe, not a payload-decoding or JSON test.
    let result = T::deserialize(probe);
    assert_eq!(result.err().unwrap().to_string(), stop);
}

#[test]
fn numeric_range_uses_its_original_container_name() {
    assert_name(
        NumericRange::Integer(IntegerRange::new(Some(1), None).unwrap()),
        "NumericRange",
        false,
    );
}

#[test]
fn allowed_value_uses_its_original_container_name() {
    assert_name(JsonAllowedValue::Int(1), "JsonAllowedValue", false);
}

#[test]
fn alternative_constraint_uses_its_original_container_name() {
    assert_name(
        GroupAlternativeConstraintValue::Int(1),
        "GroupAlternativeConstraintValue",
        false,
    );
}

#[test]
fn schema_kind_keeps_its_public_internal_tag_name() {
    assert_name(
        SchemaKind::Scalar {
            ty: ScalarType::Float,
        },
        "SchemaKind",
        true,
    );
}

#[test]
fn schema_predicate_keeps_its_public_internal_tag_name() {
    assert_name(JsonSchemaPredicate::Never, "JsonSchemaPredicate", true);
}
