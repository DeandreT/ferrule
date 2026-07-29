use std::path::Path;
use std::process::Command;

use super::*;

fn open_group(name: &str) -> SchemaNode {
    let Some(schema) = SchemaNode::group(name, Vec::new())
        .with_dynamic_fields(SchemaNode::scalar("*", ScalarType::String))
    else {
        panic!("a group schema accepts dynamic fields");
    };
    schema
}

fn fixture() -> Program {
    let binding = |target_field: &str, expression| Binding {
        target_field: target_field.into(),
        expression,
        target_type: ScalarType::String,
        repeating: false,
    };
    Program {
        source: SchemaNode::group("Source", vec![open_group("Properties")]),
        extra_sources: vec![NamedSourceProgram {
            name: "Config".into(),
            source: open_group("Config"),
        }],
        target: SchemaNode::group(
            "Target",
            ["Found", "Missing", "ExplicitNull", "WrongKey", "Named"]
                .into_iter()
                .map(|name| SchemaNode::scalar(name, ScalarType::String))
                .collect(),
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::Const {
                    value: Value::String("selected".into()),
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: Value::String("missing".into()),
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::Const {
                    value: Value::String("nil".into()),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 1,
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 2,
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 3,
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::DynamicSourceField {
                    object: vec!["Properties".into()],
                    frame: None,
                    key: 4,
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::DynamicSourceField {
                    object: vec!["Config".into()],
                    frame: None,
                    key: 1,
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
                binding("Found", 5),
                binding("Missing", 6),
                binding("ExplicitNull", 7),
                binding("WrongKey", 8),
                binding("Named", 9),
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn generated_dynamic_source_fields_execute_exact_null_semantics() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap_or_default();
    let output = TempDir::new("rust_dynamic_source_codegen");
    let artifacts = emit(
        &fixture(),
        &Options {
            package_name: "generated-dynamic-source".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .expect("dynamic source program emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, Value, field, group, scalar};

fn main() {
    let source = group([field(
        "Properties",
        group([
            field("selected", scalar(Value::String("primary".into()))),
            field("nil", scalar(Value::json_null())),
        ]),
    )]);
    let config = group([field(
        "selected",
        scalar(Value::String("named".into())),
    )]);
    let result = generated_dynamic_source::execute_with_sources(
        &source,
        &[generated_dynamic_source::NamedInput {
            name: "Config",
            instance: &config,
        }],
    )
    .unwrap();
    assert_eq!(
        result,
        group([
            field("Found", scalar(Value::String("primary".into()))),
            field("Missing", scalar(Value::Null)),
            field("ExplicitNull", scalar(Value::json_null())),
            field("WrongKey", scalar(Value::Null)),
            field("Named", scalar(Value::String("named".into()))),
        ]),
    );

    let structural = group([field(
        "Properties",
        group([field("selected", group([]))]),
    )]);
    let result = generated_dynamic_source::execute_with_sources(
        &structural,
        &[generated_dynamic_source::NamedInput {
            name: "Config",
            instance: &config,
        }],
    )
    .unwrap();
    assert_eq!(
        result.field("Found").and_then(Instance::as_scalar),
        Some(&Value::Null),
    );
}
"#,
    )
    .expect("generated harness is written");

    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()
        .expect("generated Rust harness starts");
    assert!(
        result.status.success(),
        "generated Rust dynamic-source project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
