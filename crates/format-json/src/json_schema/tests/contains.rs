use ir::{Instance, JsonContainsPredicate, SchemaNode, Value};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn direct_default_and_modern_matching_counts_execute_on_input_and_output() {
    let default = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "items":{"type":"integer"},
  "contains":{"type":"integer","minimum":1}
}"#,
    );
    assert!(crate::from_str("[0,1]", &default).is_ok());
    assert!(matches!(
        crate::from_str("[0]", &default),
        Err(JsonFormatError::ContainsCountMismatch { got: 0, .. })
    ));

    let bounded = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "title":"Values",
  "type":"array",
  "items":{"type":"integer"},
  "contains":{"type":"integer","minimum":1},
  "minContains":2,
  "maxContains":2
}"#,
    );
    let instance = Instance::Repeated(vec![
        Instance::Scalar(Value::Int(1)),
        Instance::Scalar(Value::Int(2)),
    ]);
    assert!(crate::from_str("[1,2]", &bounded).is_ok());
    assert!(crate::to_string(&bounded, &instance).is_ok());
    assert!(matches!(
        crate::from_str("[1,2,3]", &bounded),
        Err(JsonFormatError::ContainsCountMismatch { got: 3, .. })
    ));
}

#[test]
fn draft_dialects_select_contains_and_count_keyword_semantics() {
    let draft_four = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "type":"array",
  "items":{"type":"integer"},
  "contains":false,
  "minContains":"ignored",
  "maxContains":"ignored"
}"#,
    );
    assert!(draft_four.json_contains.is_none());
    assert!(crate::from_str("[]", &draft_four).is_ok());

    for dialect in [
        "http://json-schema.org/draft-06/schema#",
        "http://json-schema.org/draft-07/schema#",
    ] {
        let schema = import_str(&format!(
            r#"{{
  "$schema":"{dialect}",
  "type":"array",
  "items":{{"type":"integer"}},
  "contains":{{"type":"integer"}},
  "minContains":"ignored",
  "maxContains":"ignored"
}}"#
        ));
        assert!(crate::from_str("[1]", &schema).is_ok());
        assert!(crate::from_str("[]", &schema).is_err());
    }
}

#[test]
fn boolean_predicates_canonicalize_tautologies_and_impossibilities() {
    let always = import_str(
        r#"{
  "type":"array",
  "items":{"type":"integer"},
  "contains":true,
  "minContains":2,
  "maxContains":3
}"#,
    );
    assert!(always.json_contains.is_none());
    let range = always
        .item_count_range
        .unwrap_or_else(|| panic!("true predicate becomes an item-count range"));
    assert_eq!((range.minimum(), range.maximum()), (2, Some(3)));

    let tautology = import_str(
        r#"{
  "type":"array",
  "items":{"type":"integer"},
  "contains":false,
  "minContains":0,
  "maxContains":0
}"#,
    );
    assert!(tautology.json_contains.is_none());

    let impossible = import_str(r#"{"type":"array","items":{"type":"integer"},"contains":false}"#);
    let predicate = impossible
        .json_contains
        .as_ref()
        .and_then(|constraints| constraints.as_slice().first())
        .map(|constraint| constraint.predicate())
        .unwrap_or_else(|| panic!("false predicate must remain explicit"));
    assert!(matches!(predicate, JsonContainsPredicate::Never));
    assert!(crate::from_str("[]", &impossible).is_err());
    assert!(crate::from_str("[1]", &impossible).is_err());
}

#[test]
fn refs_follow_owning_dialect_and_local_predicate_refs_resolve() {
    let modern = import_str(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/Rows",
  "contains":{"$ref":"#/$defs/Positive"},
  "$defs":{
    "Rows":{"type":"array","items":{"type":"integer"}},
    "Positive":{"type":"integer","minimum":1}
  }
}"##,
    );
    assert!(crate::from_str("[0,1]", &modern).is_ok());
    assert!(crate::from_str("[0]", &modern).is_err());

    let legacy = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "$ref":"#/definitions/Rows",
  "contains":false,
  "definitions":{"Rows":{"type":"array","items":{"type":"integer"}}}
}"##,
    );
    assert!(legacy.json_contains.is_none());
    assert!(crate::from_str("[]", &legacy).is_ok());
}

#[test]
fn all_of_requires_every_distinct_contains_assertion_and_roundtrips() {
    let schema = import_str(
        r#"{
  "title":"Mixed",
  "type":"array",
  "items":{"type":["string","integer"]},
  "allOf":[
    {"contains":{"type":"integer"}},
    {"contains":{"type":"string"}}
  ]
}"#,
    );
    assert_eq!(
        schema
            .json_contains
            .as_ref()
            .map(|constraints| constraints.as_slice().len()),
        Some(2)
    );
    assert!(crate::from_str(r#"[1,"x"]"#, &schema).is_ok());
    assert!(crate::from_str("[1,2]", &schema).is_err());
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn any_of_keeps_only_provable_array_containment_and_rejects_differing_predicates() {
    let unconstrained_superset = import_str(
        r#"{
  "anyOf":[
    {"type":"array","items":{"type":"integer"},"contains":{"type":"integer","minimum":1}},
    {"type":"array","items":{"type":"integer"}}
  ]
}"#,
    );
    assert!(unconstrained_superset.json_contains.is_none());
    assert!(crate::from_str("[-1]", &unconstrained_superset).is_ok());

    assert!(matches!(
        import_str_result(
            r#"{
  "anyOf":[
    {"type":"array","items":{"type":"integer"},"contains":{"type":"integer","minimum":1}},
    {"type":"array","items":{"type":"integer"},"contains":{"type":"integer","maximum":0}}
  ]
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { .. })
    ));
    assert!(matches!(
        import_str_result(
            r#"{
  "oneOf":[
    {"type":"array","items":{"type":"integer"},"contains":{"type":"integer","minimum":1}},
    {"type":"array","items":{"type":"integer"},"contains":{"type":"integer","maximum":0}}
  ]
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { .. })
    ));
}

#[test]
fn non_array_and_malformed_count_boundaries_do_not_narrow_silently() {
    let scalar = import_str(r#"{"type":"string","contains":{"type":"integer"},"minContains":2}"#);
    assert!(!scalar.repeating);
    assert!(scalar.json_contains.is_none());

    assert!(import_str_result(r#"{"contains":{"type":"integer"}}"#).is_err());
    assert!(
        import_str_result(r#"{"type":"string","contains":{"allOf":[]},"minContains":0}"#).is_err()
    );
    assert!(
        import_str_result(r#"{"type":"array","items":{},"contains":{},"minContains":-1}"#).is_err()
    );
    let impossible =
        import_str(r#"{"type":"array","items":{},"contains":{},"minContains":3,"maxContains":2}"#);
    assert!(crate::from_str("[]", &impossible).is_err());
    assert!(crate::from_str("[1,2,3]", &impossible).is_err());
    assert!(
        import_str_result(
            r#"{"type":"array","items":{},"minContains":"ignored","maxContains":-1}"#
        )
        .is_ok()
    );
}

#[test]
fn normalized_object_output_and_json_lines_count_logical_items() {
    let objects = import_str(
        r#"{
  "type":"array",
  "items":{
    "type":"object",
    "properties":{"optional":{"type":"string"}},
    "additionalProperties":false
  },
  "contains":{"type":"object","maxProperties":0}
}"#,
    );
    let instance = crate::from_str("[{}]", &objects)
        .unwrap_or_else(|error| panic!("empty object should match: {error}"));
    let output = crate::to_string(&objects, &instance)
        .unwrap_or_else(|error| panic!("normalized empty object should match: {error}"));
    assert_eq!(output, "[\n  {}\n]\n");

    let lines = import_str(
        r#"{
  "type":"array",
  "items":{"type":"integer"},
  "contains":{"type":"integer","minimum":1},
  "minContains":2
}"#,
    );
    let rows = crate::from_lines("1\n2\n", &lines)
        .unwrap_or_else(|error| panic!("two logical line items should match: {error}"));
    assert!(crate::to_lines(&lines, &rows).is_ok());
    assert!(matches!(
        crate::from_lines("1\n-1\n", &lines),
        Err(JsonFormatError::ContainsCountMismatch { got: 1, .. })
    ));
}

#[test]
fn nested_object_and_array_predicates_use_full_boundary_validation() {
    let objects = import_str(
        r#"{
  "type":"array",
  "items":{
    "type":"object",
    "properties":{"kind":{"type":"string"},"value":{"type":"integer"}},
    "additionalProperties":false
  },
  "contains":{
    "type":"object",
    "properties":{"kind":{"const":"selected"}},
    "required":["kind"],
    "additionalProperties":true
  }
}"#,
    );
    assert!(crate::from_str(r#"[{"kind":"selected","value":1}]"#, &objects).is_ok());
    assert!(crate::from_str(r#"[{"kind":"other","value":1}]"#, &objects).is_err());

    let arrays = import_str(
        r#"{
  "type":"array",
  "items":{},
  "contains":{
    "type":"array",
    "items":{"type":"integer"},
    "minItems":2
  }
}"#,
    );
    assert!(arrays.metadata_is_valid());
    assert!(crate::from_str(r#"["x",[1,2]]"#, &arrays).is_ok());
    assert!(crate::from_str(r#"["x",[1]]"#, &arrays).is_err());
}

#[test]
fn nested_predicate_unique_items_is_conditional_and_recursive() {
    let arrays = import_str(
        r#"{
  "type":"array",
  "items":{},
  "contains":{
    "type":"array",
    "items":{"type":"integer"},
    "uniqueItems":true
  }
}"#,
    );
    assert!(crate::from_str("[[1,1]]", &arrays).is_err());
    assert!(crate::from_str("[[1,1],[1,2]]", &arrays).is_ok());

    let nested_objects = import_str(
        r#"{
  "type":"array",
  "items":{},
  "contains":{
    "type":"object",
    "properties":{
      "codes":{
        "type":"array",
        "items":{"type":"integer"},
        "uniqueItems":true
      }
    },
    "required":["codes"],
    "additionalProperties":false
  }
}"#,
    );
    assert!(crate::from_str(r#"[{"codes":[1,1]}]"#, &nested_objects).is_err());
    assert!(crate::from_str(r#"[{"codes":[1,1]},{"codes":[1,2]}]"#, &nested_objects).is_ok());
}

#[test]
fn predicate_patterns_share_the_document_work_budget() {
    let schema = import_str(
        r#"{
  "type":"array",
  "items":{"type":"string"},
  "contains":{"type":"string","pattern":"^(a?){8000}$"}
}"#,
    );
    let input = format!("[\"{}\"]", "a".repeat(8000));
    assert!(matches!(
        crate::from_str(&input, &schema),
        Err(JsonFormatError::PatternWorkLimit { .. })
    ));
}

#[test]
fn export_rejects_programmatically_invalid_contains_placement() {
    let mut scalar = SchemaNode::scalar("value", ir::ScalarType::String);
    let Some(range) = ir::ItemCountRange::new(1, None) else {
        panic!("test range is valid");
    };
    scalar.json_contains = ir::JsonContainsConstraints::new([ir::JsonContainsConstraint::new(
        JsonContainsPredicate::never(),
        range,
    )]);
    assert!(matches!(
        super::super::export(&scalar),
        Err(JsonFormatError::InvalidContainsMetadata { .. })
    ));
}
