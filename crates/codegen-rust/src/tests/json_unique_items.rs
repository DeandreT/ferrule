use super::*;
use codegen::{Expression, ExpressionNode};

fn unique_rows() -> Result<SchemaNode, &'static str> {
    SchemaNode::group(
        "Rows",
        vec![
            SchemaNode::scalar("Count", ScalarType::Float),
            SchemaNode::scalar("Tags", ScalarType::Int).repeating(),
            SchemaNode::group(
                "Meta",
                vec![SchemaNode::scalar("Label", ScalarType::String)],
            ),
        ],
    )
    .repeating()
    .with_json_unique_items()
    .ok_or("test row array accepts uniqueItems")
}

fn unique_items_program() -> Result<Program, &'static str> {
    let target = SchemaNode::scalar("Amount", ScalarType::Float)
        .repeating()
        .with_json_unique_items()
        .ok_or("test target array accepts uniqueItems")?;
    Ok(Program {
        source: SchemaNode::group(
            "Source",
            vec![
                unique_rows()?,
                SchemaNode::scalar("Raw", ScalarType::String).repeating(),
            ],
        ),
        extra_sources: Vec::new(),
        target,
        expressions: vec![ExpressionNode {
            id: 1,
            expression: Expression::SourceField {
                frame: None,
                path: Vec::new(),
            },
        }],
        user_functions: Vec::new(),
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: true,
            iteration: Some(IterationPlan::source(vec!["Raw".into()])),
            construction: TargetConstruction::Scalar {
                expression: 1,
                target_domain: ScalarTargetDomain::Single(ScalarType::Float),
            },
            bindings: Vec::new(),
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    })
}

#[test]
fn generated_json_entry_point_enforces_exact_unique_items() -> Result<(), Box<dyn std::error::Error>>
{
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .ok_or("codegen runtime has a workspace parent")?;
    let artifacts = emit(
        &unique_items_program()?,
        &Options {
            package_name: "generated-unique-items".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )?;
    let generated_source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .ok_or("generated Rust source is present UTF-8")?;
    assert!(generated_source.contains(r#"\"json_unique_items\":true"#));

    let output = TempDir::new("rust_json_unique_items_codegen");
    write_artifacts(output.path(), &artifacts);
    let harness = r##"use codegen_runtime::JsonBoundaryError;

fn main() {
    assert_eq!(
        generated_unique_items::execute_json(
            r#"{
                "Rows":[
                    {"Count":1,"Tags":[1,2],"Meta":{"Label":"A"}},
                    {"Count":2,"Tags":[2,1],"Meta":{"Label":"A"}}
                ],
                "Raw":["1","2.0"]
            }"#,
        ).as_deref(),
        Ok("[\n  1.0,\n  2.0\n]\n"),
    );
    assert!(matches!(
        generated_unique_items::execute_json(
            r#"{
                "Rows":[
                    {"Count":1,"Tags":[1,2],"Meta":{"Label":"A"}},
                    {"Meta":{"Label":"A"},"Tags":[1,2],"Count":1.0}
                ],
                "Raw":["1","2"]
            }"#,
        ),
        Err(JsonBoundaryError::InvalidInput { message })
            if message.contains("indexes 1 and 2"),
    ));
    assert!(matches!(
        generated_unique_items::execute_json(
            r#"{
                "Rows":[],
                "Raw":["9007199254740993.0","9007199254740992"]
            }"#,
        ),
        Err(JsonBoundaryError::InvalidOutput { message })
            if message.contains("indexes 1 and 2"),
    ));
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
        "generated uniqueItems mapping failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    Ok(())
}
