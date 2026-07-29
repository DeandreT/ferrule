use super::{import_str, import_str_result};
use crate::JsonFormatError;

#[test]
fn validation_effective_conditionals_not_and_tuple_keywords_never_widen_silently() {
    for schema in [
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"array",
  "prefixItems":[{"type":"string"},{"type":"integer"}],
  "items":false
}"#,
        r#"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "type":"array",
  "unevaluatedItems":false
}"#,
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"object",
  "if":{"required":["a"]},
  "then":{"required":["b"]}
}"#,
        r#"{"type":"string","not":{"const":"forbidden"}}"#,
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"array",
  "items":[{"type":"string"}],
  "additionalItems":false
}"#,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { .. })
                | Err(JsonFormatError::UnsupportedSchemaObject { .. })
        ));
    }
}

#[test]
fn effective_dynamic_reference_keywords_never_widen_silently() {
    for schema in [
        r##"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "$recursiveAnchor":true,
  "type":"object",
  "properties":{"trigger":{"type":"string"},"value":{"type":"string"}},
  "dependentSchemas":{"trigger":{"$recursiveRef":"#"}}
}"##,
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$defs":{"required-value":{
    "$dynamicAnchor":"required-value",
    "type":"object",
    "required":["value"]
  }},
  "type":"object",
  "properties":{"trigger":{"type":"string"},"value":{"type":"string"}},
  "dependentSchemas":{"trigger":{"$dynamicRef":"#required-value"}}
}"##,
        r##"{
  "type":"object",
  "$recursiveRef":"#"
}"##,
        r##"{
  "type":"object",
  "$dynamicRef":"#required-value"
}"##,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                if reason.contains("dynamic reference validation is not supported")
        ));
    }
}

#[test]
fn dialect_unknown_and_validation_neutral_keywords_remain_no_ops() {
    let draft_2019_prefix = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2019-09/schema",
  "type":"array",
  "prefixItems":[{"type":"string"}]
}"#,
    );
    assert!(draft_2019_prefix.repeating);

    let draft_six_conditional = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-06/schema#",
  "type":"object",
  "if":{"required":["a"]},
  "then":{"required":["b"]}
}"#,
    );
    assert!(crate::from_str("{}", &draft_six_conditional).is_ok());

    let lone_keywords = import_str(
        r#"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "type":"object",
  "if":{"required":["a"]}
}"#,
    );
    assert!(crate::from_str("{}", &lone_keywords).is_ok());

    let ineffective_additional_items = import_str(
        r#"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "type":"array",
  "items":{"type":"string"},
  "additionalItems":false
}"#,
    );
    assert!(crate::from_str(r#"["a","b"]"#, &ineffective_additional_items).is_ok());

    for (dialect, keyword) in [
        (
            "https://json-schema.org/draft/2019-09/schema",
            "$dynamicRef",
        ),
        (
            "https://json-schema.org/draft/2020-12/schema",
            "$recursiveRef",
        ),
        ("http://json-schema.org/draft-07/schema#", "$dynamicRef"),
        ("http://json-schema.org/draft-07/schema#", "$recursiveRef"),
    ] {
        let schema = import_str(&format!(
            r##"{{
  "$schema":"{dialect}",
  "type":"object",
  "properties":{{"trigger":{{"type":"string"}}}},
  "dependentSchemas":{{"trigger":{{"{keyword}":"#ignored"}}}}
}}"##
        ));
        assert!(crate::from_str(r#"{"trigger":"present"}"#, &schema).is_ok());
    }
}
