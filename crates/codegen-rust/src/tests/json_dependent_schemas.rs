use super::*;
use codegen::{Expression, ExpressionNode};
use ir::{
    JsonDependentSchemaConstraint, JsonDependentSchemaConstraints, JsonPatternConstraints,
    JsonSchemaPredicate,
};

fn arbitrary_json(name: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .json_any()
        .ok_or("arbitrary JSON schema is valid")
}

fn predicate(required: &[&str], children: Vec<SchemaNode>) -> Result<SchemaNode, &'static str> {
    SchemaNode::group("dependent object", children)
        .with_dynamic_fields(arbitrary_json("*")?)
        .ok_or("dependent predicate accepts other object properties")?
        .with_required_fields(required.iter().map(|name| (*name).to_string()).collect())
        .ok_or("dependent predicate required fields are declared")
}

fn dependent(
    name: &str,
    trigger: &str,
    predicate: SchemaNode,
    children: Vec<SchemaNode>,
) -> Result<SchemaNode, &'static str> {
    let constraints = JsonDependentSchemaConstraints::new([JsonDependentSchemaConstraint::new(
        trigger,
        JsonSchemaPredicate::schema(predicate),
    )])
    .ok_or("dependent schema constraint is effective and bounded")?;
    SchemaNode::group(name, children)
        .with_json_dependent_schemas(constraints)
        .ok_or("dependent schema metadata belongs to the object")
}

fn exact_string(name: &str, value: &str) -> Result<SchemaNode, &'static str> {
    SchemaNode::scalar(name, ScalarType::String)
        .with_fixed(value)
        .ok_or("fixed string belongs to the string domain")
}

fn conditional_object(name: &str) -> Result<SchemaNode, &'static str> {
    dependent(
        name,
        "A",
        predicate(&["B"], vec![SchemaNode::scalar("B", ScalarType::Int)])?,
        vec![
            SchemaNode::scalar("A", ScalarType::Int),
            SchemaNode::scalar("B", ScalarType::Int),
        ],
    )
}

fn output_schema(name: &str, expected: &str) -> Result<SchemaNode, &'static str> {
    dependent(
        name,
        "Label",
        predicate(&["Present"], vec![exact_string("Present", expected)?])?,
        vec![
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Present", ScalarType::String),
        ],
    )
}

fn output_scope(present_expression: u32) -> TargetScope {
    TargetScope {
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
                expression: present_expression,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
                repeating: false,
            },
        ],
        children: Vec::new(),
    }
}

fn dependent_schema_program() -> Result<Program, &'static str> {
    let mut maybe = conditional_object("Maybe")?;
    maybe.container_nullable = true;
    let source = dependent(
        "Source",
        "Trigger",
        predicate(
            &["Guard", "Embedded"],
            vec![
                exact_string("Guard", "accepted")?,
                dependent(
                    "Embedded",
                    "X",
                    predicate(&["Y"], vec![exact_string("Y", "nested")?])?,
                    vec![
                        SchemaNode::scalar("X", ScalarType::Int),
                        SchemaNode::scalar("Y", ScalarType::String),
                    ],
                )?,
            ],
        )?,
        vec![
            SchemaNode::scalar("Trigger", ScalarType::String)
                .nullable()
                .ok_or("trigger is nullable")?,
            SchemaNode::scalar("Guard", ScalarType::String),
            SchemaNode::group(
                "Embedded",
                vec![
                    SchemaNode::scalar("X", ScalarType::Int),
                    SchemaNode::scalar("Y", ScalarType::String),
                ],
            ),
            conditional_object("Nested")?,
            conditional_object("Rows")?.repeating(),
            maybe,
            SchemaNode::scalar("Label", ScalarType::String),
            SchemaNode::scalar("Optional", ScalarType::String),
            SchemaNode::scalar("AuditOptional", ScalarType::String),
        ],
    )?;
    Ok(Program {
        source,
        extra_sources: vec![NamedSourceProgram {
            name: "Named".into(),
            source: conditional_object("Named")?,
            dynamic: None,
        }],
        target: output_schema("Target", "present")?,
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
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["AuditOptional".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: output_scope(2),
        extra_targets: vec![NamedTargetProgram {
            name: "Audit".into(),
            target: output_schema("Audit", "audit")?,
            root: output_scope(3),
        }],
    })
}

fn pattern_budget_program() -> Result<Program, &'static str> {
    let patterns = JsonPatternConstraints::new([["^(a?){5000}$"]])
        .map_err(|_| "test predicate pattern remains structurally bounded")?;
    let ordinary = SchemaNode::scalar("Ordinary", ScalarType::String)
        .with_json_patterns(patterns.clone())
        .ok_or("ordinary pattern belongs to a string")?;
    let guard = SchemaNode::scalar("Guard", ScalarType::String)
        .with_json_patterns(patterns)
        .ok_or("dependent pattern belongs to a string")?;
    let source = dependent(
        "Source",
        "Trigger",
        predicate(&["Guard"], vec![guard])?,
        vec![
            ordinary,
            SchemaNode::scalar("Trigger", ScalarType::Bool),
            SchemaNode::scalar("Guard", ScalarType::String),
        ],
    )?;
    Ok(Program {
        source,
        extra_sources: Vec::new(),
        target: SchemaNode::scalar("Target", ScalarType::String),
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Ordinary".into()],
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Scalar {
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::String),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

fn runtime_path() -> Result<PathBuf, &'static str> {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")
}

#[test]
fn generated_json_boundaries_enforce_dependent_schemas() -> Result<(), Box<dyn std::error::Error>> {
    let artifacts = emit(
        &dependent_schema_program()?,
        &Options {
            package_name: "generated-dependent-schemas".into(),
            runtime_dependency: RuntimeDependency::Path(runtime_path()?.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains(r#"\"json_dependent_schemas\":[{\"trigger\":\"Trigger\""#));

    let output = TempDir::new("rust_json_dependent_schemas_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;
use generated_dependent_schemas::{NamedJsonBytesInput, NamedJsonInput};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let valid = r#"{
        "Trigger":null,"Guard":"accepted","Embedded":{"X":1,"Y":"nested"},
        "Nested":{"A":1,"B":2},
        "Rows":[{"A":1,"B":2},{}],"Maybe":null,
        "Label":"kept","Optional":"present","AuditOptional":"audit"
    }"#;
    let named = [NamedJsonInput {
        name: "Named",
        document: r#"{"A":1,"B":2}"#,
    }];
    assert_eq!(
        generated_dependent_schemas::execute_json_with_sources(valid, &named)?,
        "{\n  \"Label\": \"kept\",\n  \"Present\": \"present\"\n}\n",
    );
    let outputs =
        generated_dependent_schemas::execute_json_outputs_with_sources(valid, &named)?;
    assert_eq!(outputs.extras[0].name, "Audit");
    assert!(outputs.extras[0].document.contains("\"Present\": \"audit\""));

    for invalid in [
        valid.replace(r#","Guard":"accepted""#, ""),
        valid.replace(r#""Y":"nested""#, r#""Y":"wrong""#),
        valid.replace(r#""Nested":{"A":1,"B":2}"#, r#""Nested":{"A":1}"#),
        valid.replace(r#""Rows":[{"A":1,"B":2},{}]"#, r#""Rows":[{"A":1}]"#),
        valid.replace(r#""Maybe":null"#, r#""Maybe":{"A":1}"#),
    ] {
        assert!(matches!(
            generated_dependent_schemas::execute_json_with_sources(&invalid, &named),
            Err(JsonBoundaryError::InvalidInput { message })
                if message.contains("dependent schema"),
        ));
    }
    let invalid_named = [NamedJsonInput {
        name: "Named",
        document: r#"{"A":1}"#,
    }];
    assert!(matches!(
        generated_dependent_schemas::execute_json_with_sources(valid, &invalid_named),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("dependent schema"),
    ));
    assert!(matches!(
        generated_dependent_schemas::execute_json_with_sources(
            &valid.replace(r#","Optional":"present""#, ""),
            &named,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("Target") && message.contains("Label"),
    ));
    assert!(matches!(
        generated_dependent_schemas::execute_json_outputs_with_sources(
            &valid.replace(r#","AuditOptional":"audit""#, ""),
            &named,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("Audit") && message.contains("Label"),
    ));

    let named_bytes = [NamedJsonBytesInput {
        name: "Named",
        document: br#"{"A":1,"B":2}"#,
    }];
    let byte_outputs =
        generated_dependent_schemas::execute_json_bytes_outputs_with_sources(
            valid.as_bytes(),
            &named_bytes,
        )?;
    assert!(byte_outputs.primary.starts_with(b"{\n  \"Label\""));
    assert_eq!(byte_outputs.extras[0].name, "Audit");
    assert!(
        byte_outputs.extras[0]
            .document
            .windows(b"\"Present\": \"audit\"".len())
            .any(|window| window == b"\"Present\": \"audit\"")
    );
    assert!(matches!(
        generated_dependent_schemas::execute_json_bytes_with_sources(
            valid
                .replace(r#""Y":"nested""#, r#""Y":"wrong""#)
                .as_bytes(),
            &named_bytes,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("dependent schema"),
    ));
    assert!(matches!(
        generated_dependent_schemas::execute_json_bytes_outputs_with_sources(
            valid.replace(r#","AuditOptional":"audit""#, "").as_bytes(),
            &named_bytes,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("Audit") && message.contains("Label"),
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
        "generated dependent-schema mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}

#[test]
fn generated_dependent_schema_patterns_share_the_document_budget()
-> Result<(), Box<dyn std::error::Error>> {
    let artifacts = emit(
        &pattern_budget_program()?,
        &Options {
            package_name: "generated-dependent-schema-budget".into(),
            runtime_dependency: RuntimeDependency::Path(runtime_path()?.display().to_string()),
        },
    )?;
    let output = TempDir::new("rust_json_dependent_schema_budget_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r#"use codegen_runtime::JsonBoundaryError;

fn main() {
    let value = "a".repeat(5_000);
    let document = format!("{{\"Ordinary\":{value:?},\"Trigger\":true,\"Guard\":{value:?}}}");
    assert!(matches!(
        generated_dependent_schema_budget::execute_json(&document),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("work limit"),
    ));
}
"#;
    fs::write(output.path().join("src/main.rs"), harness)?;
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()?;
    assert!(
        run.status.success(),
        "generated dependent-schema budget mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
