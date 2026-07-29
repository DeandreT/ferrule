use ir::{Instance, JsonSchemaPredicate, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn required_only_normalizes_and_nontrivial_predicates_validate_input_and_output() {
    let required_only = import_str(
        r#"{
  "type":"object",
  "properties":{
    "trigger":{"type":"string"},
    "required":{"type":"string"}
  },
  "dependentSchemas":{"trigger":{"required":["required"]}},
  "additionalProperties":false
}"#,
    );
    assert!(required_only.json_dependent_schemas.is_none());
    assert!(required_only.json_property_dependencies.is_some());
    assert!(matches!(
        crate::from_str(r#"{"trigger":"yes"}"#, &required_only),
        Err(JsonFormatError::MissingDependentProperty { .. })
    ));

    let schema = import_str(
        r#"{
  "title":"Record",
  "type":"object",
  "properties":{
    "trigger":{"type":"string"},
    "mode":{"type":"string"}
  },
  "dependentSchemas":{
    "trigger":{
      "type":"object",
      "properties":{"mode":{"const":"strict"}},
      "required":["mode"]
    }
  },
  "additionalProperties":false
}"#,
    );
    let constraints = schema
        .json_dependent_schemas
        .as_ref()
        .unwrap_or_else(|| panic!("nontrivial dependent predicate should be retained"));
    assert_eq!(constraints.as_slice().len(), 1);
    assert!(matches!(
        constraints.as_slice()[0].predicate(),
        JsonSchemaPredicate::Schema { .. }
    ));
    assert!(crate::from_str(r#"{"trigger":"yes","mode":"strict"}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"trigger":"yes","mode":"loose"}"#, &schema),
        Err(JsonFormatError::DependentSchemaMismatch {
            ref trigger, ..
        }) if trigger == "trigger"
    ));
    assert!(crate::from_str(r#"{"mode":"loose"}"#, &schema).is_ok());

    let omitted = Instance::Group(vec![
        (
            "trigger".into(),
            Instance::Scalar(Value::String("yes".into())),
        ),
        ("mode".into(), Instance::Scalar(Value::Null)),
    ]);
    assert!(matches!(
        crate::to_string(&schema, &omitted),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
}

#[test]
fn false_and_all_of_false_predicates_remain_never_with_empty_name_triggers() {
    for predicate in ["false", r#"{"allOf":[true,false,{"required":[""]}]}"#] {
        let schema = import_str(&format!(
            r#"{{
  "type":"object",
  "properties":{{"":{{"type":"string"}}}},
  "dependentSchemas":{{"":{predicate}}},
  "additionalProperties":false
}}"#
        ));
        let constraints = schema
            .json_dependent_schemas
            .as_ref()
            .unwrap_or_else(|| panic!("never predicate should be retained"));
        assert!(constraints.as_slice()[0].predicate().is_never());
        assert!(crate::from_str("{}", &schema).is_ok());
        assert!(matches!(
            crate::from_str(r#"{"":"present"}"#, &schema),
            Err(JsonFormatError::DependentSchemaMismatch {
                ref trigger, ..
            }) if trigger.is_empty()
        ));
    }
}

#[test]
fn repeated_trigger_all_of_terms_conjoin_and_roundtrip_canonically() {
    let schema = import_str(
        r#"{
  "type":"object",
  "properties":{
    "trigger":{"type":"string"},
    "left":{"type":"string"},
    "right":{"type":"string"}
  },
  "additionalProperties":false,
  "allOf":[
    {"dependentSchemas":{"trigger":{
      "properties":{"left":{"type":"string","minLength":2}},
      "required":["left"]
    }}},
    {"dependentSchemas":{"trigger":{
      "properties":{"right":{"const":"ok"}},
      "required":["right"]
    }}}
  ]
}"#,
    );
    let constraints = schema
        .json_dependent_schemas
        .as_ref()
        .unwrap_or_else(|| panic!("both dependent predicates should be retained"));
    assert_eq!(constraints.as_slice().len(), 2);
    assert_eq!(constraints.as_slice()[0].trigger(), "trigger");
    assert_eq!(constraints.as_slice()[1].trigger(), "trigger");
    assert!(crate::from_str(r#"{"trigger":"yes","left":"ab","right":"ok"}"#, &schema).is_ok());
    assert!(crate::from_str(r#"{"trigger":"yes","left":"a","right":"ok"}"#, &schema).is_err());
    assert!(crate::from_str(r#"{"trigger":"yes","left":"ab","right":"no"}"#, &schema).is_err());

    let rendered = export(&schema);
    let json: serde_json::Value =
        serde_json::from_str(&rendered).unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        json.pointer("/dependentSchemas/trigger/allOf")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(2)
    );
    assert_eq!(import_str(&rendered), schema);
}

#[test]
fn interleaved_trigger_runs_roundtrip_in_retained_order() {
    let schema = import_str(
        r#"{
  "type":"object",
  "properties":{
    "b":{"type":"string"},
    "a":{"type":"string"},
    "left":{"type":"string"},
    "middle":{"type":"string"},
    "right":{"type":"string"}
  },
  "additionalProperties":false,
  "allOf":[
    {"dependentSchemas":{"b":{
      "properties":{"left":{"const":"b1"}},
      "required":["left"]
    }}},
    {"dependentSchemas":{"a":{
      "properties":{"middle":{"const":"a1"}},
      "required":["middle"]
    }}},
    {"dependentSchemas":{"b":{
      "properties":{"right":{"const":"b2"}},
      "required":["right"]
    }}}
  ]
}"#,
    );
    let constraints = schema
        .json_dependent_schemas
        .as_ref()
        .unwrap_or_else(|| panic!("all three dependent predicates should be retained"));
    assert_eq!(
        constraints
            .as_slice()
            .iter()
            .map(|constraint| constraint.trigger())
            .collect::<Vec<_>>(),
        ["b", "a", "b"]
    );
    assert!(
        crate::from_str(
            r#"{"b":"yes","a":"yes","left":"b1","middle":"a1","right":"b2"}"#,
            &schema
        )
        .is_ok()
    );
    for invalid in [
        r#"{"b":"yes","a":"yes","left":"wrong","middle":"a1","right":"b2"}"#,
        r#"{"b":"yes","a":"yes","left":"b1","middle":"wrong","right":"b2"}"#,
        r#"{"b":"yes","a":"yes","left":"b1","middle":"a1","right":"wrong"}"#,
    ] {
        assert!(crate::from_str(invalid, &schema).is_err(), "{invalid}");
    }

    let rendered = export(&schema);
    let json: serde_json::Value =
        serde_json::from_str(&rendered).unwrap_or_else(|error| panic!("{error}"));
    let all_of = json
        .get("allOf")
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| {
            panic!("interleaved triggers should export as ordered allOf branches: {rendered}")
        });
    assert_eq!(all_of.len(), 3);
    assert!(all_of[0].pointer("/dependentSchemas/b").is_some());
    assert!(all_of[1].pointer("/dependentSchemas/a").is_some());
    assert!(all_of[2].pointer("/dependentSchemas/b").is_some());

    let reimported = import_str(&rendered);
    let reimported_constraints = reimported
        .json_dependent_schemas
        .as_ref()
        .unwrap_or_else(|| panic!("canonical predicates should re-import"));
    assert_eq!(
        reimported_constraints
            .as_slice()
            .iter()
            .map(|constraint| constraint.trigger())
            .collect::<Vec<_>>(),
        ["b", "a", "b"]
    );
    assert_eq!(reimported, schema);
}

#[test]
fn interleaved_trigger_runs_roundtrip_inside_object_alternatives() {
    let branch = |kind: &str| {
        format!(
            r#"{{
  "title":"{kind}",
  "type":"object",
  "properties":{{
    "kind":{{"const":"{kind}"}},
    "b":{{"type":"string"}},
    "a":{{"type":"string"}},
    "left":{{"type":"string"}},
    "middle":{{"type":"string"}},
    "right":{{"type":"string"}}
  }},
  "required":["kind"],
  "additionalProperties":false,
  "allOf":[
    {{"dependentSchemas":{{"b":{{"properties":{{"left":{{"const":"b1"}}}}}}}}}},
    {{"dependentSchemas":{{"a":{{"properties":{{"middle":{{"const":"a1"}}}}}}}}}},
    {{"dependentSchemas":{{"b":{{"properties":{{"right":{{"const":"b2"}}}}}}}}}}
  ]
}}"#
        )
    };
    let schema = import_str(&format!(
        r#"{{"oneOf":[{},{}]}}"#,
        branch("first"),
        branch("second")
    ));
    let rendered = export(&schema);
    let json: serde_json::Value =
        serde_json::from_str(&rendered).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        json.get("allOf").is_none(),
        "ordered constraints cannot conflict with root oneOf: {rendered}"
    );
    for index in 0..2 {
        let all_of = json
            .pointer(&format!("/oneOf/{index}/allOf"))
            .and_then(serde_json::Value::as_array)
            .unwrap_or_else(|| panic!("alternative should contain ordered constraints"));
        assert_eq!(all_of.len(), 3);
        assert!(all_of[0].pointer("/dependentSchemas/b").is_some());
        assert!(all_of[1].pointer("/dependentSchemas/a").is_some());
        assert!(all_of[2].pointer("/dependentSchemas/b").is_some());
    }
    assert_eq!(import_str(&rendered), schema);
}

#[test]
fn declared_and_undeclared_dialects_apply_only_their_dependency_keywords() {
    let declared_modern = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"object",
  "properties":{"a":{"type":"string"},"b":{"type":"string"}},
  "dependencies":{"a":["b"]},
  "additionalProperties":false
}"#,
    );
    assert!(declared_modern.json_property_dependencies.is_none());

    let declared_legacy = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"object",
  "properties":{"a":{"type":"string"},"b":{"type":"string"}},
  "dependentSchemas":{"a":{"required":["b"]}},
  "dependencies":{"a":{"required":["b"]}},
  "additionalProperties":false
}"#,
    );
    assert!(declared_legacy.json_property_dependencies.is_some());

    let undeclared = import_str(
        r#"{
  "type":"object",
  "properties":{"a":{"type":"string"},"b":{"type":"string"},"c":{"type":"string"}},
  "dependencies":{"a":["b"]},
  "dependentSchemas":{"a":{"required":["c"]}},
  "additionalProperties":false
}"#,
    );
    let dependencies = undeclared
        .json_property_dependencies
        .as_ref()
        .unwrap_or_else(|| panic!("both compatibility spellings should normalize"));
    assert_eq!(
        dependencies.requirements("a"),
        Some(["b".to_string(), "c".to_string()].as_slice())
    );

    assert!(matches!(
        import_str_result(
            r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "type":"object",
  "properties":{"a":{"type":"string"}},
  "dependencies":{"a":false},
  "additionalProperties":false
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaObject { ref reason, .. })
            if reason.contains("boolean schemas require Draft 6")
    ));
    let draft_six_boolean = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-06/schema#",
  "type":"object",
  "properties":{"a":{"type":"string"}},
  "dependencies":{"a":false},
  "additionalProperties":false
}"#,
    );
    assert!(crate::from_str("{}", &draft_six_boolean).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"a":"present"}"#, &draft_six_boolean),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
}

#[test]
fn object_alternatives_require_identical_dependent_predicates() {
    let common = import_str(
        r#"{
  "oneOf":[
    {
      "title":"a",
      "type":"object",
      "properties":{"kind":{"const":"a"},"trigger":{"type":"string"},"value":{"type":"string"}},
      "required":["kind"],
      "dependentSchemas":{"trigger":{"properties":{"value":{"type":"string","minLength":2}}}},
      "additionalProperties":false
    },
    {
      "title":"b",
      "type":"object",
      "properties":{"kind":{"const":"b"},"trigger":{"type":"string"},"value":{"type":"string"}},
      "required":["kind"],
      "dependentSchemas":{"trigger":{"properties":{"value":{"type":"string","minLength":2}}}},
      "additionalProperties":false
    }
  ]
}"#,
    );
    assert!(common.json_dependent_schemas.is_some());

    let differing = import_str_result(
        r#"{
  "oneOf":[
    {
      "title":"a",
      "type":"object",
      "properties":{"kind":{"const":"a"},"trigger":{"type":"string"},"value":{"type":"string"}},
      "required":["kind"],
      "dependentSchemas":{"trigger":{"properties":{"value":{"type":"string","minLength":2}}}},
      "additionalProperties":false
    },
    {
      "title":"b",
      "type":"object",
      "properties":{"kind":{"const":"b"},"trigger":{"type":"string"},"value":{"type":"string"}},
      "required":["kind"],
      "dependentSchemas":{"trigger":{"properties":{"value":{"type":"string","minLength":3}}}},
      "additionalProperties":false
    }
  ]
}"#,
    );
    assert!(matches!(
        differing,
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("differing dependent schemas")
    ));
}

#[test]
fn object_alternatives_accept_reordered_equivalent_dependent_conjunctions() {
    let schema = import_str(
        r#"{
  "oneOf":[
    {
      "title":"a",
      "type":"object",
      "properties":{
        "kind":{"const":"a"},
        "trigger":{"type":"string"},
        "left":{"type":"string"},
        "right":{"type":"string"}
      },
      "required":["kind"],
      "dependentSchemas":{"trigger":{"allOf":[
        {"properties":{"left":{"type":"string","minLength":2}},"required":["left"]},
        {"properties":{"right":{"const":"ok"}},"required":["right"]}
      ]}},
      "additionalProperties":false
    },
    {
      "title":"b",
      "type":"object",
      "properties":{
        "kind":{"const":"b"},
        "trigger":{"type":"string"},
        "left":{"type":"string"},
        "right":{"type":"string"}
      },
      "required":["kind"],
      "dependentSchemas":{"trigger":{"allOf":[
        {"properties":{"right":{"const":"ok"}},"required":["right"]},
        {"properties":{"left":{"type":"string","minLength":2}},"required":["left"]}
      ]}},
      "additionalProperties":false
    }
  ]
}"#,
    );
    let constraints = schema
        .json_dependent_schemas
        .as_ref()
        .unwrap_or_else(|| panic!("common dependent conjunction should be retained"));
    assert_eq!(constraints.as_slice().len(), 2);

    assert!(
        crate::from_str(
            r#"{"kind":"a","trigger":"yes","left":"ab","right":"ok"}"#,
            &schema
        )
        .is_ok()
    );
    assert!(matches!(
        crate::from_str(
            r#"{"kind":"b","trigger":"yes","left":"a","right":"ok"}"#,
            &schema
        ),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));

    let exported = export(&schema);
    let rendered: serde_json::Value =
        serde_json::from_str(&exported).unwrap_or_else(|error| panic!("{error}"));
    assert!(
        rendered
            .pointer("/dependentSchemas/trigger/allOf/0/properties/left")
            .is_some(),
        "export should retain the first branch's declaration order: {exported}"
    );
    assert_eq!(import_str(&exported), schema);
}

#[test]
fn nullable_repeated_json_lines_and_nested_predicates_execute() {
    let nullable = import_str(
        r#"{
  "type":["object","null"],
  "properties":{"trigger":{"type":"string"},"value":{"type":"string"}},
  "dependentSchemas":{"trigger":{"properties":{"value":{"const":"ok"}}}},
  "additionalProperties":false
}"#,
    );
    assert!(crate::from_str("null", &nullable).is_ok());

    let rows = import_str(
        r#"{
  "type":"array",
  "items":{
    "type":"object",
    "properties":{"trigger":{"type":"string"},"child":{
      "type":"object",
      "properties":{"nested":{"type":"string"},"value":{"type":"string"}},
      "dependentSchemas":{"nested":{"properties":{"value":{"const":"ok"}}}},
      "additionalProperties":false
    }},
    "dependentSchemas":{"trigger":{"required":["child"]}},
    "additionalProperties":false
  }
}"#,
    );
    assert!(
        crate::from_lines(
            r#"{"trigger":"yes","child":{"nested":"yes","value":"ok"}}
{"child":{"value":"anything"}}
"#,
            &rows
        )
        .is_ok()
    );
    assert!(matches!(
        crate::from_lines(
            r#"{"child":{"nested":"yes","value":"wrong"}}
    "#,
            &rows
        ),
        Err(JsonFormatError::DependentSchemaMismatch {
            ref trigger, ..
        }) if trigger == "nested"
    ));
}

#[test]
fn predicate_patterns_are_compiled_into_the_shared_schema_budget() {
    let schema = import_str(
        r#"{
  "type":"object",
  "properties":{"trigger":{"type":"string"},"code":{"type":"string"}},
  "dependentSchemas":{"trigger":{
    "properties":{"code":{"type":"string","pattern":"^ok$"}},
    "required":["code"]
  }},
  "additionalProperties":false
}"#,
    );
    assert!(crate::from_str(r#"{"trigger":"yes","code":"ok"}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"trigger":"yes","code":"wrong"}"#, &schema),
        Err(JsonFormatError::DependentSchemaMismatch { .. })
    ));
}

#[test]
fn malformed_and_corrupted_metadata_reject_without_widening() {
    for malformed in [
        r#"{"type":"object","dependentSchemas":[]}"#,
        r#"{"type":"object","dependentSchemas":{"a":[]}}"#,
        r#"{"type":"object","dependentSchemas":{"a":1}}"#,
    ] {
        assert!(import_str_result(malformed).is_err(), "{malformed}");
    }

    let constraints =
        ir::JsonDependentSchemaConstraints::new([ir::JsonDependentSchemaConstraint::new(
            "a",
            JsonSchemaPredicate::never(),
        )])
        .unwrap_or_else(|| panic!("one never predicate is valid"));
    let mut scalar = ir::SchemaNode::scalar("value", ir::ScalarType::String);
    scalar.json_dependent_schemas = Some(constraints);
    assert!(matches!(
        super::super::export(&scalar),
        Err(JsonFormatError::InvalidDependentSchemasMetadata { .. })
    ));
}

#[test]
fn typeless_dependent_schema_that_admits_non_objects_rejects() {
    assert!(matches!(
        import_str_result(
            r#"{
  "dependentSchemas":{
    "trigger":{"properties":{"value":{"const":"required"}}}
  }
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaObject { ref reason, .. })
            if reason.contains("admit unconstrained non-object values")
    ));
}
