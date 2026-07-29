use ir::{
    Instance, JsonFormatAnnotations, JsonPatternConstraints, ScalarType, ScalarTypeSet, SchemaKind,
    SchemaNode, StringLengthRange, Value,
};

use super::{export, import_str, import_str_result};
use crate::{
    JsonFormatError, from_lines, from_str, json_schema::import_with_root, to_lines, to_string,
};

fn patterns(schema: &SchemaNode) -> &[Vec<String>] {
    schema
        .json_patterns
        .as_ref()
        .map(JsonPatternConstraints::any_of)
        .unwrap_or_default()
}

#[test]
fn concrete_string_domains_retain_patterns_at_the_value_level() {
    let scalar = import_str(r#"{"title":"Code","type":"string","pattern":"^[A-Z]+$"}"#);
    assert_eq!(patterns(&scalar), &[vec!["^[A-Z]+$".to_string()]]);

    let union = import_str(r#"{"type":["string","integer"],"pattern":"^[A-Z]+$"}"#);
    assert!(matches!(union.kind, SchemaKind::ScalarUnion { .. }));
    assert_eq!(patterns(&union), patterns(&scalar));

    let array = import_str(
        r#"{
          "type":"array",
          "pattern":"ignored-but-valid",
          "items":{"type":"string","pattern":"^item$"}
        }"#,
    );
    assert!(array.repeating);
    assert_eq!(patterns(&array), &[vec!["^item$".to_string()]]);

    let integer = import_str(r#"{"type":"integer","pattern":"valid"}"#);
    assert!(integer.json_patterns.is_none());
}

#[test]
fn empty_pattern_is_retained_only_on_concrete_string_domains() {
    let concrete = import_str(r#"{"type":"string","pattern":""}"#);
    assert_eq!(patterns(&concrete), &[vec![String::new()]]);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&export(&concrete))
            .unwrap_or_default()
            .get("pattern"),
        Some(&serde_json::Value::String(String::new()))
    );

    let ambiguous = import_str(r#"{"pattern":""}"#);
    assert!(ambiguous.json_any);
    assert!(ambiguous.json_patterns.is_none());
    let all_of = import_str(r#"{"allOf":[{"pattern":""}]}"#);
    assert!(all_of.json_any);
    assert!(all_of.json_patterns.is_none());

    let dynamic = import_str(
        r#"{
          "type":"object",
          "additionalProperties":{"pattern":""}
        }"#,
    );
    let SchemaKind::Group {
        dynamic: Some(dynamic),
        ..
    } = dynamic.kind
    else {
        panic!("object should retain unconstrained dynamic values");
    };
    assert!(dynamic.json_any);
    assert!(dynamic.json_patterns.is_none());

    let unresolved = import_str(r##"{"$ref":"#/$defs/missing","pattern":""}"##);
    assert!(unresolved.json_patterns.is_none());
}

#[test]
fn canonical_tautological_dnf_roundtrips_without_narrowing() {
    let constraints = JsonPatternConstraints::new([vec!["", "^A$"], vec![""]])
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(constraints.any_of(), &[vec![String::new()]]);
    let Some(schema) =
        SchemaNode::scalar("Value", ScalarType::String).with_json_patterns(constraints)
    else {
        panic!("canonical tautology matches a string schema");
    };
    let rendered = export(&schema);
    let imported = import_str(&rendered);
    assert_eq!(imported, schema);
}

#[test]
fn malformed_or_nonportable_patterns_reject_even_when_the_type_ignores_them() {
    for pattern in [
        r#""[""#,
        r#""\\d+""#,
        r#""(?=x)""#,
        r#""(?<name>x)""#,
        r#""\\1""#,
        r#""(?i)x""#,
    ] {
        for ty in ["string", "integer", "array", "null"] {
            let schema = if ty == "array" {
                format!(r#"{{"type":"array","items":{{"type":"string"}},"pattern":{pattern}}}"#)
            } else {
                format!(r#"{{"type":"{ty}","pattern":{pattern}}}"#)
            };
            assert!(
                import_str_result(&schema).is_err(),
                "{ty} unexpectedly accepted {pattern}"
            );
        }
    }
    assert!(import_str_result(r#"{"type":"string","pattern":7}"#).is_err());
}

#[test]
fn ambiguous_patterns_nested_arrays_and_pattern_only_all_of_reject() {
    assert!(import_str_result(r#"{"pattern":"required"}"#).is_err());
    assert!(
        import_str_result(r#"{"type":"object","additionalProperties":{"pattern":"required"}}"#)
            .is_err()
    );
    assert!(
        import_str_result(
            r#"{
              "type":"array",
              "items":{"type":"array","items":{"type":"string","pattern":"^x$"}}
            }"#
        )
        .is_err()
    );
    assert!(import_str_result(r#"{"allOf":[{"pattern":"^x$"}]}"#).is_err());
    assert!(import_str_result(r#"{"allOf":[{"format":"email"},{"pattern":"^x$"}]}"#).is_err());
}

#[test]
fn all_of_intersects_pattern_dnf_without_losing_declaration_order() {
    let schema = import_str(
        r#"{
          "allOf":[
            {"anyOf":[
              {"type":"string","pattern":"^A"},
              {"type":"string","pattern":"^B"}
            ]},
            {"anyOf":[
              {"type":"string","pattern":"Z$"},
              {"type":"string","pattern":"^A"}
            ]}
          ]
        }"#,
    );
    assert_eq!(
        patterns(&schema),
        &[
            vec!["^A".to_string(), "Z$".to_string()],
            vec!["^A".to_string()],
            vec!["^B".to_string(), "Z$".to_string()],
            vec!["^B".to_string(), "^A".to_string()],
        ]
    );
}

#[test]
fn scalar_any_of_unites_string_patterns_and_unconstrained_string_wins() {
    let schema = import_str(
        r#"{
          "anyOf":[
            {"type":"string","pattern":"^A$"},
            {"type":"integer"},
            {"type":"string","pattern":"^B$"}
          ]
        }"#,
    );
    assert_eq!(
        patterns(&schema),
        &[vec!["^A$".to_string()], vec!["^B$".to_string()]]
    );

    let unconstrained = import_str(
        r#"{
          "anyOf":[
            {"type":"string","pattern":"^A$"},
            {"type":"string"},
            {"type":"integer"}
          ]
        }"#,
    );
    assert!(unconstrained.json_patterns.is_none());

    let tautological = import_str(
        r#"{
          "anyOf":[
            {"type":"string","pattern":"^A$"},
            {"type":"string","pattern":""},
            {"type":"integer"}
          ]
        }"#,
    );
    assert!(tautological.json_patterns.is_none());

    let exclusive = import_str(
        r#"{
          "oneOf":[
            {"type":"string","pattern":"^A$"},
            {"type":"integer"}
          ]
        }"#,
    );
    assert_eq!(patterns(&exclusive), &[vec!["^A$".to_string()]]);
    assert!(
        import_str_result(
            r#"{"oneOf":[
              {"type":"string","pattern":"^A$"},
              {"type":"string","pattern":"^B$"}
            ]}"#
        )
        .is_err()
    );
}

#[test]
fn scalar_any_of_rejects_correlated_length_and_pattern_domains() {
    for schema in [
        r#"{
          "anyOf":[
            {"type":"string","minLength":1,"maxLength":1,"pattern":"^A$"},
            {"type":"string","minLength":2,"maxLength":2,"pattern":"^B+$"}
          ]
        }"#,
        r#"{
          "anyOf":[
            {"type":"string","minLength":1,"maxLength":1,"pattern":"^A$"},
            {"type":"string","minLength":2,"maxLength":2}
          ]
        }"#,
    ] {
        let error = import_str_result(schema).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("correlates different string-length and pattern constraints")
        );
    }

    let shared_length = import_str(
        r#"{
          "anyOf":[
            {"type":"string","minLength":1,"maxLength":1,"pattern":"^A$"},
            {"type":"string","minLength":1,"maxLength":1,"pattern":"^B$"}
          ]
        }"#,
    );
    assert_eq!(
        patterns(&shared_length),
        &[vec!["^A$".to_string()], vec!["^B$".to_string()]]
    );

    let shared_pattern = import_str(
        r#"{
          "anyOf":[
            {"type":"string","minLength":1,"maxLength":1,"pattern":"^[AB]+$"},
            {"type":"string","minLength":2,"maxLength":2,"pattern":"^[AB]+$"}
          ]
        }"#,
    );
    assert_eq!(patterns(&shared_pattern), &[vec!["^[AB]+$".to_string()]]);
    assert_eq!(
        shared_pattern
            .string_length_range
            .map(|range| (range.minimum(), range.maximum())),
        Some((1, Some(2)))
    );
}

#[test]
fn nullable_fixed_and_dynamic_string_patterns_retain_exact_semantics() {
    let nullable = import_str(
        r#"{
          "anyOf":[
            {"type":"string","pattern":"^A+$"},
            {"type":"null"}
          ]
        }"#,
    );
    assert!(nullable.nullable);
    assert_eq!(patterns(&nullable), &[vec!["^A+$".to_string()]]);

    assert!(import_str_result(r#"{"type":"string","const":"B","pattern":"^A$"}"#).is_err());

    let dynamic = import_str(
        r#"{
          "type":"object",
          "additionalProperties":{"type":"string","pattern":"^[a-z]+$"}
        }"#,
    );
    let SchemaKind::Group {
        dynamic: Some(value),
        ..
    } = &dynamic.kind
    else {
        panic!("object should retain typed dynamic values");
    };
    assert_eq!(patterns(value), &[vec!["^[a-z]+$".to_string()]]);
    assert_eq!(import_str(&export(&dynamic)), dynamic);
}

#[test]
fn array_any_of_requires_identical_patterns_or_an_unconstrained_superset() {
    let identical = import_str(
        r#"{
          "anyOf":[
            {"type":"array","minItems":1,"items":{"type":"string","pattern":"^A"}},
            {"type":"array","minItems":2,"items":{"type":"string","pattern":"^A"}}
          ]
        }"#,
    );
    assert_eq!(patterns(&identical), &[vec!["^A".to_string()]]);

    let unconstrained = import_str(
        r#"{
          "anyOf":[
            {"type":"array","items":{"type":"string"}},
            {"type":"array","items":{"type":"string","pattern":"^A"}}
          ]
        }"#,
    );
    assert!(unconstrained.json_patterns.is_none());

    assert!(
        import_str_result(
            r#"{
              "anyOf":[
                {"type":"array","items":{"type":"string","pattern":"^A"}},
                {"type":"array","items":{"type":"string","pattern":"^B"}}
              ]
            }"#
        )
        .is_err()
    );
}

#[test]
fn modern_ref_siblings_intersect_while_legacy_siblings_are_ignored() {
    let modern = import_str(
        r##"{
          "$schema":"https://json-schema.org/draft/2020-12/schema",
          "$ref":"#/$defs/code",
          "pattern":"Z$",
          "$defs":{"code":{"type":"string","pattern":"^A"}}
        }"##,
    );
    assert_eq!(
        patterns(&modern),
        &[vec!["^A".to_string(), "Z$".to_string()]]
    );

    let legacy = import_str(
        r##"{
          "$schema":"http://json-schema.org/draft-07/schema#",
          "$ref":"#/definitions/code",
          "pattern":7,
          "definitions":{"code":{"type":"string","pattern":"^A"}}
        }"##,
    );
    assert_eq!(patterns(&legacy), &[vec!["^A".to_string()]]);

    assert!(import_str_result(r##"{"$ref":"#/$defs/missing","pattern":"x"}"##).is_err());
    assert!(
        import_str_result(
            r##"{
              "$ref":"#/$defs/loop",
              "$defs":{"loop":{"$ref":"#/$defs/loop","pattern":"x"}}
            }"##
        )
        .is_err()
    );
}

#[test]
fn external_resources_apply_their_own_pattern_ref_policy() -> Result<(), Box<dyn std::error::Error>>
{
    let dir = std::env::temp_dir().join(format!(
        "ferrule-json-pattern-dialects-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(
        dir.join("legacy.json"),
        r##"{
          "$schema":"http://json-schema.org/draft-07/schema#",
          "$defs":{
            "base":{"type":"string","pattern":"^A"},
            "use":{"$ref":"#/$defs/base","pattern":7}
          }
        }"##,
    )?;
    std::fs::write(
        dir.join("modern.json"),
        r##"{
          "$schema":"https://json-schema.org/draft/2020-12/schema",
          "$defs":{
            "base":{"type":"string","pattern":"^A"},
            "use":{"$ref":"#/$defs/base","pattern":"Z$"}
          }
        }"##,
    )?;

    for (resource, expected) in [
        ("legacy.json#/$defs/use", vec![vec!["^A".to_string()]]),
        (
            "modern.json#/$defs/use",
            vec![vec!["^A".to_string(), "Z$".to_string()]],
        ),
    ] {
        std::fs::write(dir.join("root.json"), format!(r#"{{"$ref":"{resource}"}}"#))?;
        let imported = import_with_root(&dir.join("root.json"), &dir)?;
        assert_eq!(patterns(&imported), expected);
    }
    std::fs::remove_dir_all(dir)?;
    Ok(())
}

#[test]
fn export_roundtrip_preserves_dnf_conjunctions_lengths_formats_and_wrappers() {
    let patterns = JsonPatternConstraints::new([
        vec!["^A".to_string(), "Z$".to_string()],
        vec!["^B+$".to_string()],
    ])
    .unwrap_or_else(|error| panic!("{error}"));
    let formats =
        JsonFormatAnnotations::new(["email".to_string(), "custom".to_string()]).unwrap_or_default();
    let Some(length) = StringLengthRange::new(2, Some(8)) else {
        panic!("test length range is valid");
    };
    let Some(types) = ScalarTypeSet::new([ScalarType::String, ScalarType::Int]) else {
        panic!("test scalar union is valid");
    };
    let Some(mut schema) = SchemaNode::scalar_union("Values", types)
        .with_string_length_range(length)
        .and_then(|node| node.with_json_formats(formats))
        .and_then(|node| node.with_json_patterns(patterns))
        .map(SchemaNode::repeating)
    else {
        panic!("test pattern schema is valid");
    };
    schema.container_nullable = true;

    let rendered = export(&schema);
    let value: serde_json::Value = serde_json::from_str(&rendered).unwrap_or_default();
    assert!(value.pointer("/anyOf/0/items/anyOf").is_some());
    assert_eq!(import_str(&rendered), schema);
}

#[test]
fn fixed_string_with_pattern_disjunction_roundtrips_without_duplication() {
    let patterns = JsonPatternConstraints::new([
        vec!["^A".to_string(), "Z$".to_string()],
        vec!["^B+$".to_string()],
    ])
    .unwrap_or_else(|error| panic!("{error}"));
    let Some(schema) = SchemaNode::scalar("Code", ScalarType::String)
        .with_fixed("ABZ")
        .and_then(|node| node.with_json_patterns(patterns))
    else {
        panic!("fixed value satisfies one pattern alternative");
    };

    let rendered = export(&schema);
    let imported = import_str_result(&rendered)
        .unwrap_or_else(|error| panic!("{error}\nrendered schema:\n{rendered}"));
    assert_eq!(imported, schema);
}

#[test]
fn excessive_pattern_disjunction_rejects_without_widening() {
    let alternatives = (0..33)
        .map(|index| format!(r#"{{"type":"string","pattern":"^value-{index}$"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    assert!(import_str_result(&format!(r#"{{"anyOf":[{alternatives}]}}"#)).is_err());
}

#[test]
fn schema_wide_pattern_program_budget_rejects_during_import() {
    let properties = (0..=ir::MAX_DISTINCT_JSON_PATTERNS)
        .map(|index| format!(r#""field{index}":{{"type":"string","pattern":"^value{index}$"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let error = import_str_result(&format!(
        r#"{{"type":"object","properties":{{{properties}}}}}"#
    ))
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("schema-wide metadata, program, or fixed-value work budget")
    );
}

#[test]
fn repeated_identical_expansion_heavy_patterns_compile_once_at_the_root() {
    let properties = (0..1_024)
        .map(|index| format!(r#""field{index}":{{"type":"string","pattern":"a{{16000}}"}}"#))
        .collect::<Vec<_>>()
        .join(",");
    let schema = import_str(&format!(
        r#"{{"type":"object","properties":{{{properties}}}}}"#
    ));
    let SchemaKind::Group { children, .. } = &schema.kind else {
        panic!("expansion-heavy property schema imports as an object");
    };
    assert_eq!(children.len(), 1_024);
    assert!(schema.json_pattern_budget_is_valid());
}

#[test]
fn native_boundaries_execute_dnf_and_only_test_string_union_values() {
    let schema = import_str(
        r#"{
          "anyOf":[
            {"type":"integer"},
            {"type":"string","allOf":[
              {"pattern":"^A"},
              {"pattern":"Z$"}
            ]},
            {"type":"string","pattern":"^B+$"}
          ]
        }"#,
    );
    assert!(from_str("42", &schema).is_ok());
    assert!(from_str(r#""ABZ""#, &schema).is_ok());
    assert!(from_str(r#""BBB""#, &schema).is_ok());
    assert!(matches!(
        from_str(r#""AZQ""#, &schema),
        Err(JsonFormatError::PatternMismatch { .. })
    ));

    assert!(to_lines(&schema, &Instance::Scalar(Value::Int(42))).is_ok());
    assert!(matches!(
        to_lines(
            &schema,
            &Instance::Scalar(Value::String("unmatched".to_string()))
        ),
        Err(JsonFormatError::PatternMismatch { .. })
    ));
}

#[test]
fn native_output_matches_after_string_normalization() {
    let schema = import_str(r#"{"type":"string","pattern":"^true$"}"#);
    assert!(matches!(
        to_string(&schema, &Instance::Scalar(Value::Bool(true))).as_deref(),
        Ok("\"true\"\n")
    ));
    assert!(matches!(
        to_string(&schema, &Instance::Scalar(Value::Bool(false))),
        Err(JsonFormatError::PatternMismatch { .. })
    ));
}

#[test]
fn portable_patterns_use_unicode_scalar_and_anchor_semantics() {
    let one_scalar = import_str(r#"{"type":"string","pattern":"^.$"}"#);
    assert!(from_str(r#""😀""#, &one_scalar).is_ok());
    assert!(matches!(
        from_str(r#""e\u0301""#, &one_scalar),
        Err(JsonFormatError::PatternMismatch { .. })
    ));

    let line = import_str(r#"{"type":"string","pattern":"^first\\nsecond$"}"#);
    assert!(from_str(r#""first\nsecond""#, &line).is_ok());
    assert!(from_str(r#""first\r\nsecond""#, &line).is_err());
}

#[test]
fn json_lines_share_one_bounded_pattern_work_budget() {
    let source = format!("{}a*", "a?".repeat(1_000));
    let compiled = json_pattern::PortableJsonPattern::compile(&source)
        .unwrap_or_else(|error| panic!("{error}"));
    let per_line = usize::try_from(
        json_pattern::DEFAULT_MATCH_WORK_LIMIT
            / u64::try_from(compiled.instruction_count()).unwrap_or(u64::MAX)
            / 2
            + 1,
    )
    .unwrap_or(60_000);
    let patterns =
        JsonPatternConstraints::new([[source]]).unwrap_or_else(|error| panic!("{error}"));
    let Some(schema) = SchemaNode::scalar("Value", ScalarType::String).with_json_patterns(patterns)
    else {
        panic!("test pattern schema is valid");
    };
    let value = serde_json::to_string(&"a".repeat(per_line)).unwrap_or_default();
    let lines = format!("{value}\n{value}\n");
    assert!(matches!(
        from_lines(&lines, &schema),
        Err(JsonFormatError::PatternWorkLimit { .. })
    ));
}

#[test]
fn native_boundaries_reject_invalid_programmatic_pattern_metadata() {
    let constraints =
        JsonPatternConstraints::new([["^A$"]]).unwrap_or_else(|error| panic!("{error}"));

    let mut integer = SchemaNode::scalar("Count", ScalarType::Int);
    integer.json_patterns = Some(constraints.clone());
    assert!(matches!(
        from_str("1", &integer),
        Err(JsonFormatError::InvalidPatternMetadata { .. })
    ));

    let mut fixed = SchemaNode::scalar_fixed("Code", ScalarType::String, "B");
    fixed.json_patterns = Some(constraints);
    assert!(matches!(
        to_string(&fixed, &Instance::Scalar(Value::String("B".to_string()))),
        Err(JsonFormatError::InvalidPatternMetadata { .. })
    ));
}
