use serde::{Deserialize, Deserializer};

use super::{Key, LiveKeySeed, MapKey, NumberDeserializer};
use crate::FiniteF64;

#[derive(Debug, Deserialize)]
#[serde(tag = "tag", rename_all = "snake_case")]
enum Buffered {
    Value { value: FiniteF64 },
}

#[test]
fn stacked_adapters_do_not_reclassify_a_replayed_literal_key() {
    for value in [
        r#"{"$serde_json::private::Number":"1.5"}"#,
        r#"{"\u0024serde_json::private::Number":"\u0031.5"}"#,
        r#"{"$serde_json::private::Number":"1.5","extra":true}"#,
    ] {
        let text = format!(r#"{{"tag":"value","value":{value}}}"#);
        let mut source = serde_json::Deserializer::from_str(&text);
        let result = Buffered::deserialize(NumberDeserializer(NumberDeserializer(
            NumberDeserializer(&mut source),
        )));
        assert!(result.is_err(), "accepted {text}");
    }
    let mut source = serde_json::Deserializer::from_str(r#"{"tag":"value","value":1.5}"#);
    let Buffered::Value { value } = Buffered::deserialize(NumberDeserializer(NumberDeserializer(
        NumberDeserializer(&mut source),
    )))
    .unwrap();
    assert_eq!(value.get(), 1.5);
}

#[test]
fn replayed_keys_keep_the_ordinary_newtype_dispatch() {
    use serde::de::DeserializeSeed;
    let key = Key::String("$serde_json::private::Number".into());
    assert!(matches!(
        LiveKeySeed.deserialize(key).unwrap(),
        MapKey::Ordinary(_)
    ));
}

#[test]
fn typed_wide_integer_requests_are_forwarded_without_rounding() {
    use serde::de::value::{Error, I128Deserializer, U128Deserializer};
    for value in [0, u64::MAX as u128, u64::MAX as u128 + 1, u128::MAX] {
        let actual =
            u128::deserialize(NumberDeserializer(U128Deserializer::<Error>::new(value))).unwrap();
        assert_eq!(actual, value);
    }
    for value in [i128::MIN, i64::MIN as i128, i64::MAX as i128, i128::MAX] {
        let actual =
            i128::deserialize(NumberDeserializer(I128Deserializer::<Error>::new(value))).unwrap();
        assert_eq!(actual, value);
    }
    struct Untyped;
    impl<'de> serde::de::Visitor<'de> for Untyped {
        type Value = u64;
        fn expecting(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
            formatter.write_str("an exact u64")
        }
        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<u64, E> {
            Ok(value)
        }
    }
    let actual = NumberDeserializer(U128Deserializer::<Error>::new(u64::MAX as u128))
        .deserialize_any(Untyped)
        .unwrap();
    assert_eq!(actual, u64::MAX);
}
