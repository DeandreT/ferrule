use std::error::Error;

use ir::{Instance, ScalarType, SchemaNode, Value};

use super::{JsonBoundaryError, parse_json, parse_json_bytes};

#[test]
fn required_property_precedes_undeclared_property_at_text_and_byte_input()
-> Result<(), Box<dyn Error>> {
    let schema = SchemaNode::group("Envelope", vec![SchemaNode::scalar("id", ScalarType::Int)])
        .with_required_fields(vec!["id".to_string()])
        .ok_or("required field belongs to the closed Envelope")?;
    let nullable = schema
        .clone()
        .nullable_container()
        .ok_or("Envelope is a group")?;
    let encoded = serde_json::to_string(&schema)?;
    let nullable_encoded = serde_json::to_string(&nullable)?;
    let cases = [
        ("mixed-invalid", r#"{"extra":1}"#, encoded.as_str()),
        ("missing-only", "{}", encoded.as_str()),
        ("extra-only", r#"{"id":7,"extra":1}"#, encoded.as_str()),
        ("valid", r#"{"id":7}"#, encoded.as_str()),
        ("nullable-object-null", "null", nullable_encoded.as_str()),
        ("malformed-json", r#"{"extra":1"#, encoded.as_str()),
    ];
    // Both routes and every control complete before the first assertion.
    let text_results = cases.map(|(_, text, schema)| parse_json(schema, text));
    let byte_results = cases.map(|(_, text, schema)| parse_json_bytes(schema, text.as_bytes()));
    let mut originals = Vec::new();
    for (api, results) in [
        ("parse_json", &text_results),
        ("parse_json_bytes", &byte_results),
    ] {
        for ((case, text, schema), result) in cases.iter().zip(results) {
            let actual = match result {
                Ok(instance) => serde_json::json!({
                    "status": "ok", "instance": instance, "debug": format!("{instance:#?}"),
                }),
                Err(error) => {
                    let typed = match error {
                        JsonBoundaryError::InvalidInput { message } => {
                            serde_json::json!({"category": "InvalidInput", "message": message})
                        }
                        _ => {
                            serde_json::json!({"category": "other", "debug": format!("{error:#?}")})
                        }
                    };
                    let mut sources = Vec::new();
                    let mut source = error.source();
                    while let Some(original) = source {
                        sources.push(serde_json::json!({
                            "display": original.to_string(), "debug": format!("{original:#?}"),
                        }));
                        source = original.source();
                    }
                    serde_json::json!({
                        "status": "error", "typed": typed, "display": error.to_string(),
                        "debug": format!("{error:#?}"), "sources": sources,
                    })
                }
            };
            let expected = match *case {
                "mixed-invalid" | "missing-only" => serde_json::json!({
                    "category": "InvalidInput", "message": "object `Envelope` requires property `id`", "source": null,
                }),
                "extra-only" => serde_json::json!({
                    "category": "InvalidInput", "message": "closed object `Envelope` does not declare property `extra`", "source": null,
                }),
                "valid" => serde_json::json!({"id": 7}),
                "nullable-object-null" => serde_json::json!({"scalar_kind": "JsonNull"}),
                _ => {
                    serde_json::json!({"category": "InvalidInput", "message_prefix": "json error: EOF", "source": null})
                }
            };
            originals.push(serde_json::json!({
                "case": case, "api": api, "schema_text": schema, "input": text,
                "utf8_bytes": text.as_bytes(), "expected": expected, "actual": actual,
            }));
        }
    }
    eprintln!(
        "ISSUE171_RUST_ORIGINALS {}",
        serde_json::json!({"cases": originals})
    );

    for results in [&text_results, &byte_results] {
        for result in &results[..2] {
            assert!(
                matches!(result,
                    Err(JsonBoundaryError::InvalidInput { message })
                        if message == "object `Envelope` requires property `id`"
                ),
                "{result:#?}"
            );
        }
        assert!(
            matches!(&results[2],
                Err(JsonBoundaryError::InvalidInput { message })
                    if message == "closed object `Envelope` does not declare property `extra`"
            ),
            "{:#?}",
            results[2]
        );
        assert_eq!(
            results[3].as_ref().ok(),
            Some(&Instance::Group(
                vec![("id".to_string(), Instance::Scalar(Value::Int(7)))].into()
            ))
        );
        assert_eq!(
            results[4].as_ref().ok(),
            Some(&Instance::Scalar(Value::json_null()))
        );
        assert!(
            matches!(&results[5], Err(JsonBoundaryError::InvalidInput { message })
            if message.starts_with("json error: EOF")),
            "{:#?}",
            results[5]
        );
        for result in results.iter().filter_map(|result| result.as_ref().err()) {
            assert!(result.source().is_none(), "{result:#?}");
        }
    }
    assert_eq!(text_results, byte_results);
    Ok(())
}
