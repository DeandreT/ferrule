use ir::{Instance, SchemaKind, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn direct_dependencies_execute_on_distinct_input_and_normalized_output_properties()
-> Result<(), JsonFormatError> {
    let schema = import_str(
        r#"{
  "title":"Record",
  "type":"object",
  "properties":{
    "trigger":{"type":"string"},
    "required":{"type":["string","null"]}
  },
  "dependentRequired":{"trigger":["required"]},
  "additionalProperties":false
}"#,
    );
    let dependencies = schema
        .json_property_dependencies
        .as_ref()
        .unwrap_or_else(|| panic!("dependency metadata should be retained"));
    assert_eq!(
        dependencies.requirements("trigger"),
        Some(["required".to_string()].as_slice())
    );
    assert!(matches!(
        crate::from_str(r#"{"trigger":"yes"}"#, &schema),
        Err(JsonFormatError::MissingDependentProperty {
            ref trigger,
            ref property,
            ..
        }) if trigger == "trigger" && property == "required"
    ));
    assert!(
        crate::from_str(
            r#"{"trigger":"first","trigger":"last","required":null}"#,
            &schema
        )
        .is_ok()
    );

    let omitted = Instance::Group(vec![
        (
            "trigger".into(),
            Instance::Scalar(Value::String("yes".into())),
        ),
        ("required".into(), Instance::Scalar(Value::Null)),
    ]);
    assert!(matches!(
        crate::to_string(&schema, &omitted),
        Err(JsonFormatError::MissingDependentProperty { .. })
    ));
    let explicit_null = Instance::Group(vec![
        (
            "trigger".into(),
            Instance::Scalar(Value::String("yes".into())),
        ),
        ("required".into(), Instance::Scalar(Value::json_null())),
    ]);
    assert!(crate::to_string(&schema, &explicit_null).is_ok());

    let rendered: serde_json::Value = serde_json::from_str(&export(&schema))?;
    assert_eq!(
        rendered.get("dependentRequired"),
        Some(&serde_json::json!({"trigger":["required"]}))
    );
    assert_eq!(import_str(&export(&schema)), schema);
    Ok(())
}

#[test]
fn empty_and_self_rules_normalize_away_while_empty_property_names_remain_exact() {
    let no_op = import_str(
        r#"{
  "type":"object",
  "properties":{"a":{"type":"string"}},
  "dependentRequired":{"a":[],"missing":["missing"]},
  "additionalProperties":false
}"#,
    );
    assert!(no_op.json_property_dependencies.is_none());

    let schema = import_str(
        r#"{
  "type":"object",
  "properties":{"":{"type":"string"},"target":{"type":"string"}},
  "dependentRequired":{"":["target"],"target":[""]},
  "additionalProperties":false
}"#,
    );
    assert!(crate::from_str(r#"{"":"present"}"#, &schema).is_err());
    assert!(crate::from_str(r#"{"":"present","target":"also present"}"#, &schema).is_ok());
}

#[test]
fn all_of_unions_dependencies_and_rejects_impossible_required_closures() {
    let schema = import_str(
        r#"{
  "type":"object",
  "properties":{
    "a":{"type":"string"},
    "b":{"type":"string"},
    "c":{"type":"string"}
  },
  "additionalProperties":false,
  "allOf":[
    {"dependentRequired":{"a":["b"]}},
    {"dependencies":{"a":["c"]}}
  ]
}"#,
    );
    let dependencies = schema
        .json_property_dependencies
        .as_ref()
        .unwrap_or_else(|| panic!("allOf dependencies should be retained"));
    assert_eq!(
        dependencies.requirements("a"),
        Some(["b".to_string(), "c".to_string()].as_slice())
    );
    assert!(crate::from_str(r#"{"a":"x","b":"y"}"#, &schema).is_err());
    assert!(crate::from_str(r#"{"a":"x","b":"y","c":"z"}"#, &schema).is_ok());

    for impossible in [
        r#"{
  "type":"object",
  "required":["a"],
  "maxProperties":1,
  "properties":{"a":{"type":"string"},"b":{"type":"string"}},
  "dependentRequired":{"a":["b"]},
  "additionalProperties":false
}"#,
        r#"{
  "type":"object",
  "required":["a"],
  "properties":{"a":{"type":"string"}},
  "dependentRequired":{"a":["missing"]},
  "additionalProperties":false
}"#,
    ] {
        assert!(import_str_result(impossible).is_err(), "{impossible}");
    }
}

#[test]
fn object_alternatives_lift_only_identical_effective_dependency_rules() {
    let common = import_str(
        r#"{
  "oneOf":[
    {
      "title":"first",
      "type":"object",
      "properties":{
        "kind":{"const":"first"},
        "trigger":{"type":"string"},
        "target":{"type":"string"}
      },
      "required":["kind"],
      "dependentRequired":{"trigger":["target"]},
      "additionalProperties":false
    },
    {
      "title":"second",
      "type":"object",
      "properties":{
        "kind":{"const":"second"},
        "trigger":{"type":"string"},
        "target":{"type":"string"}
      },
      "required":["kind"],
      "dependentRequired":{"trigger":["target"]},
      "additionalProperties":false
    }
  ]
}"#,
    );
    assert!(common.json_property_dependencies.is_some());
    assert!(crate::from_str(r#"{"kind":"first","trigger":"x"}"#, &common).is_err());
    assert!(crate::from_str(r#"{"kind":"second","trigger":"x","target":"y"}"#, &common).is_ok());
    assert_eq!(import_str(&export(&common)), common);

    let differing = import_str_result(
        r#"{
  "anyOf":[
    {
      "title":"first",
      "type":"object",
      "properties":{"a":{"type":"string"},"b":{"type":"string"}},
      "dependentRequired":{"a":["b"]},
      "additionalProperties":false
    },
    {
      "title":"second",
      "type":"object",
      "properties":{"a":{"type":"string"},"b":{"type":"string"}},
      "dependentRequired":{"b":["a"]},
      "additionalProperties":false
    }
  ]
}"#,
    );
    assert!(matches!(
        differing,
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("differing property dependencies")
    ));
}

#[test]
fn references_follow_dialect_policy_and_legacy_property_arrays_normalize() {
    let modern = import_str(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/Record",
  "dependentRequired":{"a":["b"]},
  "$defs":{"Record":{
    "type":"object",
    "properties":{"a":{"type":"string"},"b":{"type":"string"}},
    "additionalProperties":false
  }}
}"##,
    );
    assert!(modern.json_property_dependencies.is_some());

    let legacy = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "$ref":"#/definitions/Record",
  "dependentRequired":"ignored",
  "definitions":{"Record":{
    "type":"object",
    "properties":{"a":{"type":"string"},"b":{"type":"string"}},
    "dependencies":{"a":["b"]},
    "additionalProperties":false
  }}
}"##,
    );
    assert!(legacy.json_property_dependencies.is_some());
    assert!(crate::from_str(r#"{"a":"x"}"#, &legacy).is_err());

    assert!(matches!(
        import_str_result(
            r##"{
  "$ref":"#/$defs/Missing",
  "dependentRequired":{"a":["b"]}
}"##
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("unresolved or cyclic")
    ));
}

#[test]
fn malformed_ambiguous_and_schema_valued_dependencies_reject_without_widening() {
    for malformed in [
        r#"{"type":"object","dependentRequired":[]}"#,
        r#"{"type":"object","dependentRequired":{"a":"b"}}"#,
        r#"{"type":"object","dependentRequired":{"a":[1]}}"#,
        r#"{"type":"object","dependentRequired":{"a":["b","b"]}}"#,
        r#"{"type":"object","dependencies":{"a":{"required":["b"]}}}"#,
        r#"{"type":"object","dependentSchemas":{}}"#,
        r#"{"dependentRequired":{"a":["b"]}}"#,
    ] {
        assert!(import_str_result(malformed).is_err(), "{malformed}");
    }
    let scalar = import_str(r#"{"type":"string","dependentRequired":{"a":["b"]}}"#);
    assert!(matches!(scalar.kind, SchemaKind::Scalar { .. }));
    assert!(scalar.json_property_dependencies.is_none());

    let closed_no_op = import_str(
        r#"{
  "type":"object",
  "properties":{"known":{"type":"string"}},
  "dependentRequired":{"undeclared":["also-undeclared"]},
  "additionalProperties":false
}"#,
    );
    assert!(closed_no_op.json_property_dependencies.is_none());

    let conditionally_forbidden = import_str(
        r#"{
  "type":"object",
  "properties":{"trigger":{"type":"string"}},
  "dependentRequired":{"trigger":["undeclared"]},
  "additionalProperties":false
}"#,
    );
    assert!(crate::from_str("{}", &conditionally_forbidden).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"trigger":"x"}"#, &conditionally_forbidden),
        Err(JsonFormatError::MissingDependentProperty { .. })
    ));
}

#[test]
fn nullable_repeating_and_json_lines_objects_validate_each_item() {
    let nullable = import_str(
        r#"{
  "type":["object","null"],
  "properties":{"a":{"type":"string"},"b":{"type":"string"}},
  "dependentRequired":{"a":["b"]},
  "additionalProperties":false
}"#,
    );
    assert!(crate::from_str("null", &nullable).is_ok());

    let rows = import_str(
        r#"{
  "type":"array",
  "items":{
    "type":"object",
    "properties":{"a":{"type":"string"},"b":{"type":"string"}},
    "dependentRequired":{"a":["b"]},
    "additionalProperties":false
  }
}"#,
    );
    assert!(crate::from_str(r#"[{"a":"x","b":"y"},{"b":"z"}]"#, &rows).is_ok());
    assert!(matches!(
        crate::from_str(r#"[{"a":"x"}]"#, &rows),
        Err(JsonFormatError::MissingDependentProperty { .. })
    ));
    assert!(matches!(
        crate::from_lines(
            r#"{"a":"x","b":"y"}
{"a":"missing"}
"#,
            &rows
        ),
        Err(JsonFormatError::MissingDependentProperty { .. })
    ));
}

#[test]
fn export_rejects_corrupted_dependency_metadata() {
    let dependencies = ir::JsonPropertyDependencies::new(std::collections::BTreeMap::from([(
        "a".into(),
        vec!["b".into()],
    )]))
    .unwrap_or_else(|error| panic!("{error}"));
    let mut scalar = ir::SchemaNode::scalar("value", ir::ScalarType::String);
    scalar.json_property_dependencies = Some(dependencies.clone());
    assert!(matches!(
        super::super::export(&scalar),
        Err(JsonFormatError::InvalidPropertyDependenciesMetadata { .. })
    ));

    let mut nested = ir::SchemaNode::group(
        "root",
        vec![ir::SchemaNode::scalar("value", ir::ScalarType::String)],
    );
    let SchemaKind::Group { children, .. } = &mut nested.kind else {
        panic!("test root is a group");
    };
    children[0].json_property_dependencies = Some(dependencies);
    assert!(matches!(
        super::super::export(&nested),
        Err(JsonFormatError::InvalidPropertyDependenciesMetadata { .. })
    ));
}
