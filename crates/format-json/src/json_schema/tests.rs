use ir::{
    GroupAlternativeConstraintValue, GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode,
};

use super::{export, import};
use crate::JsonFormatError;

fn import_str(text: &str) -> SchemaNode {
    import_str_result(text).unwrap()
}

fn import_str_result(text: &str) -> Result<SchemaNode, JsonFormatError> {
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "ferrule_json_schema_test_{}_{}.json",
        std::process::id(),
        text.len()
    ));
    std::fs::write(&path, text).unwrap();
    let schema = import(&path);
    std::fs::remove_file(&path).unwrap();
    schema
}

#[test]
fn compatible_object_one_of_preserves_and_roundtrips_alternatives() {
    let schema = import_str(
        r##"{
  "title": "Address",
  "type": "object",
  "oneOf": [
    { "$ref": "#/definitions/domestic" },
    { "$ref": "#/definitions/international" }
  ],
  "definitions": {
    "domestic": {
      "type": "object",
      "additionalProperties": false,
      "required": ["name", "state"],
      "properties": {
        "name": { "type": "string" },
        "state": { "type": "string" }
      }
    },
    "international": {
      "additionalProperties": false,
      "properties": {
        "name": { "type": "string" },
        "postcode": { "type": "string" }
      },
      "required": ["name", "postcode"]
    }
  }
}"##,
    );
    let SchemaKind::Group {
        children,
        alternatives,
        ..
    } = &schema.kind
    else {
        panic!("oneOf should import as a group projection");
    };
    assert_eq!(
        children
            .iter()
            .map(|child| child.name.as_str())
            .collect::<Vec<_>>(),
        ["name", "state", "postcode"]
    );
    assert_eq!(
        alternatives
            .iter()
            .map(|alternative| alternative.name.as_str())
            .collect::<Vec<_>>(),
        ["domestic", "international"]
    );

    let path = std::env::temp_dir().join(format!(
        "ferrule_json_schema_one_of_roundtrip_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, export(&schema)).unwrap();
    let roundtrip = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(roundtrip, schema);
}

#[test]
fn compatible_object_any_of_preserves_inclusive_matching_and_roundtrips() {
    let schema = import_str(
        r##"{
  "title": "Record",
  "anyOf": [
    { "$ref": "#/$defs/labeled" },
    { "$ref": "#/$defs/detailed" }
  ],
  "$defs": {
    "labeled": {
      "title": "labeled",
      "type": "object",
      "additionalProperties": false,
      "required": ["id", "label"],
      "properties": {
        "id": { "type": "integer" },
        "label": { "type": "string" }
      }
    },
    "detailed": {
      "title": "detailed",
      "type": "object",
      "additionalProperties": false,
      "required": ["id"],
      "properties": {
        "id": { "type": "integer" },
        "label": { "type": "string" },
        "note": { "type": "string" }
      }
    }
  }
}"##,
    );
    assert_eq!(schema.alternative_mode(), GroupAlternativeMode::Inclusive);
    let alternatives = schema.alternatives();
    let universally_required = alternatives[0]
        .required
        .iter()
        .filter(|field| {
            alternatives[1..]
                .iter()
                .all(|alternative| alternative.required.contains(field))
        })
        .map(String::as_str)
        .collect::<Vec<_>>();
    assert_eq!(universally_required, ["id"]);
    assert!(crate::from_str(r#"{"id":7,"label":"both"}"#, &schema).is_ok());
    assert!(crate::from_str(r#"{"id":7}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str("{}", &schema),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));

    let exported = export(&schema);
    assert!(exported.contains("\"anyOf\""));
    assert!(!exported.contains("\"oneOf\""));
    let path = std::env::temp_dir().join(format!(
        "ferrule_json_schema_any_of_roundtrip_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, exported).unwrap();
    let roundtrip = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(roundtrip, schema);
}

#[test]
fn incompatible_object_any_of_is_rejected_actionably() {
    let conflicting = import_str_result(
        r#"{
  "title": "Conflict",
  "anyOf": [
    { "type": "object", "additionalProperties": false,
      "properties": { "value": { "type": "string" } } },
    { "type": "object", "additionalProperties": false,
      "properties": { "value": { "type": "integer" } } }
  ]
}"#,
    )
    .unwrap_err();
    assert!(
        conflicting
            .to_string()
            .contains("field `value` has incompatible schemas")
    );

    let mixed = import_str_result(
        r#"{
  "title": "Mixed",
  "anyOf": [
    { "type": "object", "additionalProperties": false, "properties": {} },
    { "type": "string" }
  ]
}"#,
    )
    .unwrap_err();
    assert!(mixed.to_string().contains("only object alternatives"));
}

#[test]
fn pure_nested_one_of_wrappers_flatten_and_roundtrip() {
    let schema = import_str(
        r##"{
  "title":"Event",
  "oneOf":[
    {"$ref":"#/$defs/action"},
    {"title":"heartbeat","type":"object","additionalProperties":false,
      "required":["kind"],"properties":{"kind":{"const":"heartbeat"}}}
  ],
  "$defs":{"action":{"title":"action","oneOf":[
    {"title":"created","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{"kind":{"const":"created"},"value":{"type":"string"}}},
    {"title":"deleted","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{"kind":{"const":"deleted"},"value":{"type":"string"}}}
  ]}}
}"##,
    );
    assert_eq!(
        schema
            .alternatives()
            .iter()
            .map(|alternative| alternative.name.as_str())
            .collect::<Vec<_>>(),
        ["action/created", "action/deleted", "heartbeat"]
    );
    for value in [
        r#"{"kind":"created","value":"one"}"#,
        r#"{"kind":"deleted","value":"two"}"#,
        r#"{"kind":"heartbeat"}"#,
    ] {
        assert!(crate::from_str(value, &schema).is_ok(), "{value}");
    }
    assert!(matches!(
        crate::from_str(r#"{"kind":"unknown"}"#, &schema),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));

    let roundtrip = import_str(&export(&schema));
    assert_eq!(roundtrip, schema);
}

#[test]
fn nested_union_flattening_preserves_base_fields_and_proves_cross_mode_safety() {
    let inclusive = import_str(
        r#"{
  "title":"Inclusive",
  "anyOf":[
    {"title":"nested","anyOf":[
      {"title":"alpha","type":"object","additionalProperties":false,"required":["alpha"],"properties":{"alpha":{"type":"string"}}},
      {"title":"beta","type":"object","additionalProperties":false,"required":["beta"],"properties":{"beta":{"type":"string"}}}
    ]},
    {"title":"gamma","type":"object","additionalProperties":false,"required":["gamma"],"properties":{"gamma":{"type":"string"}}}
  ]
}"#,
    );
    assert_eq!(
        inclusive.alternative_mode(),
        GroupAlternativeMode::Inclusive
    );
    assert_eq!(inclusive.alternatives().len(), 3);
    assert!(crate::from_str(r#"{"beta":"yes"}"#, &inclusive).is_ok());
    assert_eq!(import_str(&export(&inclusive)), inclusive);

    let disjoint_mixed_mode = import_str(
        r#"{
  "title":"Mixed",
  "oneOf":[
    {"anyOf":[
      {"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"}}},
      {"type":"object","additionalProperties":false,"required":["b"],"properties":{"b":{"type":"string"}}}
    ]},
    {"type":"object","additionalProperties":false,"required":["c"],"properties":{"c":{"type":"string"}}}
  ]
}"#,
    );
    assert_eq!(disjoint_mixed_mode.alternatives().len(), 3);
    assert!(crate::from_str(r#"{"a":"one"}"#, &disjoint_mixed_mode).is_ok());
    assert_eq!(
        import_str(&export(&disjoint_mixed_mode)),
        disjoint_mixed_mode
    );

    let overlapping_mixed_mode = import_str_result(
        r#"{
  "title":"Overlapping",
  "oneOf":[
    {"anyOf":[
      {"title":"plain","type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"type":"string"}}},
      {"title":"tagged","type":"object","additionalProperties":false,"required":["value"],"properties":{"value":{"type":"string"},"tag":{"type":"string"}}}
    ]},
    {"type":"object","additionalProperties":false,"required":["other"],"properties":{"other":{"type":"string"}}}
  ]
}"#,
    )
    .unwrap_err();
    assert!(
        overlapping_mixed_mode
            .to_string()
            .contains("provably mutually exclusive")
    );

    let constrained = import_str(
        r#"{
  "title":"Constrained",
  "anyOf":[
    {"required":["base"],"properties":{"base":{"type":"string"}},"anyOf":[
      {"type":"object","additionalProperties":false,"required":["base","a"],"properties":{"base":{"type":"string"},"a":{"type":"string"}}},
      {"type":"object","additionalProperties":false,"required":["base","b"],"properties":{"base":{"type":"string"},"b":{"type":"string"}}}
    ]},
    {"type":"object","additionalProperties":false,"required":["c"],"properties":{"c":{"type":"string"}}}
  ]
}"#,
    );
    assert_eq!(constrained.alternatives().len(), 3);
    assert!(crate::from_str(r#"{"base":"root","a":"one"}"#, &constrained).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"a":"missing base"}"#, &constrained),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));
    assert_eq!(import_str(&export(&constrained)), constrained);

    let closed_wrapper = import_str(
        r#"{
  "title":"ClosedWrapper",
  "oneOf":[
    {"additionalProperties":false,"properties":{"a":{"type":"string"},"b":{"type":"string"}},"oneOf":[
      {"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"}}},
      {"type":"object","additionalProperties":false,"required":["b"],"properties":{"b":{"type":"string"}}}
    ]},
    {"type":"object","additionalProperties":false,"required":["c"],"properties":{"c":{"type":"string"}}}
  ]
}"#,
    );
    assert!(crate::from_str(r#"{"a":"one"}"#, &closed_wrapper).is_ok());
    assert!(crate::from_str(r#"{"b":"two"}"#, &closed_wrapper).is_ok());
    assert_eq!(import_str(&export(&closed_wrapper)), closed_wrapper);

    let impossible_closure = import_str_result(
        r#"{
  "title":"Impossible",
  "oneOf":[
    {"additionalProperties":false,"oneOf":[
      {"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"}}},
      {"type":"object","additionalProperties":false,"required":["b"],"properties":{"b":{"type":"string"}}}
    ]},
    {"type":"object","additionalProperties":false,"required":["c"],"properties":{"c":{"type":"string"}}}
  ]
}"#,
    )
    .unwrap_err();
    assert!(impossible_closure.to_string().contains("requires a field"));
}

#[test]
fn alternative_wrappers_apply_closed_branch_field_intersections() {
    let schema = import_str(
        r#"{
  "title":"Intersection",
  "properties":{"wrapperOnly":{"type":"string"}},
  "oneOf":[
    {"title":"a","type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"}}},
    {"title":"b","type":"object","additionalProperties":false,"required":["b"],"properties":{"b":{"type":"string"}}}
  ]
}"#,
    );
    assert!(schema.child("wrapperOnly").is_none());
    assert!(crate::from_str(r#"{"a":"one"}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"wrapperOnly":"x","a":"one"}"#, &schema),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn incompatible_and_scalar_one_of_are_rejected() {
    let incompatible = import_str_result(
            r#"{
  "title": "Bad",
  "oneOf": [
    { "type": "object", "additionalProperties": false, "properties": { "value": { "type": "string" } } },
    { "type": "object", "additionalProperties": false, "properties": { "value": { "type": "integer" } } }
  ]
}"#,
        )
        .unwrap_err();
    assert!(incompatible.to_string().contains("incompatible schemas"));

    let scalar = import_str_result(
        r#"{
  "title": "Scalar",
  "oneOf": [{ "type": "string" }, { "type": "integer" }]
}"#,
    )
    .unwrap_err();
    assert!(scalar.to_string().contains("only object alternatives"));
}

#[test]
fn required_scalar_const_discriminators_roundtrip_and_validate_instances() {
    let schema = import_str(
        r#"{
  "title": "Event",
  "oneOf": [
    { "title": "created", "type": "object", "additionalProperties": false,
      "required": ["kind", "value"],
      "properties": {
        "kind": { "type": "string", "const": "created" },
        "value": { "type": "string" }
      } },
    { "title": "deleted", "type": "object", "additionalProperties": false,
      "required": ["kind", "value"],
      "properties": {
        "kind": { "type": "string", "const": "deleted" },
        "value": { "type": "string" }
      } }
  ]
}"#,
    );
    assert_eq!(
        schema
            .alternatives()
            .iter()
            .map(|alternative| {
                let constraint = &alternative.constraints[0];
                (constraint.member.as_str(), &constraint.value)
            })
            .collect::<Vec<_>>(),
        [
            (
                "kind",
                &GroupAlternativeConstraintValue::String("created".into())
            ),
            (
                "kind",
                &GroupAlternativeConstraintValue::String("deleted".into())
            )
        ]
    );
    for text in [
        r#"{"kind":"created","value":"one"}"#,
        r#"{"kind":"deleted","value":"two"}"#,
    ] {
        let instance = crate::from_str(text, &schema).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &crate::to_string(&schema, &instance).unwrap()
            )
            .unwrap(),
            serde_json::from_str::<serde_json::Value>(text).unwrap()
        );
    }
    for text in [
        r#"{"kind":"changed","value":"three"}"#,
        r#"{"value":"four"}"#,
    ] {
        assert!(matches!(
            crate::from_str(text, &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
    }

    let exported = export(&schema);
    let exported_value: serde_json::Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        exported_value["oneOf"][0]["properties"]["kind"]["const"],
        "created"
    );
    assert_eq!(
        exported_value["oneOf"][1]["properties"]["kind"]["const"],
        "deleted"
    );
    let path = std::env::temp_dir().join(format!(
        "ferrule_json_schema_discriminator_roundtrip_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, exported).unwrap();
    let roundtrip = import(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    assert_eq!(roundtrip, schema);
}

#[test]
fn inclusive_alternatives_honor_required_scalar_const_discriminators() {
    let schema = import_str(
        r#"{
  "title":"Message",
  "anyOf":[
    {"type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":"text"}}},
    {"type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":"image"}}}
  ]
}"#,
    );
    assert_eq!(schema.alternative_mode(), GroupAlternativeMode::Inclusive);
    assert!(crate::from_str(r#"{"kind":"text"}"#, &schema).is_ok());
    assert!(matches!(
        crate::from_str(r#"{"kind":"audio"}"#, &schema),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));
    let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
    assert_eq!(exported["anyOf"][1]["properties"]["kind"]["const"], "image");
}

#[test]
fn bool_integer_and_number_const_discriminators_roundtrip() {
    let cases = [
        (
            "boolean",
            "true",
            "false",
            r#"{"kind":true,"value":"one"}"#,
            r#"{"kind":false,"value":"two"}"#,
            r#"{"kind":"true","value":"bad"}"#,
        ),
        (
            "integer",
            "7",
            "9",
            r#"{"kind":7,"value":"one"}"#,
            r#"{"kind":9,"value":"two"}"#,
            r#"{"kind":8,"value":"bad"}"#,
        ),
        (
            "number",
            "1.25",
            "2.5",
            r#"{"kind":1.25,"value":"one"}"#,
            r#"{"kind":2.5,"value":"two"}"#,
            r#"{"kind":3.75,"value":"bad"}"#,
        ),
    ];
    for (ty, first, second, first_instance, second_instance, rejected) in cases {
        let text = format!(
            r#"{{
  "title":"TypedEvent",
  "oneOf":[
    {{"title":"first","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{{"kind":{{"type":"{ty}","const":{first}}},"value":{{"type":"string"}}}}}},
    {{"title":"second","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{{"kind":{{"type":"{ty}","const":{second}}},"value":{{"type":"string"}}}}}}
  ]
}}"#
        );
        let schema = import_str(&text);
        for instance_text in [first_instance, second_instance] {
            let instance = crate::from_str(instance_text, &schema).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    &crate::to_string(&schema, &instance).unwrap()
                )
                .unwrap(),
                serde_json::from_str::<serde_json::Value>(instance_text).unwrap()
            );
        }
        assert!(matches!(
            crate::from_str(rejected, &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));

        let path = std::env::temp_dir().join(format!(
            "ferrule_json_schema_typed_discriminator_{}_{}.json",
            ty,
            std::process::id()
        ));
        std::fs::write(&path, export(&schema)).unwrap();
        let roundtrip = import(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        assert_eq!(roundtrip, schema);
    }
}

#[test]
fn const_discriminators_infer_scalar_types() {
    let schema = import_str(
        r#"{
  "title":"Implicit",
  "oneOf":[
    {"title":"yes","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":true}}},
    {"title":"no","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"const":false}}}
  ]
}"#,
    );
    assert!(matches!(
        schema.child("kind").map(|child| &child.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Bool
        })
    ));
    assert!(crate::from_str(r#"{"kind":true}"#, &schema).is_ok());
    let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
    assert_eq!(exported["oneOf"][0]["properties"]["kind"]["const"], true);
}

#[test]
fn unsupported_const_discriminators_are_rejected_actionably() {
    for (property, required, expected) in [
        (r#"{"type":"string","const":"a"}"#, "", "must be required"),
        (
            r#"{"type":"string","const":1}"#,
            r#", "required":["kind"]"#,
            "does not match its declared scalar type",
        ),
        (
            r#"{"type":"string","const":null}"#,
            r#", "required":["kind"]"#,
            "cannot be null",
        ),
        (
            r#"{"type":"integer","const":9223372036854775808}"#,
            r#", "required":["kind"]"#,
            "signed 64-bit integer",
        ),
        (
            r#"{"type":"number","const":9007199254740993}"#,
            r#", "required":["kind"]"#,
            "finite exactly supported number",
        ),
    ] {
        let text = format!(
            r#"{{
  "title":"Unsupported",
  "oneOf":[
    {{"type":"object","additionalProperties":false{required},"properties":{{"kind":{property}}}}},
    {{"type":"object","additionalProperties":false,"required":["other"],"properties":{{"other":{{"type":"string"}}}}}}
  ]
}}"#
        );
        let error = import_str_result(&text).unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }

    let ambiguous = import_str_result(
        r#"{
  "title":"Ambiguous",
  "oneOf":[
    {"title":"first","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"type":"boolean","const":true}}},
    {"title":"second","type":"object","additionalProperties":false,"required":["kind"],
      "properties":{"kind":{"type":"boolean","const":true}}}
  ]
}"#,
    )
    .unwrap_err();
    assert!(
        ambiguous
            .to_string()
            .contains("alternatives are not distinguishable"),
        "{ambiguous}"
    );

    for property in [
        r#"{"type":"array","const":[],"items":{"type":"string"}}"#,
        r#"{"type":"object","const":{},"additionalProperties":false}"#,
    ] {
        let text = format!(
            r#"{{
  "title":"StructuredConst",
  "oneOf":[
    {{"type":"object","additionalProperties":false,"required":["kind"],"properties":{{"kind":{property}}}}},
    {{"type":"object","additionalProperties":false,"required":["other"],"properties":{{"other":{{"type":"string"}}}}}}
  ]
}}"#
        );
        let error = import_str_result(&text).unwrap_err();
        assert!(
            error.to_string().contains("const discriminator `kind`"),
            "{error}"
        );
    }
}

#[test]
fn imports_nested_arrays_and_objects() {
    let schema = import_str(
        r#"{
  "title": "Orders",
  "type": "object",
  "properties": {
    "Date": { "type": "string" },
    "Order": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "Order_ID": { "type": "string" },
          "Total": { "type": "number" },
          "Line_Count": { "type": "integer" },
          "Rush": { "type": "boolean" }
        }
      }
    }
  }
}"#,
    );

    assert_eq!(schema.name, "Orders");
    assert!(!schema.repeating);
    assert!(!schema.child("Date").unwrap().repeating);

    let order = schema.child("Order").unwrap();
    assert!(order.repeating);
    assert!(matches!(
        order.child("Total").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Float
        }
    ));
    assert!(matches!(
        order.child("Line_Count").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert!(matches!(
        order.child("Rush").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Bool
        }
    ));
}

#[test]
fn resolves_local_refs_including_root_and_defs() {
    let schema = import_str(
        r##"{
  "$ref": "#/definitions/company",
  "definitions": {
    "company": {
      "title": "Company",
      "type": "object",
      "properties": {
        "Name": { "type": "string" },
        "Office": {
          "type": "array",
          "items": { "$ref": "#/$defs/office" }
        }
      }
    }
  },
  "$defs": {
    "office": {
      "type": "object",
      "properties": {
        "City": { "type": "string" },
        "Staff": { "type": "integer" }
      }
    }
  }
}"##,
    );

    assert_eq!(schema.name, "Company");
    let office = schema.child("Office").unwrap();
    assert!(office.repeating);
    assert!(matches!(
        office.child("Staff").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
}

#[test]
fn cyclic_and_external_refs_degrade_to_string_scalars() {
    let schema = import_str(
        r##"{
  "title": "Tree",
  "type": "object",
  "properties": {
    "Label": { "type": "string" },
    "Child": { "$ref": "#/properties/Child" },
    "Remote": { "$ref": "other.json#/definitions/x" }
  }
}"##,
    );

    for field in ["Child", "Remote"] {
        assert!(matches!(
            schema.child(field).unwrap().kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));
    }
}

#[test]
fn nullable_type_arrays_use_the_only_non_null_type() {
    let schema = import_str(
        r#"{
  "title": "Row",
  "type": "object",
  "properties": {
    "Count": { "type": ["integer", "null"] }
  }
}"#,
    );
    assert!(matches!(
        schema.child("Count").unwrap().kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
}

#[test]
fn type_arrays_with_multiple_non_null_types_are_rejected() {
    let error = import_str_result(
        r#"{
  "title":"Ambiguous",
  "type":["string", "integer", "null"]
}"#,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("type arrays may contain only one non-null type")
    );
}

#[test]
fn repeating_object_alternatives_are_rejected() {
    let error = import_str_result(
        r#"{
  "title":"Sequences",
  "oneOf":[
    {"type":"array","items":{"type":"object","additionalProperties":false,"properties":{}}},
    {"type":"array","items":{"type":"object","additionalProperties":false,"properties":{}}}
  ]
}"#,
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("array alternatives are not supported")
    );
}

#[test]
fn export_then_import_roundtrips() {
    let schema = SchemaNode::group(
        "Orders",
        vec![
            SchemaNode::scalar("Date", ScalarType::String),
            SchemaNode::group(
                "Order",
                vec![
                    SchemaNode::scalar("Qty", ScalarType::Int),
                    SchemaNode::scalar("Price", ScalarType::Float),
                    SchemaNode::scalar("Rush", ScalarType::Bool),
                ],
            )
            .repeating(),
        ],
    );
    let text = export(&schema);
    let path = std::env::temp_dir().join(format!(
        "ferrule_json_schema_export_test_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, text).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn repeating_root_exports_as_top_level_array() {
    let schema =
        SchemaNode::group("Rows", vec![SchemaNode::scalar("Name", ScalarType::String)]).repeating();
    let text = export(&schema);
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value["type"], "array");
    assert_eq!(value["items"]["type"], "object");

    let path = std::env::temp_dir().join(format!(
        "ferrule_json_schema_export_arr_test_{}.json",
        std::process::id()
    ));
    std::fs::write(&path, export(&schema)).unwrap();
    let imported = import(&path).unwrap();
    std::fs::remove_file(&path).unwrap();
    assert_eq!(imported, schema);
}

#[test]
fn typed_additional_properties_roundtrip_as_dynamic_fields() {
    let schema = import_str(
        r#"{
  "title": "Metrics",
  "type": "object",
  "properties": { "source": { "type": "string" } },
  "additionalProperties": { "type": "number" }
}"#,
    );
    assert!(matches!(
        schema.dynamic_fields().map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Float
        })
    ));
    let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
    assert_eq!(exported["additionalProperties"]["type"], "number");
}

#[test]
fn typed_object_additional_properties_preserve_their_exact_shape() {
    let schema = import_str(
        r#"{
  "title": "Directory",
  "type": "object",
  "additionalProperties": {
    "type": "object",
    "properties": { "name": { "type": "string" } },
    "additionalProperties": false
  }
}"#,
    );
    let dynamic = schema.dynamic_fields().unwrap();
    assert!(matches!(dynamic.kind, SchemaKind::Group { .. }));
    assert_eq!(
        dynamic.child("name").map(|child| &child.kind),
        Some(&SchemaKind::Scalar {
            ty: ScalarType::String,
        })
    );

    let exported = export(&schema);
    let value: serde_json::Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(value["additionalProperties"]["additionalProperties"], false);
    assert_eq!(import_str(&exported), schema);
}

#[test]
fn explicit_unconstrained_additional_properties_are_rejected() {
    for additional in ["true", "{}"] {
        let text =
            format!(r#"{{"title":"Open","type":"object","additionalProperties":{additional}}}"#);
        assert!(matches!(
            import_str_result(&text),
            Err(JsonFormatError::UnsupportedSchemaObject { reason, .. })
                if reason.contains("unconstrained additionalProperties")
        ));
    }
}

#[test]
fn closed_groups_export_explicit_closed_object_semantics() {
    let schema = SchemaNode::group(
        "Closed",
        vec![SchemaNode::scalar("value", ScalarType::String)],
    );
    let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
    assert_eq!(exported["additionalProperties"], false);
}
