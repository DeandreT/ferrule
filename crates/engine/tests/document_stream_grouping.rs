use std::ops::ControlFlow;
use std::path::Path;

use engine::{DocumentVisit, ExecutionContext, InvocationCounterLimits, TargetSelection};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{AggregateOp, Binding, Graph, Node, Project, Scope, ScopeIteration};

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Key", ScalarType::String)])
                    .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::scalar("Value", ScalarType::Int),
                SchemaNode::scalar("Position", ScalarType::Int),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::SourceField {
                        path: vec!["Key".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Aggregate {
                        function: AggregateOp::LastPresentText,
                        collection: vec!["Rows".into()],
                        value: vec!["Key".into()],
                        expression: None,
                        arg: None,
                    },
                ),
                (
                    3,
                    Node::InvocationCounter {
                        key: "groups".into(),
                        start: 4,
                        step: 4,
                        restart: None,
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (5, Node::Position { collection: vec![] }),
                (
                    6,
                    Node::Const {
                        value: Value::String(".json".into()),
                    },
                ),
                (
                    7,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![2, 6],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec!["Rows".into()],
                output_path: 7,
            },
            group_by: Some(1),
            bindings: vec![
                Binding {
                    target_field: "Value".into(),
                    node: 3,
                },
                Binding {
                    target_field: "Position".into(),
                    node: 5,
                },
            ],
            ..Default::default()
        },
    }
}

fn source() -> Instance {
    Instance::Group(
        vec![(
            "Rows".into(),
            Instance::Repeated(
                ["A", "B", "A"]
                    .into_iter()
                    .map(|key| {
                        Instance::Group(
                            vec![("Key".into(), Instance::Scalar(Value::String(key.into())))]
                                .into(),
                        )
                    })
                    .collect(),
            ),
        )]
        .into(),
    )
}

fn context() -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("groups.ferrule")).with_invocation_counter_limits(
        InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 32,
        },
    )
}

#[test]
fn grouped_document_order_counter_and_position_match_materialized_execution() {
    let project = project();
    assert!(
        engine::validate(&project).is_empty(),
        "{:?}",
        engine::validate(&project)
    );
    let input = source();
    let materialized = engine::run_with_context(&project, &input, &context()).unwrap();
    let Instance::DocumentSet(expected) = materialized else {
        panic!("expected documents")
    };
    let mut actual = vec![];
    let result = engine::visit_target_documents(
        &project,
        &input,
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            actual.push(document);
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 2 });
    assert_eq!(actual, expected);
    assert_eq!(
        actual.iter().map(|doc| doc.path()).collect::<Vec<_>>(),
        ["A.json", "B.json"]
    );
    for (index, document) in actual.iter().enumerate() {
        assert_eq!(
            document
                .value()
                .field("Value")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(index as i64 + 1))
        );
        assert_eq!(
            document
                .value()
                .field("Position")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(index as i64 + 1))
        );
    }
}

#[test]
fn stopping_after_first_group_skips_the_later_group_body() {
    let mut project = project();
    project.graph.nodes.extend([
        (
            8,
            Node::Const {
                value: Value::String("B".into()),
            },
        ),
        (
            9,
            Node::Call {
                function: "equal".into(),
                args: vec![2, 8],
            },
        ),
        (10, Node::Raise { message: None }),
        (
            11,
            Node::If {
                condition: 9,
                then: 10,
                else_: 3,
            },
        ),
    ]);
    project.root.bindings[0].node = 11;
    let mut paths = vec![];
    let result = engine::visit_target_documents(
        &project,
        &source(),
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            paths.push(document.path().to_owned());
            ControlFlow::Break("done")
        },
    )
    .unwrap();
    assert_eq!(
        result,
        DocumentVisit::Stopped {
            accepted: 0,
            reason: "done"
        }
    );
    assert_eq!(paths, ["A.json"]);
}

#[test]
fn outer_document_survives_an_empty_inner_row_sequence() {
    let mut project = project();
    project.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    project.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    project.root.bindings.clear();
    project.root.children.push(Scope {
        target_field: "Rows".into(),
        iteration: ScopeIteration::Source(vec!["Rows".into()]),
        filter: Some(8),
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 3,
        }],
        ..Default::default()
    });
    let mut documents = vec![];
    let execution = ExecutionContext::new(Path::new("groups.ferrule"));
    let result = engine::visit_target_documents(
        &project,
        &source(),
        vec![],
        &execution,
        TargetSelection::Primary,
        |document| {
            documents.push(document);
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 2 });
    assert_eq!(
        documents.iter().map(|doc| doc.path()).collect::<Vec<_>>(),
        ["A.json", "B.json"]
    );
    assert!(
        documents
            .iter()
            .all(|doc| doc.value().field("Rows") == Some(&Instance::Repeated(vec![])))
    );
}
