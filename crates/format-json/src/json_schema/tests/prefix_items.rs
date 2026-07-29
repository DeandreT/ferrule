use ir::{ScalarType, SchemaKind};

use super::{import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn homogeneous_prefix_and_identical_tail_retain_explicit_counts() {
    let schema = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"array",
  "prefixItems":[{"type":"string"},{"type":"string"}],
  "items":{"type":"string"},
  "minItems":1,
  "maxItems":4
}"#,
    );

    assert_string_array(&schema, false);
    assert_count_range(&schema, 1, Some(4));
    assert!(crate::from_str(r#"["a","b","c","d"]"#, &schema).is_ok());
}

#[test]
fn closed_tail_caps_the_prefix_and_intersects_explicit_counts() {
    let schema = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"array",
  "prefixItems":[{"type":"integer"},{"type":"integer"}],
  "items":false,
  "minItems":1,
  "maxItems":9
}"#,
    );

    assert!(matches!(
        schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert_count_range(&schema, 1, Some(2));
    assert!(crate::from_str("[1,2]", &schema).is_ok());
    assert!(crate::from_str("[1,2,3]", &schema).is_err());
}

#[test]
fn explicit_maximum_can_make_a_different_parsed_tail_unreachable() {
    let schema = import_str(
        r#"{
  "type":"array",
  "prefixItems":[{"type":"string"},{"type":"string"}],
  "items":{"type":"integer","minimum":10},
  "maxItems":2
}"#,
    );

    assert_string_array(&schema, false);
    assert_count_range(&schema, 0, Some(2));
    assert!(crate::from_str(r#"["a","b"]"#, &schema).is_ok());
    assert!(crate::from_str("[10]", &schema).is_err());
}

#[test]
fn empty_prefix_normalizes_each_exact_tail_form() {
    let typed = import_str(
        r#"{
  "type":"array",
  "prefixItems":[],
  "items":{"type":"integer"},
  "maxItems":3
}"#,
    );
    assert!(matches!(
        typed.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));
    assert_count_range(&typed, 0, Some(3));

    let closed = import_str(r#"{"type":"array","prefixItems":[],"items":false}"#);
    assert!(closed.json_any);
    assert_count_range(&closed, 0, Some(0));
    assert!(crate::from_str("[]", &closed).is_ok());
    assert!(crate::from_str("[null]", &closed).is_err());

    for text in [
        r#"{"type":"array","prefixItems":[]}"#,
        r#"{"type":"array","prefixItems":[],"items":true}"#,
    ] {
        let open = import_str(text);
        assert!(open.json_any);
        assert!(open.item_count_range.is_none());
    }
}

#[test]
fn unconstrained_prefix_and_open_tail_are_exactly_homogeneous() {
    let schema = import_str(
        r#"{
  "type":"array",
  "prefixItems":[{},true],
  "items":true
}"#,
    );

    assert!(schema.repeating);
    assert!(schema.json_any);
    assert!(schema.item_count_range.is_none());
    assert!(crate::from_str(r#"[null,{"x":1},[2]]"#, &schema).is_ok());
}

#[test]
fn references_all_of_and_nullable_item_composition_compare_after_parsing() {
    let schema = import_str(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$defs":{"item":{"type":["string","null"],"minLength":2}},
  "type":["array","null"],
  "prefixItems":[
    {"$ref":"#/$defs/item"},
    {"allOf":[{"$ref":"#/$defs/item"}]}
  ],
  "items":{"$ref":"#/$defs/item"},
  "minItems":2,
  "maxItems":4
}"##,
    );

    assert_string_array(&schema, true);
    assert!(schema.nullable);
    assert_count_range(&schema, 2, Some(4));
    assert!(crate::from_str("null", &schema).is_ok());
    assert!(crate::from_str(r#"["ok",null,"yes"]"#, &schema).is_ok());
    assert!(crate::from_str(r#"["x",null]"#, &schema).is_err());
}

#[test]
fn referenced_and_all_of_wrapped_arrays_retain_normalized_prefix_semantics() {
    let referenced = import_str(
        r##"{
  "$ref":"#/$defs/rows",
  "$defs":{
    "item":{"type":"string"},
    "rows":{
      "type":"array",
      "prefixItems":[{"$ref":"#/$defs/item"},{"$ref":"#/$defs/item"}],
      "items":{"$ref":"#/$defs/item"},
      "minItems":2,
      "maxItems":4
    }
  }
}"##,
    );
    assert_string_array(&referenced, false);
    assert_count_range(&referenced, 2, Some(4));

    let intersected = import_str(
        r##"{
  "$defs":{"item":{"type":"string"}},
  "allOf":[
    {
      "type":"array",
      "prefixItems":[{"$ref":"#/$defs/item"},{"$ref":"#/$defs/item"}],
      "items":{"$ref":"#/$defs/item"},
      "minItems":1,
      "maxItems":4
    },
    {
      "type":"array",
      "items":{"$ref":"#/$defs/item"},
      "minItems":2,
      "maxItems":3
    }
  ]
}"##,
    );
    assert_string_array(&intersected, false);
    assert_count_range(&intersected, 2, Some(3));
}

#[test]
fn legacy_dialects_ignore_prefix_items_completely() {
    for dialect in [
        "http://json-schema.org/draft-04/schema#",
        "http://json-schema.org/draft-06/schema#",
        "http://json-schema.org/draft-07/schema#",
        "https://json-schema.org/draft/2019-09/schema",
    ] {
        let schema = import_str(&format!(
            r#"{{
  "$schema":"{dialect}",
  "type":"array",
  "prefixItems":"not active in this dialect",
  "items":{{"type":"integer"}}
}}"#
        ));
        assert!(matches!(
            schema.kind,
            SchemaKind::Scalar {
                ty: ScalarType::Int
            }
        ));
        assert!(crate::from_str("[1,2,3]", &schema).is_ok());
    }
}

#[test]
fn malformed_or_unrepresentable_prefix_members_reject() {
    for (schema, reason) in [
        (
            r#"{"type":"array","prefixItems":[{"type":"string"},{"type":"integer"}],"items":false}"#,
            "one identical item schema",
        ),
        (
            r#"{"type":"array","prefixItems":[false],"items":false}"#,
            "cannot use the false schema",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"array","items":{"type":"string"}}],"items":false}"#,
            "nested array wrapper",
        ),
        (
            r#"{"type":"array","prefixItems":[7],"items":false}"#,
            "objects or booleans",
        ),
        (
            r#"{"type":"array","prefixItems":{},"items":false}"#,
            "must be an array",
        ),
    ] {
        assert_rejects(schema, reason);
    }
}

#[test]
fn every_tail_schema_is_validated_even_when_maximum_makes_it_unreachable() {
    for (schema, reason) in [
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"items":7,"maxItems":1}"#,
            "objects or booleans",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"items":{"type":"array","items":{"type":"string"}},"maxItems":1}"#,
            "nested array wrapper",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"items":false,"minItems":2}"#,
            "empty intersection",
        ),
    ] {
        assert_rejects(schema, reason);
    }
}

#[test]
fn reachable_open_or_different_tails_cannot_widen_the_prefix() {
    for schema in [
        r#"{"type":"array","prefixItems":[{"type":"string"}]}"#,
        r#"{"type":"array","prefixItems":[{"type":"string"}],"items":true}"#,
        r#"{"type":"array","prefixItems":[{"type":"string"}],"items":{"type":"integer"}}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
        ));
    }
}

#[test]
fn active_composition_unevaluated_items_and_ref_siblings_reject() {
    for (schema, reason) in [
        (
            r#"{"prefixItems":[{"type":"string"}],"maxItems":1}"#,
            "direct concrete array",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"items":false,"allOf":[{"maxItems":1}]}"#,
            "direct concrete array",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"items":false,"anyOf":[{"maxItems":1}]}"#,
            "direct concrete array",
        ),
        (
            r#"{"type":"array","items":{"type":"string"},"allOf":[{"prefixItems":[{"type":"string"}]}]}"#,
            "direct concrete array",
        ),
        (
            r#"{"anyOf":[{"prefixItems":[{"type":"string"}]},{"type":"string"}]}"#,
            "direct concrete array",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"items":false,"unevaluatedItems":false}"#,
            "unevaluatedItems",
        ),
        (
            r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/base",
  "prefixItems":[{"type":"string"}],
  "$defs":{"base":{"type":"array","items":{"type":"string"}}}
}"##,
            "modern `$ref` sibling `prefixItems`",
        ),
    ] {
        assert_rejects(schema, reason);
    }
}

#[test]
fn prefix_list_has_a_fixed_work_limit() {
    let members = std::iter::repeat_n(r#"{"type":"string"}"#, 4_097)
        .collect::<Vec<_>>()
        .join(",");
    let schema = format!(r#"{{"type":"array","prefixItems":[{members}],"items":false}}"#);
    assert_rejects(&schema, "4096-entry limit");
}

fn assert_string_array(schema: &ir::SchemaNode, container_nullable: bool) {
    assert!(schema.repeating);
    assert_eq!(schema.container_nullable, container_nullable);
    assert!(matches!(
        schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    ));
}

fn assert_count_range(schema: &ir::SchemaNode, minimum: u64, maximum: Option<u64>) {
    let Some(range) = schema.item_count_range else {
        panic!("expected an item-count range");
    };
    assert_eq!(range.minimum(), minimum);
    assert_eq!(range.maximum(), maximum);
}

fn assert_rejects(schema: &str, reason: &str) {
    assert!(matches!(
        import_str_result(schema),
        Err(JsonFormatError::UnsupportedSchemaUnion {
            reason: actual,
            ..
        }) if actual.contains(reason)
    ));
}
