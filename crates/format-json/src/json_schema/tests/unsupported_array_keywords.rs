use ir::{ScalarType, SchemaKind};

use super::import_str_result;
use crate::JsonFormatError;

#[test]
fn active_contains_validation_rejects_instead_of_widening() {
    for schema in [
        r#"{
  "title":"Rows",
  "type":"array",
  "items":{"type":"integer"},
  "contains":{"minimum":1}
}"#,
        r#"{
  "$schema":"http://json-schema.org/draft-06/schema#",
  "title":"Rows",
  "type":"array",
  "items":{"type":"integer"},
  "contains":{"type":"integer"}
}"#,
        r#"{
  "title":"Rows",
  "allOf":[
    {"type":"array","items":{"type":"integer"}},
    {"contains":{"type":"integer"}}
  ]
}"#,
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/Rows",
  "contains":{"type":"integer"},
  "$defs":{"Rows":{"type":"array","items":{"type":"integer"}}}
}"##,
    ] {
        let error = import_str_result(schema);
        assert!(
            matches!(
                error,
                Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                    if reason.contains("contains")
            ),
            "{error:?}"
        );
    }
}

#[test]
fn active_contains_count_modifiers_reject_with_the_owning_schema() {
    for modifier in [r#""minContains":2"#, r#""maxContains":3"#] {
        let schema = format!(
            r#"{{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "title":"Rows",
  "type":"array",
  "items":{{"type":"integer"}},
  "contains":{{"type":"integer"}},
  {modifier}
}}"#
        );
        let error = import_str_result(&schema);
        assert!(
            matches!(
                error,
                Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                    if reason.contains("contains")
            ),
            "{error:?}"
        );
    }
}

#[test]
fn legacy_unknown_keywords_and_ref_siblings_remain_ignored() -> Result<(), JsonFormatError> {
    let draft_four = import_str_result(
        r#"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "title":"Rows",
  "type":"array",
  "items":{"type":"integer"},
  "contains":{"not":{"type":"integer"}},
  "minContains":"not an integer",
  "maxContains":"not an integer"
}"#,
    )?;
    assert!(draft_four.repeating);
    assert_eq!(
        draft_four.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    );

    let draft_seven_ref = import_str_result(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "$ref":"#/definitions/Rows",
  "contains":{"not":{"type":"integer"}},
  "definitions":{
    "Rows":{
      "title":"Rows",
      "type":"array",
      "items":{"type":"integer"}
    }
  }
}"##,
    )?;
    assert!(draft_seven_ref.repeating);
    assert_eq!(
        draft_seven_ref.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    );
    Ok(())
}
