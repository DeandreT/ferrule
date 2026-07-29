use ir::{Instance, Value};

use super::super::import;
use super::export;
use crate::{JsonFormatError, from_str, to_string};

fn import_text(text: &str, label: &str) -> Result<ir::SchemaNode, JsonFormatError> {
    let path = std::env::temp_dir().join(format!(
        "ferrule_json_schema_unconstrained_{label}_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, text)?;
    let result = import(&path);
    std::fs::remove_file(path)?;
    result
}

#[test]
fn imports_true_and_empty_schemas_as_arbitrary_json() -> Result<(), Box<dyn std::error::Error>> {
    for (label, schema_text) in [("true", "true"), ("empty", "{}")] {
        let schema = import_text(schema_text, label)?;
        assert!(schema.json_any);
        for input in [
            "null",
            "true",
            "42",
            r#""text""#,
            r#"["x",1,false]"#,
            r#"{"nested":{"value":1}}"#,
        ] {
            let instance = from_str(input, &schema)?;
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(&to_string(&schema, &instance)?)?,
                serde_json::from_str::<serde_json::Value>(input)?,
                "{label}: {input}"
            );
        }
        let exported = export(&schema);
        assert!(
            serde_json::from_str::<serde_json::Value>(&exported)?
                .as_object()
                .is_some_and(|root| root.keys().all(|key| key == "title")),
            "{label}: {exported}"
        );
    }
    Ok(())
}

#[test]
fn arrays_without_items_retain_arbitrary_values() -> Result<(), Box<dyn std::error::Error>> {
    let schema = import_text(r#"{"title":"Values","type":"array"}"#, "array")?;
    assert!(schema.repeating);
    assert!(schema.json_any);
    assert!(schema.metadata_is_valid());
    assert_eq!(
        serde_json::from_str::<ir::SchemaNode>(&serde_json::to_string(&schema)?)?,
        schema
    );
    let input = r#"[1,"two",null,{"three":3}]"#;
    let instance = from_str(input, &schema)?;
    let items = instance
        .as_repeated()
        .ok_or_else(|| std::io::Error::other("missing repeated values"))?;
    assert_eq!(
        items,
        [
            Instance::Scalar(Value::String("1".into())),
            Instance::Scalar(Value::String(r#""two""#.into())),
            Instance::Scalar(Value::String("null".into())),
            Instance::Scalar(Value::String(r#"{"three":3}"#.into())),
        ]
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&to_string(&schema, &instance)?)?,
        serde_json::from_str::<serde_json::Value>(input)?
    );
    assert!(export(&schema).contains(r#""items": {}"#));
    Ok(())
}

#[test]
fn no_op_string_length_does_not_narrow_dynamic_values() -> Result<(), Box<dyn std::error::Error>> {
    let schema = import_text(
        r#"{
          "type":"object",
          "additionalProperties":{"minLength":0}
        }"#,
        "dynamic-no-op-length",
    )?;
    let input = r#"{"number":42,"object":{"nested":true},"array":[1,"two"]}"#;
    let instance = from_str(input, &schema)?;
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&to_string(&schema, &instance)?)?,
        serde_json::from_str::<serde_json::Value>(input)?
    );
    Ok(())
}

#[test]
fn false_schemas_fail_instead_of_weakening_to_strings() -> Result<(), Box<dyn std::error::Error>> {
    let property = import_text(r#"{"type":"object","properties":{"Never":false}}"#, "false");
    assert!(matches!(
        property,
        Err(JsonFormatError::UnsupportedSchemaUnion { ref name, ref reason })
            if name == "Never" && reason.contains("accepts no JSON value")
    ));
    assert!(matches!(
        import_text("false", "root-false"),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("accepts no JSON value")
    ));
    Ok(())
}
