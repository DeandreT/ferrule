use std::collections::BTreeMap;

use codegen::{
    InnerJoin, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, ProgramValidationError,
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
                        plan,
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
            ]),
            group([
                field("Sku", scalar(string("1"))),
                field("Price", scalar(Value::Int(20))),
                field("Label", scalar(string("second"))),
            ]),
            group([
                field("Sku", scalar(string("2"))),
                field("Price", scalar(Value::Int(5))),
                field("Label", scalar(string("third"))),
            ]),
            group([
                field("Sku", scalar(Value::Null)),
                field("Price", scalar(Value::Int(100))),
                field("Label", scalar(string("null"))),
            ]),
            group([
                field("Sku", scalar(Value::xml_nil())),
                field("Price", scalar(Value::Int(100))),
                field("Label", scalar(string("xml-nil"))),
            ]),
        ]),
    )])
}

#[test]
fn generated_correlated_join_aggregates_match_engine_and_retain_typed_failures() {
    let project = project();
    let input = source();
    let named = catalog();
    let expected =
        engine::run_with_sources(&project, &input, vec![("Catalog".into(), named.clone())])
            .expect("engine executes correlated join aggregates");
    let program = codegen::lower(&project).expect("correlated aggregates lower");
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
    .expect("correlated aggregate package emits");
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
        row([("Sku", Value::Int(1)), ("Price", Value::Int(10)), ("Label", string("first"))]),
        row([("Sku", string("1")), ("Price", Value::Int(20)), ("Label", string("second"))]),
        row([("Sku", string("2")), ("Price", Value::Int(5)), ("Label", string("third"))]),
        row([("Sku", Value::Null), ("Price", Value::Int(100)), ("Label", string("null"))]),
        row([("Sku", Value::xml_nil()), ("Price", Value::Int(100)), ("Label", string("xml-nil"))]),
    ]))]);
    let inputs = [NamedInput { name: "Catalog", instance: &catalog }];
    let output = correlated_join_map::execute_with_sources(&source, &inputs).unwrap();
    assert_eq!(format!("{output:?}"), std::env::var("EXPECTED_OUTPUT").unwrap());

    let malformed_catalog = group([field("Product", repeated([row([
        ("Sku", Value::Int(1)),
        ("Label", string("missing-price")),
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
