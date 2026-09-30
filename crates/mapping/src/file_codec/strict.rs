//! Strict envelope parsing; legacy model parsing never uses this visitor.
use serde::de::{self, Deserialize, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};
use std::{collections::BTreeMap, fmt};

pub(super) fn has_marker(text: &str) -> bool {
    struct Probe<'a>(&'a mut bool);
    impl<'de> DeserializeSeed<'de> for Probe<'_> {
        type Value = ();
        fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<(), D::Error> {
            d.deserialize_map(self)
        }
    }
    impl<'de> Visitor<'de> for Probe<'_> {
        type Value = ();
        fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            f.write_str("a model object")
        }
        fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<(), M::Error> {
            while let Some(key) = map.next_key::<String>()? {
                if key == "__ferrule_file" {
                    *self.0 = true;
                    return Ok(());
                }
                map.next_value::<IgnoredAny>()?;
            }
            Ok(())
        }
    }
    let mut found = false;
    let _ = Probe(&mut found).deserialize(&mut serde_json::Deserializer::from_str(text));
    found
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Envelope {
    #[serde(rename = "__ferrule_file")]
    pub header: Header,
    pub document: StrictValue,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Header {
    pub kind: String,
    pub version: u64,
    pub float_bits: FloatBits,
}
pub(super) struct FloatBits(pub BTreeMap<String, String>);
impl<'de> Deserialize<'de> for FloatBits {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct BitsVisitor;
        impl<'de> Visitor<'de> for BitsVisitor {
            type Value = FloatBits;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a unique float pointer table")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut bits = BTreeMap::new();
                while let Some((key, value)) = map.next_entry::<String, String>()? {
                    if bits.insert(key, value).is_some() {
                        return Err(de::Error::custom("duplicate float pointer"));
                    }
                }
                Ok(FloatBits(bits))
            }
        }
        d.deserialize_map(BitsVisitor)
    }
}
pub(super) struct StrictValue(pub Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct ValueVisitor;
        impl<'de> Visitor<'de> for ValueVisitor {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON value without duplicate object members")
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Number(v.into())))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| de::Error::custom("non-finite JSON number"))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_seq<S: SeqAccess<'de>>(self, mut seq: S) -> Result<Self::Value, S::Error> {
                let mut values = Vec::new();
                while let Some(value) = seq.next_element::<StrictValue>()? {
                    values.push(value.0);
                }
                Ok(StrictValue(Value::Array(values)))
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut values = Map::new();
                while let Some((key, value)) = map.next_entry::<String, StrictValue>()? {
                    if values.insert(key, value.0).is_some() {
                        return Err(de::Error::custom("duplicate document member"));
                    }
                }
                Ok(StrictValue(Value::Object(values)))
            }
        }
        d.deserialize_any(ValueVisitor)
    }
}
