use super::*;

fn named_source_program() -> Program {
    let directory_schema = SchemaNode::group(
        "Directory",
        vec![
            SchemaNode::scalar("Name", ScalarType::String),
            SchemaNode::group(
                "Files",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            )
            .repeating(),
            SchemaNode::recursive_group("Children", "Directory").repeating(),
        ],
    );
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Customer", ScalarType::Int),
                SchemaNode::scalar("Required", ScalarType::String),
                SchemaNode::group(
                    "Settings",
                    vec![SchemaNode::scalar("Label", ScalarType::String)],
                ),
            ],
        ),
        extra_sources: vec![
            NamedSourceProgram {
                name: "Catalog".into(),
                source: SchemaNode::group(
                    "CatalogRows",
                    vec![
                        SchemaNode::scalar("Key", ScalarType::Int),
                        SchemaNode::scalar("Name", ScalarType::String),
                    ],
                )
                .repeating(),
            },
            NamedSourceProgram {
                name: "Settings".into(),
                source: SchemaNode::group(
                    "SettingsDocument",
                    vec![
                        SchemaNode::scalar("Label", ScalarType::String),
                        SchemaNode::scalar("Other", ScalarType::String),
                    ],
                ),
            },
            NamedSourceProgram {
                name: "Tree".into(),
                source: directory_schema,
            },
        ],
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("LookupName", ScalarType::String),
                SchemaNode::scalar("PrimarySetting", ScalarType::String),
                SchemaNode::scalar("FallbackSetting", ScalarType::String),
                SchemaNode::scalar("CatalogCount", ScalarType::Int),
                SchemaNode::scalar("Required", ScalarType::String),
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Key", ScalarType::Int),
                        SchemaNode::scalar("Name", ScalarType::String),
                        SchemaNode::scalar("Position", ScalarType::Int),
                    ],
                )
                .repeating(),
                SchemaNode::scalar("Paths", ScalarType::String).repeating(),
            ],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Customer".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Lookup {
                    collection: vec!["Catalog".into()],
                    key: vec!["Key".into()],
                    matches: 1,
                    value: vec!["Name".into()],
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Settings".into(), "Label".into()],
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Settings".into(), "Other".into()],
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Aggregate {
                    function: AggregateFunction::Count,
                    collection: vec!["Catalog".into()],
                    value: AggregateValue::Path(vec!["Key".into()]),
                    arg: None,
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Required".into()],
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Key".into()],
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Name".into()],
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::Position {
                    collection: vec!["Catalog".into()],
                },
            },
            ExpressionNode {
                id: 10,
                expression: Expression::Const {
                    value: Value::String(String::new()),
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::Const {
                    value: Value::String("/".into()),
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
        ],
        failure_rules: Vec::new(),
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![
                Binding {
                    target_field: "LookupName".into(),
                    expression: 2,
                    target_type: ScalarType::String,
                    repeating: false,
                },
                Binding {
                    target_field: "PrimarySetting".into(),
                    expression: 3,
                    target_type: ScalarType::String,
                    repeating: false,
                },
                Binding {
                    target_field: "FallbackSetting".into(),
                    expression: 4,
                    target_type: ScalarType::String,
                    repeating: false,
                },
                Binding {
                    target_field: "CatalogCount".into(),
                    expression: 5,
                    target_type: ScalarType::Int,
                    repeating: false,
                },
                Binding {
                    target_field: "Required".into(),
                    expression: 6,
                    target_type: ScalarType::String,
                    repeating: false,
                },
            ],
            children: vec![
                TargetScope {
                    target_field: "Rows".into(),
                    repeating: true,
                    iteration: Some(IterationPlan::new(
                        SourceIteration::new(vec!["Catalog".into()]),
                        None,
                        None,
                        Vec::new(),
                        IterationOutput::Repeated,
                    )),
                    construction: TargetConstruction::Group,
                    bindings: vec![
                        Binding {
                            target_field: "Key".into(),
                            expression: 7,
                            target_type: ScalarType::Int,
                            repeating: false,
                        },
                        Binding {
                            target_field: "Name".into(),
                            expression: 8,
                            target_type: ScalarType::String,
                            repeating: false,
                        },
                        Binding {
                            target_field: "Position".into(),
                            expression: 9,
                            target_type: ScalarType::Int,
                            repeating: false,
                        },
                    ],
                    children: Vec::new(),
                },
                TargetScope {
                    target_field: "Paths".into(),
                    repeating: true,
                    iteration: Some(IterationPlan::generated(
                        GeneratedSequence::RecursiveCollect {
                            collection: vec!["Tree".into()],
                            children: vec!["Children".into()],
                            descent_value: vec!["Name".into()],
                            values: vec!["Files".into()],
                            value: vec!["Name".into()],
                            prefix: 10,
                            separator: 11,
                            item: 12,
                        },
                    )),
                    construction: TargetConstruction::Scalar { expression: 12 },
                    bindings: Vec::new(),
                    children: Vec::new(),
                },
            ],
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn emits_exact_named_input_contract_in_declaration_order() {
    let artifacts = emit(
        &named_source_program(),
        &Options {
            package_name: "named-inputs".into(),
            runtime_dependency: RuntimeDependency::Version("0.1.0".into()),
        },
    )
    .unwrap();
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .unwrap();

    assert!(source.contains("pub use codegen_runtime::NamedInput;"));
    assert!(source.contains(
        "pub const EXTRA_SOURCE_NAMES: &[&str] = &[\"Catalog\", \"Settings\", \"Tree\"]"
    ));
    assert!(source.contains("pub fn execute_with_sources("));
    assert!(source.contains("pub fn execute_with_sources_and_context("));
    assert!(source.contains("pub fn execute_outputs_with_sources("));
    assert!(source.contains("pub fn execute_outputs_with_sources_and_context("));
    assert!(source.contains("execute_outputs_with_sources(source, &[])"));
    assert!(source.contains("execute_outputs_with_sources_and_context(source, &[], execution)"));
    assert!(source.contains("ScopeContext::with_named_inputs(source, &inputs)"));
    assert!(source.contains("ScopeContext::with_named_inputs_and_execution_context("));
}

#[test]
fn generated_named_input_project_builds_and_enforces_the_boundary_before_evaluation() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap();
    let output = TempDir::new("rust_named_input_codegen");
    let artifacts = emit(
        &named_source_program(),
        &Options {
            package_name: "named-input-map".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use std::path::Path;

use codegen_runtime::{
    ExecutionContext, Instance, RuntimeError, SourcePathError, Value, field, group, repeated,
    scalar,
};
use named_input_map::NamedInput;

fn main() {
    let source = primary(true);
    let catalog = repeated([catalog_row(1, "one"), catalog_row(2, "two")]);
    let settings = group([
        field("Label", scalar(text("named"))),
        field("Other", scalar(text("fallback"))),
    ]);
    let tree = directory(
        "root",
        &["top.txt"],
        vec![directory("child", &["nested.txt"], Vec::new())],
    );
    let inputs = [
        NamedInput { name: "Tree", instance: &tree },
        NamedInput { name: "Settings", instance: &settings },
        NamedInput { name: "Catalog", instance: &catalog },
    ];

    let expected = group([
        field("LookupName", scalar(text("two"))),
        field("PrimarySetting", scalar(text("primary"))),
        field("FallbackSetting", scalar(text("fallback"))),
        field("CatalogCount", scalar(Value::Int(2))),
        field("Required", scalar(text("present"))),
        field("Rows", repeated([
            row(1, "one", 1),
            row(2, "two", 2),
        ])),
        field("Paths", repeated([
            scalar(text("/root/top.txt")),
            scalar(text("/root/child/nested.txt")),
        ])),
    ]);

    assert_eq!(
        named_input_map::execute_with_sources(&source, &inputs).unwrap(),
        expected,
    );
    let outputs = named_input_map::execute_outputs_with_sources(&source, &inputs).unwrap();
    assert_eq!(outputs.primary, expected);
    assert!(outputs.extras.is_empty());

    let execution = ExecutionContext::new(Path::new("mapping.ferrule.json"));
    assert_eq!(
        named_input_map::execute_with_sources_and_context(&source, &inputs, &execution).unwrap(),
        expected,
    );
    assert_eq!(
        named_input_map::execute_outputs_with_sources_and_context(
            &source,
            &inputs,
            &execution,
        )
        .unwrap()
        .primary,
        expected,
    );

    assert_eq!(
        named_input_map::execute(&source),
        Err(RuntimeError::MissingNamedSource { name: "Catalog" }),
    );
    assert_eq!(
        named_input_map::execute_with_sources(
            &source,
            &[NamedInput { name: "Catalog", instance: &catalog }],
        ),
        Err(RuntimeError::MissingNamedSource { name: "Settings" }),
    );
    assert_eq!(
        named_input_map::execute_with_sources(
            &source,
            &[NamedInput { name: "Unknown", instance: &catalog }],
        ),
        Err(RuntimeError::UnexpectedNamedSource { name: "Unknown".into() }),
    );
    assert_eq!(
        named_input_map::execute_with_sources(
            &source,
            &[
                NamedInput { name: "Catalog", instance: &catalog },
                NamedInput { name: "Catalog", instance: &catalog },
            ],
        ),
        Err(RuntimeError::DuplicateNamedSource { name: "Catalog" }),
    );

    // Named-input validation happens before the missing primary field can fail.
    let invalid_source = primary(false);
    assert_eq!(
        named_input_map::execute(&invalid_source),
        Err(RuntimeError::MissingNamedSource { name: "Catalog" }),
    );
    assert!(matches!(
        named_input_map::execute_with_sources(&invalid_source, &inputs),
        Err(RuntimeError::SourcePath(SourcePathError::MissingField { field, .. }))
            if field == "Required"
    ));
}

fn text(value: &str) -> Value {
    Value::String(value.into())
}

fn primary(required: bool) -> Instance {
    let mut fields = vec![
        field("Customer", scalar(Value::Int(2))),
        field("Settings", group([field("Label", scalar(text("primary")))])),
    ];
    if required {
        fields.push(field("Required", scalar(text("present"))));
    }
    group(fields)
}

fn catalog_row(key: i64, name: &str) -> Instance {
    group([
        field("Key", scalar(Value::Int(key))),
        field("Name", scalar(text(name))),
    ])
}

fn row(key: i64, name: &str, position: i64) -> Instance {
    group([
        field("Key", scalar(Value::Int(key))),
        field("Name", scalar(text(name))),
        field("Position", scalar(Value::Int(position))),
    ])
}

fn directory(name: &str, files: &[&str], children: Vec<Instance>) -> Instance {
    group([
        field("Name", scalar(text(name))),
        field(
            "Files",
            repeated(files.iter().map(|name| {
                group([field("Name", scalar(text(name)))])
            })),
        ),
        field("Children", repeated(children)),
    ])
}
"#,
    )
    .unwrap();

    let result = Command::new("cargo")
        .args(["run", "--quiet"])
        .current_dir(output.path())
        .env("CARGO_TARGET_DIR", output.path().join("target"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "generated Rust named-input project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
