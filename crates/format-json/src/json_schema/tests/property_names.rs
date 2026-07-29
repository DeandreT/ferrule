use ir::{Instance, JsonPropertyNameConstraints, SchemaKind, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn direct_property_name_constraints_execute_and_roundtrip() -> Result<(), JsonFormatError> {
    let schema = import_str(
        r#"{
  "title":"Labels",
  "type":"object",
  "properties":{
    "alpha":{"type":"string"},
    "é":{"type":"string"},
    "invalid":{"type":"string"}
  },
  "propertyNames":{
    "type":"string",
    "enum":["é","alpha"],
    "minLength":1,
    "maxLength":5,
    "pattern":"^(alpha|é)$",
    "format":"field-name"
  },
  "additionalProperties":false
}"#,
    );
    let constraints = schema
        .json_property_names
        .as_ref()
        .unwrap_or_else(|| panic!("property-name constraints should be retained"));
    assert_eq!(
        constraints.allowed().map(ir::JsonPropertyNameSet::as_slice),
        Some(["alpha".to_string(), "é".to_string()].as_slice())
    );
    assert!(crate::from_str(r#"{"alpha":"x","é":"y"}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"invalid":"x"}"#, &schema),
        Err(JsonFormatError::InvalidPropertyName { ref property, .. })
            if property == "invalid"
    ));

    let invalid_output = Instance::Group(vec![(
        "invalid".into(),
        Instance::Scalar(Value::String("x".into())),
    )]);
    assert!(matches!(
        crate::to_string(&schema, &invalid_output),
        Err(JsonFormatError::InvalidPropertyName { .. })
    ));
    let omitted = Instance::Group(vec![("invalid".into(), Instance::Scalar(Value::Null))]);
    assert!(crate::to_string(&schema, &omitted).is_ok());

    let rendered: serde_json::Value = serde_json::from_str(&export(&schema))?;
    assert!(rendered.get("propertyNames").is_some());
    assert_eq!(import_str(&export(&schema)), schema);
    Ok(())
}

#[test]
fn false_property_names_admit_only_empty_objects_and_required_conflicts_reject() {
    let schema = import_str(
        r#"{
  "type":"object",
  "propertyNames":false,
  "additionalProperties":true
}"#,
    );
    assert!(matches!(
        schema.json_property_names,
        Some(JsonPropertyNameConstraints::Never)
    ));
    assert!(crate::from_str("{}", &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"":null}"#, &schema),
        Err(JsonFormatError::InvalidPropertyName { ref property, .. }) if property.is_empty()
    ));
    assert!(
        import_str_result(
            r#"{
  "type":"object",
  "required":["value"],
  "properties":{"value":{"type":"string"}},
  "propertyNames":false,
  "additionalProperties":false
}"#
        )
        .is_err()
    );
}

#[test]
fn local_refs_and_dialects_apply_property_names_at_the_owning_resource() {
    let modern = import_str(
        r##"{
  "$defs":{"Name":{"type":"string","minLength":2}},
  "type":"object",
  "properties":{"ab":{"type":"string"}},
  "propertyNames":{"$ref":"#/$defs/Name","maxLength":2},
  "additionalProperties":false
}"##,
    );
    assert!(crate::from_str(r#"{"ab":"x"}"#, &modern).is_ok());
    assert!(crate::from_str(r#"{"a":"x"}"#, &modern).is_err());

    let draft_seven = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "definitions":{"Name":{"type":"string","minLength":2}},
  "type":"object",
  "properties":{"abc":{"type":"string"}},
  "propertyNames":{"$ref":"#/definitions/Name","maxLength":2},
  "additionalProperties":false
}"##,
    );
    assert!(crate::from_str(r#"{"abc":"x"}"#, &draft_seven).is_ok());

    let draft_four = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "type":"object",
  "propertyNames":{"type":17,"pattern":false},
  "additionalProperties":true
}"#,
    );
    assert!(draft_four.json_property_names.is_none());
    assert!(crate::from_str(r#"{"anything":1}"#, &draft_four).is_ok());
}

#[test]
fn all_of_and_representable_unions_compose_without_widening() {
    let all_of = import_str(
        r#"{
  "type":"object",
  "propertyNames":{
    "allOf":[
      {"minLength":2},
      {"maxLength":3},
      {"pattern":"^[a-z]+$"}
    ]
  },
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str(r#"{"ab":1}"#, &all_of).is_ok());
    assert!(crate::from_str(r#"{"a":1}"#, &all_of).is_err());
    assert!(crate::from_str(r#"{"AB":1}"#, &all_of).is_err());

    let any_of = import_str(
        r#"{
  "type":"object",
  "propertyNames":{"anyOf":[{"enum":["a","b"]},{"const":"c"}]},
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str(r#"{"a":1,"c":2}"#, &any_of).is_ok());
    assert!(crate::from_str(r#"{"d":1}"#, &any_of).is_err());

    let one_of = import_str(
        r#"{
  "type":"object",
  "propertyNames":{"oneOf":[{"enum":["a","b"]},{"enum":["c","d"]}]},
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str(r#"{"b":1,"d":2}"#, &one_of).is_ok());

    for invalid in [
        r#"{
  "type":"object",
  "propertyNames":{"oneOf":[{},{"const":"a"}]}
}"#,
        r#"{
  "type":"object",
  "propertyNames":{"oneOf":[{},{}]}
}"#,
    ] {
        assert!(matches!(
            import_str_result(invalid),
            Err(JsonFormatError::UnsupportedSchemaObject { .. })
        ));
    }
    assert!(matches!(
        import_str_result(
            r#"{
  "type":"object",
  "propertyNames":{
    "anyOf":[
      {"minLength":1,"pattern":"^a"},
      {"minLength":3,"pattern":"^b"}
    ]
  }
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaObject { ref reason, .. })
            if reason.contains("correlated")
    ));
}

#[test]
fn finite_property_name_exclusions_execute_compose_and_roundtrip() -> Result<(), JsonFormatError> {
    let schema = import_str(
        r#"{
  "type":"object",
  "properties":{
    "allowed":{"type":"integer"},
    "blocked":{"type":"integer"},
    "also-blocked":{"type":"integer"}
  },
  "propertyNames":{
    "allOf":[
      {"not":{"enum":["blocked","also-blocked"]}},
      {"minLength":2}
    ]
  },
  "additionalProperties":true
}"#,
    );
    let constraints = schema
        .json_property_names
        .as_ref()
        .unwrap_or_else(|| panic!("finite exclusions should be retained"));
    assert_eq!(
        constraints
            .excluded()
            .map(ir::JsonPropertyNameSet::as_slice),
        Some(["also-blocked".to_string(), "blocked".to_string()].as_slice())
    );
    assert!(crate::from_str(r#"{"allowed":1,"other":2}"#, &schema).is_ok());
    let blocked = crate::from_str(r#"{"blocked":1}"#, &schema);
    assert!(
        matches!(
            &blocked,
            Err(JsonFormatError::InvalidPropertyName { property, .. })
                if property == "blocked"
        ),
        "{blocked:?}"
    );

    let rendered = export(&schema);
    assert!(rendered.contains(r#""not""#));
    assert_eq!(import_str(&rendered), schema);

    let any_of = import_str(
        r#"{
  "type":"object",
  "propertyNames":{
    "anyOf":[
      {"not":{"enum":["a","shared"]}},
      {"not":{"enum":["b","shared"]}}
    ]
  },
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str(r#"{"a":1,"b":2}"#, &any_of).is_ok());
    assert!(crate::from_str(r#"{"shared":1}"#, &any_of).is_err());
    assert_eq!(import_str(&export(&any_of)), any_of);

    let double_negative = import_str(
        r#"{
  "type":"object",
  "propertyNames":{"not":{"not":{"enum":["a","b"]}}},
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str(r#"{"a":1,"b":2}"#, &double_negative).is_ok());
    assert!(crate::from_str(r#"{"c":1}"#, &double_negative).is_err());
    Ok(())
}

#[test]
fn pattern_property_name_exclusions_execute_compose_and_roundtrip() -> Result<(), JsonFormatError> {
    let schema = import_str(
        r#"{
  "type":"object",
  "propertyNames":{
    "allOf":[
      {"not":{"pattern":"^private-"}},
      {"not":{"pattern":"-internal$"}}
    ]
  },
  "additionalProperties":true
}"#,
    );
    let constraints = schema
        .json_property_names
        .as_ref()
        .unwrap_or_else(|| panic!("pattern exclusions should be retained"));
    let excluded_patterns = vec![
        vec!["^private-".to_string()],
        vec!["-internal$".to_string()],
    ];
    assert_eq!(
        constraints
            .excluded_patterns()
            .map(ir::JsonPatternConstraints::any_of),
        Some(excluded_patterns.as_slice())
    );
    assert!(crate::from_str(r#"{"public":1}"#, &schema).is_ok());
    assert!(crate::from_str(r#"{"private-token":1}"#, &schema).is_err());
    assert!(crate::from_str(r#"{"token-internal":1}"#, &schema).is_err());
    let invalid_output = Instance::Group(vec![(
        "private-token".to_string(),
        Instance::Scalar(Value::String("1".to_string())),
    )]);
    assert!(matches!(
        crate::to_string(&schema, &invalid_output),
        Err(JsonFormatError::InvalidPropertyName { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);

    let any_of = import_str(
        r#"{
  "type":"object",
  "propertyNames":{
    "anyOf":[
      {"not":{"pattern":"^x-"}},
      {"not":{"pattern":"-hidden$"}}
    ]
  },
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str(r#"{"x-public":1,"public-hidden":2}"#, &any_of).is_ok());
    assert!(crate::from_str(r#"{"x-secret-hidden":1}"#, &any_of).is_err());
    assert_eq!(import_str(&export(&any_of)), any_of);

    let double_negative = import_str(
        r#"{
  "type":"object",
  "propertyNames":{"not":{"not":{"pattern":"^[a-z]+$"}}},
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str(r#"{"lower":1}"#, &double_negative).is_ok());
    assert!(crate::from_str(r#"{"UPPER":1}"#, &double_negative).is_err());
    assert_eq!(import_str(&export(&double_negative)), double_negative);
    Ok(())
}

#[test]
fn malformed_ambiguous_and_unsupported_name_schemas_reject_exactly() {
    for invalid in [
        r#"{"type":"object","propertyNames":1}"#,
        r#"{"type":"object","propertyNames":{"type":17}}"#,
        r#"{"type":"object","propertyNames":{"type":"unknown"}}"#,
        r#"{"type":"object","propertyNames":{"enum":[]}}"#,
        r#"{"type":"object","propertyNames":{"minLength":2,"maxLength":1}}"#,
        r#"{"type":"object","propertyNames":{"pattern":"(?=x)"}}"#,
        r#"{"propertyNames":{"const":"x"}}"#,
    ] {
        assert!(import_str_result(invalid).is_err(), "{invalid}");
    }
    let scalar = import_str(r#"{"type":"string","propertyNames":{"const":"x"}}"#);
    assert!(matches!(scalar.kind, SchemaKind::Scalar { .. }));
    assert!(scalar.json_property_names.is_none());
}

#[test]
fn nullable_repeating_and_json_lines_objects_validate_each_key() {
    let nullable = import_str(
        r#"{
  "type":["object","null"],
  "propertyNames":{"pattern":"^[a-z]+$"},
  "additionalProperties":true
}"#,
    );
    assert!(crate::from_str("null", &nullable).is_ok());

    let rows = import_str(
        r#"{
  "type":"array",
  "items":{
    "type":"object",
    "propertyNames":{"pattern":"^[a-z]+$"},
    "additionalProperties":true
  }
}"#,
    );
    assert!(crate::from_str(r#"[{"first":1},{"second":2}]"#, &rows).is_ok());
    assert!(crate::from_str(r#"[{"NotValid":1}]"#, &rows).is_err());
    assert!(matches!(
        crate::from_lines(
            r#"{"first":1}
{"NotValid":2}
"#,
            &rows
        ),
        Err(JsonFormatError::InvalidPropertyName { .. })
    ));
}

#[test]
fn property_name_patterns_share_the_document_work_budget() {
    let schema = import_str(
        r#"{
  "type":"object",
  "propertyNames":{"pattern":"^(a?){8000}$"},
  "additionalProperties":true
}"#,
    );
    let input = format!("{{\"{}\":1}}", "a".repeat(8000));
    assert!(matches!(
        crate::from_str(&input, &schema),
        Err(JsonFormatError::PatternWorkLimit { .. })
    ));
    let excluded = import_str(
        r#"{
  "type":"object",
  "propertyNames":{"not":{"pattern":"^(a?){8000}$"}},
  "additionalProperties":true
}"#,
    );
    assert!(matches!(
        crate::from_str(&input, &excluded),
        Err(JsonFormatError::PatternWorkLimit { .. })
    ));
}

#[test]
fn object_alternatives_lift_only_identical_property_name_constraints() {
    let shared = import_str(
        r#"{
  "oneOf":[
    {
      "title":"a",
      "type":"object",
      "properties":{"a":{"const":"a"}},
      "required":["a"],
      "propertyNames":{"pattern":"^[a-z]$"},
      "additionalProperties":false
    },
    {
      "title":"b",
      "type":"object",
      "properties":{"b":{"const":"b"}},
      "required":["b"],
      "propertyNames":{"pattern":"^[a-z]$"},
      "additionalProperties":false
    }
  ]
}"#,
    );
    assert!(shared.json_property_names.is_some());
    assert_eq!(import_str(&export(&shared)), shared);

    assert!(matches!(
        import_str_result(
            r#"{
  "anyOf":[
    {
      "title":"a",
      "type":"object",
      "properties":{"a":{"type":"string"}},
      "propertyNames":{"const":"a"},
      "additionalProperties":false
    },
    {
      "title":"b",
      "type":"object",
      "properties":{"b":{"type":"string"}},
      "propertyNames":{"const":"b"},
      "additionalProperties":false
    }
  ]
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("differing propertyNames")
    ));
}

#[test]
fn export_rejects_corrupted_property_name_metadata() {
    let mut scalar = ir::SchemaNode::scalar("value", ir::ScalarType::String);
    scalar.json_property_names = Some(JsonPropertyNameConstraints::never());
    assert!(matches!(
        super::super::export(&scalar),
        Err(JsonFormatError::InvalidPropertyNameMetadata { .. })
    ));
}
