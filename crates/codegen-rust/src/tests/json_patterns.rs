use super::*;
use codegen::{Expression, ExpressionNode};
use ir::JsonPatternConstraints;

fn patterned_string(name: &str, alternatives: &[&[&str]]) -> SchemaNode {
    let patterns = JsonPatternConstraints::new(
        alternatives
            .iter()
            .map(|terms| terms.iter().copied().map(str::to_string)),
    );
    let Ok(patterns) = patterns else {
        panic!("test JSON patterns are valid: {patterns:?}");
    };
    let schema = SchemaNode::scalar(name, ScalarType::String).with_json_patterns(patterns);
    let Some(schema) = schema else {
        panic!("test JSON patterns match a string schema");
    };
    schema
}

fn pattern_program() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![patterned_string("Code", &[&["^[A-Z]+$"], &["^😀$"]])],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group("Target", vec![patterned_string("Code", &[&["^OK$"]])]),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Code".into()],
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Code".into(),
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_json_entry_point_enforces_input_and_normalized_output_patterns() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"));
    let Some(runtime) = runtime else {
        panic!("codegen runtime has a workspace parent");
    };
    let artifacts = emit(
        &pattern_program(),
        &Options {
            package_name: "generated-patterns".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    );
    let Ok(artifacts) = artifacts else {
        panic!("pattern program emits: {artifacts:?}");
    };
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok());
    let Some(generated_source) = generated_source else {
        panic!("generated Rust source is present UTF-8");
    };
    assert!(generated_source.contains(r#"\"json_patterns\":{\"any_of\":[[\"^[A-Z]+$\"]"#));

    let output = TempDir::new("rust_json_pattern_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;

fn main() {
    assert_eq!(
        generated_patterns::execute_json(r#"{"Code":"OK"}"#).as_deref(),
        Ok("{\n  \"Code\": \"OK\"\n}\n"),
    );
    assert!(matches!(
        generated_patterns::execute_json(r#"{"Code":"lower"}"#),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("pattern constraints"),
    ));
    assert!(matches!(
        generated_patterns::execute_json(r#"{"Code":"OTHER"}"#),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("pattern constraints"),
    ));
}
"##;
    if let Err(error) = fs::write(output.path().join("src/main.rs"), harness) {
        panic!("generated pattern harness is written: {error}");
    }
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output();
    let Ok(run) = run else {
        panic!("generated pattern cargo run starts");
    };
    assert!(
        run.status.success(),
        "generated pattern mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}
