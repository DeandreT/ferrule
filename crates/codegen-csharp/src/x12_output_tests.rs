use crate::{X12EmitError, emit, emit_with_x12};
use codegen::{
    Binding, Expression, ExpressionNode, Program, TargetConstruction, TargetScope,
    X12BoundaryOptions, X12BoundaryPolicy,
};
use ir::{ScalarType, SchemaKind, SchemaNode};

use crate::generated_artifact_evidence::Evidence;

fn segment(name: &str, width: usize) -> SchemaNode {
    SchemaNode::group(
        name,
        (1..=width)
            .map(|index| SchemaNode::scalar(format!("{name}{index:02}"), ScalarType::String))
            .collect(),
    )
}

fn schema() -> SchemaNode {
    let mut isa = segment("ISA", 16);
    let SchemaKind::Group { children, .. } = &mut isa.kind else {
        unreachable!()
    };
    children[11].fixed = Some("00401".into());
    let mut gs = segment("GS", 8);
    let SchemaKind::Group { children, .. } = &mut gs.kind else {
        unreachable!()
    };
    children[7].fixed = Some("004010".into());
    SchemaNode::group(
        "Order",
        vec![
            isa,
            gs,
            segment("ST", 2),
            segment("W05", 3),
            segment("SE", 2),
            segment("GE", 2),
            segment("IEA", 2),
        ],
    )
}

fn program() -> Program {
    Program {
        xml_boundary: None,
        source: schema(),
        extra_sources: vec![],
        target: SchemaNode::group(
            "Result",
            vec![SchemaNode::scalar("OrderNumber", ScalarType::String)],
        ),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["W05".into(), "W0502".into()],
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
                target_field: "OrderNumber".into(),
                expression: 1,
                target_domain: ScalarType::String.into(),
                repeating: false,
            }],
            children: vec![],
        },
    }
}

fn companion(artifacts: &codegen::ArtifactSet) -> String {
    String::from_utf8(
        artifacts
            .files()
            .iter()
            .find(|file| file.path.as_str() == "GeneratedMapping.X12.cs")
            .unwrap()
            .contents
            .clone(),
    )
    .unwrap()
}

#[test]
fn x12_opt_in_is_deterministic_and_preserves_every_ordinary_runtime_source() {
    let candidate = program();
    let policy = X12BoundaryPolicy {
        source: Some(X12BoundaryOptions::default()),
        target: None,
    };
    let evidence = Evidence::new("csharp-x12-selection");
    evidence.debug("PROGRAM.original.txt", &candidate);
    evidence.debug("POLICY.original.txt", &policy);
    let before = emit(&candidate);
    let optional = emit_with_x12(&candidate, &policy);
    let repeated = emit_with_x12(&candidate, &policy);
    let after = emit(&candidate);
    evidence.artifacts("ordinary-before", &before);
    evidence.artifacts("optional", &optional);
    evidence.artifacts("repeated", &repeated);
    evidence.artifacts("ordinary-after", &after);
    let before = before.unwrap();
    let optional = optional.unwrap();
    evidence.assert_retained("optional", &optional);
    assert_eq!(before, after.unwrap());
    assert_eq!(optional, repeated.unwrap());
    assert_eq!(optional.files().len(), before.files().len() + 4);
    for original in before.files() {
        let expected = match original.path.as_str() {
            "GeneratedMapping.cs" => String::from_utf8(original.contents.clone())
                .unwrap()
                .replacen(
                    "public static class GeneratedMapping\n{",
                    "public static partial class GeneratedMapping\n{",
                    1,
                )
                .into_bytes(),
            "Ferrule.Generated.csproj" => String::from_utf8(original.contents.clone())
                .unwrap()
                .replacen(
                    "    <Compile Include=\"GeneratedMapping.cs\" />\n",
                    "    <Compile Include=\"GeneratedMapping.cs\" />\n    <Compile Include=\"GeneratedMapping.X12.cs\" />\n",
                    1,
                )
                .into_bytes(),
            _ => original.contents.clone(),
        };
        assert_eq!(
            optional
                .files()
                .iter()
                .find(|file| file.path == original.path)
                .unwrap()
                .contents,
            expected,
            "{}",
            original.path.as_str()
        );
    }
    for (path, source) in super::x12_output::X12_SOURCES {
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
    let source = companion(&optional);
    assert!(source.contains("ParseX12(string source)"));
    assert!(source.contains("ParseX12Bytes(byte[] source)"));
    assert_eq!(
        source
            .matches("public static string ExecuteX12ToJson(")
            .count(),
        2
    );
    assert_eq!(
        source
            .matches("public static byte[] ExecuteX12ToJsonBytes(")
            .count(),
        2
    );
    assert!(!source.contains("SerializeX12("));
    assert!(!source.contains("ExecuteJsonToX12"));
    assert!(!source.contains("WithSources"));
}

#[test]
fn x12_direction_selection_exposes_only_its_owned_raw_boundaries() {
    let mut candidate = program();
    candidate.target = schema();
    candidate.expressions.clear();
    candidate.root.bindings.clear();
    candidate.root.construction = TargetConstruction::CopyCurrentSource;
    let evidence = Evidence::new("csharp-x12-directions");
    evidence.debug("PROGRAM.original.txt", &candidate);
    for source_x12 in [false, true] {
        let policy = X12BoundaryPolicy {
            source: source_x12.then(X12BoundaryOptions::default),
            target: Some(X12BoundaryOptions::default()),
        };
        evidence.debug(&format!("POLICY-{source_x12}.original.txt"), &policy);
        let observed = emit_with_x12(&candidate, &policy);
        evidence.artifacts(&format!("direction-{source_x12}"), &observed);
        let source = companion(&observed.unwrap());
        assert!(source.contains("SerializeX12(global::Ferrule.Runtime.FerruleInstance target)"));
        assert!(
            source.contains("SerializeX12Bytes(global::Ferrule.Runtime.FerruleInstance target)")
        );
        assert_eq!(source.contains("ParseX12(string source)"), source_x12);
        let method = if source_x12 {
            "ExecuteX12ToX12"
        } else {
            "ExecuteJsonToX12"
        };
        assert_eq!(
            source
                .matches(&format!("public static string {method}("))
                .count(),
            2
        );
        assert_eq!(
            source
                .matches(&format!("public static byte[] {method}Bytes("))
                .count(),
            2
        );
        assert_eq!(source.matches("var target = Execute(input);").count(), 2);
        assert_eq!(
            source
                .matches("var target = Execute(input, executionContext);")
                .count(),
            2
        );
    }
}

#[test]
fn x12_policy_refusal_precedes_artifacts_and_keeps_the_original_error() {
    let mut candidate = program();
    candidate.source.repeating = true;
    let policy = X12BoundaryPolicy {
        source: Some(X12BoundaryOptions::default()),
        target: None,
    };
    let evidence = Evidence::new("csharp-x12-refusal");
    evidence.debug("PROGRAM.original.txt", &candidate);
    evidence.debug("POLICY.original.txt", &policy);
    let observed = emit_with_x12(&candidate, &policy);
    evidence.artifacts("typed-refusal", &observed);
    assert!(
        evidence
            .path()
            .join("typed-refusal/ERROR.original.json")
            .is_file()
    );
    assert!(matches!(observed, Err(X12EmitError::Policy(_))));
}
