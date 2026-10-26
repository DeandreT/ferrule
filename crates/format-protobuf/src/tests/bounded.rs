use ir::{Instance, Value};

use crate::{
    BoundedEncodeError, EncodeLimit, EncodeLimits, ProtobufError, to_vec, to_vec_bounded,
    to_vec_message, to_vec_message_bounded,
};

use super::{group, parse, scalar};

// Every successful wire fixture also exercises both bounded entry points at
// the exact byte limit, including imported layouts.
pub(super) fn assert_parity(layout: &crate::Layout, root: &str, instance: &Instance, bytes: &[u8]) {
    let limits = limits(bytes.len());
    assert_eq!(
        to_vec_bounded(layout, root, instance, &limits).unwrap(),
        bytes
    );
    assert_eq!(
        to_vec_message_bounded(
            layout,
            layout.resolve_message(root).unwrap(),
            instance,
            &limits,
        )
        .unwrap(),
        bytes
    );
}

fn limits(bytes: usize) -> EncodeLimits {
    EncodeLimits {
        max_output_bytes: bytes,
        ..Default::default()
    }
}

fn assert_limit(result: Result<Vec<u8>, BoundedEncodeError>, expected: EncodeLimit) {
    assert!(
        matches!(result, Err(BoundedEncodeError::LimitExceeded { limit, .. }) if limit == expected),
        "expected {expected:?}, got {result:?}"
    );
}

#[test]
fn empty_output_accepts_zero_byte_budget_but_presence_still_costs_bytes() {
    let layout = parse(
        r#"syntax = "proto3";
        enum E { ZERO = 0; ONE = 1; }
        message Root {
          string text = 1; bytes data = 2; int32 count = 3;
          double ratio = 4; float small = 5; bool flag = 6; E state = 7;
          optional string present = 8;
        }"#,
    );
    let empty = group(vec![]);
    assert!(
        to_vec_bounded(&layout, "Root", &empty, &limits(0))
            .unwrap()
            .is_empty()
    );
    let defaults = group(vec![
        ("text", scalar(Value::String(String::new()))),
        ("data", scalar(Value::String(String::new()))),
        ("count", scalar(Value::Int(0))),
        ("ratio", scalar(Value::Float(-0.0))),
        ("small", scalar(Value::Float(-0.0))),
        ("flag", scalar(Value::Bool(false))),
        ("state", scalar(Value::String("ZERO".into()))),
        ("present", scalar(Value::Null)),
    ]);
    assert!(
        to_vec_bounded(&layout, "Root", &defaults, &limits(0))
            .unwrap()
            .is_empty()
    );
    let present = group(vec![("present", scalar(Value::String(String::new())))]);
    assert_eq!(
        to_vec_bounded(&layout, "Root", &present, &limits(2)).unwrap(),
        [0x42, 0]
    );
    assert_limit(
        to_vec_bounded(&layout, "Root", &present, &limits(0)),
        EncodeLimit::OutputBytes,
    );
}

#[test]
fn string_bytes_and_repeated_lengths_include_tags_and_varint_prefixes() {
    for ty in ["string", "bytes"] {
        for length in [0, 1, 127, 128, 16_383, 16_384] {
            for repeated in [false, true] {
                let cardinality = if repeated { "repeated" } else { "optional" };
                let layout = parse(&format!("message Root {{ {cardinality} {ty} data = 17; }}"));
                let value = scalar(Value::String("x".repeat(length)));
                let value = if repeated {
                    Instance::Repeated(vec![value.clone(), value])
                } else {
                    value
                };
                let instance = group(vec![("data", value)]);
                let expected = to_vec(&layout, "Root", &instance).unwrap();
                assert_eq!(
                    to_vec_bounded(&layout, "Root", &instance, &limits(expected.len())).unwrap(),
                    expected
                );
                assert_limit(
                    to_vec_bounded(&layout, "Root", &instance, &limits(expected.len() - 1)),
                    EncodeLimit::OutputBytes,
                );
            }
        }
    }
}

#[test]
fn nested_packed_and_empty_message_overhead_are_bounded() {
    let layout = parse(
        r#"syntax = "proto3";
        message Root { repeated Child children = 1; }
        message Child { repeated sint64 numbers = 1; optional Child child = 2; }"#,
    );
    let packed = Instance::MappedSequence((0..128).map(|n| scalar(Value::Int(-n))).collect());
    let child = group(vec![
        ("numbers", packed),
        (
            "child",
            group(vec![(
                "numbers",
                Instance::Repeated(vec![scalar(Value::Int(i64::MIN))]),
            )]),
        ),
    ]);
    let instance = group(vec![(
        "children",
        Instance::Repeated(vec![child, group(vec![])]),
    )]);
    let expected = to_vec(&layout, "Root", &instance).unwrap();
    for budget in [0, 1, 127, 128, expected.len() - 1] {
        assert_limit(
            to_vec_bounded(&layout, "Root", &instance, &limits(budget)),
            EncodeLimit::OutputBytes,
        );
    }
    assert_eq!(
        to_vec_bounded(&layout, "Root", &instance, &limits(expected.len())).unwrap(),
        expected
    );
    let empty_child = group(vec![("children", Instance::Repeated(vec![group(vec![])]))]);
    assert_eq!(
        to_vec_bounded(&layout, "Root", &empty_child, &limits(2)).unwrap(),
        [0x0a, 0]
    );
    assert_limit(
        to_vec_bounded(&layout, "Root", &empty_child, &limits(1)),
        EncodeLimit::OutputBytes,
    );
}

#[test]
fn input_values_fields_utf8_bytes_and_depth_have_exact_limits() {
    let layout = parse(
        r#"syntax = "proto3"; message Root { repeated int32 values = 1; Child child = 2; }
        message Child { string s = 1; }"#,
    );
    let instance = group(vec![
        (
            "values",
            Instance::Repeated(vec![
                scalar(Value::Int(1)),
                scalar(Value::Int(2)),
                scalar(Value::Int(3)),
            ]),
        ),
        (
            "child",
            group(vec![("s", scalar(Value::String("é".into())))]),
        ),
    ]);
    let exact = EncodeLimits {
        max_output_bytes: to_vec(&layout, "Root", &instance).unwrap().len(),
        max_values: 7,
        max_fields: 3,
        max_input_bytes: 14,
        max_depth: 2,
    };
    assert!(to_vec_bounded(&layout, "Root", &instance, &exact).is_ok());
    for (limit, lowered) in [
        (
            EncodeLimit::Values,
            EncodeLimits {
                max_values: 6,
                ..exact
            },
        ),
        (
            EncodeLimit::Fields,
            EncodeLimits {
                max_fields: 2,
                ..exact
            },
        ),
        (
            EncodeLimit::InputBytes,
            EncodeLimits {
                max_input_bytes: 13,
                ..exact
            },
        ),
        (
            EncodeLimit::Depth,
            EncodeLimits {
                max_depth: 1,
                ..exact
            },
        ),
    ] {
        assert_limit(to_vec_bounded(&layout, "Root", &instance, &lowered), limit);
        assert!(to_vec_bounded(&layout, "Root", &instance, &exact).is_ok());
    }
    let empty = EncodeLimits {
        max_output_bytes: 0,
        max_values: 1,
        max_fields: 0,
        max_input_bytes: 0,
        max_depth: 0,
    };
    assert!(to_vec_bounded(&layout, "Root", &group(vec![]), &empty).is_ok());
    assert_limit(
        to_vec_bounded(
            &layout,
            "Root",
            &group(vec![]),
            &EncodeLimits {
                max_values: 0,
                ..empty
            },
        ),
        EncodeLimit::Values,
    );
}

#[test]
fn late_format_errors_keep_native_category_path_message_and_order() {
    let layout = parse(
        r#"syntax = "proto2";
        enum E { ZERO = 0; ONE = 1; }
        message Root {
          optional string large = 1;
          repeated int32 packed = 2 [packed = true];
          optional Child child = 3;
          optional E state = 4;
          oneof choice { string text = 5; int32 count = 6; }
          map<string, int32> counts = 7;
          required int32 last = 8;
        }
        message Child { required bool valid = 1; }"#,
    );
    let cases = [
        vec![("last", scalar(Value::String("not an int".into())))],
        vec![], // Missing required field after an oversized valid field.
        vec![(
            "packed",
            Instance::Repeated(vec![
                scalar(Value::Int(1)),
                scalar(Value::String("bad".into())),
            ]),
        )],
        vec![("child", group(vec![("valid", scalar(Value::Int(2)))]))],
        vec![("state", scalar(Value::String("UNKNOWN".into())))],
        vec![
            ("text", scalar(Value::String("a".into()))),
            ("count", scalar(Value::Int(0))),
        ],
        vec![(
            "counts",
            Instance::Repeated(vec![
                group(vec![]),
                group(vec![("key", scalar(Value::String(String::new())))]),
            ]),
        )],
        vec![("packed", scalar(Value::Int(1)))],
        vec![("unknown", scalar(Value::Int(1)))],
        vec![
            ("last", scalar(Value::Int(1))),
            ("last", scalar(Value::Int(2))),
        ],
    ];
    for mut fields in cases {
        fields.insert(0, ("large", scalar(Value::String("x".repeat(4096)))));
        let instance = group(fields);
        let native = to_vec(&layout, "Root", &instance).unwrap_err();
        let bounded = to_vec_bounded(&layout, "Root", &instance, &limits(0)).unwrap_err();
        assert!(matches!(
            bounded,
            BoundedEncodeError::Protobuf(ProtobufError::InvalidInstance { .. })
        ));
        assert_eq!(bounded.to_string(), native.to_string());
    }
    let corrected = group(vec![("last", scalar(Value::Int(7)))]);
    assert_eq!(
        to_vec_bounded(&layout, "Root", &corrected, &limits(2)).unwrap(),
        [0x40, 7]
    );
}

#[test]
fn failed_output_then_corrected_retry_is_stateless_and_legacy_apis_unchanged() {
    let layout = parse("message Root { required bytes data = 1; }");
    let id = layout.resolve_message("Root").unwrap();
    let large = group(vec![("data", scalar(Value::String("abcdef".into())))]);
    let budget = limits(3);
    assert_limit(
        to_vec_message_bounded(&layout, id, &large, &budget),
        EncodeLimit::OutputBytes,
    );
    assert_eq!(
        to_vec_message(&layout, id, &large).unwrap(),
        [0x0a, 6, b'a', b'b', b'c', b'd', b'e', b'f']
    );
    let corrected = group(vec![("data", scalar(Value::String("a".into())))]);
    assert_eq!(
        to_vec_message_bounded(&layout, id, &corrected, &budget).unwrap(),
        [0x0a, 1, b'a']
    );
}

#[test]
fn invalid_configuration_and_admission_precedence_are_explicit() {
    let layout = parse("message Root { optional int32 n = 1; }");
    let empty = group(vec![]);
    for options in [
        EncodeLimits {
            max_output_bytes: usize::MAX,
            ..limits(0)
        },
        EncodeLimits {
            max_values: usize::MAX,
            ..limits(0)
        },
        EncodeLimits {
            max_fields: usize::MAX,
            ..limits(0)
        },
        EncodeLimits {
            max_input_bytes: usize::MAX,
            ..limits(0)
        },
        EncodeLimits {
            max_depth: 129,
            ..limits(0)
        },
    ] {
        assert!(matches!(
            to_vec_bounded(&layout, "Root", &empty, &options),
            Err(BoundedEncodeError::InvalidLimit { .. })
        ));
        assert!(matches!(
            to_vec_bounded(&layout, "Missing", &empty, &options),
            Err(BoundedEncodeError::Protobuf(ProtobufError::UnknownRoot(_)))
        ));
    }
    let malformed = group(vec![("n", scalar(Value::String("bad".into())))]);
    assert_limit(
        to_vec_bounded(
            &layout,
            "Root",
            &malformed,
            &EncodeLimits {
                max_fields: 0,
                ..limits(0)
            },
        ),
        EncodeLimit::Fields,
    );
}

#[test]
fn native_errors_remain_owned_and_available_as_the_typed_source() {
    let layout = parse("message Root { required int32 n = 1; }");
    let error = to_vec_bounded(&layout, "Root", &group(vec![]), &limits(0)).unwrap_err();
    let source = std::error::Error::source(&error)
        .unwrap()
        .downcast_ref::<ProtobufError>()
        .unwrap();
    assert!(matches!(source, ProtobufError::InvalidInstance { path, .. } if path == "Root.n"));
    let BoundedEncodeError::Protobuf(ProtobufError::InvalidInstance { path, message }) = error
    else {
        panic!("bounded error should own the native invalid-instance error");
    };
    assert_eq!(path, "Root.n");
    assert_eq!(message, "required field is absent or null");
}

#[test]
fn deeply_nested_malformed_input_is_refused_before_codec_recursion() {
    let layout = parse("message Root { optional Root child = 1; }");
    let mut instance = group(vec![]);
    for _ in 0..128 {
        instance = group(vec![("child", instance)]);
    }
    let expected = to_vec(&layout, "Root", &instance).unwrap();
    assert_eq!(
        to_vec_bounded(&layout, "Root", &instance, &limits(expected.len())).unwrap(),
        expected
    );
    instance = group(vec![("child", instance)]);
    assert_limit(
        to_vec_bounded(&layout, "Root", &instance, &limits(0)),
        EncodeLimit::Depth,
    );
}

#[test]
fn malformed_repeated_strings_and_bytes_after_large_items_keep_native_errors() {
    for ty in ["string", "bytes"] {
        let layout = parse(&format!("message Root {{ repeated {ty} items = 1; }}"));
        let instance = group(vec![(
            "items",
            Instance::Repeated(vec![
                scalar(Value::String("x".repeat(4096))),
                scalar(Value::Null),
            ]),
        )]);
        let native = to_vec(&layout, "Root", &instance).unwrap_err();
        let bounded = to_vec_bounded(&layout, "Root", &instance, &limits(0)).unwrap_err();
        assert!(matches!(
            bounded,
            BoundedEncodeError::Protobuf(ProtobufError::InvalidInstance { .. })
        ));
        assert_eq!(bounded.to_string(), native.to_string());
        assert!(bounded.to_string().contains("Root.items[1]"));
    }
}
