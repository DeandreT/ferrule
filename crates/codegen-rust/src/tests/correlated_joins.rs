use std::collections::BTreeMap;

use codegen::{
    InnerJoin, IterationPlan, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource,
    ProgramValidationError,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding as MappingBinding, Graph, JoinConditions as MappingJoinConditions,
    JoinId as MappingJoinId, JoinKey as MappingJoinKey, JoinPlan as MappingJoinPlan,
    JoinSource as MappingJoinSource, NamedSource, Node, Project, Scope, ScopeIteration,
};

use super::*;

fn project() -> Project {
    let join = MappingJoinId::new(8);
    let plan = MappingJoinPlan::new(
        MappingJoinSource::singleton(vec!["Sku".into()]),
        MappingJoinSource::new(vec!["Catalog".into(), "Product".into()]),
        MappingJoinConditions::new(MappingJoinKey::new(
            vec!["Sku".into()],
            Vec::new(),
            vec!["Sku".into()],
        )),
    )
    .expect("correlated join plan");
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::group(
                    "Line",
                    vec![
                        SchemaNode::scalar("Sku", ScalarType::String),
                        SchemaNode::scalar("Quantity", ScalarType::Int),
                        SchemaNode::scalar("Separator", ScalarType::String),
                    ],
                )
                .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![
                        SchemaNode::scalar("Sku", ScalarType::String),
                        SchemaNode::scalar("Total", ScalarType::Int),
                        SchemaNode::scalar("Matches", ScalarType::Int),
                        SchemaNode::scalar("Labels", ScalarType::String),
                        SchemaNode::group(
                            "MatchedProduct",
                            vec![
                                SchemaNode::scalar("Label", ScalarType::String),
                                SchemaNode::scalar("Price", ScalarType::Int),
                                SchemaNode::scalar("JoinPosition", ScalarType::Int),
                                SchemaNode::scalar("ProductPosition", ScalarType::Int),
                                SchemaNode::scalar("OuterQuantity", ScalarType::Int),
                                SchemaNode::group(
                                    "Details",
                                    vec![SchemaNode::scalar("Summary", ScalarType::String)],
                                ),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![NamedSource {
            name: "Catalog".into(),
            path: "catalog.json".into(),
            schema: SchemaNode::group(
                "Catalog",
                vec![
                    SchemaNode::group(
                        "Product",
                        vec![
                            SchemaNode::scalar("Sku", ScalarType::String),
                            SchemaNode::scalar("Price", ScalarType::Int),
                            SchemaNode::scalar("Label", ScalarType::String),
                            SchemaNode::scalar("Rank", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            ),
            options: Default::default(),
            dynamic_path: None,
        }],
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: BTreeMap::new(),
        graph: Graph {
            nodes: BTreeMap::from([
                (
                    1,
                    Node::SourceField {
                        frame: Some(vec!["Line".into()]),
                        path: vec!["Quantity".into()],
                    },
                ),
                (
                    2,
                    Node::JoinField {
                        join,
                        collection: vec!["Catalog".into(), "Product".into()],
                        path: vec!["Price".into()],
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "multiply".into(),
                        args: vec![1, 2],
                    },
                ),
                (
                    4,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Sum,
                        join,
                        plan: plan.clone(),
                        expression: Some(3),
                        arg: None,
                    },
                ),
                (
                    5,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Count,
                        join,
                        plan: plan.clone(),
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    6,
                    Node::JoinField {
                        join,
                        collection: vec!["Catalog".into(), "Product".into()],
                        path: vec!["Label".into()],
                    },
                ),
                (
                    7,
                    Node::SourceField {
                        frame: Some(vec!["Line".into()]),
                        path: vec!["Separator".into()],
                    },
                ),
                (
                    8,
                    Node::JoinAggregate {
                        function: mapping::AggregateOp::Join,
                        join,
                        plan: plan.clone(),
                        expression: Some(6),
                        arg: Some(7),
                    },
                ),
                (
                    9,
                    Node::SourceField {
                        frame: Some(vec!["Line".into()]),
                        path: vec!["Sku".into()],
                    },
                ),
                (
                    10,
                    Node::JoinField {
                        join,
                        collection: vec!["Catalog".into(), "Product".into()],
                        path: vec!["Rank".into()],
                    },
                ),
                (
                    11,
                    Node::Const {
                        value: Value::Int(9),
                    },
                ),
                (
                    12,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![10, 11],
                    },
                ),
                (13, Node::JoinPosition { join }),
                (
                    14,
                    Node::Position {
                        collection: vec!["Catalog".into(), "Product".into()],
                    },
                ),
                (
                    15,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![6, 7, 9],
                    },
                ),
                (
                    16,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
            ]),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Line".into()]),
                bindings: vec![
                    MappingBinding {
                        target_field: "Sku".into(),
                        node: 9,
                    },
                    MappingBinding {
                        target_field: "Total".into(),
                        node: 4,
                    },
                    MappingBinding {
                        target_field: "Matches".into(),
                        node: 5,
                    },
                    MappingBinding {
                        target_field: "Labels".into(),
                        node: 8,
                    },
                ],
                children: vec![Scope {
                    target_field: "MatchedProduct".into(),
                    iteration: ScopeIteration::InnerJoin { id: join, plan },
                    filter: Some(12),
                    sort_by: Some(10),
                    sort_descending: true,
                    windows: vec![mapping::SequenceWindow::First { count: 16 }],
                    bindings: vec![
                        MappingBinding {
                            target_field: "Label".into(),
                            node: 6,
                        },
                        MappingBinding {
                            target_field: "Price".into(),
                            node: 2,
                        },
                        MappingBinding {
                            target_field: "JoinPosition".into(),
                            node: 13,
                        },
                        MappingBinding {
                            target_field: "ProductPosition".into(),
                            node: 14,
                        },
                        MappingBinding {
                            target_field: "OuterQuantity".into(),
                            node: 1,
                        },
                    ],
                    children: vec![Scope {
                        target_field: "Details".into(),
                        bindings: vec![MappingBinding {
                            target_field: "Summary".into(),
                            node: 15,
                        }],
                        ..Scope::default()
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    }
}

fn field(name: &str, value: Instance) -> (String, Instance) {
    (name.into(), value)
}

fn group(fields: impl IntoIterator<Item = (String, Instance)>) -> Instance {
    Instance::Group(fields.into_iter().collect())
}

fn repeated(items: impl IntoIterator<Item = Instance>) -> Instance {
    Instance::Repeated(items.into_iter().collect())
}

fn scalar(value: Value) -> Instance {
    Instance::Scalar(value)
}

fn string(value: &str) -> Value {
    Value::String(value.into())
}

fn source() -> Instance {
    group([field(
        "Line",
        repeated([
            group([
                field("Sku", scalar(string("1"))),
                field("Quantity", scalar(Value::Int(2))),
                field("Separator", scalar(string("|"))),
            ]),
            group([
                field("Sku", scalar(string("2"))),
                field("Quantity", scalar(Value::Int(3))),
                field("Separator", scalar(string("/"))),
            ]),
            group([
                field("Sku", scalar(Value::Null)),
                field("Quantity", scalar(Value::Int(4))),
                field("Separator", scalar(string("-"))),
            ]),
            group([
                field("Sku", scalar(Value::xml_nil())),
                field("Quantity", scalar(Value::Int(5))),
                field("Separator", scalar(string("-"))),
            ]),
            group([
                field("Sku", scalar(string("9"))),
                field("Quantity", scalar(Value::Int(6))),
                field("Separator", scalar(string("-"))),
            ]),
        ]),
    )])
}

fn catalog() -> Instance {
    group([field(
        "Product",
        repeated([
            group([
                field("Sku", scalar(Value::Int(1))),
                field("Price", scalar(Value::Int(10))),
                field("Label", scalar(string("first"))),
                field("Rank", scalar(Value::Int(10))),
            ]),
            group([
                field("Sku", scalar(string("1"))),
                field("Price", scalar(Value::Int(20))),
                field("Label", scalar(string("second"))),
                field("Rank", scalar(Value::Int(30))),
            ]),
            group([
                field("Sku", scalar(string("2"))),
                field("Price", scalar(Value::Int(5))),
                field("Label", scalar(string("third"))),
                field("Rank", scalar(Value::Int(5))),
            ]),
            group([
                field("Sku", scalar(Value::Null)),
                field("Price", scalar(Value::Int(100))),
                field("Label", scalar(string("null"))),
                field("Rank", scalar(Value::Int(99))),
            ]),
            group([
                field("Sku", scalar(Value::xml_nil())),
                field("Price", scalar(Value::Int(100))),
                field("Label", scalar(string("xml-nil"))),
                field("Rank", scalar(Value::Int(99))),
            ]),
        ]),
    )])
}

#[test]
fn generated_correlated_joins_match_engine_and_retain_typed_failures() {
    let project = project();
    let input = source();
    let named = catalog();
    let expected =
        engine::run_with_sources(&project, &input, vec![("Catalog".into(), named.clone())])
            .expect("engine executes correlated joins");
    let program = codegen::lower(&project).expect("correlated joins lower");
    let runtime_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../codegen-runtime")
        .canonicalize()
        .expect("runtime path resolves");
    let artifacts = emit(
        &program,
        &Options {
            package_name: "correlated-join-map".into(),
            runtime_dependency: RuntimeDependency::Path(
                runtime_path.to_string_lossy().into_owned(),
            ),
        },
    )
    .expect("correlated join package emits");
    let output = TempDir::new("rust_correlated_join_codegen");
    write_artifacts(output.path(), &artifacts);
    fs::write(
        output.path().join("src/main.rs"),
        r#"use codegen_runtime::{Instance, NamedInput, RuntimeError, SourcePathError, Value, field, group, repeated, scalar, string};

fn row(fields: impl IntoIterator<Item = (&'static str, Value)>) -> Instance {
    group(fields.into_iter().map(|(name, value)| field(name, scalar(value))))
}

fn main() {
    let source = group([field("Line", repeated([
        row([("Sku", string("1")), ("Quantity", Value::Int(2)), ("Separator", string("|"))]),
        row([("Sku", string("2")), ("Quantity", Value::Int(3)), ("Separator", string("/"))]),
        row([("Sku", Value::Null), ("Quantity", Value::Int(4)), ("Separator", string("-"))]),
        row([("Sku", Value::xml_nil()), ("Quantity", Value::Int(5)), ("Separator", string("-"))]),
        row([("Sku", string("9")), ("Quantity", Value::Int(6)), ("Separator", string("-"))]),
    ]))]);
    let catalog = group([field("Product", repeated([
        row([("Sku", Value::Int(1)), ("Price", Value::Int(10)), ("Label", string("first")), ("Rank", Value::Int(10))]),
        row([("Sku", string("1")), ("Price", Value::Int(20)), ("Label", string("second")), ("Rank", Value::Int(30))]),
        row([("Sku", string("2")), ("Price", Value::Int(5)), ("Label", string("third")), ("Rank", Value::Int(5))]),
        row([("Sku", Value::Null), ("Price", Value::Int(100)), ("Label", string("null")), ("Rank", Value::Int(99))]),
        row([("Sku", Value::xml_nil()), ("Price", Value::Int(100)), ("Label", string("xml-nil")), ("Rank", Value::Int(99))]),
    ]))]);
    let inputs = [NamedInput { name: "Catalog", instance: &catalog }];
    let output = correlated_join_map::execute_with_sources(&source, &inputs).unwrap();
    assert_eq!(format!("{output:?}"), std::env::var("EXPECTED_OUTPUT").unwrap());

    let malformed_catalog = group([field("Product", repeated([row([
        ("Sku", Value::Int(1)),
        ("Price", Value::Int(10)),
        ("Label", string("missing-rank")),
    ])]))]);
    let malformed_inputs = [NamedInput { name: "Catalog", instance: &malformed_catalog }];
    assert!(matches!(
        correlated_join_map::execute_with_sources(&source, &malformed_inputs),
        Err(RuntimeError::SourcePath(SourcePathError::MissingJoinField {
            join: 8,
            ..
        }))
    ));
}
"#,
    )
    .expect("generated harness is written");
    let run = Command::new("cargo")
        .args(["run", "--quiet"])
        .env("EXPECTED_OUTPUT", format!("{expected:?}"))
        .current_dir(output.path())
        .output()
        .expect("generated package starts");
    assert!(
        run.status.success(),
        "generated correlated join package failed:\n{}\n{}",
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

#[test]
fn rejects_unbounded_correlated_join_aggregate_before_artifact_creation() {
    let mut program = codegen::lower(&project()).expect("fixture lowers");
    let Some(expression) = program
        .expressions
        .iter_mut()
        .find(|expression| expression.id == 4)
    else {
        panic!("fixture contains correlated sum");
    };
    let Expression::JoinAggregate { join, .. } = &mut expression.expression else {
        panic!("fixture contains correlated sum");
    };
    *join = InnerJoin::new(
        JoinId::new(8),
        JoinPlan::new(
            JoinSource::new(vec!["Line".into()]),
            JoinSource::new(vec!["Catalog".into(), "Product".into()]),
            JoinConditions::new(JoinKey::new(
                vec!["Line".into()],
                vec!["Sku".into()],
                vec!["Sku".into()],
            )),
        )
        .expect("unbounded plan remains structurally valid"),
    );

    assert!(matches!(
        emit(
            &program,
            &Options {
                package_name: "invalid-correlated-join".into(),
                runtime_dependency: RuntimeDependency::Version("1".into()),
            }
        ),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::JoinAggregateRequiresRootContext {
                node: 4,
                join,
            }
        )) if join == JoinId::new(8)
    ));
}

#[test]
fn rejects_unbounded_correlated_join_scope_before_artifact_creation() {
    let mut program = codegen::lower(&project()).expect("fixture lowers");
    let Some(iteration) = program.root.children[0].children[0].iteration.as_mut() else {
        panic!("fixture contains correlated join scope");
    };
    let filter = iteration.filter();
    let sort = iteration.sort().cloned();
    let windows = iteration.windows().to_vec();
    let output = iteration.output();
    *iteration = IterationPlan::new(
        InnerJoin::new(
            JoinId::new(8),
            JoinPlan::new(
                JoinSource::new(vec!["Line".into()]),
                JoinSource::new(vec!["Catalog".into(), "Product".into()]),
                JoinConditions::new(JoinKey::new(
                    vec!["Line".into()],
                    vec!["Sku".into()],
                    vec!["Sku".into()],
                )),
            )
            .expect("unbounded plan remains structurally valid"),
        ),
        filter,
        sort,
        windows,
        output,
    );

    assert!(matches!(
        emit(
            &program,
            &Options {
                package_name: "invalid-correlated-join-scope".into(),
                runtime_dependency: RuntimeDependency::Version("1".into()),
            }
        ),
        Err(EmitError::InvalidProgram(
            ProgramValidationError::JoinRequiresRootContext {
                target_path,
                join,
            }
        )) if target_path == ["Row", "MatchedProduct"] && join == JoinId::new(8)
    ));
}
