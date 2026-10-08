use std::cell::Cell;
use std::error::Error;
use std::fmt::Debug;

use codegen_schema::json5_profile::{Json5ProfileError, encode_schema};
use format_json::JsonFormatError;
use format_json::json5_boundary::{Json5SyntaxError, Json5SyntaxKind, Json5SyntaxResource};
use ir::{ScalarType, SchemaKind, SchemaNode, Value};

use crate::{FunctionError, Instance, RuntimeError, field, group, scalar};

use super::*;

fn original<T: Debug>(
    label: &str,
    result: Result<T, Json5BoundaryError>,
) -> Result<T, Json5BoundaryError> {
    eprintln!("original {label}: {result:#?}");
    if let Err(error) = &result {
        eprintln!("original display: {error}");
        let mut cause = error.source();
        while let Some(error) = cause {
            eprintln!("original cause: {error:?}; {error}");
            cause = error.source();
        }
    }
    result
}

fn required(mut schema: SchemaNode, names: &[&str]) -> SchemaNode {
    let SchemaKind::Group { required, .. } = &mut schema.kind else {
        panic!("test object expected");
    };
    *required = names.iter().map(|name| (*name).to_owned()).collect();
    schema
}

fn schema() -> SchemaNode {
    let mut nil = SchemaNode::scalar("nil", ScalarType::String);
    nil.nullable = true;
    required(
        SchemaNode::group(
            "Source",
            vec![
                required(
                    SchemaNode::group("Info", vec![SchemaNode::scalar("n", ScalarType::Int)]),
                    &["n"],
                ),
                SchemaNode::scalar("text", ScalarType::String),
                SchemaNode::scalar("f", ScalarType::Float),
                SchemaNode::scalar("optional", ScalarType::String),
                nil,
                SchemaNode::scalar("flag", ScalarType::Bool),
                SchemaNode::scalar("雪", ScalarType::String),
            ],
        ),
        &["Info", "text"],
    )
}

fn descriptor(schema: &SchemaNode) -> String {
    let result = encode_schema(schema);
    eprintln!("original test descriptor={result:#?}");
    result.unwrap()
}

const INPUT: &str = "\u{feff}/*before*/ {Info:{n:0x2,}, text:'first', text:'last', f:+16., nil:null, flag:true, '雪':'ok',} //after\n";
const EXPECTED: &str = "{\n  \"Info\": {\n    \"n\": 2\n  },\n  \"text\": \"last\",\n  \"f\": 16.0,\n  \"nil\": null,\n  \"flag\": true,\n  \"雪\": \"ok\"\n}\n";

#[test]
fn nested_required_nullable_and_last_value_projection_retain_manual_tree_and_strict_bytes() {
    let schema = descriptor(&schema());
    let expected_tree = group([
        field("Info", group([field("n", scalar(Value::Int(2)))])),
        field("text", scalar(Value::String("last".into()))),
        field("f", scalar(Value::Float(16.0))),
        field("optional", scalar(Value::Null)),
        field("nil", scalar(Value::json_null())),
        field("flag", scalar(Value::Bool(true))),
        field("雪", scalar(Value::String("ok".into()))),
    ]);
    let calls = Cell::new(0);
    let text = original(
        "nested text",
        map_json5_with(&schema, &schema, INPUT, |input| {
            eprintln!("original projected tree={input:#?}");
            assert_eq!(input, &expected_tree);
            calls.set(calls.get() + 1);
            Ok(input.clone())
        }),
    );
    let bytes = original(
        "nested bytes",
        map_json5_bytes_with(&schema, &schema, INPUT.as_bytes(), |input| {
            eprintln!("original projected bytes tree={input:#?}");
            assert_eq!(input, &expected_tree);
            calls.set(calls.get() + 1);
            Ok(input.clone())
        }),
    );
    assert_eq!(text.unwrap(), EXPECTED);
    assert_eq!(bytes.unwrap(), EXPECTED.as_bytes());
    assert_eq!(calls.get(), 2);
}

#[test]
fn complete_syntax_precedes_projection_and_schema_input_failures_never_map() {
    let schema = descriptor(&schema());
    let calls = Cell::new(0);
    for input in [
        "{Info:{n:2},text:'x',ignored:NaN}",
        "{Info:{n:2},text:'x',ignored:Infinity}",
        "{Info:{n:2},text:'x',ignored:1e9999}",
        "{Info:{n:2},text:'x',f:NaN,f:0}",
    ] {
        let result = original(
            "global nonfinite",
            map_json5_with(&schema, &schema, input, |_| {
                calls.set(calls.get() + 1);
                Ok(group([]))
            }),
        );
        assert!(matches!(
            result,
            Err(Json5BoundaryError::Syntax(Json5SyntaxError::Syntax {
                kind: Json5SyntaxKind::NonFiniteNumber,
                ..
            }))
        ));
    }
    for input in ["[]", "null", "true", "'text'", "2"] {
        let result = original(
            "object-only input",
            map_json5_with(&schema, &schema, input, |_| {
                calls.set(calls.get() + 1);
                Ok(group([]))
            }),
        );
        assert!(matches!(
            result,
            Err(Json5BoundaryError::Input(JsonFormatError::Shape {
                expected: "object",
                ..
            }))
        ));
    }
    let missing = original(
        "required before mapping",
        map_json5_with(&schema, &schema, "{Info:{n:2}}", |_| {
            calls.set(calls.get() + 1);
            Ok(group([]))
        }),
    );
    assert!(
        matches!(missing, Err(Json5BoundaryError::Input(JsonFormatError::MissingRequiredProperty { property, .. })) if property == "text")
    );
    let absent_nested = original(
        "nested required",
        map_json5_with(&schema, &schema, "{Info:{},text:'x'}", |_| {
            calls.set(calls.get() + 1);
            Ok(group([]))
        }),
    );
    assert!(
        matches!(absent_nested, Err(Json5BoundaryError::Input(JsonFormatError::MissingRequiredProperty { property, .. })) if property == "n")
    );
    let unknown = original(
        "undeclared",
        map_json5_with(&schema, &schema, "{Info:{n:2},text:'x',extra:1}", |_| {
            calls.set(calls.get() + 1);
            Ok(group([]))
        }),
    );
    assert!(
        matches!(unknown, Err(Json5BoundaryError::Input(JsonFormatError::UndeclaredProperty { property, .. })) if property == "extra")
    );
    let null = original(
        "nonnull scalar",
        map_json5_with(&schema, &schema, "{Info:{n:2},text:null}", |_| {
            calls.set(calls.get() + 1);
            Ok(group([]))
        }),
    );
    assert!(matches!(
        null,
        Err(Json5BoundaryError::Input(JsonFormatError::Shape {
            got: "null",
            ..
        }))
    ));
    assert_eq!(calls.get(), 0);
}

#[test]
fn exact_numeric_projection_and_duplicate_order_use_existing_scalar_domains() {
    let schema = descriptor(&SchemaNode::group(
        "Source",
        vec![
            SchemaNode::scalar("n", ScalarType::Int),
            SchemaNode::scalar("f", ScalarType::Float),
        ],
    ));
    for (input, n, bits) in [
        ("{n:-0x2,f:-0.}", -2, 0x8000_0000_0000_0000),
        (
            "{n:-9223372036854775808,f:1e-307}",
            i64::MIN,
            0x0031_fa18_2c40_c60d,
        ),
        (
            "{n:9223372036854775807,f:9007199254740992}",
            i64::MAX,
            0x4340_0000_0000_0000,
        ),
        ("{n:'wrong',n:2,f:.5}", 2, 0x3fe0_0000_0000_0000),
        ("{n:1,\\u006e:2,f:.5}", 2, 0x3fe0_0000_0000_0000),
    ] {
        let result = original(
            "literal scalar domains",
            map_json5_with(&schema, &schema, input, |input| {
                eprintln!("original numeric projected={input:#?}");
                assert_eq!(input.field("n"), Some(&scalar(Value::Int(n))));
                let Some(Instance::Scalar(Value::Float(actual))) = input.field("f") else {
                    panic!("float tag expected");
                };
                assert_eq!(actual.to_bits(), bits);
                Ok(input.clone())
            }),
        );
        assert!(result.is_ok());
    }
    for input in [
        "{n:9223372036854775808,f:0}",
        "{n:2,f:9007199254740993}",
        "{n:2,n:'wrong',f:0}",
    ] {
        let result = original(
            "scalar refusal",
            map_json5_with(&schema, &schema, input, |_| {
                panic!("refused projection cannot execute")
            }),
        );
        assert!(matches!(
            result,
            Err(Json5BoundaryError::Input(JsonFormatError::Shape { .. }))
        ));
    }
}

#[test]
fn encoding_both_schema_owners_and_original_causes_dominate_later_stages() {
    let schema = descriptor(&SchemaNode::group("Source", Vec::new()));
    let bytes = [b'{', 0xff];
    let actual = original(
        "UTF8 before malformed schemas",
        map_json5_bytes_with("{", "{", &bytes, |_| panic!("encoding refused")),
    );
    let error = actual.unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<std::str::Utf8Error>()
            .is_some()
    );
    assert!(
        matches!(error, Json5BoundaryError::Encoding(error) if error.valid_up_to()==1 && error.error_len()==Some(1))
    );
    let source = original(
        "source schema before syntax",
        map_json5_with("{", &schema, "NaN", |_| panic!("schema refused")),
    );
    assert!(matches!(
        source,
        Err(Json5BoundaryError::SourceSchema(
            Json5ProfileError::DescriptorSyntax(_)
        ))
    ));
    let target = original(
        "target schema before syntax",
        map_json5_with(&schema, "FERRULE-EMBEDDED-SCHEMA/3\n{}", "NaN", |_| {
            panic!("schema refused")
        }),
    );
    let error = target.unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<Json5ProfileError>()
            .is_some()
    );
    assert!(
        error
            .source()
            .unwrap()
            .source()
            .unwrap()
            .downcast_ref::<codegen_schema::CodecError>()
            .is_some()
    );
    assert!(matches!(
        error,
        Json5BoundaryError::TargetSchema(Json5ProfileError::Codec(
            codegen_schema::CodecError::UnsupportedVersion
        ))
    ));
    let long_name = "x".repeat(4097);
    let descriptor =
        format!("{{\"name\":\"{long_name}\",\"kind\":{{\"kind\":\"group\",\"children\":[]}}}}");
    let limit = original(
        "schema limit payload before syntax",
        map_json5_with(&descriptor, &schema, "NaN", |_| {
            panic!("schema limit refused")
        }),
    );
    assert!(matches!(
        limit,
        Err(Json5BoundaryError::SourceSchema(Json5ProfileError::Limit {
            resource: codegen_schema::json5_profile::Json5ProfileResource::NameLength,
            requested: 4097,
            max: 4096
        }))
    ));
    let duplicate =
        r#"{"name":"S","kind":{"kind":"group","children":[]},"repeating":true,"repeating":false}"#;
    let result = original(
        "duplicate descriptor identity",
        map_json5_with(duplicate, &schema, "{}", |_| {
            panic!("duplicate schema refused")
        }),
    );
    assert!(
        matches!(result, Err(Json5BoundaryError::SourceSchema(Json5ProfileError::DuplicateDescriptorField { field })) if field == "repeating")
    );
}

#[test]
fn mapping_message_states_and_output_failures_retain_typed_originals_without_partial_text() {
    let empty = descriptor(&SchemaNode::group("Source", Vec::new()));
    for message in [None, Some(String::new()), Some("selected".into())] {
        let expected = message.clone();
        let result = original(
            "mapping optional message",
            map_json5_with(&empty, &empty, "{}", |_| {
                Err(RuntimeError::MappingException { node: 17, message })
            }),
        );
        let error = result.unwrap_err();
        assert!(
            error
                .source()
                .unwrap()
                .downcast_ref::<RuntimeError>()
                .is_some()
        );
        assert!(
            matches!(error, Json5BoundaryError::Mapping(RuntimeError::MappingException { node: 17, message }) if message == expected)
        );
    }
    let global = original(
        "legacy global mapping payload",
        map_json5_with(&empty, &empty, "{}", |_| {
            Err(RuntimeError::MappingFailure {
                rule: 1,
                message: Some("global".into()),
            })
        }),
    );
    assert!(
        matches!(global, Err(Json5BoundaryError::Mapping(RuntimeError::MappingFailure { rule: 1, message: Some(message) })) if message == "global")
    );
    let cause = original(
        "mapping function cause",
        map_json5_with(&empty, &empty, "{}", |_| {
            Err(RuntimeError::Function(FunctionError::DivideByZero))
        }),
    );
    assert!(matches!(
        cause,
        Err(Json5BoundaryError::Mapping(RuntimeError::Function(
            FunctionError::DivideByZero
        )))
    ));
    let root_array = original(
        "output object-only",
        map_json5_with(&empty, &empty, "{}", |_| {
            Ok(Instance::Repeated(vec![group([])]))
        }),
    );
    assert!(matches!(
        root_array,
        Err(Json5BoundaryError::Output(JsonFormatError::Shape {
            expected: "object",
            got: "array",
            ..
        }))
    ));
    let number = descriptor(&SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("n", ScalarType::Float)],
    ));
    for value in [
        Value::Float(f64::NAN),
        Value::Float(f64::INFINITY),
        Value::Int(9_007_199_254_740_993),
    ] {
        let result = original(
            "strict scalar output",
            map_json5_with(&empty, &number, "{}", |_| {
                Ok(group([field("n", scalar(value))]))
            }),
        );
        assert!(matches!(
            result,
            Err(Json5BoundaryError::Output(JsonFormatError::Shape { .. }))
        ));
    }
    let required = descriptor(&required(
        SchemaNode::group("Target", vec![SchemaNode::scalar("n", ScalarType::String)]),
        &["n"],
    ));
    let result = original(
        "required output",
        map_json5_with(&empty, &required, "{}", |_| Ok(group([]))),
    );
    assert!(
        matches!(result, Err(Json5BoundaryError::Output(JsonFormatError::MissingRequiredProperty { property, .. })) if property == "n")
    );
}

#[test]
fn exact_and_plus_one_size_helpers_and_syntax_depth_keep_resource_payloads() {
    // This small test checks the actual shared size seams without default 64 MiB allocations.
    for (check, resource) in [
        (
            super::codec::check_original_size as fn(usize) -> Result<(), Json5BoundaryError>,
            Json5BoundaryResource::OriginalDocumentBytes,
        ),
        (
            super::codec::check_output_size as fn(usize) -> Result<(), Json5BoundaryError>,
            Json5BoundaryResource::OutputDocumentBytes,
        ),
    ] {
        let exact = original("size exact", check(67_108_864));
        assert!(exact.is_ok());
        let over = original("size plus one", check(67_108_865));
        assert!(
            matches!(over, Err(Json5BoundaryError::Limit { resource: actual, requested: 67_108_865, max: 67_108_864 }) if actual == resource)
        );
    }
    let schema = descriptor(&SchemaNode::group("Source", Vec::new()));
    let exact = format!("{}0{}", "[".repeat(127), "]".repeat(127));
    let result = original(
        "syntax exact before shape",
        map_json5_with(&schema, &schema, &exact, |_| panic!("root array refused")),
    );
    assert!(matches!(
        result,
        Err(Json5BoundaryError::Input(JsonFormatError::Shape {
            got: "array",
            ..
        }))
    ));
    let over = format!("{}0{}", "[".repeat(128), "]".repeat(128));
    let result = original(
        "syntax depth original",
        map_json5_with(&schema, &schema, &over, |_| panic!("depth refused")),
    );
    let error = result.unwrap_err();
    assert!(
        error
            .source()
            .unwrap()
            .downcast_ref::<Json5SyntaxError>()
            .is_some()
    );
    assert!(matches!(
        error,
        Json5BoundaryError::Syntax(Json5SyntaxError::Limit {
            resource: Json5SyntaxResource::ContainerDepth,
            offset: 127,
            requested: 128,
            max: 127
        })
    ));
}
