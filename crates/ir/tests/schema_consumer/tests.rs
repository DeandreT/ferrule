#![cfg(test)]

mod attacks;
mod compatibility;
mod metadata;

use ir::*;
use serde::de::DeserializeOwned;

fn routes<T: DeserializeOwned>(text: &str) -> Vec<T> {
    let mut values = live_routes(text);
    values.push(
        serde_json::from_value(serde_json::from_str::<serde_json::Value>(text).unwrap()).unwrap(),
    );
    values
}

fn live_routes<T: DeserializeOwned>(text: &str) -> Vec<T> {
    let mut stream = serde_json::Deserializer::from_str(text).into_iter::<T>();
    let first = stream.next().unwrap().unwrap();
    assert!(stream.next().is_none());
    vec![
        serde_json::from_str(text).unwrap(),
        serde_json::from_slice(text.as_bytes()).unwrap(),
        serde_json::from_reader(text.as_bytes()).unwrap(),
        first,
    ]
}

fn reversed(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .rev()
                .map(|(key, value)| (key, reversed(value)))
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(reversed).collect())
        }
        other => other,
    }
}

#[test]
fn feature_is_selected_only_by_this_external_consumer() {
    assert_eq!(
        serde_json::from_str::<serde_json::Number>("1e9999").is_ok(),
        cfg!(feature = "arbitrary_precision")
    );
}

#[test]
fn every_schema_metadata_field_and_finite_bit_survives_all_routes_and_tag_orders() {
    let expected = metadata::expected();
    let text = serde_json::to_string(&expected).unwrap();
    let backwards =
        serde_json::to_string(&reversed(serde_json::to_value(&expected).unwrap())).unwrap();
    for text in [&text, &backwards] {
        for actual in routes::<SchemaNode>(text) {
            metadata::assert_exact(&actual, &expected);
        }
    }
    // The public kind itself also owns a buffer, outside SchemaNode's entry.
    for text in [
        serde_json::to_string(&expected.kind).unwrap(),
        serde_json::to_string(&reversed(serde_json::to_value(&expected.kind).unwrap())).unwrap(),
    ] {
        for actual in routes::<SchemaKind>(&text) {
            let mut node = expected.clone();
            node.kind = actual;
            metadata::assert_exact(&node, &expected);
        }
    }
    let predicate = JsonSchemaPredicate::schema(expected.clone());
    for text in [
        serde_json::to_string(&predicate).unwrap(),
        serde_json::to_string(&reversed(serde_json::to_value(&predicate).unwrap())).unwrap(),
    ] {
        for actual in routes::<JsonSchemaPredicate>(&text) {
            metadata::assert_exact(actual.as_schema().unwrap(), &expected);
        }
    }
}

#[test]
fn direct_numeric_types_accept_real_tokens_without_losing_finite_bits() {
    for (token, expected) in [
        ("1.5", 1.5_f64),
        ("1.50", 1.5),
        ("1e0", 1.0),
        ("0", 0.0),
        ("-0.0", -0.0),
        ("5e-324", f64::from_bits(1)),
        ("1.7976931348623157e308", f64::MAX),
        ("-1.7976931348623157e308", -f64::MAX),
        ("18446744073709551616", 18_446_744_073_709_551_616.0),
        ("-170141183460469231731687303715884105728", i128::MIN as f64),
        ("1.50000000000000000000000000000000000001", 1.5),
    ] {
        for actual in routes::<FiniteF64>(token) {
            assert_eq!(actual.get().to_bits(), expected.to_bits(), "{token}");
        }
        let bound = format!(r#"{{"value":{token},"exclusive":true}}"#);
        for actual in routes::<NumberBound>(&bound) {
            assert_eq!(actual.value().get().to_bits(), expected.to_bits());
            assert!(actual.is_exclusive());
        }
        let bounds = format!(r#"{{"minimum":{{"value":{token}}}}}"#);
        for actual in routes::<NumberRange>(&bounds) {
            assert_eq!(
                actual.minimum().unwrap().value().get().to_bits(),
                expected.to_bits()
            );
        }
        for range in [
            format!(r#"{{"kind":"number","bounds":{bounds}}}"#),
            format!(r#"{{"bounds":{bounds},"kind":"number"}}"#),
        ] {
            for actual in routes::<NumericRange>(&range) {
                let NumericRange::Number(range) = actual else {
                    panic!("number range required");
                };
                assert_eq!(
                    range.minimum().unwrap().value().get().to_bits(),
                    expected.to_bits(),
                    "{token}"
                );
            }
        }
    }
    for bits in [0x0031_fa18_2c40_c60d, 0x0031_fa18_2c40_c60e] {
        let value = f64::from_bits(bits);
        let token = serde_json::to_string(&value).unwrap();
        for actual in routes::<NumericRange>(&format!(
            r#"{{"bounds":{{"minimum":{{"value":{token}}}}},"kind":"number"}}"#
        )) {
            let NumericRange::Number(range) = actual else {
                panic!("number range required");
            };
            assert_eq!(range.minimum().unwrap().value().get().to_bits(), bits);
        }
    }
}

#[test]
fn direct_adjacent_scalar_values_keep_both_tag_orders() {
    for text in [
        r#"{"type":"float","value":1.50}"#,
        r#"{"value":1.50,"type":"float"}"#,
    ] {
        for actual in routes::<JsonAllowedValue>(text) {
            assert_eq!(actual, JsonAllowedValue::Float(metadata::finite(1.5)));
        }
        for actual in routes::<GroupAlternativeConstraintValue>(text) {
            assert_eq!(
                actual,
                GroupAlternativeConstraintValue::Float(metadata::finite(1.5))
            );
        }
    }
}

#[test]
fn negative_integer_zero_has_an_explicit_upstream_value_limitation() {
    // Typed JSON requests preserve -0. Generic Number::deserialize_any in
    // serde_json/arbitrary_precision can classify its stored spelling as u64(0).
    for (text, typed) in [
        (
            r#"{"kind":"number","bounds":{"minimum":{"value":-0}}}"#,
            true,
        ),
        (
            r#"{"bounds":{"minimum":{"value":-0}},"kind":"number"}"#,
            false,
        ),
    ] {
        for actual in [
            serde_json::from_str::<NumericRange>(text).unwrap(),
            serde_json::from_slice(text.as_bytes()).unwrap(),
            serde_json::from_reader(text.as_bytes()).unwrap(),
        ] {
            let NumericRange::Number(range) = actual else {
                panic!("number range required");
            };
            assert_eq!(
                range.minimum().unwrap().value().get().to_bits(),
                if typed {
                    (-0.0_f64).to_bits()
                } else {
                    // Some serde_json versions already classify the raw -0
                    // token as i64(0) on an untyped read. That sign is gone
                    // before the bridge sees it; -0.0 is unambiguous instead.
                    serde_json::from_str::<serde_json::Number>("-0")
                        .unwrap()
                        .as_f64()
                        .unwrap()
                        .to_bits()
                }
            );
        }
    }
    let text = r#"{"bounds":{"minimum":{"value":-0}},"kind":"number"}"#;
    let actual: NumericRange =
        serde_json::from_value(serde_json::from_str::<serde_json::Value>(text).unwrap()).unwrap();
    let NumericRange::Number(range) = actual else {
        panic!("number range required");
    };
    let expected = if cfg!(feature = "arbitrary_precision") {
        0.0_f64
    } else {
        -0.0_f64
    };
    assert_eq!(
        range.minimum().unwrap().value().get().to_bits(),
        expected.to_bits()
    );
}
