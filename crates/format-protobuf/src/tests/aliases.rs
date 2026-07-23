use ir::{Instance, Value};

use crate::{DefaultValue, Layout};

use super::{decode, encode, error_text, group, parse, scalar};

#[test]
fn proto2_aliases_encode_by_every_name_and_decode_to_the_first_name() {
    let layout = parse(
        r#"
        syntax = "proto2";
        enum State {
          option allow_alias = true;
          UNKNOWN = 0;
          STARTED = 1;
          RUNNING = 1;
          ACTIVE = 1;
        }
        message Envelope {
          optional State status = 1;
          optional State fallback = 2 [default = ACTIVE];
        }
        "#,
    );
    let enumeration = match layout.enums().first() {
        Some(enumeration) => enumeration,
        None => panic!("State should be present in the layout"),
    };
    assert!(enumeration.allows_aliases());
    assert_eq!(
        enumeration.value_by_number(1).map(|value| value.name()),
        Some("STARTED")
    );
    for alias in ["STARTED", "RUNNING", "ACTIVE"] {
        assert_eq!(
            enumeration.value_by_name(alias).map(|value| value.number()),
            Some(1)
        );
        assert_eq!(
            encode(
                &layout,
                "Envelope",
                &group(vec![("status", scalar(Value::String(alias.to_string())),)]),
            ),
            [0x08, 0x01]
        );
    }

    let envelope_id = match layout.resolve_message("Envelope") {
        Ok(id) => id,
        Err(error) => panic!("Envelope should resolve: {error}"),
    };
    let envelope = match layout.message(envelope_id) {
        Some(message) => message,
        None => panic!("resolved Envelope id should belong to the layout"),
    };
    assert_eq!(
        envelope.field("fallback").and_then(|field| field.default()),
        Some(&DefaultValue::Enum(1))
    );

    let decoded = decode(&layout, "Envelope", &[0x08, 0x01]);
    let number = decoded
        .field("status")
        .and_then(Instance::as_scalar)
        .and_then(|value| match value {
            Value::Int(number) => i32::try_from(*number).ok(),
            _ => None,
        });
    assert_eq!(
        number
            .and_then(|number| enumeration.value_by_number(number))
            .map(|value| value.name()),
        Some("STARTED")
    );
}

#[test]
fn proto3_alias_option_is_order_independent_and_preserves_canonical_name() {
    let layout = parse(
        r#"
        syntax = "proto3";
        enum State {
          UNSPECIFIED = 0;
          READY = 1;
          ACTIVE = 1;
          option allow_alias = true;
        }
        message Envelope { State status = 1; }
        "#,
    );
    let enumeration = match layout.enums().first() {
        Some(enumeration) => enumeration,
        None => panic!("State should be present in the layout"),
    };
    assert!(enumeration.allows_aliases());
    assert_eq!(
        enumeration.value_by_number(1).map(|value| value.name()),
        Some("READY")
    );
    for alias in ["READY", "ACTIVE"] {
        assert_eq!(
            encode(
                &layout,
                "Envelope",
                &group(vec![("status", scalar(Value::String(alias.to_string())),)]),
            ),
            [0x08, 0x01]
        );
    }
}

#[test]
fn duplicate_enum_numbers_require_one_true_allow_alias_option() {
    let cases = [
        (
            r#"syntax = "proto2"; enum E { A = 0; B = 0; } message M {}"#,
            "without `option allow_alias = true`",
        ),
        (
            r#"syntax = "proto3"; enum E { ZERO = 0; A = 1; B = 1; } message M {}"#,
            "without `option allow_alias = true`",
        ),
        (
            r#"syntax = "proto2"; enum E { option allow_alias = false; A = 0; B = 0; } message M {}"#,
            "without `option allow_alias = true`",
        ),
        (
            r#"syntax = "proto2"; enum E { option allow_alias = maybe; A = 0; } message M {}"#,
            "must be true or false",
        ),
        (
            r#"syntax = "proto2"; enum E { option allow_alias = true; option allow_alias = true; A = 0; } message M {}"#,
            "declares `allow_alias` more than once",
        ),
        (
            r#"syntax = "proto2"; enum E { option allow_alias = true; A = 0; A = 0; } message M {}"#,
            "duplicate value `A`",
        ),
    ];
    for (source, expected) in cases {
        let error = error_text(Layout::parse(source));
        assert!(
            error.contains(expected),
            "`{error}` should contain `{expected}`"
        );
    }
}
