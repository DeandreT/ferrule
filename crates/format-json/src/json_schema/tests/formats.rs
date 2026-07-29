use ir::{ScalarType, SchemaKind};

use super::{export, import_str, import_str_result};
use crate::JsonFormatError;

fn annotations(schema: &ir::SchemaNode) -> Vec<&str> {
    schema
        .json_formats
        .as_slice()
        .iter()
        .map(String::as_str)
        .collect()
}

#[test]
fn direct_unknown_and_empty_annotations_are_retained_without_assertion() {
    let schema = import_str(
        r#"{
  "title":"Formats",
  "type":"object",
  "properties":{
    "known":{"type":"string","format":"date-time"},
    "unknown":{"type":"string","format":"warehouse-code"},
    "empty":{"type":"string","format":""},
    "number":{"type":"integer","format":"counter"}
  }
}"#,
    );
    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("format fixture must import as a group");
    };
    assert_eq!(annotations(&children[0]), ["date-time"]);
    assert_eq!(annotations(&children[1]), ["warehouse-code"]);
    assert_eq!(annotations(&children[2]), [""]);
    assert!(annotations(&children[3]).is_empty());
}

#[test]
fn fixed_values_and_arbitrary_json_follow_format_domain_policy() {
    let fixed_string = import_str(r#"{"type":"string","const":"not-an-email","format":"email"}"#);
    assert_eq!(fixed_string.fixed.as_deref(), Some("not-an-email"));
    assert_eq!(annotations(&fixed_string), ["email"]);

    let fixed_integer = import_str(r#"{"type":"integer","const":7,"format":"counter"}"#);
    assert_eq!(fixed_integer.fixed.as_deref(), Some("7"));
    assert!(annotations(&fixed_integer).is_empty());

    let arbitrary_ref = import_str(
        r##"{
  "$ref":"#/$defs/Anything",
  "format":"email",
  "$defs":{"Anything":{}}
}"##,
    );
    assert!(arbitrary_ref.json_any);
    assert!(annotations(&arbitrary_ref).is_empty());

    let arbitrary_array = import_str(
        r#"{
  "anyOf":[
    {"type":"array","items":{}},
    {"type":"array","items":{"type":"string","format":"email"}}
  ]
}"#,
    );
    assert!(arbitrary_array.repeating);
    assert!(arbitrary_array.json_any);
    assert!(annotations(&arbitrary_array).is_empty());

    assert!(matches!(
        import_str_result(
            r##"{
  "$ref":"#/$defs/Anything",
  "format":7,
  "$defs":{"Anything":{}}
}"##
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("`format` must be a string")
    ));
}

#[test]
fn malformed_annotations_reject_even_on_non_string_and_array_schemas() {
    for schema in [
        r#"{"type":"string","format":7}"#,
        r#"{"type":"integer","format":false}"#,
        r#"{"type":"object","format":[]}"#,
        r#"{"type":"array","format":{},"items":{"type":"string"}}"#,
        r#"{"type":"array","format":null}"#,
    ] {
        let result = import_str_result(schema);
        assert!(
            matches!(
                result,
                Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                    if reason.contains("`format` must be a string")
            ),
            "{schema}: {result:?}"
        );
    }
}

#[test]
fn annotations_apply_to_string_union_values_and_array_items() {
    let union =
        import_str(r#"{"title":"Value","type":["string","integer"],"format":"identifier"}"#);
    assert!(matches!(union.kind, SchemaKind::ScalarUnion { .. }));
    assert_eq!(annotations(&union), ["identifier"]);

    let array = import_str(
        r#"{
  "title":"Values",
  "type":"array",
  "format":"ignored-container",
  "items":{"type":"string","format":"item-code"}
}"#,
    );
    assert!(array.repeating);
    assert_eq!(annotations(&array), ["item-code"]);

    let rendered = export(&array);
    let Ok(rendered) = serde_json::from_str::<serde_json::Value>(&rendered) else {
        panic!("exported schema must be JSON");
    };
    assert!(rendered.get("format").is_none());
    assert_eq!(
        rendered
            .pointer("/items/format")
            .and_then(|value| value.as_str()),
        Some("item-code")
    );
    assert_eq!(import_str(&export(&array)), array);
}

#[test]
fn all_of_accumulates_in_traversal_order_and_deduplicates() {
    let schema = import_str(
        r#"{
  "title":"Ordered",
  "format":"base",
  "allOf":[
    {"format":"before"},
    {"type":"string","format":"branch"},
    {"format":"before"},
    {"format":""}
  ]
}"#,
    );
    assert_eq!(annotations(&schema), ["base", "before", "branch", ""]);

    let format_only =
        import_str(r#"{"title":"Only","allOf":[{"format":"first"},{"format":"second"}]}"#);
    assert_eq!(
        format_only.kind,
        SchemaKind::Scalar {
            ty: ScalarType::String
        }
    );
    assert_eq!(annotations(&format_only), ["first", "second"]);
}

#[test]
fn all_of_drops_annotations_when_string_is_removed() {
    let schema = import_str(
        r#"{
  "title":"IntegerOnly",
  "allOf":[
    {"type":["string","integer"],"format":"vacuous"},
    {"type":"integer"}
  ]
}"#,
    );
    assert_eq!(
        schema.kind,
        SchemaKind::Scalar {
            ty: ScalarType::Int
        }
    );
    assert!(annotations(&schema).is_empty());
}

#[test]
fn all_of_merges_nested_scalar_and_repeated_item_annotations() {
    let schema = import_str(
        r#"{
  "title":"Nested",
  "allOf":[
    {
      "type":"object",
      "properties":{
        "value":{"type":"string","format":"left"},
        "items":{"type":"array","items":{"type":"string","format":"item-left"}}
      }
    },
    {
      "type":"object",
      "properties":{
        "value":{"type":"string","format":"right"},
        "items":{"type":"array","items":{"type":"string","format":"item-right"}}
      }
    }
  ]
}"#,
    );
    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("nested allOf must import as a group");
    };
    assert_eq!(annotations(&children[0]), ["left", "right"]);
    assert!(children[1].repeating);
    assert_eq!(annotations(&children[1]), ["item-left", "item-right"]);
}

#[test]
fn refs_apply_or_ignore_sibling_annotations_by_dialect() {
    let modern = import_str(
        r##"{
  "$schema":"https://json-schema.org/draft/2020-12/schema",
  "$ref":"#/$defs/Value",
  "format":"sibling",
  "$defs":{"Value":{"type":"string","format":"target"}}
}"##,
    );
    assert_eq!(annotations(&modern), ["target", "sibling"]);

    let legacy = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "$ref":"#/definitions/Value",
  "format":"ignored",
  "definitions":{"Value":{"type":"string","format":"target"}}
}"##,
    );
    assert_eq!(annotations(&legacy), ["target"]);
}

#[test]
fn unresolved_or_cyclic_modern_ref_annotations_reject_but_legacy_ignores_them() {
    for schema in [
        r##"{"$ref":"#/$defs/Missing","format":"unknown"}"##,
        r##"{
  "$ref":"#/$defs/Loop",
  "$defs":{"Loop":{"$ref":"#/$defs/Loop","format":"cyclic"}}
}"##,
    ] {
        assert!(matches!(
            import_str_result(schema),
            Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
                if reason.contains("unresolved or cyclic")
        ));
    }

    let legacy = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-04/schema#",
  "$ref":"#/definitions/Loop",
  "format":42,
  "definitions":{"Loop":{"$ref":"#/definitions/Loop","format":false}}
}"##,
    );
    assert!(annotations(&legacy).is_empty());
}

#[test]
fn multiple_annotations_export_as_annotation_only_all_of_and_roundtrip() {
    let schema = import_str(
        r#"{
  "title":"Many",
  "allOf":[
    {"type":"string","format":"uuid"},
    {"format":"custom"},
    {"format":""}
  ]
}"#,
    );
    let rendered = export(&schema);
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&rendered) else {
        panic!("exported schema must be JSON");
    };
    assert!(value.get("format").is_none());
    assert_eq!(
        value
            .get("allOf")
            .and_then(serde_json::Value::as_array)
            .map(Vec::len),
        Some(3)
    );
    assert_eq!(import_str(&rendered), schema);
}

#[test]
fn scalar_composition_accumulates_top_level_branch_and_ref_formats() {
    for keyword in ["anyOf", "oneOf"] {
        for alternatives in [
            r##"[
    {"type":"string","format":"branch"},
    {"type":"integer"}
  ]"##,
            r##"[
    {"type":"integer"},
    {"$ref":"#/$defs/Text","format":"sibling"}
  ]"##,
        ] {
            let schema = format!(
                r##"{{
  "title":"BranchLocal",
  "format":"top",
  "{keyword}":{alternatives},
  "$defs":{{"Text":{{"type":"string","format":"target"}}}}
}}"##
            );
            let schema = import_str(&schema);
            let expected = if alternatives.contains("$ref") {
                vec!["top", "target", "sibling"]
            } else {
                vec!["top", "branch"]
            };
            assert_eq!(annotations(&schema), expected);
        }
    }

    assert!(matches!(
        import_str_result(
            r#"{
  "title":"Overlap",
  "oneOf":[
    {"type":"string","format":"left"},
    {"type":"string","format":"right"}
  ]
}"#
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("overlap")
    ));

    assert!(matches!(
        import_str_result(
            r##"{
  "title":"MalformedRefSibling",
  "anyOf":[
    {"$ref":"#/$defs/Count","format":7},
    {"type":"boolean"}
  ],
  "$defs":{"Count":{"type":"integer"}}
}"##
        ),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("`format` must be a string")
    ));
}

#[test]
fn scalar_composition_ref_siblings_follow_legacy_policy() {
    let schema = import_str(
        r##"{
  "$schema":"http://json-schema.org/draft-07/schema#",
  "title":"Legacy",
  "anyOf":[
    {"$ref":"#/definitions/Text","format":"ignored"},
    {"type":"integer"}
  ],
  "definitions":{"Text":{"type":"string","format":"target"}}
}"##,
    );
    assert_eq!(annotations(&schema), ["target"]);
}

#[test]
fn nullable_scalar_formats_keep_wrapper_before_content_and_ref_annotations() {
    let schema = import_str(
        r##"{
  "title":"Nullable",
  "format":"wrapper",
  "oneOf":[
    {
      "$ref":"#/$defs/Text",
      "format":"sibling"
    },
    {"type":"null"}
  ],
  "$defs":{"Text":{"type":"string","format":"target"}}
}"##,
    );
    assert!(schema.nullable);
    assert_eq!(annotations(&schema), ["wrapper", "target", "sibling"]);
    assert_eq!(import_str(&export(&schema)), schema);
}

#[test]
fn array_any_of_accumulates_item_annotations_in_both_branch_orders() {
    for branches in [
        r#"[
  {"type":"array","items":{"type":"string","format":"annotated"}},
  {"type":"array","items":{"type":"string"}}
]"#,
        r#"[
  {"type":"array","items":{"type":"string"}},
  {"type":"array","items":{"type":"string","format":"annotated"}}
]"#,
        r#"[
  {"type":"array","items":{"type":["string","integer"],"format":"broad"}},
  {"type":"array","items":{"type":"string","format":"narrow"}}
]"#,
        r#"[
  {"type":"array","items":{"type":"string","format":"narrow"}},
  {"type":"array","items":{"type":["string","integer"],"format":"broad"}}
]"#,
    ] {
        let schema = import_str(&format!(r#"{{"title":"Arrays","anyOf":{branches}}}"#));
        assert!(schema.repeating);
        assert!(!annotations(&schema).is_empty());
        if branches.contains("broad") {
            let expected = if branches.find("broad") < branches.find("narrow") {
                vec!["broad", "narrow"]
            } else {
                vec!["narrow", "broad"]
            };
            assert_eq!(annotations(&schema), expected);
        } else {
            assert_eq!(annotations(&schema), ["annotated"]);
        }
    }
}

#[test]
fn annotation_bounds_reject_without_truncation() {
    let too_long = "x".repeat(ir::MAX_JSON_FORMAT_ANNOTATION_BYTES + 1);
    let schema = format!(
        r#"{{"type":"string","format":{}}}"#,
        serde_json::to_string(&too_long).unwrap_or_default()
    );
    assert!(matches!(
        import_str_result(&schema),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("byte limit")
    ));

    let branches = (0..=ir::MAX_JSON_FORMAT_ANNOTATIONS)
        .map(|index| format!(r#"{{"format":"format-{index}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    assert!(matches!(
        import_str_result(&format!(
            r#"{{"title":"TooMany","allOf":[{{"type":"string"}},{branches}]}}"#
        )),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("annotation limit")
    ));

    let excessive_total = (0..17)
        .map(|index| {
            let prefix = index.to_string();
            let annotation = format!(
                "{prefix}{}",
                "x".repeat(ir::MAX_JSON_FORMAT_ANNOTATION_BYTES - prefix.len())
            );
            format!(
                r#"{{"format":{}}}"#,
                serde_json::to_string(&annotation).unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    assert!(matches!(
        import_str_result(&format!(
            r#"{{"title":"TooLarge","allOf":[{{"type":"string"}},{excessive_total}]}}"#
        )),
        Err(JsonFormatError::UnsupportedSchemaUnion { ref reason, .. })
            if reason.contains("byte total limit")
    ));
}
