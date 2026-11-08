use super::{Collector, field_trace_project};
use crate::{ExecutionContext, TraceEvent, run_with_context};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, Graph, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, Node, Scope,
};
use std::error::Error;
use std::path::Path;

fn node_event_order(events: &[TraceEvent]) -> Vec<String> {
    events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeValue { node, value, .. } => {
                Some(format!("node {node}={}", value.preview))
            }
            TraceEvent::NodeInputValue {
                consumer,
                input,
                input_index,
                value,
                ..
            } => Some(format!(
                "input {consumer}:{input_index}<-{input}={}",
                value.preview
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn aggregate_inputs_keep_item_positions_and_parent_argument_order() -> Result<(), Box<dyn Error>> {
    let mut project = field_trace_project(
        SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Joined", ScalarType::String)],
        ),
        Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Separator".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: vec!["Row".into()],
                        value: Vec::new(),
                        expression: Some(0),
                        arg: Some(1),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            bindings: vec![Binding {
                target_field: "Joined".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    );
    project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Separator", ScalarType::String),
            SchemaNode::group("Row", vec![SchemaNode::scalar("Text", ScalarType::String)])
                .repeating(),
        ],
    );
    let row = |text: &str| {
        Instance::Group(
            (vec![("Text".into(), Instance::Scalar(Value::String(text.into())))]).into(),
        )
    };
    let source = Instance::Group(
        (vec![
            (
                "Separator".into(),
                Instance::Scalar(Value::String("|".into())),
            ),
            ("Row".into(), Instance::Repeated(vec![row("a"), row("b")])),
        ])
        .into(),
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
    let output = run_with_context(&project, &source, &execution)?;
    assert_eq!(
        output.field("Joined").and_then(Instance::as_scalar),
        Some(&Value::String("a|b".into()))
    );
    let events = collector.0.into_inner();
    assert_eq!(
        node_event_order(&events),
        [
            "node 0=a",
            "input 2:0<-0=a",
            "node 0=b",
            "input 2:0<-0=b",
            "node 1=|",
            "input 2:1<-1=|",
            "node 2=a|b",
        ]
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 2,
                    positions,
                    ..
                } => Some(
                    positions
                        .iter()
                        .map(|position| (position.collection.join("/"), position.index))
                        .collect::<Vec<_>>(),
                ),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [vec![("Row".into(), 1)], vec![("Row".into(), 2)], vec![]]
    );

    let Some(Node::Aggregate {
        value, expression, ..
    }) = project.graph.nodes.get_mut(&2)
    else {
        panic!("expected aggregate");
    };
    *value = vec!["Text".into()];
    *expression = None;
    let empty_source = Instance::Group(
        (vec![
            (
                "Separator".into(),
                Instance::Scalar(Value::String("|".into())),
            ),
            ("Row".into(), Instance::Repeated(Vec::new())),
        ])
        .into(),
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
    let output = run_with_context(&project, &empty_source, &execution)?;
    assert_eq!(
        output.field("Joined").and_then(Instance::as_scalar),
        Some(&Value::String(String::new()))
    );
    assert_eq!(
        node_event_order(&collector.0.into_inner()),
        ["node 1=|", "input 2:0<-1=|", "node 2="]
    );
    Ok(())
}

#[test]
fn join_aggregate_inputs_keep_tuple_positions_and_parent_argument() -> Result<(), Box<dyn Error>> {
    let join = JoinId::new(7);
    let plan = JoinPlan::new(
        JoinSource::new(vec!["A".into()]),
        JoinSource::new(vec!["B".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["A".into()],
            vec!["Id".into()],
            vec!["AId".into()],
        )),
    )?;
    let mut project = field_trace_project(
        SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Joined", ScalarType::String)],
        ),
        Graph {
            nodes: [
                (
                    0,
                    Node::JoinField {
                        join,
                        collection: vec!["A".into()],
                        path: vec!["Text".into()],
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Separator".into()],
                        frame: None,
                    },
                ),
                (
                    2,
                    Node::JoinAggregate {
                        function: AggregateOp::Join,
                        join,
                        plan,
                        expression: Some(0),
                        arg: Some(1),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            bindings: vec![Binding {
                target_field: "Joined".into(),
                node: 2,
            }],
            ..Scope::default()
        },
    );
    project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::scalar("Separator", ScalarType::String),
            SchemaNode::group(
                "A",
                vec![
                    SchemaNode::scalar("Id", ScalarType::Int),
                    SchemaNode::scalar("Text", ScalarType::String),
                ],
            )
            .repeating(),
            SchemaNode::group("B", vec![SchemaNode::scalar("AId", ScalarType::Int)]).repeating(),
        ],
    );
    let a = |text: &str| {
        Instance::Group(
            (vec![
                ("Id".into(), Instance::Scalar(Value::Int(1))),
                ("Text".into(), Instance::Scalar(Value::String(text.into()))),
            ])
            .into(),
        )
    };
    let b = || Instance::Group((vec![("AId".into(), Instance::Scalar(Value::Int(1)))]).into());
    let source = Instance::Group(
        (vec![
            (
                "Separator".into(),
                Instance::Scalar(Value::String("|".into())),
            ),
            ("A".into(), Instance::Repeated(vec![a("a"), a("b")])),
            ("B".into(), Instance::Repeated(vec![b(), b()])),
        ])
        .into(),
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
    let output = run_with_context(&project, &source, &execution)?;
    assert_eq!(
        output.field("Joined").and_then(Instance::as_scalar),
        Some(&Value::String("a|a|b|b".into()))
    );
    let events = collector.0.into_inner();
    assert_eq!(
        node_event_order(&events),
        [
            "node 0=a",
            "input 2:0<-0=a",
            "node 0=a",
            "input 2:0<-0=a",
            "node 0=b",
            "input 2:0<-0=b",
            "node 0=b",
            "input 2:0<-0=b",
            "node 1=|",
            "input 2:1<-1=|",
            "node 2=a|a|b|b",
        ]
    );
    let inputs = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeInputValue {
                consumer: 2,
                input: 0,
                positions,
                ..
            } => Some(positions),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        inputs
            .iter()
            .map(|positions| {
                (
                    positions
                        .iter()
                        .map(|position| position.index)
                        .collect::<Vec<_>>(),
                    positions.last().and_then(|position| position.join_position),
                )
            })
            .collect::<Vec<_>>(),
        [
            (vec![1, 1], Some((join, 1))),
            (vec![1, 2], Some((join, 2))),
            (vec![2, 1], Some((join, 3))),
            (vec![2, 2], Some((join, 4))),
        ]
    );
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 2,
            input: 1,
            positions,
            ..
        } if positions.is_empty()
    )));
    Ok(())
}

#[test]
fn failed_aggregate_expression_emits_no_input_or_parent_argument() {
    let mut project = field_trace_project(
        SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Joined", ScalarType::String)],
        ),
        Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        path: vec!["Fail".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Text".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    2,
                    Node::RuntimeParameter {
                        name: "missing".into(),
                        ty: ScalarType::String,
                        preview: None,
                    },
                ),
                (
                    3,
                    Node::If {
                        condition: 0,
                        then: 2,
                        else_: 1,
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::String("|".into()),
                    },
                ),
                (
                    5,
                    Node::Aggregate {
                        function: AggregateOp::Join,
                        collection: vec!["Row".into()],
                        value: Vec::new(),
                        expression: Some(3),
                        arg: Some(4),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            bindings: vec![Binding {
                target_field: "Joined".into(),
                node: 5,
            }],
            ..Scope::default()
        },
    );
    project.source = SchemaNode::group(
        "Input",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Fail", ScalarType::Bool),
                    SchemaNode::scalar("Text", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    let row = |fail, text: &str| {
        Instance::Group(
            (vec![
                ("Fail".into(), Instance::Scalar(Value::Bool(fail))),
                ("Text".into(), Instance::Scalar(Value::String(text.into()))),
            ])
            .into(),
        )
    };
    let source = Instance::Group(
        (vec![(
            "Row".into(),
            Instance::Repeated(vec![row(false, "a"), row(true, "b")]),
        )])
        .into(),
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);
    let error = run_with_context(&project, &source, &execution)
        .expect_err("second item needs an unavailable runtime parameter");
    assert!(matches!(
        error,
        crate::EngineError::MissingRuntimeParameter { node: 2, .. }
    ));
    assert_eq!(
        node_event_order(&collector.0.into_inner()),
        [
            "node 0=false",
            "input 3:0<-0=false",
            "node 1=a",
            "input 3:2<-1=a",
            "node 3=a",
            "input 5:0<-3=a",
            "node 0=true",
            "input 3:0<-0=true",
        ]
    );
}

#[test]
fn input_trace_records_only_the_taken_conditional_branch() -> Result<(), Box<dyn Error>> {
    let project = field_trace_project(
        SchemaNode::group(
            "Output",
            vec![SchemaNode::scalar("Result", ScalarType::String)],
        ),
        Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("é".repeat(200)),
                    },
                ),
                (
                    2,
                    Node::RuntimeParameter {
                        name: "unused".into(),
                        ty: ScalarType::String,
                        preview: None,
                    },
                ),
                (
                    3,
                    Node::If {
                        condition: 0,
                        then: 1,
                        else_: 2,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            bindings: vec![Binding {
                target_field: "Result".into(),
                node: 3,
            }],
            ..Scope::default()
        },
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)?;

    let events = collector.0.into_inner();
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 3,
                    input,
                    input_index,
                    value,
                    ..
                } => Some((
                    *input,
                    *input_index,
                    value.preview.chars().count(),
                    value.truncated,
                )),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(0, 0, 4, false), (1, 1, 160, true)]
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeValue { node: 2, .. }
            | TraceEvent::NodeInputValue {
                input: 2,
                consumer: 3,
                ..
            }
    )));
    Ok(())
}
