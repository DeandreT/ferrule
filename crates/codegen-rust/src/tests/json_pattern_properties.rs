use super::*;

fn open_group(
    name: &str,
    children: Vec<SchemaNode>,
    dynamic: SchemaNode,
) -> Result<SchemaNode, Box<dyn std::error::Error>> {
    let selectors = ir::JsonPatternPropertyNames::new(["^[a-z][A-Za-z]*$", "meta|overlap"])?;
    SchemaNode::group(name, children)
        .with_dynamic_fields(dynamic)
        .and_then(|schema| schema.with_json_pattern_property_names(selectors))
        .ok_or_else(|| "test patternProperties object is valid".into())
}

fn program() -> Result<Program, Box<dyn std::error::Error>> {
    let source = open_group(
        "Source",
        vec![
            SchemaNode::scalar("Fixed", ScalarType::String),
            SchemaNode::scalar("Key", ScalarType::String),
            SchemaNode::scalar("Value", ScalarType::String),
        ],
        SchemaNode::scalar("*", ScalarType::String),
    )?;
    let target = open_group(
        "Target",
        Vec::new(),
        SchemaNode::scalar("*", ScalarType::String),
    )?;
    Ok(Program {
        source,
        extra_sources: Vec::new(),
        target,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Key".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Value".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::DynamicGroup {
                fixed_fields: Vec::new(),
                bindings: vec![codegen::DynamicTargetBinding {
                    key: 1,
                    value: 2,
                    target_domain: codegen::ScalarTargetDomain::Single(ScalarType::String),
                }],
                children: Vec::new(),
                merge: false,
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn generated_boundaries_enforce_pattern_property_names() -> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &program()?,
        &Options {
            package_name: "generated-pattern-properties".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains(
        r#"\"json_pattern_property_names\":{\"sources\":[\"^[a-z][A-Za-z]*$\",\"meta|overlap\"]}"#
    ));

    let output = TempDir::new("rust_json_pattern_properties_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let valid = r#"{
        "Fixed":"declared string schema wins",
        "Key":"overlap",
        "Value":"mapped",
        "metadata":"selected twice"
    }"#;
    assert_eq!(
        generated_pattern_properties::execute_json(valid)?,
        "{\n  \"overlap\": \"mapped\"\n}\n",
    );
    assert!(matches!(
        generated_pattern_properties::execute_json(
            &valid.replace(r#""metadata""#, r#""bad-key""#),
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("bad-key") && message.contains("patternProperties"),
    ));
    assert!(matches!(
        generated_pattern_properties::execute_json(
            &valid.replace(r#""overlap""#, r#""bad-key""#),
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("bad-key") && message.contains("patternProperties"),
    ));
    Ok(())
}
"##;
    fs::write(output.path().join("src/main.rs"), harness)?;
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()?;
    assert!(
        run.status.success(),
        "generated patternProperties mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
