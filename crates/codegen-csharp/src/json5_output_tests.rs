use std::error::Error;

use crate::{Json5EmitError, emit, emit_with_json5, runtime};
use codegen::{Binding, Expression, ExpressionNode, Program, TargetScope};
use ir::{ScalarType, SchemaNode};

#[path = "../../codegen/tests/fixtures/generated_artifact_evidence.rs"]
mod generated_artifact_evidence;
use generated_artifact_evidence::Evidence;

fn program() -> Program {
    Program {
        xml_boundary: None,
        source: SchemaNode::group("Input", vec![SchemaNode::scalar("n", ScalarType::Int)]),
        extra_sources: vec![],
        target: SchemaNode::group("Output", vec![SchemaNode::scalar("n", ScalarType::Int)]),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["n".into()],
            },
        }],
        user_functions: vec![],
        failure_rules: vec![],
        extra_targets: vec![],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: Default::default(),
            bindings: vec![Binding {
                target_field: "n".into(),
                expression: 1,
                target_domain: ScalarType::Int.into(),
                repeating: false,
            }],
            children: vec![],
        },
    }
}

#[test]
fn json5_opt_in_preserves_ordinary_artifacts_and_all73_sources() {
    let candidate = program();
    let evidence = Evidence::new("csharp-json5-selection");
    evidence.debug("PROGRAM.original.txt", &candidate);
    let before = emit(&candidate);
    let optional = emit_with_json5(&candidate);
    let repeated = emit_with_json5(&candidate);
    let after = emit(&candidate);
    evidence.artifacts("ordinary-before", &before);
    evidence.artifacts("optional", &optional);
    evidence.artifacts("repeated", &repeated);
    evidence.artifacts("ordinary-after", &after);
    let before = before.unwrap();
    let optional = optional.unwrap();
    let repeated = repeated.unwrap();
    let after = after.unwrap();
    assert_eq!(before, after);
    assert_eq!(optional, repeated);
    assert_eq!(runtime::SOURCES.len(), 73);
    assert_eq!(
        optional.files().len(),
        before.files().len() + runtime::JSON5_SOURCES.len() + 1
    );
    for original in before.files() {
        let expected = match original.path.as_str() {
            "GeneratedMapping.cs" => String::from_utf8(original.contents.clone()).unwrap().replacen(
                "public static class GeneratedMapping\n{", "public static partial class GeneratedMapping\n{", 1).into_bytes(),
            "Ferrule.Generated.csproj" => String::from_utf8(original.contents.clone()).unwrap().replacen(
                "    <Compile Include=\"GeneratedMapping.cs\" />\n", "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.Json5.cs\" />\n", 1).into_bytes(),
            _ => original.contents.clone(),
        };
        let actual = optional
            .files()
            .iter()
            .find(|file| file.path == original.path)
            .unwrap();
        assert_eq!(actual.contents, expected, "{}", original.path.as_str());
    }
    for (path, source) in runtime::JSON5_SOURCES {
        assert!(!before.files().iter().any(|file| file.path.as_str() == path));
        assert_eq!(
            optional
                .files()
                .iter()
                .find(|file| file.path.as_str() == path)
                .unwrap()
                .contents,
            source.as_bytes()
        );
    }
    let companion = String::from_utf8(
        optional
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.Json5.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap();
    assert_eq!(
        companion
            .matches("public static string ExecuteJson5(")
            .count(),
        2
    );
    assert_eq!(
        companion
            .matches("public static byte[] ExecuteJson5Bytes(")
            .count(),
        2
    );
    assert_eq!(
        companion
            .matches("input => Execute(input, executionContext)")
            .count(),
        2
    );
    assert!(!companion.contains("WithSources"));
}

#[test]
fn json5_admission_refuses_expanded_schema_before_artifacts() {
    let evidence = Evidence::new("csharp-json5-admission");
    let mut candidate = program();
    candidate.source.repeating = true;
    evidence.debug("repeated-source-PROGRAM.original.txt", &candidate);
    let repeated = emit_with_json5(&candidate);
    evidence.artifacts("repeated-source", &repeated);
    assert!(matches!(repeated, Err(Json5EmitError::Policy(_))));
    let mut candidate = program();
    candidate.target.nullable = true;
    evidence.debug("nullable-target-PROGRAM.original.txt", &candidate);
    let nullable_group = emit_with_json5(&candidate);
    evidence.artifacts("nullable-target", &nullable_group);
    assert!(matches!(nullable_group, Err(Json5EmitError::Policy(_))));
    let mut candidate = program();
    candidate.source.json_any = true;
    evidence.debug("arbitrary-source-PROGRAM.original.txt", &candidate);
    let arbitrary = emit_with_json5(&candidate);
    evidence.artifacts("arbitrary-source", &arbitrary);
    assert!(matches!(arbitrary, Err(Json5EmitError::Policy(_))));
}

#[test]
fn json5_failed_comparison_retains_actual_artifacts_and_typed_originals() {
    let evidence = Evidence::new("csharp-json5-failed-comparison");
    let candidate = program();
    evidence.debug("PROGRAM.original.txt", &candidate);
    let actual = emit(&candidate);
    evidence.artifacts("actual", &actual);
    let mut invalid = candidate;
    invalid.source.repeating = true;
    evidence.debug("INVALID_PROGRAM.original.txt", &invalid);
    let refused = emit_with_json5(&invalid);
    evidence.artifacts("typed-error", &refused);
    let artifacts = actual.unwrap();
    let error = refused.unwrap_err();
    let first = artifacts
        .files()
        .first()
        .expect("actual emitted artifact required for the failing comparison");
    let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(
            first.contents.as_slice() == b"deliberately wrong generated artifact comparison",
            "deliberately failing comparison; originals at {}",
            evidence.path().display()
        );
    }));
    assert!(
        outcome.is_err(),
        "the deliberate comparison must actually fail"
    );
    evidence.assert_retained("actual", &artifacts);
    let original: serde_json::Value = serde_json::from_slice(
        &std::fs::read(evidence.path().join("typed-error/ERROR.original.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(original["debug"], format!("{error:#?}"));
    assert_eq!(original["display"], error.to_string());
    let mut sources = Vec::new();
    let mut current = error.source();
    while let Some(source) = current {
        sources.push(serde_json::json!({"debug": format!("{source:#?}"),
            "display": source.to_string()}));
        current = source.source();
    }
    assert_eq!(original["source_chain"], serde_json::Value::Array(sources));
}
