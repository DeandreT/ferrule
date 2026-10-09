use crate::{Json5EmitError, emit, emit_with_json5, runtime};
use codegen::{Binding, Expression, ExpressionNode, Program, TargetScope};
use ir::{ScalarType, SchemaNode};

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
    let before = emit(&candidate);
    let optional = emit_with_json5(&candidate);
    let repeated = emit_with_json5(&candidate);
    let after = emit(&candidate);
    eprintln!(
        "complete original candidate={candidate:#?} before={before:#?} optional={optional:#?} repeated={repeated:#?} after={after:#?}"
    );
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
    let mut candidate = program();
    candidate.source.repeating = true;
    let repeated = emit_with_json5(&candidate);
    eprintln!("repeated source original result: {repeated:?}");
    assert!(matches!(repeated, Err(Json5EmitError::Policy(_))));
    let mut candidate = program();
    candidate.target.nullable = true;
    let nullable_group = emit_with_json5(&candidate);
    eprintln!("nullable target original result: {nullable_group:?}");
    assert!(matches!(nullable_group, Err(Json5EmitError::Policy(_))));
    let mut candidate = program();
    candidate.source.json_any = true;
    let arbitrary = emit_with_json5(&candidate);
    eprintln!("arbitrary source original result: {arbitrary:?}");
    assert!(matches!(arbitrary, Err(Json5EmitError::Policy(_))));
}
