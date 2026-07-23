use super::*;

fn failure_program() -> Program {
    Program {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Code", ScalarType::String),
                        SchemaNode::scalar("Valid", ScalarType::Bool),
                        SchemaNode::scalar("Message", ScalarType::String),
                    ],
                )
                .repeating(),
                SchemaNode::scalar("Tokens", ScalarType::String),
                SchemaNode::scalar("Pattern", ScalarType::String),
                SchemaNode::scalar("Stop", ScalarType::Bool),
                SchemaNode::scalar("TargetExplodes", ScalarType::Int),
            ],
        ),
        extra_sources: Vec::new(),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Status", ScalarType::String)],
        ),
        expressions: vec![
            ExpressionNode {
                id: 1,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Valid".into()],
                },
            },
            ExpressionNode {
                id: 2,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Message".into()],
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
                    value: Value::Int(1),
                },
            },
            ExpressionNode {
                id: 5,
                expression: Expression::Call {
                    function: ScalarFunction::GreaterThan,
                    args: vec![12, 4],
                },
            },
            ExpressionNode {
                id: 6,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Stop".into()],
                },
            },
            ExpressionNode {
                id: 7,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["TargetExplodes".into()],
                },
            },
            ExpressionNode {
                id: 8,
                expression: Expression::Call {
                    function: ScalarFunction::NormalizeSpace,
                    args: vec![7],
                },
            },
            ExpressionNode {
                id: 9,
                expression: Expression::Const {
                    value: Value::String("ok".into()),
                },
            },
            ExpressionNode {
                id: 10,
                expression: Expression::RuntimeValue {
                    value: codegen::RuntimeValue::MappingFilePath,
                },
            },
            ExpressionNode {
                id: 11,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Tokens".into()],
                },
            },
            ExpressionNode {
                id: 12,
                expression: Expression::Position {
                    collection: Vec::new(),
                },
            },
            ExpressionNode {
                id: 13,
                expression: Expression::Call {
                    function: ScalarFunction::NormalizeSpace,
                    args: vec![2],
                },
            },
            ExpressionNode {
                id: 14,
                expression: Expression::SourceField {
                    frame: None,
                    path: vec!["Pattern".into()],
                },
            },
        ],
        user_functions: Vec::new(),
        failure_rules: vec![
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(vec!["Rows".into()])),
                selection: FailureSelection::WhenFalse(1),
                message: Some(13),
            },
            FailureRule {
                iteration: FailureIteration::Generated(GeneratedSequence::TokenizeRegex {
                    input: 11,
                    pattern: 14,
                    flags: None,
                    item: 3,
                }),
                selection: FailureSelection::WhenTrue(5),
                message: Some(3),
            },
            FailureRule {
                iteration: FailureIteration::Source(SourceIteration::new(Vec::new())),
                selection: FailureSelection::WhenTrue(6),
                message: Some(10),
            },
        ],
        root: TargetScope {
            target_field: String::new(),
            repeating: false,
            iteration: None,
            construction: TargetConstruction::Group,
            bindings: vec![Binding {
                target_field: "Status".into(),
                expression: 9,
                target_type: ScalarType::String,
                repeating: false,
            }],
            children: Vec::new(),
        },
        extra_targets: vec![NamedTargetProgram {
            name: "late".into(),
            target: SchemaNode::group(
                "Late",
                vec![SchemaNode::scalar("Value", ScalarType::String)],
            ),
            root: TargetScope {
                target_field: String::new(),
                repeating: false,
                iteration: None,
                construction: TargetConstruction::Group,
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    expression: 8,
                    target_type: ScalarType::String,
                    repeating: false,
                }],
                children: Vec::new(),
            },
        }],
    }
}

#[test]
fn emits_ordered_lazy_failures_before_every_target() {
    let mut program = failure_program();
    program.extra_sources.push(NamedSourceProgram {
        name: "Catalog".into(),
        source: SchemaNode::group(
            "CatalogRow",
            vec![
                SchemaNode::scalar("Code", ScalarType::String),
                SchemaNode::scalar("Valid", ScalarType::Bool),
            ],
        )
        .repeating(),
    });
    program.failure_rules.insert(
        1,
        FailureRule {
            iteration: FailureIteration::Source(SourceIteration::new(vec!["Catalog".into()])),
            selection: FailureSelection::All,
            message: None,
        },
    );
    let artifacts = emit(
        &program,
        &Options {
            package_name: "failure-map".into(),
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

    let failure_call = source.find("evaluate_failure_rules(context)?;").unwrap();
    let primary = source.find("let primary = scope_root(context)?;").unwrap();
    assert!(failure_call < primary);
    assert!(source.contains("context.walk_source(&[\"Rows\"]);"));
    assert!(source.contains("context.walk_source(&[\"Catalog\"]);"));
    assert!(source.contains("let selected = !require_bool(1, predicate)?;"));
    assert!(source.contains("let message = Some(expression_13(&item_context)?);"));
    assert!(source.contains("return Err(codegen_runtime::mapping_failure(1, message));"));
    let sequence_input = source
        .find("let sequence_input = expression_11(context)?;")
        .unwrap();
    let sequence_pattern = source
        .find("let sequence_pattern = expression_14(context)?;")
        .unwrap();
    let materialize = source.find("tokenize_regex(sequence_input").unwrap();
    let iterate = source
        .find("context.generated_items(&generated_items)")
        .unwrap();
    assert!(sequence_input < sequence_pattern);
    assert!(sequence_pattern < materialize);
    assert!(materialize < iterate);
    let first = source.find("evaluate_failure_rule_0(context)?;").unwrap();
    let second = source.find("evaluate_failure_rule_1(context)?;").unwrap();
    assert!(first < second);
}

#[test]
fn generated_failure_project_builds_and_preserves_pre_target_semantics() {
    let runtime = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .map(|parent| parent.join("codegen-runtime"))
        .unwrap();
    let output = TempDir::new("rust_failure_codegen");
    let artifacts = emit(
        &failure_program(),
        &Options {
            package_name: "failure-map".into(),
            runtime_dependency: RuntimeDependency::Path(runtime.display().to_string()),
        },
    )
    .unwrap();
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use std::fmt::Debug;
use std::path::Path;

use codegen_runtime::{
    ExecutionContext, FunctionError, Instance, RuntimeError, RuntimeValue, Value, field, group,
    repeated, scalar,
};

fn main() {
    let execution = ExecutionContext::new(Path::new("active.ferrule"));
    let invalid = source(&[("A", Value::Bool(true)), ("B", Value::Bool(false))], 3, false);
    assert_failure(failure_map::execute(&invalid), 1, Some("B"));
    assert_failure(failure_map::execute_outputs(&invalid), 1, Some("B"));
    assert_failure(failure_map::execute_with_context(&invalid, &execution), 1, Some("B"));
    assert_failure(
        failure_map::execute_outputs_with_context(&invalid, &execution),
        1,
        Some("B"),
    );
    assert_failure(failure_map::execute_with_sources(&invalid, &[]), 1, Some("B"));
    assert_failure(
        failure_map::execute_outputs_with_sources(&invalid, &[]),
        1,
        Some("B"),
    );
    assert_failure(
        failure_map::execute_with_sources_and_context(&invalid, &[], &execution),
        1,
        Some("B"),
    );
    assert_failure(
        failure_map::execute_outputs_with_sources_and_context(&invalid, &[], &execution),
        1,
        Some("B"),
    );

    let generated = source(&[("A", Value::Bool(true))], 3, false);
    assert_failure(failure_map::execute(&generated), 2, Some("bad"));

    let invalid_pattern = source_with_sequence(
        &[("A", Value::Bool(true))],
        Value::String("skip,bad".into()),
        Value::String("(".into()),
        false,
    );
    assert!(matches!(
        failure_map::execute(&invalid_pattern),
        Err(RuntimeError::InvalidTokenizeRegex { .. })
    ));

    let oversized_pattern = source_with_sequence(
        &[("A", Value::Bool(true))],
        Value::String("skip,bad".into()),
        Value::String("a".repeat(65_537)),
        false,
    );
    assert!(matches!(
        failure_map::execute(&oversized_pattern),
        Err(RuntimeError::TokenizeRegexPatternTooLarge {
            bytes: 65_537,
            max: 65_536,
        })
    ));

    let null_input = source_with_sequence(
        &[("A", Value::Bool(true))],
        Value::Null,
        Value::String("(".into()),
        false,
    );
    assert!(matches!(
        failure_map::execute(&null_input),
        Err(RuntimeError::Function(FunctionError::TypeMismatch { .. }))
    ));

    let runtime_message = source(&[("A", Value::Bool(true))], 1, true);
    assert_eq!(
        failure_map::execute(&runtime_message),
        Err(RuntimeError::MissingRuntimeValue {
            value: RuntimeValue::MappingFilePath,
        }),
    );
    assert_failure(
        failure_map::execute_with_context(&runtime_message, &execution),
        3,
        Some("active.ferrule"),
    );

    let not_bool = source(&[("A", Value::String("not-bool".into()))], 1, false);
    assert_eq!(
        failure_map::execute(&not_bool),
        Err(RuntimeError::NotABool {
            node: 1,
            found: "string",
        }),
    );

    let no_failure = source(&[("A", Value::Bool(true))], 1, false);
    assert!(matches!(
        failure_map::execute(&no_failure),
        Err(RuntimeError::Function(FunctionError::TypeMismatch { .. }))
    ));
}

fn assert_failure<T: Debug>(result: Result<T, RuntimeError>, rule: usize, message: Option<&str>) {
    assert_eq!(
        result.unwrap_err(),
        RuntimeError::MappingFailure {
            rule,
            message: message.map(str::to_string),
        },
    );
}

fn source(rows: &[(&str, Value)], candidate_count: i64, stop: bool) -> Instance {
    source_with_sequence(
        rows,
        Value::String(if candidate_count > 1 {
            "skip,bad,later".into()
        } else {
            "only".into()
        }),
        Value::String(",".into()),
        stop,
    )
}

fn source_with_sequence(
    rows: &[(&str, Value)],
    tokens: Value,
    pattern: Value,
    stop: bool,
) -> Instance {
    group([
        field(
            "Rows",
            repeated(rows.iter().map(|(code, valid)| {
                group([
                    field("Code", scalar(Value::String((*code).into()))),
                    field("Valid", scalar(valid.clone())),
                    field(
                        "Message",
                        scalar(if matches!(valid, Value::Bool(true)) {
                            Value::Int(7)
                        } else {
                            Value::String((*code).into())
                        }),
                    ),
                ])
            })),
        ),
        field("Tokens", scalar(tokens)),
        field("Pattern", scalar(pattern)),
        field("Stop", scalar(Value::Bool(stop))),
        field("TargetExplodes", scalar(Value::Int(7))),
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
        "generated Rust failure project failed:\n{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
