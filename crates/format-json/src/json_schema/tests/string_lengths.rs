use ir::{Instance, ScalarType, ScalarTypeSet, SchemaKind, SchemaNode, StringLengthRange, Value};

use super::{export, import_str, import_str_result};
use crate::{
    JsonFormatError, from_lines, from_str, json_schema::import_with_root, to_lines, to_string,
};

fn length(schema: &SchemaNode) -> (u64, Option<u64>) {
    let Some(range) = schema.string_length_range else {
        panic!("schema should retain a string-length range");
    };
    (range.minimum(), range.maximum())
}

#[test]
fn imports_exact_unicode_string_length_bounds() {
    let schema = import_str(r#"{"title":"Label","type":"string","minLength":1,"maxLength":2}"#);
    assert_eq!(length(&schema), (1, Some(2)));

    assert!(from_str(r#""😀""#, &schema).is_ok());
    assert!(from_str(r#""e\u0301""#, &schema).is_ok());
    assert!(matches!(
        from_str(r#""""#, &schema),
        Err(JsonFormatError::StringLengthMismatch { got: 0, .. })
    ));
    assert!(matches!(
        from_str(r#""e\u0301x""#, &schema),
        Err(JsonFormatError::StringLengthMismatch { got: 3, .. })
    ));
    assert!(to_string(&schema, &Instance::Scalar(Value::String("😀".to_string()))).is_ok());
    assert!(matches!(
        to_string(
            &schema,
            &Instance::Scalar(Value::String("e\u{301}x".to_string()))
        ),
        Err(JsonFormatError::StringLengthMismatch { got: 3, .. })
    ));
}

#[test]
fn rejects_non_integer_negative_overflow_and_reversed_keywords() {
    for schema in [
        r#"{"type":"string","minLength":-1}"#,
        r#"{"type":"string","minLength":1.0}"#,
        r#"{"type":"string","maxLength":1e0}"#,
        r#"{"type":"string","minLength":18446744073709551616}"#,
        r#"{"type":"string","minLength":2,"maxLength":1}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
        ));
    }

    let maximum = import_str(r#"{"type":"string","maxLength":18446744073709551615}"#);
    assert_eq!(length(&maximum), (0, Some(u64::MAX)));
}

#[test]
fn concrete_non_strings_ignore_valid_bounds_but_validate_their_tokens() {
    let integer = import_str(r#"{"type":"integer","minLength":2,"maxLength":4}"#);
    assert!(integer.string_length_range.is_none());
    assert!(import_str_result(r#"{"type":"integer","minLength":1.5}"#).is_err());

    let array = import_str(
        r#"{
          "type":"array",
          "minLength":7,
          "maxLength":9,
          "items":{"type":"string","minLength":1,"maxLength":2}
        }"#,
    );
    assert_eq!(length(&array), (1, Some(2)));
    assert!(from_str(r#"["a","😀"]"#, &array).is_ok());
}

#[test]
fn ambiguous_untyped_bounds_and_constrained_nested_arrays_reject() {
    assert!(import_str_result(r#"{"minLength":1}"#).is_err());
    assert!(
        import_str_result(r#"{"additionalProperties":{"minLength":1},"type":"object"}"#).is_err()
    );
    assert!(
        import_str_result(
            r#"{
          "type":"array",
          "items":{"type":"array","items":{"type":"string","minLength":2}}
        }"#
        )
        .is_err()
    );

    let no_op = import_str(r#"{"minLength":0}"#);
    assert!(no_op.string_length_range.is_none());
}

#[test]
fn nullable_and_scalar_union_domains_validate_only_string_values() {
    let nullable = import_str(r#"{"type":["string","null"],"minLength":2,"maxLength":3}"#);
    assert_eq!(length(&nullable), (2, Some(3)));
    assert!(from_str("null", &nullable).is_ok());
    assert!(from_str(r#""ab""#, &nullable).is_ok());
    assert!(from_str(r#""a""#, &nullable).is_err());

    let union = import_str(r#"{"type":["string","integer"],"minLength":2,"maxLength":3}"#);
    assert!(matches!(union.kind, SchemaKind::ScalarUnion { .. }));
    assert_eq!(length(&union), (2, Some(3)));
    assert!(from_str("7", &union).is_ok());
    assert!(from_str(r#""ab""#, &union).is_ok());
    assert!(from_str(r#""a""#, &union).is_err());
    assert!(to_string(&union, &Instance::Scalar(Value::Int(7))).is_ok());

    let nullable_branch = import_str(
        r#"{
          "anyOf":[
            {"type":"string","minLength":2,"maxLength":3},
            {"type":"null"}
          ]
        }"#,
    );
    assert!(nullable_branch.nullable);
    assert_eq!(length(&nullable_branch), (2, Some(3)));
    assert!(from_str("null", &nullable_branch).is_ok());
}

#[test]
fn all_of_intersects_and_any_of_unites_contiguous_string_intervals() {
    let intersection = import_str(
        r#"{
          "allOf":[
            {"type":"string","minLength":1,"maxLength":5},
            {"minLength":2,"maxLength":4}
          ]
        }"#,
    );
    assert_eq!(length(&intersection), (2, Some(4)));
    assert!(import_str_result(r#"{"allOf":[{"format":"email"},{"minLength":2}]}"#).is_err());
    assert!(
        import_str_result(r#"{"allOf":[{"type":"string","maxLength":1},{"minLength":2}]}"#)
            .is_err()
    );

    let union = import_str(
        r#"{
          "anyOf":[
            {"type":"string","minLength":1,"maxLength":2},
            {"type":"string","minLength":3,"maxLength":4},
            {"type":"integer"}
          ]
        }"#,
    );
    assert_eq!(length(&union), (1, Some(4)));
    assert!(from_str(r#""abc""#, &union).is_ok());
    assert!(from_str("9", &union).is_ok());
    assert!(
        import_str_result(
            r#"{
          "anyOf":[
            {"type":"string","maxLength":1},
            {"type":"string","minLength":3}
          ]
        }"#
        )
        .is_err()
    );

    let unconstrained_string = import_str(
        r#"{
          "anyOf":[
            {"type":"string","minLength":3},
            {"type":"string"},
            {"type":"integer"}
          ]
        }"#,
    );
    assert!(unconstrained_string.string_length_range.is_none());
}

#[test]
fn modern_ref_siblings_apply_and_legacy_siblings_are_ignored() {
    let modern = import_str(
        r##"{
          "$schema":"https://json-schema.org/draft/2020-12/schema",
          "$ref":"#/$defs/value",
          "minLength":2,
          "$defs":{"value":{"type":"string","maxLength":4}}
        }"##,
    );
    assert_eq!(length(&modern), (2, Some(4)));

    let legacy = import_str(
        r##"{
          "$schema":"http://json-schema.org/draft-07/schema#",
          "$ref":"#/definitions/value",
          "minLength":"ignored",
          "definitions":{"value":{"type":"string","maxLength":4}}
        }"##,
    );
    assert_eq!(length(&legacy), (0, Some(4)));

    assert!(import_str_result(r##"{"$ref":"#/missing","minLength":1}"##).is_err());

    let legacy_cycle = import_str(
        r##"{
          "$schema":"http://json-schema.org/draft-07/schema#",
          "$ref":"#/definitions/loop",
          "definitions":{"loop":{"$ref":"#/definitions/loop","minLength":"ignored"}}
        }"##,
    );
    assert!(legacy_cycle.string_length_range.is_none());
    assert!(
        import_str_result(
            r##"{
          "$ref":"#/$defs/loop",
          "$defs":{"loop":{"$ref":"#/$defs/loop","minLength":1}}
        }"##
        )
        .is_err()
    );
}

#[test]
fn fixed_strings_and_output_coercion_obey_the_exact_length_range() {
    let fixed = import_str(r#"{"type":"string","const":"😀x","minLength":2,"maxLength":2}"#);
    assert_eq!(fixed.fixed.as_deref(), Some("😀x"));
    assert_eq!(length(&fixed), (2, Some(2)));
    assert!(import_str_result(r#"{"type":"string","const":"x","minLength":2}"#).is_err());

    let schema = import_str(r#"{"type":"string","minLength":4,"maxLength":4}"#);
    assert!(matches!(
        to_string(&schema, &Instance::Scalar(Value::Bool(true))).as_deref(),
        Ok("\"true\"\n")
    ));
    assert!(matches!(
        to_string(&schema, &Instance::Scalar(Value::Bool(false))),
        Err(JsonFormatError::StringLengthMismatch { got: 5, .. })
    ));
}

#[test]
fn external_resources_select_their_own_ref_sibling_dialect()
-> Result<(), Box<dyn std::error::Error>> {
    let dir = std::env::temp_dir().join(format!(
        "ferrule-json-string-length-dialects-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("legacy.json"),
        r##"{
          "$schema":"http://json-schema.org/draft-07/schema#",
          "$defs":{
            "base":{"type":"string","maxLength":4},
            "use":{"$ref":"#/$defs/base","minLength":"ignored"}
          }
        }"##,
    )?;
    std::fs::write(
        dir.join("modern.json"),
        r##"{
          "$schema":"https://json-schema.org/draft/2020-12/schema",
          "$defs":{
            "base":{"type":"string","maxLength":4},
            "use":{"$ref":"#/$defs/base","minLength":2}
          }
        }"##,
    )?;

    for (resource, expected) in [
        ("legacy.json#/$defs/use", (0, Some(4))),
        ("modern.json#/$defs/use", (2, Some(4))),
    ] {
        std::fs::write(dir.join("root.json"), format!(r#"{{"$ref":"{resource}"}}"#))?;
        let imported = import_with_root(&dir.join("root.json"), &dir)?;
        assert_eq!(length(&imported), expected, "{resource}");
    }
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn typed_additional_properties_enforce_string_lengths() {
    let schema = import_str(
        r#"{
          "title":"Labels",
          "type":"object",
          "additionalProperties":{"type":"string","minLength":2,"maxLength":3}
        }"#,
    );
    assert!(from_str(r#"{"first":"ab","second":"😀x"}"#, &schema).is_ok());
    assert!(matches!(
        from_str(r#"{"first":"x"}"#, &schema),
        Err(JsonFormatError::StringLengthMismatch { ref name, got: 1, .. })
            if name == "*"
    ));
}

#[test]
fn export_roundtrip_preserves_lengths_formats_arrays_and_unions() {
    let formats = ir::JsonFormatAnnotations::new(["email".to_string(), "custom".to_string()])
        .unwrap_or_default();
    let Some(range) = StringLengthRange::new(2, Some(8)) else {
        panic!("test range is valid");
    };
    let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test union is valid");
    };
    let Some(schema) = SchemaNode::scalar_union("Values", types)
        .repeating()
        .with_string_length_range(range)
        .and_then(|node| node.with_json_formats(formats))
    else {
        panic!("test metadata is valid");
    };
    let rendered = export(&schema);
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_default();
    assert_eq!(
        value.pointer("/items/minLength").and_then(|v| v.as_u64()),
        Some(2)
    );
    assert_eq!(
        value.pointer("/items/maxLength").and_then(|v| v.as_u64()),
        Some(8)
    );
    assert!(value.get("minLength").is_none());
    let roundtrip = import_str(&rendered);
    assert_eq!(roundtrip, schema);
}

#[test]
fn json_lines_enforce_each_string_item_on_input_and_output() {
    let Some(range) = StringLengthRange::new(1, Some(1)) else {
        panic!("test range is valid");
    };
    let Some(schema) = SchemaNode::scalar("Code", ScalarType::String)
        .repeating()
        .with_string_length_range(range)
    else {
        panic!("test metadata is valid");
    };
    assert!(from_lines("\"😀\"\n\"x\"\n", &schema).is_ok());
    assert!(from_lines("\"e\\u0301\"\n", &schema).is_err());
    assert!(
        to_lines(
            &schema,
            &Instance::Repeated(vec![Instance::Scalar(Value::String("😀".to_string()))])
        )
        .is_ok()
    );
    assert!(
        to_lines(
            &schema,
            &Instance::Repeated(vec![Instance::Scalar(Value::String(
                "e\u{301}".to_string()
            ))])
        )
        .is_err()
    );
}
