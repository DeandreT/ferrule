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
            dynamic: None,
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

fn dynamic_document_fixture() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Files",
                    vec![SchemaNode::scalar("path", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        extra_sources: vec![NamedSourceProgram {
            name: "Catalog".into(),
            source: SchemaNode::group(
                "CatalogDocument",
                vec![
                    SchemaNode::group(
                        "Rows",
                        vec![SchemaNode::scalar("value", ScalarType::String)],
                    )
                    .repeating(),
                ],
            ),
            dynamic: Some(DynamicSourceProgram {
                path: 1,
                driver: SourceIteration::new(vec!["Files".into()]),
            }),
        }],
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("path", ScalarType::String),
                        SchemaNode::scalar("value", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["path".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["value".into()],
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
            bindings: Vec::new(),
            children: vec![TargetScope {
                target_field: "Rows".into(),
                repeating: true,
                iteration: Some(IterationPlan::new(
                    SourceIteration::new(vec!["Catalog".into(), "Rows".into()]),
                    None,
                    None,
                    Vec::new(),
                    IterationOutput::Repeated,
                )),
                construction: TargetConstruction::Group,
                bindings: vec![
                    Binding {
                        target_field: "path".into(),
                        expression: 1,
                        target_type: ScalarType::String,
                        repeating: false,
                    },
                    Binding {
                        target_field: "value".into(),
                        expression: 2,
                        target_type: ScalarType::String,
                        repeating: false,
                    },
                ],
                children: Vec::new(),
            }],
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

#[test]
fn generated_per_driver_dynamic_sources_execute_typed_and_json_host_contracts() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap_or_default();
    let output = TempDir::new("rust_per_driver_dynamic_source_codegen");
    let artifacts = emit(
        &dynamic_document_fixture(),
        &Options {
            package_name: "generated-per-driver-source".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .expect("per-driver dynamic source program emits");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r##"use std::cell::RefCell;
use codegen_runtime::{Instance, RuntimeError, Value, field, group, repeated, scalar};
use generated_per_driver_source::{
    DynamicJsonSourceLoader, DynamicSourceLoader,
};

struct TypedLoader {
    calls: RefCell<Vec<String>>,
}

impl DynamicSourceLoader for TypedLoader {
    fn load(&self, source: &str, path: &str) -> Result<Instance, String> {
        assert_eq!(source, "Catalog");
        self.calls.borrow_mut().push(path.to_string());
        Ok(group([field(
            "Rows",
            repeated([group([field(
                "value",
                scalar(Value::String(format!("loaded:{path}"))),
            )])]),
        )]))
    }
}

struct JsonLoader;

impl DynamicJsonSourceLoader for JsonLoader {
    fn load(&self, source: &str, path: &str) -> Result<Vec<u8>, String> {
        assert_eq!(source, "Catalog");
        Ok(format!(r#"{{"Rows":[{{"value":"loaded:{path}"}}]}}"#).into_bytes())
    }
}

fn main() {
    let source = group([field(
        "Files",
        repeated([
            group([field("path", scalar(Value::String("a.json".into())))]),
            group([field("path", scalar(Value::String("b.json".into())))]),
        ]),
    )]);
    assert_eq!(
        generated_per_driver_source::execute(&source),
        Err(RuntimeError::MissingDynamicSourceLoader { source: "Catalog" }),
    );

    let loader = TypedLoader {
        calls: RefCell::new(Vec::new()),
    };
    let output = generated_per_driver_source::execute_with_dynamic_source_loader(&source, &loader)
        .unwrap();
    assert_eq!(loader.calls.borrow().as_slice(), ["a.json", "b.json"]);
    let rows = output
        .field("Rows")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("path").and_then(Instance::as_scalar),
        Some(&Value::String("a.json".into())),
    );
    assert_eq!(
        rows[1].field("value").and_then(Instance::as_scalar),
        Some(&Value::String("loaded:b.json".into())),
    );

    let json = generated_per_driver_source::execute_json_with_dynamic_source_loader(
        r#"{"Files":[{"path":"a.json"},{"path":"b.json"}]}"#,
        &JsonLoader,
    )
    .unwrap();
    assert_eq!(
        json,
        "{\n  \"Rows\": [\n    {\n      \"path\": \"a.json\",\n      \"value\": \"loaded:a.json\"\n    },\n    {\n      \"path\": \"b.json\",\n      \"value\": \"loaded:b.json\"\n    }\n  ]\n}\n",
    );
}
"##,
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
        "generated Rust per-driver dynamic-source project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
