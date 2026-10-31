//! The direct `unevaluatedItems` form outside the shared tuple corpus:
//! homogeneous results, dialect routing, and the forms that stay refused.

use ir::{JsonTupleTail, ScalarType, SchemaKind, SchemaNode};

use super::{import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn evaluated_items_leave_unevaluated_items_without_effect() {
    for schema in [
        r#"{"type":"array","prefixItems":[{"type":"string"}],"items":false,"unevaluatedItems":false}"#,
        r#"{"type":"array","prefixItems":[{"type":"string"}],"items":{"type":"string"},"unevaluatedItems":false}"#,
        r#"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "type":"array",
  "items":[{"type":"string"}],
  "additionalItems":{"type":"string"},
  "unevaluatedItems":false
}"#,
        r#"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "type":"array",
  "items":{"type":"string"},
  "unevaluatedItems":{"type":"integer"}
}"#,
    ] {
        let imported = import_str(schema);
        assert_scalar_array(&imported, ScalarType::String);
    }
}

#[test]
fn unevaluated_items_is_the_tail_of_direct_positional_members() {
    let closed = import_str(
        r#"{"type":"array","items":[{"type":"integer"},{"type":"integer"}],"unevaluatedItems":false}"#,
    );
    assert_scalar_array(&closed, ScalarType::Int);
    let range = closed
        .item_count_range
        .expect("a closed tail bounds the array");
    assert_eq!((range.minimum(), range.maximum()), (0, Some(2)));
    assert!(crate::from_str("[1,2]", &closed).is_ok());
    assert!(crate::from_str("[1,2,3]", &closed).is_err());

    let identical = import_str(
        r#"{"type":"array","prefixItems":[{"type":"string"}],"unevaluatedItems":{"type":"string"}}"#,
    );
    assert_scalar_array(&identical, ScalarType::String);

    let open =
        import_str(r#"{"type":"array","prefixItems":[{"type":"string"},{"type":"integer"}]}"#);
    let tail = import_str(
        r#"{"type":"array","prefixItems":[{"type":"string"},{"type":"integer"}],"unevaluatedItems":true}"#,
    );
    assert_eq!(tail, open);
}

#[test]
fn unevaluated_items_without_members_acts_as_items() {
    let nullable = import_str(r#"{"type":["array","null"],"unevaluatedItems":{"type":"string"}}"#);
    assert_scalar_array(&nullable, ScalarType::String);
    assert!(nullable.container_nullable);

    // Draft 2019-09 ignores `prefixItems`, so no item is evaluated.
    let draft_2019 = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "type":"array",
  "prefixItems":[{"type":"integer"}],
  "unevaluatedItems":{"type":"string"}
}"#,
    );
    assert_scalar_array(&draft_2019, ScalarType::String);

    let nested = import_str(
        r#"{"type":"array","unevaluatedItems":{"type":"array","items":{"type":"string"}}}"#,
    );
    assert!(nested.repeating && nested.json_array_wrapper);
    let value = crate::from_str(r#"[["a"],["b","c"]]"#, &nested).expect("nested arrays read");
    let written = crate::to_string(&nested, &value).expect("nested arrays write");
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&written).expect("written JSON"),
        serde_json::json!([["a"], ["b", "c"]])
    );
}

#[test]
fn a_referenced_schema_and_an_all_of_branch_qualify_on_their_own() {
    let referenced = import_str(
        r##"{
  "$defs":{"codes":{"type":"array","unevaluatedItems":{"type":"string"}}},
  "type":"object",
  "properties":{"codes":{"$ref":"#/$defs/codes"}}
}"##,
    );
    let SchemaKind::Group { children, .. } = &referenced.kind else {
        panic!("expected an object");
    };
    assert_scalar_array(&children[0], ScalarType::String);

    let branch = import_str(
        r#"{"allOf":[
  {"type":"array","prefixItems":[{"type":"string"},{"type":"integer"}],"unevaluatedItems":false},
  {"type":"array","minItems":1}
]}"#,
    );
    let layout = branch.json_tuple.expect("a closed frame");
    assert_eq!(layout.tail, JsonTupleTail::Closed);
    let length = layout.length.expect("a closed frame has a length");
    assert_eq!((length.minimum(), length.maximum()), (1, Some(2)));
}

#[test]
fn draft_7_ignores_unevaluated_items() {
    let schema = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"array",
  "items":{"type":"string"},
  "unevaluatedItems":{"type":"string","not":{}}
}"#,
    );
    assert_scalar_array(&schema, ScalarType::String);
}

#[test]
fn every_other_active_form_is_refused() {
    for (schema, reason) in [
        (
            r#"{"type":"array","items":{"type":"string"},"unevaluatedItems":{"type":"string","not":{}}}"#,
            "`not` validation",
        ),
        (
            r#"{"type":"array","unevaluatedItems":false}"#,
            "false schema",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"anyOf":[{"minItems":1}],"unevaluatedItems":false}"#,
            "`unevaluatedItems` beside `anyOf`",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"not":{"minItems":3},"unevaluatedItems":false}"#,
            "`unevaluatedItems` beside `not`",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"else":{"minItems":1},"unevaluatedItems":false}"#,
            "`unevaluatedItems` beside `else`",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"dependentSchemas":{"a":{}},"unevaluatedItems":false}"#,
            "`unevaluatedItems` beside `dependentSchemas`",
        ),
        (
            r##"{
  "$defs":{"t":{"type":"array","prefixItems":[{"type":"string"}]}},
  "$ref":"#/$defs/t",
  "unevaluatedItems":false
}"##,
            "`unevaluatedItems` beside `$ref`",
        ),
        (
            r#"{"type":"object","unevaluatedItems":false}"#,
            "`unevaluatedItems` requires a direct concrete array schema",
        ),
        (
            r#"{"type":"array","unevaluatedItems":3}"#,
            "`unevaluatedItems` must be a JSON Schema object or boolean",
        ),
    ] {
        match import_str_result(schema) {
            Err(
                JsonFormatError::UnsupportedSchemaUnion { reason: actual, .. }
                | JsonFormatError::UnsupportedSchemaObject { reason: actual, .. },
            ) if actual.contains(reason) => {}
            other => panic!("{schema}: expected `{reason}`, got {other:?}"),
        }
    }
}

fn assert_scalar_array(schema: &SchemaNode, ty: ScalarType) {
    assert!(schema.repeating, "{schema:?}");
    assert!(schema.json_tuple.is_none());
    assert!(matches!(&schema.kind, SchemaKind::Scalar { ty: actual } if *actual == ty));
}
