use std::cell::RefCell;
use std::error::Error;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, DynamicBinding, DynamicChild, Graph, JoinConditions, JoinId, JoinKey,
    JoinPlan, JoinSource, Node, Project, Scope, ScopeIteration, SequenceExpr, SequenceWindow,
    SortFilterOrder,
};

use crate::{ExecutionContext, TraceEvent, TraceSink, run_with_context};

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

#[test]
fn control_value_previews_are_unicode_safe_and_bounded() {
    let value = Value::String("é".repeat(200));
    let preview = crate::TraceValue::new(&value);

    assert_eq!(preview.value_type, "string");
    assert_eq!(preview.preview.chars().count(), 160);
    assert!(preview.truncated);
    assert_ne!(
        crate::TraceScope::primary(),
        crate::TraceScope::named("primary")
    );
}

#[test]
fn trace_records_post_order_values_with_iteration_positions() -> Result<(), Box<dyn Error>> {
    let project = Project {
        source: SchemaNode::group(
            "Input",
            vec![
                SchemaNode::group("Row", vec![SchemaNode::scalar("Value", ScalarType::String)])
                    .repeating(),
            ],
        ),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group(
                    "Row",
                    vec![SchemaNode::scalar("Result", ScalarType::String)],
                )
                .repeating(),
            ],
        ),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::SourceField {
                        path: vec!["Value".into()],
                        frame: Some(vec!["Row".into()]),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("!".into()),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 1],
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Row".into(),
                iteration: ScopeIteration::Source(vec!["Row".into()]),
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    };
    let source = Instance::Group(
        (vec![(
            "Row".into(),
            Instance::Repeated(
                ["first", "second"]
                    .into_iter()
                    .map(|value| {
                        Instance::Group(
                            (vec![(
                                "Value".into(),
                                Instance::Scalar(Value::String(value.into())),
                            )])
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )])
        .into(),
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    let output = run_with_context(&project, &source, &execution)?;

    assert_eq!(
        output
            .field("Row")
            .and_then(Instance::as_repeated)
            .map(<[_]>::len),
        Some(2)
    );
    let events = collector.0.into_inner();
    let nodes = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeValue { node, .. } => Some(*node),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(nodes, vec![0, 1, 2, 0, 1, 2]);
    let connector_events = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeValue { node, .. } => Some(format!("output {node}")),
            TraceEvent::NodeInputValue {
                consumer,
                input,
                input_index,
                ..
            } => Some(format!("input {consumer}:{input_index}<-{input}")),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        connector_events,
        [
            "output 0",
            "input 2:0<-0",
            "output 1",
            "input 2:1<-1",
            "output 2",
            "output 0",
            "input 2:0<-0",
            "output 1",
            "input 2:1<-1",
            "output 2",
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
                } => positions.last().map(|position| position.index),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [1, 1, 2, 2]
    );
    let positions = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeValue {
                node: 2, positions, ..
            } => positions.last(),
            _ => None,
        })
        .map(|position| (position.collection.clone(), position.index))
        .collect::<Vec<_>>();
    assert_eq!(
        positions,
        vec![(vec!["Row".into()], 1), (vec!["Row".into()], 2)]
    );
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::ScopeStarted {
            scope,
            iteration: crate::TraceIteration::Source { path },
            ..
        } if scope.target == crate::TraceTarget::Primary
            && scope.target_path == ["Row"]
            && scope.structural_path == [0]
            && path.as_slice() == ["Row"]
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, TraceEvent::IterationCandidate { .. }))
            .count(),
        3
    );
    Ok(())
}

#[test]
fn trace_records_generated_control_decisions_without_target_instances() -> Result<(), Box<dyn Error>>
{
    let project = Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![
                SchemaNode::group("Rows", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                    .repeating(),
            ],
        ),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(4),
                    },
                ),
                (
                    2,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "greater_than".into(),
                        args: vec![2, 0],
                    },
                ),
                (
                    4,
                    Node::Const {
                        value: Value::Int(2),
                    },
                ),
                (
                    5,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::Sequence(SequenceExpr::Generate {
                    from: Some(0),
                    to: 1,
                    item: 2,
                }),
                filter: Some(3),
                sort_by: Some(2),
                sort_descending: true,
                sort_then_by: Vec::new(),
                sort_filter_order: SortFilterOrder::FilterThenSort,
                group_into_blocks: Some(4),
                windows: vec![SequenceWindow::First { count: 5 }],
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    };
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)?;

    let events = collector.0.into_inner();
    let control_scope = |scope: &crate::TraceScope| {
        scope.target == crate::TraceTarget::Primary
            && scope.target_path == ["Rows"]
            && scope.structural_path == [0]
    };
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::ScopeStarted {
            scope,
            iteration: crate::TraceIteration::Generated {
                kind: "integer-range"
            },
            ..
        } if control_scope(scope)
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                TraceEvent::IterationCandidate { scope, .. } if control_scope(scope)
            ))
            .count(),
        4
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                TraceEvent::FilterDecision {
                    scope,
                    phase: crate::TraceFilterPhase::BeforeSort,
                    passed,
                    ..
                } if control_scope(scope) => Some(*passed),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [false, true, true, true]
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                TraceEvent::SortCandidate { scope, .. } if control_scope(scope)
            ))
            .count(),
        3
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                TraceEvent::SortPosition {
                    scope,
                    positions,
                    output_index,
                } if control_scope(scope) => positions
                    .last()
                    .map(|position| (position.index, *output_index)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(4, 1), (3, 2), (2, 3)]
    );
    assert_eq!(
        events
            .iter()
            .filter_map(|event| match event {
                TraceEvent::GroupProduced {
                    scope,
                    member_count,
                    ..
                } if control_scope(scope) => Some(*member_count),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [2, 1]
    );
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::WindowApplied {
            scope,
            window: crate::TraceWindow::First(1),
            before: 2,
            after: 1,
            ..
        } if control_scope(scope)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::TargetProduced {
            scope,
            kind: crate::TraceOutputKind::Group,
            ..
        } if control_scope(scope)
    )));
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::ScopeFinished {
            scope,
            candidates: 4,
            produced: 1,
            kind: crate::TraceOutputKind::Repeated,
        } if control_scope(scope)
    )));
    Ok(())
}

#[test]
fn trace_identifies_join_candidates_and_their_tuple_positions() -> Result<(), Box<dyn Error>> {
    let join = JoinId::new(12);
    let plan = JoinPlan::new(
        JoinSource::new(vec!["Left".into()]),
        JoinSource::new(vec!["Right".into()]),
        JoinConditions::new(JoinKey::new(
            vec!["Left".into()],
            vec!["Id".into()],
            vec!["LeftId".into()],
        )),
    )?;
    let project = Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: SchemaNode::group(
            "Output",
            vec![SchemaNode::group("Rows", Vec::new()).repeating()],
        ),
        graph: Graph::default(),
        root: Scope {
            children: vec![Scope {
                target_field: "Rows".into(),
                iteration: ScopeIteration::InnerJoin { id: join, plan },
                ..Scope::default()
            }],
            ..Scope::default()
        },
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    };
    let row = |name: &str, value: i64| {
        Instance::Group((vec![(name.into(), Instance::Scalar(Value::Int(value)))]).into())
    };
    let source = Instance::Group(
        (vec![
            (
                "Left".into(),
                Instance::Repeated(vec![row("Id", 1), row("Id", 2)]),
            ),
            (
                "Right".into(),
                Instance::Repeated(vec![row("LeftId", 2), row("LeftId", 1)]),
            ),
        ])
        .into(),
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &source, &execution)?;

    let events = collector.0.into_inner();
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::ScopeStarted {
            iteration: crate::TraceIteration::Join { join: event_join },
            ..
        } if *event_join == join
    )));
    let tuple_positions = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::IterationCandidate { positions, .. } => {
                positions.iter().find_map(|position| position.join_position)
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(tuple_positions, [(join, 1), (join, 2)]);
    Ok(())
}

fn field_trace_project(target: SchemaNode, graph: Graph, root: Scope) -> Project {
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target,
        graph,
        root,
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
    }
}

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

#[test]
fn target_field_trace_preserves_depth_first_write_order_and_scalar_states()
-> Result<(), Box<dyn Error>> {
    let target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("Missing", ScalarType::String),
            SchemaNode::scalar("Nil", ScalarType::String).nillable(),
            SchemaNode::group(
                "Child",
                vec![SchemaNode::scalar("Leaf", ScalarType::String)],
            ),
        ],
    );
    let project = field_trace_project(
        target,
        Graph {
            nodes: [
                (0, Node::Const { value: Value::Null }),
                (
                    1,
                    Node::Const {
                        value: Value::xml_nil(),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("leaf".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            bindings: vec![
                Binding {
                    target_field: "Missing".into(),
                    node: 0,
                },
                Binding {
                    target_field: "Nil".into(),
                    node: 1,
                },
            ],
            children: vec![Scope {
                target_field: "Child".into(),
                bindings: vec![Binding {
                    target_field: "Leaf".into(),
                    node: 2,
                }],
                ..Scope::default()
            }],
            ..Scope::default()
        },
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)?;

    let writes = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten {
                scope,
                field,
                binding,
                kind,
                value,
                ..
            } => Some((scope.target_path, field, binding, kind, value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 4);
    assert_eq!(
        writes
            .iter()
            .map(|(_, field, _, _, _)| field.as_str())
            .collect::<Vec<_>>(),
        ["Missing", "Nil", "Leaf", "Child"]
    );
    assert_eq!(
        writes[0].4.as_ref().map(|value| value.value_type),
        Some("null")
    );
    assert_eq!(
        writes[1].4.as_ref().map(|value| value.value_type),
        Some("xml nil")
    );
    assert_eq!(writes[2].0, ["Child"]);
    assert_eq!(
        writes[2].2,
        crate::TraceTargetFieldBinding::StaticBinding { value: 2 }
    );
    assert_eq!(writes[3].2, crate::TraceTargetFieldBinding::StaticChild);
    assert_eq!(writes[3].3, crate::TraceOutputKind::Group);
    assert!(writes[3].4.is_none());
    Ok(())
}

#[test]
fn target_field_trace_bounds_dynamic_keys_and_omits_rejected_writes() -> Result<(), Box<dyn Error>>
{
    let dynamic_field = SchemaNode::scalar("*", ScalarType::String);
    let target = SchemaNode::group("Output", Vec::new())
        .with_dynamic_fields(dynamic_field)
        .ok_or_else(|| std::io::Error::other("dynamic scalar target should be valid"))?;
    let key = "é".repeat(200);
    let project = field_trace_project(
        target,
        Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String(key),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("first".into()),
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::String("rejected".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            dynamic_bindings: vec![
                DynamicBinding { key: 0, value: 1 },
                DynamicBinding { key: 0, value: 2 },
            ],
            ..Scope::default()
        },
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    let error = run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)
        .expect_err("the duplicate dynamic field must fail");
    assert!(matches!(
        error,
        crate::EngineError::DuplicateDynamicProperty(_)
    ));

    let writes = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten {
                field,
                binding,
                value,
                ..
            } => Some((field, binding, value)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(writes.len(), 1);
    assert_eq!(writes[0].0.chars().count(), 160);
    assert_eq!(
        writes[0].1,
        crate::TraceTargetFieldBinding::DynamicBinding { key: 0, value: 1 }
    );
    assert_eq!(
        writes[0]
            .2
            .as_ref()
            .map(|value| (value.value_type, value.preview.as_str())),
        Some(("string", "first"))
    );
    Ok(())
}

#[test]
fn target_field_trace_records_dynamic_children_after_their_content() -> Result<(), Box<dyn Error>> {
    let dynamic_group =
        SchemaNode::group("*", vec![SchemaNode::scalar("Leaf", ScalarType::String)]);
    let target = SchemaNode::group("Output", Vec::new())
        .with_dynamic_fields(dynamic_group)
        .ok_or_else(|| std::io::Error::other("dynamic group target should be valid"))?;
    let project = field_trace_project(
        target,
        Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("ComputedChild".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("value".into()),
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        Scope {
            dynamic_children: vec![DynamicChild {
                key: 0,
                scope: Scope {
                    bindings: vec![Binding {
                        target_field: "Leaf".into(),
                        node: 1,
                    }],
                    ..Scope::default()
                },
            }],
            ..Scope::default()
        },
    );
    let collector = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    run_with_context(&project, &Instance::Group((Vec::new()).into()), &execution)?;

    let writes = collector
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten {
                scope,
                field,
                binding,
                ..
            } => Some((scope.target_path, field, binding)),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        writes,
        [
            (
                vec!["<dynamic>".into()],
                "Leaf".into(),
                crate::TraceTargetFieldBinding::StaticBinding { value: 1 },
            ),
            (
                Vec::new(),
                "ComputedChild".into(),
                crate::TraceTargetFieldBinding::DynamicChild { key: 0 },
            ),
        ]
    );
    Ok(())
}
