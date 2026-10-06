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
        target_domain: codegen::ScalarTargetDomain::Single(ScalarType::String),
        repeating: false,
    };
    Program {
        xml_boundary: None,
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
        xml_boundary: None,
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
                        target_domain: codegen::ScalarTargetDomain::Single(ScalarType::String),
                        repeating: false,
                    },
                    Binding {
                        target_field: "value".into(),
                        expression: 2,
                        target_domain: codegen::ScalarTargetDomain::Single(ScalarType::String),
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
        .generated_host_output(output.path())
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
        .generated_host_output(output.path())
        .expect("generated Rust harness starts");
    assert!(
        result.status.success(),
        "generated Rust per-driver dynamic-source project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn unused_second_document_fixture() -> Program {
    let mut program = dynamic_document_fixture();
    program.extra_sources.push(NamedSourceProgram {
        name: "Config".into(),
        source: SchemaNode::group(
            "ConfigDocument",
            vec![SchemaNode::scalar("Note", ScalarType::String)],
        ),
        dynamic: Some(DynamicSourceProgram {
            path: 99,
            driver: SourceIteration::new(vec!["Files".into()]),
        }),
    });
    program.expressions.push(ExpressionNode {
        id: 99,
        expression: Expression::RuntimeValue {
            value: RuntimeValue::CurrentDateTime,
        },
    });
    program
}

#[test]
fn unused_xml_dynamic_declaration_keeps_policy_without_emitting_its_path_function() {
    let mut project: ::mapping::Project = serde_json::from_str(include_str!(
        "../../../codegen/src/tests/fixtures/multiple_dynamic_named_inputs_static_primary_dynamic_named_xml_documents.json"
    )).unwrap();
    project.extra_targets.pop();
    project.graph.nodes.insert(
        107,
        ::mapping::Node::RuntimeValue {
            value: ::mapping::RuntimeValue::CurrentDateTime,
        },
    );
    project.extra_sources[3].dynamic_path.as_mut().unwrap().node = 107;
    let program = codegen::lower(&project).unwrap();
    assert!(program.expressions.iter().any(|node| node.id == 107));
    assert_eq!(program.extra_sources[3].dynamic.as_ref().unwrap().path, 107);
    let source = render_source(&program).unwrap();
    assert!(!source.contains("fn expression_107("));
    assert_eq!(source.matches("DynamicSourceItems::load(").count(), 1);
    assert!(source.contains("fn expression_2("));
    assert!(source.contains("\"codes\"") && source.contains("\"catalog\""));
    assert!(
        source.contains(
            "pub fn execute_xml_document_outputs_with_sources_and_dynamic_source_loader("
        )
    );
}

#[test]
fn unused_path_dependencies_are_omitted_but_shared_reads_and_failure_messages_remain() {
    let mut program = unused_second_document_fixture();
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == 99)
        .unwrap()
        .expression = Expression::Call {
        function: ScalarFunction::Concat,
        args: vec![100, 101],
    };
    program.expressions.extend([
        ExpressionNode {
            id: 100,
            expression: Expression::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            },
        },
        ExpressionNode {
            id: 101,
            expression: Expression::Const {
                value: Value::String(".xml".into()),
            },
        },
    ]);
    validate_program(&program).unwrap();
    let source = render_source(&program).unwrap();
    for node in [99, 100, 101] {
        assert!(!source.contains(&format!("fn expression_{node}(")));
    }
    let mut shared = program.clone();
    let SchemaKind::Group { children, .. } = &mut shared.target.kind else {
        unreachable!()
    };
    children.push(SchemaNode::scalar("Date", ScalarType::String));
    shared.root.bindings.push(Binding {
        target_field: "Date".into(),
        expression: 100,
        target_domain: codegen::ScalarTargetDomain::Single(ScalarType::String),
        repeating: false,
    });
    validate_program(&shared).unwrap();
    let source = render_source(&shared).unwrap();
    assert!(source.contains("fn expression_100("));
    assert!(!source.contains("fn expression_99(") && !source.contains("fn expression_101("));
    program.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source(SourceIteration::new(vec!["Files".into()])),
        selection: FailureSelection::All,
        message: Some(99),
    });
    validate_program(&program).unwrap();
    let source = render_source(&program).unwrap();
    for node in [99, 100, 101] {
        assert!(source.contains(&format!("fn expression_{node}(")));
    }
    assert!(source.contains("let message = Some(expression_99(&item_context)?);"));
}

#[test]
fn reached_second_source_retains_both_lazy_path_branches() {
    let mut program = unused_second_document_fixture();
    program
        .expressions
        .iter_mut()
        .find(|node| node.id == 99)
        .unwrap()
        .expression = Expression::If {
        condition: 100,
        then: 101,
        else_: 102,
    };
    program.expressions.extend([
        ExpressionNode {
            id: 100,
            expression: Expression::Const {
                value: Value::Bool(false),
            },
        },
        ExpressionNode {
            id: 101,
            expression: Expression::RuntimeValue {
                value: RuntimeValue::CurrentDateTime,
            },
        },
        ExpressionNode {
            id: 102,
            expression: Expression::Const {
                value: Value::String("config.xml".into()),
            },
        },
        ExpressionNode {
            id: 103,
            expression: Expression::SourceField {
                frame: None,
                path: vec!["Note".into()],
            },
        },
    ]);
    let SchemaKind::Group { children, .. } = &mut program.target.kind else {
        unreachable!()
    };
    children.push(
        SchemaNode::group(
            "Notes",
            vec![SchemaNode::scalar("Note", ScalarType::String)],
        )
        .repeating(),
    );
    program.root.children.push(TargetScope {
        target_field: "Notes".into(),
        repeating: true,
        iteration: Some(IterationPlan::new(
            SourceIteration::new(vec!["Config".into()]),
            None,
            None,
            Vec::new(),
            IterationOutput::Repeated,
        )),
        construction: TargetConstruction::Group,
        bindings: vec![Binding {
            target_field: "Note".into(),
            expression: 103,
            target_domain: codegen::ScalarTargetDomain::Single(ScalarType::String),
            repeating: false,
        }],
        children: Vec::new(),
    });
    validate_program(&program).unwrap();
    let source = render_source(&program).unwrap();
    for node in [1, 2, 99, 100, 101, 102, 103] {
        assert!(source.contains(&format!("fn expression_{node}(")));
    }
    assert_eq!(source.matches("DynamicSourceItems::load(").count(), 2);
    assert!(source.contains("|driver_context| expression_99(driver_context)"));
    assert!(source.contains("if require_bool(100, condition)?"));
}

#[test]
fn zero_and_one_dynamic_source_keep_legacy_expression_emission() {
    let mut program = unused_second_document_fixture();
    program.extra_sources.pop();
    validate_program(&program).unwrap();
    assert!(
        render_source(&program)
            .unwrap()
            .contains("fn expression_99(")
    );
    let mut zero = fixture();
    zero.expressions.push(ExpressionNode {
        id: 99,
        expression: Expression::RuntimeValue {
            value: RuntimeValue::CurrentDateTime,
        },
    });
    validate_program(&zero).unwrap();
    assert!(render_source(&zero).unwrap().contains("fn expression_99("));
}

#[test]
fn generated_unused_dynamic_source_keeps_loader_and_context_reads_lazy_without_warnings() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap_or_default();
    let output = TempDir::new("rust_unused_dynamic_source_codegen");
    let artifacts = emit(
        &unused_second_document_fixture(),
        &Options {
            package_name: "generated-unused-dynamic-source".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    write_artifacts(output.path(), &artifacts);
    fs::write(output.path().join("src/main.rs"), r#"use codegen_runtime::{DynamicSourceLoader, Instance, Value, field, group, repeated, scalar};

struct Loader;
impl DynamicSourceLoader for Loader {
    fn load(&self, source: &str, path: &str) -> Result<Instance, String> {
        assert_eq!(source, "Catalog");
        assert_eq!(path, "same.json");
        Ok(group([field("Rows", repeated([group([field("value", scalar(Value::String("loaded".into())))])]))]))
    }
}
fn main() {
    let source = group([field("Files", repeated([group([field("path", scalar(Value::String("same.json".into())))])]))]);
    let mapped = generated_unused_dynamic_source::execute_with_dynamic_source_loader(&source, &Loader).unwrap();
    let rows = mapped.field("Rows").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].field("value").and_then(Instance::as_scalar), Some(&Value::String("loaded".into())));
}
"#).unwrap();
    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .env("RUSTFLAGS", "-D warnings")
        .current_dir(output.path())
        .generated_host_output(output.path())
        .unwrap();
    assert!(
        result.status.success(),
        "generated unused-source host failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
