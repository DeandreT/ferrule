use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use ir::{ScalarType, SchemaKind};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn homogeneous_tuple_and_identical_tail_retain_counts_and_export_canonically() {
    let schema = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"array",
  "items":[{"type":"string"},{"type":"string"}],
  "additionalItems":{"type":"string"},
  "minItems":2,
  "maxItems":5,
  "uniqueItems":true,
  "contains":{"type":"string","const":"required"}
}"#,
    );

    assert_string_array(&schema, false);
    assert_count_range(&schema, 2, Some(5));
    assert!(schema.json_unique_items);
    assert!(schema.json_contains.is_some());
    assert!(crate::from_str(r#"["required","other","third"]"#, &schema).is_ok());
    assert!(crate::from_str(r#"["required","other","other"]"#, &schema).is_err());

    let rendered = export(&schema);
    assert!(!rendered.contains("additionalItems"));
    assert!(!rendered.contains(r#""items":["#));
    assert_eq!(import_str(&rendered), schema);
}

#[test]
fn closed_tail_caps_tuple_length_and_intersects_explicit_counts() {
    let schema = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-06/schema#",
  "type":"array",
  "items":[{"type":"integer"},{"type":"integer"}],
  "additionalItems":false,
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
fn open_tail_is_exact_for_arbitrary_members_or_when_maximum_closes_it() {
    for additional in ["", r#","additionalItems":true"#] {
        let arbitrary = import_str(&format!(
            r#"{{
  "type":"array",
  "items":[{{}},true]
  {additional}
}}"#
        ));
        assert!(arbitrary.json_any);
        assert!(arbitrary.item_count_range.is_none());
        assert!(crate::from_str(r#"[null,{"x":1},[2]]"#, &arbitrary).is_ok());
    }

    let bounded = import_str(
        r#"{
  "type":"array",
  "items":[{"type":"string"},{"type":"string"}],
  "maxItems":2
}"#,
    );
    assert_string_array(&bounded, false);
    assert_count_range(&bounded, 0, Some(2));
}

#[test]
fn explicit_maximum_can_make_a_different_parsed_tail_unreachable() {
    let schema = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "type":"array",
  "items":[{"type":"string"},{"type":"string"}],
  "additionalItems":{"type":"integer","minimum":10},
  "maxItems":2
}"#,
    );

    assert_string_array(&schema, false);
    assert_count_range(&schema, 0, Some(2));
    assert!(crate::from_str(r#"["a","b"]"#, &schema).is_ok());
    assert!(crate::from_str("[10]", &schema).is_err());
}

#[test]
fn references_all_of_nullable_items_and_container_are_retained() {
    let schema = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "definitions":{"item":{"type":["string","null"],"minLength":2}},
  "type":["array","null"],
  "items":[
    {"$ref":"#/definitions/item"},
    {"allOf":[{"$ref":"#/definitions/item"}]}
  ],
  "additionalItems":{"$ref":"#/definitions/item"},
  "minItems":2,
  "maxItems":4
}"##,
    );

    assert_string_array(&schema, true);
    assert!(schema.nullable);
    assert_count_range(&schema, 2, Some(4));
    assert!(crate::from_str("null", &schema).is_ok());
    assert!(crate::from_str(r#"["ok",null,"yes"]"#, &schema).is_ok());
}

#[test]
fn all_of_and_any_of_wrappers_compose_normalized_tuple_arrays() {
    let all_of = import_str(
        r##"{
  "definitions":{"item":{"type":"string"}},
  "allOf":[
    {
      "type":"array",
      "items":[{"$ref":"#/definitions/item"},{"$ref":"#/definitions/item"}],
      "additionalItems":{"$ref":"#/definitions/item"},
      "minItems":1,
      "maxItems":4
    },
    {
      "type":"array",
      "items":{"$ref":"#/definitions/item"},
      "minItems":2,
      "maxItems":3
    }
  ]
}"##,
    );
    assert_string_array(&all_of, false);
    assert_count_range(&all_of, 2, Some(3));

    let any_of = import_str(
        r#"{
  "anyOf":[
    {
      "type":"array",
      "items":[{"type":"string"},{"type":"string"}],
      "additionalItems":{"type":"string"},
      "minItems":1,
      "maxItems":2
    },
    {
      "type":"array",
      "items":[{"type":"string"},{"type":"string"}],
      "additionalItems":{"type":"string"},
      "minItems":3,
      "maxItems":4
    }
  ]
}"#,
    );
    assert_string_array(&any_of, false);
    assert_count_range(&any_of, 1, Some(4));
}

#[test]
fn supported_dialects_and_undeclared_compatibility_normalize_tuple_items() {
    for dialect in [
        Some("http://json-schema.org/draft-04/schema#"),
        Some("http://json-schema.org/draft-06/schema#"),
        Some("http://json-schema.org/draft-07/schema#"),
        Some("https://json-schema.org/draft/2019-09/schema"),
        None,
    ] {
        let dialect = dialect
            .map(|dialect| format!(r#""$schema":"{dialect}","#))
            .unwrap_or_default();
        let schema = import_str(&format!(
            r#"{{
  {dialect}
  "type":"array",
  "items":[{{"type":"string"}},{{"type":"string"}}],
  "additionalItems":{{"type":"string"}},
  "minItems":1,
  "maxItems":3
}}"#
        ));
        assert_string_array(&schema, false);
        assert_count_range(&schema, 1, Some(3));
    }
}

#[test]
fn json_lines_enforce_normalized_tuple_item_counts() {
    let schema = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"array",
  "items":[{"type":"integer"},{"type":"integer"}],
  "additionalItems":false,
  "minItems":2
}"#,
    );

    assert!(crate::from_lines("1\n2\n", &schema).is_ok());
    assert!(crate::from_lines("1\n", &schema).is_err());
    assert!(crate::from_lines("1\n2\n3\n", &schema).is_err());
}

#[test]
fn malformed_or_unrepresentable_tuple_members_reject() {
    for (schema, reason) in [
        (
            r#"{"type":"array","items":[],"additionalItems":false}"#,
            "at least one",
        ),
        (
            r#"{"type":"array","items":[{"type":"string"},{"type":"integer"}],"additionalItems":false}"#,
            "one identical item schema",
        ),
        (
            r#"{"type":"array","items":[false],"additionalItems":false}"#,
            "cannot use the false schema",
        ),
        (
            r#"{"type":"array","items":[{"type":"array","items":{"type":"string"}}],"additionalItems":false}"#,
            "nested array wrapper",
        ),
        (
            r#"{"type":"array","items":[7],"additionalItems":false}"#,
            "objects or booleans",
        ),
    ] {
        assert_rejects(schema, reason);
    }
}

#[test]
fn draft_four_rejects_boolean_members_but_accepts_boolean_additional_items() {
    assert_rejects(
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "type":"array",
  "items":[true],
  "additionalItems":{"type":"string"}
}"#,
        "not valid in Draft 4",
    );

    let closed = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "type":"array",
  "items":[{"type":"string"}],
  "additionalItems":false
}"#,
    );
    assert_string_array(&closed, false);
    assert_count_range(&closed, 0, Some(1));

    let open = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "type":"array",
  "items":[{}],
  "additionalItems":true
}"#,
    );
    assert!(open.json_any);
    assert!(open.item_count_range.is_none());
}

#[test]
fn every_tail_schema_is_validated_even_when_maximum_makes_it_unreachable() {
    for (schema, reason) in [
        (
            r#"{"type":"array","items":[{"type":"string"}],"additionalItems":7,"maxItems":1}"#,
            "objects or booleans",
        ),
        (
            r#"{"type":"array","items":[{"type":"string"}],"additionalItems":{"type":"array","items":{"type":"string"}},"maxItems":1}"#,
            "nested array wrapper",
        ),
        (
            r#"{"type":"array","items":[{"type":"string"}],"additionalItems":false,"minItems":2}"#,
            "empty intersection",
        ),
    ] {
        assert_rejects(schema, reason);
    }
}

#[test]
fn reachable_open_or_different_tails_cannot_widen_the_tuple() {
    for schema in [
        r#"{"type":"array","items":[{"type":"string"}]}"#,
        r#"{"type":"array","items":[{"type":"string"}],"additionalItems":true}"#,
        r#"{"type":"array","items":[{"type":"string"}],"additionalItems":{"type":"integer"}}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
        ));
    }
}

#[test]
fn unsupported_dialect_ambiguity_composition_and_active_keywords_reject() {
    for (schema, reason) in [
        (
            r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"array",
  "items":[{"type":"string"}]
}"#,
            "not supported",
        ),
        (
            r#"{"items":[{"type":"string"}],"maxItems":1}"#,
            "direct concrete array",
        ),
        (
            r#"{"type":"array","prefixItems":[{"type":"string"}],"items":[{"type":"string"}],"maxItems":1}"#,
            "ambiguous",
        ),
        (
            r#"{"type":"array","items":[{"type":"string"}],"maxItems":1,"allOf":[{"maxItems":1}]}"#,
            "direct concrete array",
        ),
        (
            r#"{"type":"array","items":[{"type":"string"}],"maxItems":1,"unevaluatedItems":false}"#,
            "unevaluatedItems",
        ),
        (
            r##"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "$ref":"#/$defs/base",
  "items":[{"type":"string"}],
  "$defs":{"base":{"type":"array","items":{"type":"string"}}}
}"##,
            "modern `$ref` sibling `items`",
        ),
    ] {
        assert_rejects(schema, reason);
    }
}

#[test]
fn tuple_list_has_a_fixed_work_limit() {
    let members = std::iter::repeat_n(r#"{"type":"string"}"#, 4_097)
        .collect::<Vec<_>>()
        .join(",");
    let schema = format!(r#"{{"type":"array","items":[{members}],"additionalItems":false}}"#);
    assert_rejects(&schema, "4096-entry limit");
}

#[test]
fn external_resources_use_their_own_tuple_dialect() -> Result<(), Box<dyn Error>> {
    let directory = TempDir::new()?;
    std::fs::write(
        directory.path().join("root.schema.json"),
        r#"{"$ref":"legacy.schema.json"}"#,
    )?;
    std::fs::write(
        directory.path().join("legacy.schema.json"),
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"array",
  "items":[{"type":"string"},{"type":"string"}],
  "additionalItems":{"type":"string"},
  "minItems":2,
  "maxItems":4
}"#,
    )?;
    let legacy = super::super::import_with_root(
        &directory.path().join("root.schema.json"),
        directory.path(),
    )?;
    assert_string_array(&legacy, false);
    assert_count_range(&legacy, 2, Some(4));

    std::fs::write(
        directory.path().join("legacy.schema.json"),
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "type":"array",
  "items":[{"type":"integer"}],
  "additionalItems":{"type":"integer"}
}"#,
    )?;
    let draft_four = super::super::import_with_root(
        &directory.path().join("root.schema.json"),
        directory.path(),
    )?;
    assert!(matches!(
        draft_four.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    ));

    std::fs::write(
        directory.path().join("legacy.schema.json"),
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"array",
  "items":[{"type":"string"}]
}"#,
    )?;
    assert!(matches!(
        super::super::import_with_root(
            &directory.path().join("root.schema.json"),
            directory.path(),
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { .. })
    ));
    Ok(())
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
    let result = import_str_result(schema);
    assert!(
        matches!(
            &result,
            Err(JsonFormatError::UnsupportedSchemaUnion {
                reason: actual,
                ..
            }) if actual.contains(reason)
        ),
        "expected rejection containing `{reason}`, got {result:?}"
    );
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, std::io::Error> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "ferrule-json-schema-tuple-items-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
