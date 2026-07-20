use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;
use codegen::{
    Binding, ExpressionNode, FailureIteration, FailureRule, FailureSelection, GeneratedSequence,
    IterationOutput, IterationPlan, NamedSourceProgram, NamedTargetProgram, ScalarFunction,
    SourceIteration, TargetConstruction,
};
use ir::{SchemaKind, SchemaNode};

mod collection_find;
mod extra_sources;
mod extra_targets;
mod failure_rules;
mod joins;
mod scalar_functions;

fn program() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("First", ScalarType::Int),
                SchemaNode::scalar("Second", ScalarType::Int),
                SchemaNode::scalar("Condition", ScalarType::Bool),
                SchemaNode::group(
                    "Parents",
                    vec![
                        SchemaNode::scalar("Id", ScalarType::Int),
                        SchemaNode::group(
                            "Children",
                            vec![
                                SchemaNode::scalar("Name", ScalarType::String),
                                SchemaNode::scalar("ExpectedRawPosition", ScalarType::Int),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Copied", ScalarType::String),
                SchemaNode::scalar("Numbers", ScalarType::Int).repeating(),
                SchemaNode::scalar("Sum", ScalarType::Int),
                SchemaNode::scalar("Selected", ScalarType::Int),
                SchemaNode::group(
                    "Nested",
                    vec![SchemaNode::scalar("Constant", ScalarType::String)],
                )
                .repeating(),
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("ParentId", ScalarType::Int),
                        SchemaNode::scalar("ChildName", ScalarType::String),
                        SchemaNode::scalar("ChildPosition", ScalarType::Int),
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
                    path: vec!["Name".to_string()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::Const {
                    value: Value::Float(7.0),
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::Const { value: Value::Null },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Const {
                    value: Value::String("fixed".to_string()),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["First".to_string()],
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Second".to_string()],
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::Call {
                    function: ScalarFunction::Add,
                    args: vec![5, 6],
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::Const {
                    value: Value::Int(0),
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::Call {
                    function: ScalarFunction::Divide,
                    args: vec![5, 8],
                },
            },
            ExpressionNode {
                id: 10,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Condition".to_string()],
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::If {
                    condition: 10,
                    then: 7,
                    else_: 9,
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::SourceField {
                    frame: Some(vec!["Parents".to_string()]),
                    path: vec!["Id".to_string()],
                },
            },
            ExpressionNode {
                id: 13,
                expression: Expression::SourceField {
                    frame: Some(vec!["Parents".to_string(), "Children".to_string()]),
                    path: vec!["Name".to_string()],
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::Position {
                    collection: vec!["Children".to_string()],
                },
            },
            ExpressionNode {
                id: 15,
                expression: Expression::SourceField {
                    frame: Some(vec!["Parents".to_string(), "Children".to_string()]),
                    path: vec!["ExpectedRawPosition".to_string()],
                },
            },
            ExpressionNode {
                id: 16,
                expression: Expression::Call {
                    function: ScalarFunction::Equal,
                    args: vec![14, 15],
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
                    target_field: "Copied".to_string(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: false,
                },
                Binding {
                    target_field: "Numbers".to_string(),
                    expression: 2,
                    target_type: ScalarType::Int,
                    repeating: true,
                },
                Binding {
                    target_field: "Numbers".to_string(),
                    expression: 3,
                    target_type: ScalarType::Int,
                    repeating: true,
                },
                Binding {
                    target_field: "Sum".to_string(),
                    expression: 7,
                    target_type: ScalarType::Int,
                    repeating: false,
                },
                Binding {
                    target_field: "Selected".to_string(),
                    expression: 11,
                    target_type: ScalarType::Int,
                    repeating: false,
                },
            ],
            children: vec![
                TargetScope {
                    target_field: "Nested".to_string(),
                    repeating: true,
                    iteration: None,
                    construction: TargetConstruction::Group,
                    bindings: vec![Binding {
                        target_field: "Constant".to_string(),
                        expression: 4,
                        target_type: ScalarType::String,
                        repeating: false,
                    }],
                    children: Vec::new(),
                },
                TargetScope {
                    target_field: "Rows".to_string(),
                    repeating: true,
                    iteration: Some(IterationPlan::new(
                        SourceIteration::new(vec!["Parents".to_string(), "Children".to_string()]),
                        Some(16),
                        None,
                        Vec::new(),
                        IterationOutput::Repeated,
                    )),
                    construction: TargetConstruction::Group,
                    bindings: vec![
                        Binding {
                            target_field: "ParentId".to_string(),
                            expression: 12,
                            target_type: ScalarType::Int,
                            repeating: false,
                        },
                        Binding {
                            target_field: "ChildName".to_string(),
                            expression: 13,
                            target_type: ScalarType::String,
                            repeating: false,
                        },
                        Binding {
                            target_field: "ChildPosition".to_string(),
                            expression: 14,
                            target_type: ScalarType::Int,
                            repeating: false,
                        },
                    ],
                    children: Vec::new(),
                },
            ],
        },
        extra_targets: Vec::new(),
    }
}

#[test]
fn emits_deterministic_rust_project() {
    let options = Options {
        package_name: "sample-map".to_string(),
        runtime_dependency: RuntimeDependency::Path("../runtime".to_string()),
    };
    let first = emit(&program(), &options).unwrap();
    let second = emit(&program(), &options).unwrap();
    assert_eq!(first, second);
    assert_eq!(
        first
            .files()
            .iter()
            .map(|file| file.path.as_str())
            .collect::<Vec<_>>(),
        ["Cargo.toml", "src/lib.rs"]
    );
    let source = first
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .unwrap();
    assert!(source.contains("adapt_target_value(expression_2(context)?, ScalarType::Int)"));
    assert!(source.contains("repeated_1.push(scalar(value_1))"));
    assert!(source.contains("scope_root_0(context)?"));
    assert!(
        source.contains("let mut candidates = context.walk_source(&[\"Parents\", \"Children\"]);")
    );
    assert!(source.contains("context.resolve_scalar_in_frame(&[\"Parents\"], &[\"Id\"])"));
    assert!(source.contains("Ok(Value::Int(context.position(&[\"Children\"]) as i64))"));
    assert!(source.contains("let filter_value = expression_16(&item_context)?;"));
    assert!(source.contains("if !require_bool(16, filter_value)?"));
    assert!(source.contains(
        "let output_context = item_context.with_compact_last_position(outputs.len() + 1);"
    ));
    assert!(
        source.contains("adapt_target_value(expression_12(&output_context)?, ScalarType::Int)")
    );
}

#[test]
fn rejects_invalid_package_names_and_preserves_non_finite_literals() {
    let options = Options {
        package_name: "not/a/package".to_string(),
        runtime_dependency: RuntimeDependency::Version("0.1.0".to_string()),
    };
    assert!(matches!(
        emit(&program(), &options),
        Err(EmitError::InvalidPackageName(_))
    ));

    let mut invalid = program();
    invalid.expressions[1].expression = Expression::Const {
        value: Value::Float(f64::NAN),
    };
    let artifacts = emit(
        &invalid,
        &Options {
            package_name: "sample-map".to_string(),
            runtime_dependency: RuntimeDependency::Version("0.1.0".to_string()),
        },
    )
    .expect("IEEE-754 literals emit by exact bits");
    let source = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
        .expect("generated Rust source artifact");
    assert!(source.contains("f64::from_bits(0x7ff8000000000000_u64)"));
}

#[test]
fn rejects_missing_expression_dependencies() {
    let mut invalid = program();
    let Some(node) = invalid.expressions.iter_mut().find(|node| node.id == 7) else {
        panic!("test program must contain call node 7");
    };
    node.expression = Expression::Call {
        function: ScalarFunction::Add,
        args: vec![5, 404],
    };

    assert!(matches!(
        emit(
            &invalid,
            &Options {
                package_name: "sample-map".to_string(),
                runtime_dependency: RuntimeDependency::Version("0.1.0".to_string()),
            }
        ),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::MissingDependency {
                node: 7,
                dependency: 404,
            }
        ))
    ));
}

#[test]
fn rejects_self_and_multi_node_expression_cycles() {
    let options = Options {
        package_name: "sample-map".to_string(),
        runtime_dependency: RuntimeDependency::Version("0.1.0".to_string()),
    };
    let mut self_cycle = program();
    let Some(node) = self_cycle.expressions.iter_mut().find(|node| node.id == 7) else {
        panic!("test program must contain call node 7");
    };
    node.expression = Expression::Call {
        function: ScalarFunction::Add,
        args: vec![7],
    };
    assert!(matches!(
        emit(&self_cycle, &options),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::ExpressionCycle { cycle }
        )) if cycle == vec![7, 7]
    ));

    let mut multi_cycle = program();
    let Some(node) = multi_cycle.expressions.iter_mut().find(|node| node.id == 7) else {
        panic!("test program must contain call node 7");
    };
    node.expression = Expression::Call {
        function: ScalarFunction::Add,
        args: vec![9],
    };
    let Some(node) = multi_cycle.expressions.iter_mut().find(|node| node.id == 9) else {
        panic!("test program must contain call node 9");
    };
    node.expression = Expression::Call {
        function: ScalarFunction::Divide,
        args: vec![7, 8],
    };
    assert!(matches!(
        emit(&multi_cycle, &options),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::ExpressionCycle { cycle }
        )) if cycle == vec![7, 9, 7]
    ));
}

#[test]
fn rejects_static_target_name_collisions_before_emission() {
    let options = Options {
        package_name: "sample-map".to_string(),
        runtime_dependency: RuntimeDependency::Version("0.1.0".to_string()),
    };
    let mut binding_child = program();
    binding_child.root.bindings.push(Binding {
        target_field: "Nested".to_string(),
        expression: 1,
        target_type: ScalarType::String,
        repeating: false,
    });
    assert!(matches!(
        emit(&binding_child, &options),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::BindingChildCollision {
                target_path,
                target_field,
                binding: 5,
                child: 0,
            }
        )) if target_path.is_empty() && target_field == "Nested"
    ));

    let mut duplicate_child = program();
    let repeated_child = duplicate_child.root.children[0].clone();
    duplicate_child.root.children.push(repeated_child);
    assert!(matches!(
        emit(&duplicate_child, &options),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::DuplicateChildTarget {
                target_path,
                target_field,
                first_child: 0,
                duplicate_child: 2,
            }
        )) if target_path.is_empty() && target_field == "Nested"
    ));
}

#[test]
fn generated_project_builds_and_matches_the_static_mapping() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap();
    let output = TempDir::new("rust_codegen");
    let mut program = program();
    let SchemaKind::Group { children, .. } = &mut program.source.kind else {
        panic!("test program source must be a group")
    };
    children.push(SchemaNode::scalar("ExtraOnly", ScalarType::String));
    program.expressions.push(ExpressionNode {
        id: 17,
        expression: Expression::SourceField {
            frame: None,
            path: vec!["ExtraOnly".into()],
        },
    });
    program.extra_targets = vec![
        NamedTargetProgram {
            name: "audit".into(),
            target: SchemaNode::group(
                "Audit",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            ),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Group,
                bindings: vec![Binding {
                    target_field: "Name".into(),
                    expression: 1,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: Vec::new(),
            },
        },
        NamedTargetProgram {
            name: "delivery".into(),
            target: SchemaNode::group(
                "Delivery",
                vec![SchemaNode::scalar("Status", ScalarType::String)],
            ),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Group,
                bindings: vec![Binding {
                    target_field: "Status".into(),
                    expression: 17,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: Vec::new(),
            },
        },
    ];
    let artifacts = emit(
        &program,
        &Options {
            package_name: "sample-map".to_string(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use std::path::Path;

use codegen_runtime::{
ExecutionContext, FunctionError, Instance, RuntimeError, SourcePathError, Value, field, group,
repeated, scalar,
};

fn main() {
let happy_source = source(Value::Int(8), Value::Int(2), Value::Bool(true));
let actual = sample_map::execute(&happy_source).unwrap();
let expected = group([
    field("Copied", scalar(Value::String("Ada".to_string()))),
    field("Numbers", repeated([scalar(Value::Int(7))])),
    field("Sum", scalar(Value::Int(10))),
    field("Selected", scalar(Value::Int(10))),
    field("Nested", repeated([group([field(
        "Constant",
        scalar(Value::String("fixed".to_string())),
    )])])),
    field("Rows", repeated([
        mapped_row(1, "b", 1),
        mapped_row(2, "d", 2),
    ])),
]);
assert_eq!(actual, expected);

assert_eq!(
    sample_map::execute_with_sources(
        &happy_source,
        &[sample_map::NamedInput {
            name: "unexpected",
            instance: &happy_source,
        }],
    ),
    Err(RuntimeError::UnexpectedNamedSource {
        name: "unexpected".into(),
    }),
);

let outputs = sample_map::execute_outputs(&happy_source).unwrap();
assert_eq!(outputs.primary, expected);
assert_eq!(outputs.extras.len(), 2);
assert_eq!(outputs.extras[0].name, "audit");
assert_eq!(
    outputs.extras[0].instance,
    group([field("Name", scalar(Value::String("Ada".into())))])
);
assert_eq!(outputs.extras[1].name, "delivery");
assert_eq!(
    outputs.extras[1].instance,
    group([field("Status", scalar(Value::String("ready".into())))])
);

let execution = ExecutionContext::new(Path::new("mapping.ferrule.json"));
let context_outputs =
    sample_map::execute_outputs_with_context(&happy_source, &execution).unwrap();
assert_eq!(context_outputs, outputs);

let missing_extra = source_without_extra(Value::Int(8), Value::Int(2), Value::Bool(true));
for error in [
    sample_map::execute(&missing_extra),
    sample_map::execute_with_context(&missing_extra, &execution),
] {
    assert!(matches!(
        error,
        Err(RuntimeError::SourcePath(SourcePathError::MissingField { field, .. }))
            if field == "ExtraOnly"
    ));
}

let arithmetic = sample_map::execute(&source(
    Value::Int(8),
    Value::Int(2),
    Value::Bool(false),
));
assert_eq!(
    arithmetic,
    Err(RuntimeError::Function(FunctionError::DivideByZero))
);

let not_bool = sample_map::execute(&source(
    Value::Int(8),
    Value::Int(2),
    Value::String("no".to_string()),
));
assert_eq!(
    not_bool,
    Err(RuntimeError::NotABool {
        node: 10,
        found: "string",
    })
);

let missing = group([
    field("Name", scalar(Value::String("Ada".to_string()))),
    field("Condition", scalar(Value::Bool(true))),
]);
assert!(matches!(
    sample_map::execute(&missing),
    Err(RuntimeError::SourcePath(SourcePathError::MissingField { field, .. }))
        if field == "First"
));
}

fn source(first: Value, second: Value, condition: Value) -> Instance {
source_with_extra(first, second, condition, true)
}

fn source_without_extra(first: Value, second: Value, condition: Value) -> Instance {
source_with_extra(first, second, condition, false)
}

fn source_with_extra(first: Value, second: Value, condition: Value, include_extra: bool) -> Instance {
let mut fields = vec![
    field("Name", scalar(Value::String("Ada".to_string()))),
    field("First", scalar(first)),
    field("Second", scalar(second)),
    field("Condition", scalar(condition)),
    field("Parents", repeated([
        parent(1, &["a", "b"]),
        parent(2, &["c", "d"]),
    ])),
];
if include_extra {
    fields.push(field("ExtraOnly", scalar(Value::String("ready".into()))));
}
group(fields)
}

fn parent(id: i64, children: &[&str]) -> Instance {
group([
    field("Id", scalar(Value::Int(id))),
    field(
        "Children",
        repeated(children.iter().map(|name| {
            group([
                field("Name", scalar(Value::String((*name).to_string()))),
                field("ExpectedRawPosition", scalar(Value::Int(2))),
            ])
        })),
    ),
])
}

fn mapped_row(parent_id: i64, child_name: &str, child_position: i64) -> Instance {
group([
    field("ParentId", scalar(Value::Int(parent_id))),
    field(
        "ChildName",
        scalar(Value::String(child_name.to_string())),
    ),
    field("ChildPosition", scalar(Value::Int(child_position))),
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
        "generated Rust project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn generated_range_project_builds_runs_and_short_circuits_null_bounds() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap();
    let output = TempDir::new("rust_generated_sequence_codegen");
    let program = Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Name", ScalarType::String),
                SchemaNode::scalar("From", ScalarType::Int),
                SchemaNode::scalar("To", ScalarType::Int),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Value", ScalarType::Int),
                        SchemaNode::scalar("Position", ScalarType::Int),
                        SchemaNode::scalar("Parent", ScalarType::String),
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
                    path: vec!["From".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["To".into()],
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Position {
                    collection: Vec::new(),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Name".into()],
                },
            },
        ],
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
                iteration: Some(IterationPlan::generated(GeneratedSequence::Range {
                    from: Some(1),
                    to: 2,
                    item: 3,
                })),
                construction: TargetConstruction::Group,
                bindings: vec![
                    Binding {
                        target_field: "Value".into(),
                        expression: 3,
                        target_type: ScalarType::Int,
                        repeating: false,
                    },
                    Binding {
                        target_field: "Position".into(),
                        expression: 4,
                        target_type: ScalarType::Int,
                        repeating: false,
                    },
                    Binding {
                        target_field: "Parent".into(),
                        expression: 5,
                        target_type: ScalarType::String,
                        repeating: false,
                    },
                ],
                children: Vec::new(),
            }],
        },
        extra_targets: Vec::new(),
    };
    let artifacts = emit(
        &program,
        &Options {
            package_name: "generated-range-map".to_string(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    let Some(generated_source) = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
    else {
        panic!("generated Rust source artifact")
    };
    assert!(generated_source.contains("context.generated_items(&generated_items)"));
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, Value, field, group, repeated, scalar};

fn main() {
    let source = group([
        field("Name", scalar(Value::String("parent".into()))),
        field("From", scalar(Value::Int(2))),
        field("To", scalar(Value::Int(4))),
    ]);
    let expected = group([field("Rows", repeated([
        row(2, 1),
        row(3, 2),
        row(4, 3),
    ]))]);
    assert_eq!(generated_range_map::execute(&source).unwrap(), expected);

    let null_from = group([
        field("Name", scalar(Value::String("parent".into()))),
        field("From", scalar(Value::Null)),
    ]);
    assert_eq!(
        generated_range_map::execute(&null_from).unwrap(),
        group([field("Rows", repeated(Vec::<Instance>::new()))]),
    );
}

fn row(value: i64, position: i64) -> Instance {
    group([
        field("Value", scalar(Value::Int(value))),
        field("Position", scalar(Value::Int(position))),
        field("Parent", scalar(Value::String("parent".into()))),
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
        "generated Rust range project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

#[test]
fn generated_sequence_reducers_build_run_and_preserve_evaluation_order() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap();
    let output = TempDir::new("rust_generated_sequence_reducers");
    let program = Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Text", ScalarType::String),
                SchemaNode::scalar("Delimiter", ScalarType::String),
                SchemaNode::scalar("Index", ScalarType::Int),
                SchemaNode::scalar("FailIndex", ScalarType::Bool),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("Selected", ScalarType::String),
                SchemaNode::scalar("Exists", ScalarType::Bool),
            ],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Text".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Delimiter".into()],
                },
            },
            ExpressionNode {
                id: 3,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 4,
                expression: Expression::Const {
                    value: Value::String("hit".into()),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Call {
                    function: ScalarFunction::Equal,
                    args: vec![3, 4],
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::Const {
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::Const {
                    value: Value::Int(0),
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::Call {
                    function: ScalarFunction::Divide,
                    args: vec![6, 7],
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::If {
                    condition: 5,
                    then: 5,
                    else_: 8,
                },
            },
            ExpressionNode {
                id: 10,
                expression: Expression::SequenceExists {
                    sequence: GeneratedSequence::Tokenize {
                        input: 1,
                        delimiter: 2,
                        item: 3,
                    },
                    predicate: 9,
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::SourceField {
                    frame: None,
                    path: Vec::new(),
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Index".into()],
                },
            },
            ExpressionNode {
                id: 13,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["FailIndex".into()],
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::If {
                    condition: 13,
                    then: 8,
                    else_: 12,
                },
            },
            ExpressionNode {
                id: 15,
                expression: Expression::SequenceItemAt {
                    sequence: GeneratedSequence::Tokenize {
                        input: 1,
                        delimiter: 2,
                        item: 11,
                    },
                    index: 14,
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
                    target_field: "Selected".into(),
                    expression: 15,
                    target_type: ScalarType::String,
                    repeating: false,
                },
                Binding {
                    target_field: "Exists".into(),
                    expression: 10,
                    target_type: ScalarType::Bool,
                    repeating: false,
                },
            ],
            children: Vec::new(),
        },
        extra_targets: Vec::new(),
    };
    let artifacts = emit(
        &program,
        &Options {
            package_name: "generated-sequence-reducers".to_string(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    let Some(generated_source) = artifacts
        .files()
        .iter()
        .find(|file| file.path.as_str() == "src/lib.rs")
        .and_then(|file| std::str::from_utf8(&file.contents).ok())
    else {
        panic!("generated Rust source artifact")
    };
    assert!(generated_source.contains("context.generated_item_contexts(&generated_items)"));
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{
    FunctionError, Instance, RuntimeError, Value, field, group, scalar,
};

fn main() {
    let source = input(Value::String("hit,bad".into()), ",", 2, false);
    assert_eq!(
        generated_sequence_reducers::execute(&source).unwrap(),
        group([
            field("Selected", scalar(Value::String("bad".into()))),
            field("Exists", scalar(Value::Bool(true))),
        ]),
    );

    let empty = input(Value::Null, ",", 2, false);
    assert_eq!(
        generated_sequence_reducers::execute(&empty).unwrap(),
        group([
            field("Selected", scalar(Value::Null)),
            field("Exists", scalar(Value::Bool(false))),
        ]),
    );

    let failing_index = input(Value::Null, ",", 2, true);
    assert_eq!(
        generated_sequence_reducers::execute(&failing_index),
        Err(RuntimeError::Function(FunctionError::DivideByZero)),
    );
}

fn input(text: Value, delimiter: &str, index: i64, fail_index: bool) -> Instance {
    group([
        field("Text", scalar(text)),
        field("Delimiter", scalar(Value::String(delimiter.into()))),
        field("Index", scalar(Value::Int(index))),
        field("FailIndex", scalar(Value::Bool(fail_index))),
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
        "generated Rust reducer project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}

fn write_artifacts(directory: &Path, artifacts: &ArtifactSet) {
    for file in artifacts.files() {
        let path = directory.join(file.path.as_str());
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, &file.contents).unwrap();
    }
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or_default();
        let path =
            std::env::temp_dir().join(format!("ferrule_{tag}_{}_{}", std::process::id(), nonce));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
