use std::collections::BTreeMap;

use super::*;
use codegen::{Expression, ExpressionNode};
use ir::JsonPropertyDependencies;

fn dependent_group(
    name: &str,
    rules: &[(&str, &[&str])],
    children: Vec<SchemaNode>,
) -> Result<SchemaNode, &'static str> {
    let rules = rules
        .iter()
        .map(|(trigger, requirements)| {
            (
                (*trigger).to_string(),
                requirements
                    .iter()
                    .map(|requirement| (*requirement).to_string())
                    .collect(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let dependencies =
        JsonPropertyDependencies::new(rules).map_err(|_| "test property dependencies are valid")?;
    SchemaNode::group(name, children)
        .with_json_property_dependencies(dependencies)
        .ok_or("property dependencies are valid on the test object")
}

fn property_dependency_program() -> Result<Program, &'static str> {
    let mut maybe = dependent_group(
        "Maybe",
        &[("A", &["B"])],
        vec![
            SchemaNode::scalar("A", ScalarType::Int),
            SchemaNode::scalar("B", ScalarType::Int),
        ],
    )?;
    maybe.container_nullable = true;
    let source = dependent_group(
        "Source",
        &[("Trigger", &["Required"])],
        vec![
            SchemaNode::scalar("Trigger", ScalarType::String)
                .nullable()
                .ok_or("trigger is nullable")?,
            SchemaNode::scalar("Required", ScalarType::String)
                .nullable()
                .ok_or("dependent value is nullable")?,
            dependent_group(
                "Nested",
                &[("A", &["B"])],
                vec![
                    SchemaNode::scalar("A", ScalarType::Int),
                    SchemaNode::scalar("B", ScalarType::Int),
                ],
            )?,
            dependent_group(
                "Rows",
                &[("A", &["B"])],
                vec![
                    SchemaNode::scalar("A", ScalarType::Int),
                    SchemaNode::scalar("B", ScalarType::Int),
                ],
            )?
            .repeating(),
            maybe,
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Optional", ScalarType::String),
        ],
    )?;
    let target = dependent_group(
        "Target",
        &[("Label", &["Present"])],
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Present", ScalarType::String),
        ],
    )?;
    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Named".into(),
            source: dependent_group(
                "Named",
                &[("X", &["Y"])],
                vec![
                    SchemaNode::scalar("X", ScalarType::Int),
                    SchemaNode::scalar("Y", ScalarType::Int),
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
fn generated_json_boundaries_enforce_object_property_dependencies()
-> Result<(), Box<dyn std::error::Error>> {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &property_dependency_program()?,
        &Options {
            package_name: "generated-property-dependencies".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(
        generated_source.contains(r#"\"json_property_dependencies\":{\"Trigger\":[\"Required\"]}"#)
    );

    let output = TempDir::new("rust_json_property_dependency_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;
use generated_property_dependencies::NamedJsonInput;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let named = [NamedJsonInput {
        name: "Named",
        document: r#"{"X":1,"Y":2}"#,
    }];
    let valid = r#"{
        "Trigger":null,
        "Required":null,
        "Nested":{"A":1,"B":2},
        "Rows":[{"A":1,"B":2}],
        "Maybe":null,
        "Label":"kept",
        "Optional":"present"
    }"#;
    assert_eq!(
        generated_property_dependencies::execute_json_with_sources(valid, &named)?,
        "{\n  \"Label\": \"kept\",\n  \"Present\": \"present\"\n}\n",
    );

    assert!(matches!(
        generated_property_dependencies::execute_json_with_sources(
            r#"{
                "Trigger":null,"Nested":{"A":1,"B":2},"Rows":[{"A":1,"B":2}],
                "Maybe":null,"Label":"kept","Optional":"present"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Trigger") && message.contains("Required"),
    ));
    assert!(matches!(
        generated_property_dependencies::execute_json_with_sources(
            r#"{
                "Nested":{"A":1},"Rows":[{"A":1,"B":2}],"Maybe":null,
                "Label":"kept","Optional":"present"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Nested") && message.contains("B"),
    ));
    assert!(matches!(
        generated_property_dependencies::execute_json_with_sources(
            r#"{
                "Nested":{"A":1,"B":2},"Rows":[{"A":1}],"Maybe":null,
                "Label":"kept","Optional":"present"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Rows") && message.contains("B"),
    ));
    assert!(matches!(
        generated_property_dependencies::execute_json_with_sources(
            r#"{
                "Nested":{"A":1,"B":2},"Rows":[{"A":1,"B":2}],"Maybe":{"A":1},
                "Label":"kept","Optional":"present"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Maybe") && message.contains("B"),
    ));

    let invalid_named = [NamedJsonInput {
        name: "Named",
        document: r#"{"X":1}"#,
    }];
    assert!(matches!(
        generated_property_dependencies::execute_json_with_sources(valid, &invalid_named),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("Named") && message.contains("Y"),
    ));

    assert!(matches!(
        generated_property_dependencies::execute_json_with_sources(
            r#"{
                "Nested":{"A":1,"B":2},"Rows":[{"A":1,"B":2}],"Maybe":null,
                "Label":"kept"
            }"#,
            &named,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("Label") && message.contains("Present"),
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
        "generated property-dependency mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
