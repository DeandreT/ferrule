use super::*;
use codegen::{Expression, ExpressionNode};
use ir::PropertyCountRange;

fn counted_group(
    name: &str,
    minimum: u64,
    maximum: Option<u64>,
    children: Vec<SchemaNode>,
) -> Result<SchemaNode, &'static str> {
    let range = PropertyCountRange::new(minimum, maximum)
        .ok_or("test property-count interval is constrained and ordered")?;
    SchemaNode::group(name, children)
        .with_property_count_range(range)
        .ok_or("property-count intervals are valid on object groups")
}

fn property_count_program() -> Result<Program, &'static str> {
    let mut maybe = counted_group(
        "Maybe",
        1,
        Some(1),
        vec![
            SchemaNode::scalar("A", ScalarType::Int),
            SchemaNode::scalar("B", ScalarType::Int),
        ],
    )?;
    maybe.container_nullable = true;
    let source = counted_group(
        "Source",
        3,
        Some(5),
        vec![
            counted_group(
                "Nested",
                1,
                Some(1),
                vec![
                    SchemaNode::scalar("A", ScalarType::Int),
                    SchemaNode::scalar("B", ScalarType::Int),
                ],
            )?,
            counted_group(
                "Rows",
                1,
                Some(1),
                vec![
                    SchemaNode::scalar("A", ScalarType::Int),
                    SchemaNode::scalar("B", ScalarType::Int),
                ],
            )?
            .repeating(),
            maybe,
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Optional", ScalarType::String),
            SchemaNode::scalar("Sixth", ScalarType::String),
        ],
    )?;
    let target = counted_group(
        "Target",
        2,
        Some(2),
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Present", ScalarType::String),
        ],
    )?;
    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Named".into(),
            source: counted_group(
                "Named",
                1,
                Some(1),
                vec![
                    SchemaNode::scalar("A", ScalarType::Int),
                    SchemaNode::scalar("B", ScalarType::Int),
                ],
            )?,
            dynamic: None,
        }],
        target,
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Label".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Optional".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![
                Binding {
                    target_field: "Label".into(),
                    expression: 1,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                    repeating: false,
                },
                Binding {
                    target_field: "Present".into(),
                    expression: 2,
                    target_domain: ScalarTargetDomain::Single(ScalarType::String),
                    repeating: false,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn generated_json_boundaries_enforce_exact_object_property_counts()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &property_count_program()?,
        &Options {
            package_name: "generated-property-counts".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains(r#"\"property_count_range\":{\"minimum\":3,\"maximum\":5}"#));

    let output = TempDir::new("rust_json_property_count_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;
use generated_property_counts::NamedJsonInput;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let named = [NamedJsonInput {
        name: "Named",
        document: r#"{"A":1}"#,
    }];
    let valid = r#"{
        "Nested":{"A":1},
        "Rows":[{"A":1},{"B":2}],
        "Maybe":null,
        "Label":"kept",
        "Optional":"present"
    }"#;
    assert_eq!(
        generated_property_counts::execute_json_with_sources(valid, &named)?,
        "{\n  \"Label\": \"kept\",\n  \"Present\": \"present\"\n}\n",
    );

    assert!(matches!(
        generated_property_counts::execute_json_with_sources(
            r#"{"Nested":{"A":1},"Rows":[{"A":1}]}"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Source") && message.contains("properties"),
    ));
    assert!(matches!(
        generated_property_counts::execute_json_with_sources(
            r#"{
                "Nested":{"A":1},"Rows":[{"A":1}],"Maybe":null,
                "Label":"kept","Optional":"present","Sixth":"extra"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Source") && message.contains("properties"),
    ));
    assert!(matches!(
        generated_property_counts::execute_json_with_sources(
            r#"{
                "Nested":{"A":1,"B":2},"Rows":[{"A":1}],"Maybe":null,
                "Label":"kept","Optional":"present"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Nested") && message.contains("properties"),
    ));
    assert!(matches!(
        generated_property_counts::execute_json_with_sources(
            r#"{
                "Nested":{"A":1},"Rows":[{"A":1,"B":2}],"Maybe":null,
                "Label":"kept","Optional":"present"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Rows") && message.contains("properties"),
    ));
    assert!(matches!(
        generated_property_counts::execute_json_with_sources(
            r#"{
                "Nested":{"A":1},"Rows":[{"A":1}],"Maybe":{"A":1,"B":2},
                "Label":"kept","Optional":"present"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Maybe") && message.contains("properties"),
    ));

    let invalid_named = [NamedJsonInput {
        name: "Named",
        document: r#"{"A":1,"B":2}"#,
    }];
    assert!(matches!(
        generated_property_counts::execute_json_with_sources(valid, &invalid_named),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Named") && message.contains("properties"),
    ));

    assert!(matches!(
        generated_property_counts::execute_json_with_sources(
            r#"{
                "Nested":{"A":1},"Rows":[{"A":1}],"Maybe":null,
                "Label":"kept"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("Target") && message.contains("properties"),
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
        "generated property-count mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
