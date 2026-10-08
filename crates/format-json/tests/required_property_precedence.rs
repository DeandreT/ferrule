use std::error::Error;

use format_json::{JsonFormatError, from_str};
use ir::{Instance, ScalarType, SchemaNode, Value};

#[test]
fn required_property_precedes_undeclared_property_at_native_input() -> Result<(), Box<dyn Error>> {
    let schema = SchemaNode::group("Envelope", vec![SchemaNode::scalar("id", ScalarType::Int)])
        .with_required_fields(vec!["id".to_string()])
        .ok_or("required field belongs to the closed Envelope")?;
    let nullable = schema
        .clone()
        .nullable_container()
        .ok_or("Envelope is a group")?;
    let cases = [
        ("mixed-invalid", r#"{"extra":1}"#, &schema),
        ("missing-only", "{}", &schema),
        ("extra-only", r#"{"id":7,"extra":1}"#, &schema),
        ("valid", r#"{"id":7}"#, &schema),
        ("nullable-object-null", "null", &nullable),
        ("malformed-json", r#"{"extra":1"#, &schema),
    ];
    // Keep every original result alive and emit all controls before asserting.
    let results = cases.map(|(_, text, schema)| from_str(text, schema));
    let originals: Vec<_> = cases.iter().zip(&results).map(|((case, text, schema), result)| {
        let actual = match result {
            Ok(instance) => serde_json::json!({
                "status": "ok", "instance": instance, "debug": format!("{instance:#?}"),
            }),
            Err(error) => {
                let typed = match error {
                    JsonFormatError::MissingRequiredProperty { object, property } =>
                        serde_json::json!({"category": "MissingRequiredProperty", "object": object, "property": property}),
                    JsonFormatError::UndeclaredProperty { object, property } =>
                        serde_json::json!({"category": "UndeclaredProperty", "object": object, "property": property}),
                    JsonFormatError::Json(inner) => serde_json::json!({
                        "category": "Json", "syntax_category": format!("{:?}", inner.classify()),
                        "line": inner.line(), "column": inner.column(), "inner_debug": format!("{inner:#?}"),
                    }),
                    _ => serde_json::json!({"category": "other", "debug": format!("{error:#?}")}),
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
                "category": "MissingRequiredProperty", "object": "Envelope", "property": "id",
            }),
            "extra-only" => serde_json::json!({
                "category": "UndeclaredProperty", "object": "Envelope", "property": "extra",
            }),
            "valid" => serde_json::json!({"group_fields": [{"name": "id", "scalar_kind": "Int", "value": 7}]}),
            "nullable-object-null" => serde_json::json!({"scalar_kind": "JsonNull"}),
            _ => serde_json::json!({"category": "Json", "syntax_category": "Eof"}),
        };
        serde_json::json!({"case": case, "api": "format_json::from_str", "schema": schema, "input": text, "expected": expected, "actual": actual})
    }).collect();
    eprintln!(
        "ISSUE171_NATIVE_ORIGINALS {}",
        serde_json::json!({"cases": originals})
    );

    for result in &results[..2] {
        assert!(
            matches!(result,
                Err(JsonFormatError::MissingRequiredProperty { object, property })
                    if object == "Envelope" && property == "id"
            ),
            "{result:#?}"
        );
    }
    assert!(
        matches!(&results[2],
            Err(JsonFormatError::UndeclaredProperty { object, property })
                if object == "Envelope" && property == "extra"
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
        matches!(&results[5], Err(JsonFormatError::Json(error)) if error.is_eof()),
        "{:#?}",
        results[5]
    );
    Ok(())
}
