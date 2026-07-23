use std::sync::atomic::{AtomicUsize, Ordering};

use ir::{
    GroupAlternativeConstraintValue, GroupAlternativeMode, ScalarType, SchemaKind, SchemaNode,
};

use super::{export, import};
use crate::JsonFormatError;

fn import_str(text: &str) -> SchemaNode {
    import_str_result(text).unwrap()
}

fn import_str_result(text: &str) -> Result<SchemaNode, JsonFormatError> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "ferrule_json_schema_test_{}_{}.json",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
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

    let optional_constraint_overlap = import_str_result(
        r#"{
  "title":"OptionalConstraintOverlap",
  "oneOf":[
    {"anyOf":[
      {"title":"alpha","type":"object","additionalProperties":false,
        "properties":{"kind":{"type":"string","const":"alpha"}}},
      {"title":"beta","type":"object","additionalProperties":false,
        "properties":{"kind":{"type":"string","const":"beta"}}}
    ]},
    {"type":"object","additionalProperties":false,"required":["other"],
      "properties":{"other":{"type":"string"}}}
  ]
}"#,
    )
    .unwrap_err();
    assert!(
        optional_constraint_overlap
            .to_string()
            .contains("provably mutually exclusive")
    );

    let one_required_constraint = import_str(
        r#"{
  "title":"OneRequiredConstraint",
  "oneOf":[
    {"anyOf":[
      {"title":"alpha","type":"object","additionalProperties":false,
        "properties":{"kind":{"type":"string","const":"alpha"}}},
      {"title":"beta","type":"object","additionalProperties":false,"required":["kind"],
        "properties":{"kind":{"type":"string","const":"beta"}}}
    ]},
    {"type":"object","additionalProperties":false,"required":["other"],
      "properties":{"other":{"type":"string"}}}
  ]
}"#,
    );
    assert!(crate::from_str("{}", &one_required_constraint).is_ok());
    assert!(crate::from_str(r#"{"kind":"beta"}"#, &one_required_constraint).is_ok());
    assert_eq!(
        import_str(&export(&one_required_constraint)),
        one_required_constraint
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
fn typed_alternative_wrappers_accept_only_identical_branch_field_schemas() {
    let schema = import_str(
        r#"{
  "title":"TypedWrapper",
  "additionalProperties":{"type":"string"},
  "oneOf":[
    {"title":"a","type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"}}},
    {"title":"b","type":"object","additionalProperties":false,"required":["b"],"properties":{"b":{"type":"string"}}}
  ]
}"#,
    );
    assert!(crate::from_str(r#"{"a":"one"}"#, &schema).is_ok());
    assert!(crate::from_str(r#"{"b":"two"}"#, &schema).is_ok());
    assert_eq!(import_str(&export(&schema)), schema);

    let mismatch = import_str_result(
        r#"{
  "title":"TypedMismatch",
  "additionalProperties":{"type":"string"},
  "oneOf":[
    {"type":"object","additionalProperties":false,"required":["a"],"properties":{"a":{"type":"string"}}},
    {"type":"object","additionalProperties":false,"required":["b"],"properties":{"b":{"type":"integer"}}}
  ]
}"#,
    )
    .unwrap_err();
    assert!(
        mismatch
            .to_string()
            .contains("typed additionalProperties schema")
    );
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
fn typed_null_const_discriminators_roundtrip_and_validate_instances() {
    for keyword in ["oneOf", "anyOf"] {
        let alternatives = if keyword == "oneOf" {
            r#"
    {"title":"missing","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{
        "kind":{"type":["string","null"],"const":null},
        "value":{"type":"string"}
      }},
    {"title":"present","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{
        "kind":{"type":"string","const":"present"},
        "value":{"type":"string"}
      }}"#
        } else {
            r#"
    {"title":"present","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{
        "kind":{"type":"string","const":"present"},
        "value":{"type":"string"}
      }},
    {"title":"missing","type":"object","additionalProperties":false,
      "required":["kind","value"],
      "properties":{
        "kind":{"type":["string","null"],"const":null},
        "value":{"type":"string"}
      }}"#
        };
        let schema = import_str(&format!(
            r#"{{
  "title":"NullableEvent",
  "{keyword}":[{alternatives}
  ]
}}"#
        ));
        let kind = schema.child("kind").unwrap();
        assert!(kind.nullable);
        assert!(matches!(
            kind.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));
        assert!(schema.alternatives().iter().any(|alternative| {
            alternative.constraints.iter().any(|constraint| {
                constraint.member == "kind"
                    && constraint.value == GroupAlternativeConstraintValue::JsonNull
            })
        }));

        for text in [
            r#"{"kind":null,"value":"none"}"#,
            r#"{"kind":"present","value":"some"}"#,
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
            r#"{"value":"absent"}"#,
            r#"{"kind":"other","value":"wrong"}"#,
        ] {
            assert!(matches!(
                crate::from_str(text, &schema),
                Err(JsonFormatError::NoMatchingAlternative { .. })
            ));
        }

        let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
        let alternatives = exported[keyword].as_array().unwrap();
        let missing = alternatives
            .iter()
            .find(|alternative| alternative["title"] == "missing")
            .unwrap();
        assert_eq!(
            missing["properties"]["kind"]["type"],
            serde_json::json!(["string", "null"])
        );
        assert!(missing["properties"]["kind"]["const"].is_null());
        assert_eq!(import_str(&export(&schema)), schema);
    }
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
fn optional_const_discriminators_match_absence_or_the_exact_value() {
    let schema = import_str(
        r#"{
  "title":"OptionalKind",
  "oneOf":[
    {"title":"alpha","type":"object","additionalProperties":false,
      "required":["alpha"],
      "properties":{"kind":{"type":"string","const":"alpha"},"alpha":{"type":"string"}}},
    {"title":"beta","type":"object","additionalProperties":false,
      "required":["beta"],
      "properties":{"kind":{"type":"string","const":"beta"},"beta":{"type":"string"}}}
  ]
}"#,
    );
    for text in [
        r#"{"alpha":"one"}"#,
        r#"{"kind":"alpha","alpha":"one"}"#,
        r#"{"beta":"two"}"#,
        r#"{"kind":"beta","beta":"two"}"#,
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
        r#"{"kind":"wrong","alpha":"one"}"#,
        r#"{"kind":"beta","alpha":"one"}"#,
    ] {
        assert!(matches!(
            crate::from_str(text, &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
    }

    let exported: serde_json::Value = serde_json::from_str(&export(&schema)).unwrap();
    assert_eq!(exported["oneOf"][0]["properties"]["kind"]["const"], "alpha");
    assert_eq!(
        exported["oneOf"][0]["required"],
        serde_json::json!(["alpha"])
    );
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn optional_constraints_preserve_one_of_ambiguity_and_any_of_overlap() {
    for (keyword, absent_result) in [("oneOf", "ambiguous"), ("anyOf", "accepted")] {
        let schema = import_str(&format!(
            r#"{{
  "title":"OptionalOnly",
  "{keyword}":[
    {{"title":"alpha","type":"object","additionalProperties":false,
      "properties":{{"kind":{{"type":"string","const":"alpha"}}}}}},
    {{"title":"beta","type":"object","additionalProperties":false,
      "properties":{{"kind":{{"type":"string","const":"beta"}}}}}}
  ]
}}"#
        ));
        match absent_result {
            "ambiguous" => assert!(matches!(
                crate::from_str("{}", &schema),
                Err(JsonFormatError::AmbiguousAlternative { .. })
            )),
            "accepted" => assert!(crate::from_str("{}", &schema).is_ok()),
            _ => unreachable!(),
        }
        assert!(crate::from_str(r#"{"kind":"alpha"}"#, &schema).is_ok());
        assert!(matches!(
            crate::from_str(r#"{"kind":"other"}"#, &schema),
            Err(JsonFormatError::NoMatchingAlternative { .. })
        ));
        assert_eq!(import_str(&export(&schema)), schema);
    }
}

#[test]
fn optional_typed_null_constraints_distinguish_absence_presence_and_wrong_values() {
    let schema = import_str(
        r#"{
  "title":"OptionalNull",
  "oneOf":[
    {"title":"null","type":"object","additionalProperties":false,
      "required":["nullValue"],
      "properties":{
        "kind":{"type":["string","null"],"const":null},
        "nullValue":{"type":"string"}
      }},
    {"title":"present","type":"object","additionalProperties":false,
      "required":["presentValue"],
      "properties":{
        "kind":{"type":"string","const":"present"},
        "presentValue":{"type":"string"}
      }}
  ]
}"#,
    );
    for text in [
        r#"{"nullValue":"absent kind"}"#,
        r#"{"kind":null,"nullValue":"explicit null"}"#,
        r#"{"presentValue":"absent kind"}"#,
        r#"{"kind":"present","presentValue":"explicit"}"#,
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
    assert!(matches!(
        crate::from_str(r#"{"kind":"wrong","nullValue":"bad"}"#, &schema),
        Err(JsonFormatError::NoMatchingAlternative { .. })
    ));
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn unsupported_const_discriminators_are_rejected_actionably() {
    for (property, required, expected) in [
        (
            r#"{"type":"string","const":1}"#,
            r#", "required":["kind"]"#,
            "does not match its declared scalar type",
        ),
        (
            r#"{"type":"string","const":null}"#,
            r#", "required":["kind"]"#,
            "explicitly includes null",
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

    for (first, second, expected) in [
        (
            r#"{"type":["string","null"],"const":null}"#,
            r#"{"type":"string"}"#,
            "incompatible schemas across alternatives",
        ),
        (
            r#"{"type":["string","null"],"const":null}"#,
            r#"{"type":"integer","const":1}"#,
            "incompatible schemas across alternatives",
        ),
    ] {
        let text = format!(
            r#"{{
  "title":"UnsupportedNullableConstraint",
  "oneOf":[
    {{"title":"first","type":"object","additionalProperties":false,
      "required":["kind"],"properties":{{"kind":{first}}}}},
    {{"title":"second","type":"object","additionalProperties":false,
      "required":["kind"],"properties":{{"kind":{second}}}}}
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
    assert!(schema.child("Count").is_some_and(|child| child.nullable));
    let exported = export(&schema);
    let value: serde_json::Value = serde_json::from_str(&exported).unwrap();
    assert_eq!(
        value["properties"]["Count"]["type"],
        serde_json::json!(["integer", "null"])
    );
    assert_eq!(import_str(&exported), schema);
}

#[test]
fn nullable_scalar_one_of_and_any_of_are_canonical_and_executable() {
    for keyword in ["oneOf", "anyOf"] {
        let text = format!(
            r#"{{
  "title":"MaybeText",
  "{keyword}":[
    {{"type":"null","description":"missing"}},
    {{"type":"string","title":"present"}}
  ]
}}"#
        );
        let schema = import_str(&text);
        assert!(schema.nullable);
        assert!(matches!(
            schema.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        ));
        for input in [r#""value""#, "null"] {
            let instance = crate::from_str(input, &schema).unwrap();
            assert_eq!(
                serde_json::from_str::<serde_json::Value>(
                    &crate::to_string(&schema, &instance).unwrap()
                )
                .unwrap(),
                serde_json::from_str::<serde_json::Value>(input).unwrap()
            );
        }
        assert!(matches!(
            crate::from_str("7", &schema),
            Err(JsonFormatError::Shape { .. })
        ));
        let exported = export(&schema);
        let value: serde_json::Value = serde_json::from_str(&exported).unwrap();
        assert_eq!(value["type"], serde_json::json!(["string", "null"]));
        assert_eq!(import_str(&exported), schema);
    }
}

#[test]
fn nullable_scalar_refs_and_array_items_preserve_null_values() {
    let referenced = import_str(
        r##"{
  "title":"Envelope",
  "type":"object",
  "properties":{"value":{"$ref":"#/$defs/maybeBoolean"}},
  "$defs":{
    "maybeBoolean":{
      "anyOf":[{"type":"boolean"},{"type":"null"}]
    }
  }
}"##,
    );
    let value = referenced.child("value").unwrap();
    assert!(value.nullable);
    assert!(matches!(
        value.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Bool
        }
    ));

    let sequence = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "items":{"oneOf":[{"type":"integer"},{"type":"null"}]}
}"#,
    );
    assert!(sequence.repeating);
    assert!(sequence.nullable);
    let instance = crate::from_str("[1,null,2]", &sequence).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&sequence, &instance).unwrap())
            .unwrap(),
        serde_json::json!([1, null, 2])
    );
    assert_eq!(import_str(&export(&sequence)), sequence);
}

#[test]
fn nullable_scalar_unions_reject_unrepresentable_validation() {
    for validation in [
        r#""minLength":1"#,
        r#""pattern":"^[a-z]+$""#,
        r#""enum":["a","b"]"#,
    ] {
        let text = format!(
            r#"{{
  "title":"Constrained",
  "oneOf":[
    {{"type":"string",{validation}}},
    {{"type":"null"}}
  ]
}}"#
        );
        let error = import_str_result(&text).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("nullable scalar alternatives cannot preserve"),
            "{error}"
        );
    }

    for text in [
        r#"{"title":"NullOnly","type":"null"}"#,
        r#"{"title":"MaybeObject","type":["object","null"],"properties":{}}"#,
        r#"{"title":"MaybeArray","type":["array","null"],"items":{"type":"string"}}"#,
    ] {
        assert!(matches!(
            import_str_result(text),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
        ));
    }
}

#[test]
fn object_alternatives_keep_nullable_field_presence_branch_neutral() {
    let schema = import_str(
        r#"{
  "title":"Events",
  "oneOf":[
    {
      "title":"created",
      "type":"object",
      "additionalProperties":false,
      "required":["kind","payload"],
      "properties":{
        "kind":{"type":"string","const":"created"},
        "payload":{"type":["string","null"]}
      }
    },
    {
      "title":"deleted",
      "type":"object",
      "additionalProperties":false,
      "required":["kind","payload"],
      "properties":{
        "kind":{"type":"string","const":"deleted"},
        "payload":{"oneOf":[{"type":"string"},{"type":"null"}]}
      }
    }
  ]
}"#,
    );
    let instance = crate::from_str(r#"{"kind":"created","payload":null}"#, &schema).unwrap();
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&crate::to_string(&schema, &instance).unwrap())
            .unwrap(),
        serde_json::json!({"kind":"created","payload":null})
    );
    assert_eq!(import_str(&export(&schema)), schema);

    let presence_sensitive = import_str(
        r#"{
  "title":"PresenceSensitive",
  "oneOf":[
    {"title":"absent","type":"object","additionalProperties":false,"properties":{}},
    {"title":"present","type":"object","additionalProperties":false,"required":["value"],
      "properties":{"value":{"type":["string","null"]}}}
  ]
}"#,
    );
    for input in [r#"{}"#, r#"{"value":null}"#] {
        let instance = crate::from_str(input, &presence_sensitive).unwrap();
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(
                &crate::to_string(&presence_sensitive, &instance).unwrap()
            )
            .unwrap(),
            serde_json::from_str::<serde_json::Value>(input).unwrap()
        );
    }
    assert_eq!(
        crate::from_str(r#"{}"#, &presence_sensitive)
            .unwrap()
            .field("value"),
        Some(&ir::Instance::Scalar(ir::Value::Null))
    );
    assert_eq!(
        crate::from_str(r#"{"value":null}"#, &presence_sensitive)
            .unwrap()
            .field("value"),
        Some(&ir::Instance::Scalar(ir::Value::json_null()))
    );
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
