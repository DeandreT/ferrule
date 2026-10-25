use super::{
    JsonBoundaryError, parse_json, parse_json_bytes, serialize_json, serialize_json_bytes,
};
use ir::{Instance, ScalarType, SchemaNode, Value};

#[test]
fn scalar_item_nullability_generated_rust_boundaries_keep_independent_values() {
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut schema = SchemaNode::scalar("Items", ScalarType::Int).repeating();
        schema.container_nullable = outer;
        schema.nullable = item;
        let descriptor = codegen_schema::encode(&schema, 1024 * 1024).unwrap();
        let int = |value| Instance::Scalar(Value::Int(value));
        let null = || Instance::Scalar(Value::json_null());
        let controls = [
            (
                "null",
                if outer {
                    Ok(null())
                } else {
                    Err(("array", "null"))
                },
            ),
            ("[]", Ok(Instance::Repeated(vec![]))),
            ("[1]", Ok(Instance::Repeated(vec![int(1)]))),
            (
                "[null]",
                if item {
                    Ok(Instance::Repeated(vec![null()]))
                } else {
                    Err(("integer", "null"))
                },
            ),
            (
                "[1,null]",
                if item {
                    Ok(Instance::Repeated(vec![int(1), null()]))
                } else {
                    Err(("integer", "null"))
                },
            ),
            ("{}", Err(("array", "object"))),
            ("[true]", Err(("integer", "bool"))),
        ];
        let originals = controls
            .into_iter()
            .map(|(input, expected)| {
                let text = parse_json(&descriptor, input);
                let bytes = parse_json_bytes(&descriptor, input.as_bytes());
                (input, expected, text, bytes)
            })
            .collect::<Vec<_>>();
        eprintln!("scalar-item Rust boundary originals {outer}/{item}: {originals:#?}");
        for (input, expected, text, bytes) in originals {
            match expected {
                Ok(expected) => {
                    assert_eq!(text.unwrap(), expected, "{outer}/{item}/{input}");
                    assert_eq!(bytes.unwrap(), expected, "{outer}/{item}/{input}");
                    let literal: serde_json::Value = serde_json::from_str(input).unwrap();
                    let serialized = serde_json::to_string_pretty(&literal).unwrap() + "\n";
                    assert_eq!(serialize_json(&descriptor, &expected).unwrap(), serialized);
                    assert_eq!(
                        serialize_json_bytes(&descriptor, &expected).unwrap(),
                        serialized.as_bytes()
                    );
                }
                Err((expected, got)) => {
                    let error = || JsonBoundaryError::InvalidInput {
                        message: format!("`Items`: expected {expected}, got {got}"),
                    };
                    assert_eq!(text, Err(error()), "{outer}/{item}/{input}");
                    assert_eq!(bytes, Err(error()), "{outer}/{item}/{input}");
                }
            }
        }
        let writer_originals = [
            (
                Instance::Scalar(Value::json_null()),
                outer,
                "array",
                "null\n",
            ),
            (
                Instance::Repeated(vec![Instance::Scalar(Value::json_null())]),
                item,
                "integer",
                "[\n  null\n]\n",
            ),
        ]
        .into_iter()
        .map(|(instance, accepted, expected_kind, expected_output)| {
            let text = serialize_json(&descriptor, &instance);
            let bytes = serialize_json_bytes(&descriptor, &instance);
            (accepted, expected_kind, expected_output, text, bytes)
        })
        .collect::<Vec<_>>();
        eprintln!("scalar-item Rust output originals {outer}/{item}: {writer_originals:#?}");
        for (accepted, expected_kind, expected_output, text, bytes) in writer_originals {
            if accepted {
                assert_eq!(text.unwrap(), expected_output);
                assert_eq!(bytes.unwrap(), expected_output.as_bytes());
            } else {
                let error = || JsonBoundaryError::InvalidOutput {
                    message: format!("`Items`: expected {expected_kind}, got json null"),
                };
                assert_eq!(text, Err(error()), "{outer}/{item}");
                assert_eq!(bytes, Err(error()), "{outer}/{item}");
            }
        }
    }
}
