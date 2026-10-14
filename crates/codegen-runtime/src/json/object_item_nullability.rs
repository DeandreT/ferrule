use super::{
    JsonBoundaryError, parse_json, parse_json_bytes, serialize_json, serialize_json_bytes,
};
use ir::{Instance, SchemaNode, Value};

#[test]
fn object_item_nullability_generated_rust_boundaries_keep_independent_values() {
    for (outer, item) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut schema = SchemaNode::group("Items", vec![]).repeating();
        schema.container_nullable = outer;
        schema.json_object_item_nullable = (outer || item).then_some(item);
        let descriptor = codegen_schema::encode(&schema, 1024 * 1024).unwrap();
        let empty = || Instance::Group(Vec::new().into());
        let controls = [
            (
                "null",
                if outer {
                    Ok(Instance::Scalar(Value::json_null()))
                } else {
                    Err(("array", "null"))
                },
            ),
            ("[]", Ok(Instance::Repeated(vec![]))),
            ("{}", Err(("array", "object"))),
            (
                "[null]",
                if item {
                    Ok(Instance::Repeated(vec![Instance::Scalar(
                        Value::json_null(),
                    )]))
                } else {
                    Err(("object", "null"))
                },
            ),
            ("[{}]", Ok(Instance::Repeated(vec![empty()]))),
            (
                "[null,{}]",
                if item {
                    Ok(Instance::Repeated(vec![
                        Instance::Scalar(Value::json_null()),
                        empty(),
                    ]))
                } else {
                    Err(("object", "null"))
                },
            ),
            ("[false]", Err(("object", "bool"))),
            ("[1]", Err(("object", "number"))),
            ("[\"null\"]", Err(("object", "string"))),
        ];
        let originals = controls
            .into_iter()
            .map(|(input, expected)| {
                let text = parse_json(&descriptor, input);
                let bytes = parse_json_bytes(&descriptor, input.as_bytes());
                (input, expected, text, bytes)
            })
            .collect::<Vec<_>>();
        eprintln!("object-item Rust boundary originals {outer}/{item}: {originals:#?}");
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
                    assert_eq!(text, Err(error()));
                    assert_eq!(bytes, Err(error()));
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
                "object",
                "[\n  null\n]\n",
            ),
        ]
        .into_iter()
        .map(|(instance, accepted, expected_kind, expected_output)| {
            let text = serialize_json(&descriptor, &instance);
            let bytes = serialize_json_bytes(&descriptor, &instance);
            (
                instance,
                accepted,
                expected_kind,
                expected_output,
                text,
                bytes,
            )
        })
        .collect::<Vec<_>>();
        eprintln!("object-item Rust output originals {outer}/{item}: {writer_originals:#?}");
        for (_, accepted, expected_kind, expected_output, text, bytes) in writer_originals {
            if accepted {
                assert_eq!(text.unwrap(), expected_output);
                assert_eq!(bytes.unwrap(), expected_output.as_bytes());
            } else {
                let error = || JsonBoundaryError::InvalidOutput {
                    message: format!("`Items`: expected {expected_kind}, got scalar"),
                };
                assert_eq!(text, Err(error()));
                assert_eq!(bytes, Err(error()));
            }
        }
    }
}
