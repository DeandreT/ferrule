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
        crate::TraceScope::named("primary", 0)
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

mod owned_trace_witness {
    use super::field_trace_project;
    use crate::{
        EngineError, OwnedCapabilityContext, SourceOwnershipObservers, TraceOwnedDomain,
        TraceOwnedFrameRef, TraceOwnedLocation, TraceOwnedOriginRef, TraceOwnedOwner,
        TraceOwnedOwnerRef, TraceOwnedPhase, TraceOwnedPositionRef, TraceOwnedRootRef,
        TraceOwnedWitness, TraceOwnedWitnessError, TracePosition,
        check_source_ownership_capability_with_observers,
    };
    use ir::{Instance, SchemaNode, Value};
    use mapping::{Graph, NamedSource, Node, OwnedSourceField, OwnedSourceRef, Scope, SourceOwner};

    fn witness_project(names: &[&str]) -> mapping::Project {
        let mut project = field_trace_project(
            SchemaNode::group("Output", Vec::new()),
            Graph {
                nodes: Default::default(),
            },
            Scope::default(),
        );
        project.extra_sources = names
            .iter()
            .map(|name| named_source((*name).into()))
            .collect();
        project
    }

    fn named_source(name: String) -> NamedSource {
        NamedSource {
            name,
            path: String::new(),
            schema: SchemaNode::group("Input", Vec::new()),
            options: Default::default(),
            dynamic_path: None,
        }
    }

    fn owner(project: &mapping::Project, declaration_index: usize) -> TraceOwnedOwnerRef<'_> {
        TraceOwnedOwnerRef::Named {
            declaration_index,
            name: &project.extra_sources[declaration_index].name,
        }
    }

    fn owner_key(
        project: &mapping::Project,
        key: crate::owned_context::OwnerKey,
    ) -> TraceOwnedOwnerRef<'_> {
        match key {
            crate::owned_context::OwnerKey::Primary => TraceOwnedOwnerRef::Primary,
            crate::owned_context::OwnerKey::Named(index) => owner(project, index),
        }
    }

    fn witness<'a>(
        project: &'a mapping::Project,
        roots: &'a [TraceOwnedRootRef<'a>],
        frames: &'a [TraceOwnedFrameRef<'a>],
        origins: &'a [TraceOwnedOriginRef<'a>],
        positions: &'a [TraceOwnedPositionRef<'a>],
    ) -> TraceOwnedWitness<'a> {
        TraceOwnedWitness {
            project,
            domain: TraceOwnedDomain::Basic,
            phase: TraceOwnedPhase::Structural,
            roots,
            frames,
            origins,
            positions,
            owned_ancestry: true,
        }
    }

    #[test]
    fn current_context_preserves_borrowed_owner_identity_and_nested_ancestry() {
        use crate::owned_context::{
            OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
        };
        use crate::source_iteration::PositionFrame;
        let project = witness_project(&[" Ref/Rows α ", " ref/Rows α "]);
        let primary = Instance::Scalar(Value::Int(7));
        let row = || {
            Instance::Group(
                vec![
                    (
                        "Leaf".into(),
                        Instance::Scalar(Value::String("outer".into())),
                    ),
                    (
                        "Sub".into(),
                        Instance::Repeated(vec![Instance::Group(
                            vec![(
                                "Leaf".into(),
                                Instance::Scalar(Value::String("inner".into())),
                            )]
                            .into(),
                        )]),
                    ),
                ]
                .into(),
            )
        };
        let extras = Instance::Group(
            vec![
                (
                    project.extra_sources[0].name.clone(),
                    Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![row()]))].into()),
                ),
                (
                    project.extra_sources[1].name.clone(),
                    Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![row()]))].into()),
                ),
            ]
            .into(),
        );
        let registry =
            SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let first = registry.root(OwnerKey::Named(0)).unwrap();
        let second = registry.root(OwnerKey::Named(1)).unwrap();
        assert_eq!(first, second);
        assert!(!std::ptr::eq(first, second));
        let outer = &first.field("Rows").unwrap().as_repeated().unwrap()[0];
        let inner = &outer.field("Sub").unwrap().as_repeated().unwrap()[0];
        let context = OwnedContext {
            roots: &registry,
            frames: vec![
                SourceFrame {
                    owner: OwnerKey::Named(0),
                    collection: vec!["Rows".into()],
                    item: outer,
                    position_slot: 0,
                },
                SourceFrame {
                    owner: OwnerKey::Named(0),
                    collection: vec!["Rows".into(), "Sub".into()],
                    item: inner,
                    position_slot: 1,
                },
            ],
            origins: vec![SourceOrigin {
                owner: OwnerKey::Named(0),
                path: vec![],
                item: first,
            }],
            owned_ancestry: true,
        };
        let outer_field = OwnedSourceField {
            source: OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: project.extra_sources[0].name.clone(),
                },
                path: vec!["Rows".into(), "Leaf".into()],
            },
            frame: Some(vec!["Rows".into()]),
        };
        let before = context.field(&outer_field).unwrap();
        let positions = [
            PositionFrame {
                collection: vec![project.extra_sources[0].name.clone(), "Rows".into()],
                index: 9,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            },
            PositionFrame {
                collection: vec![
                    project.extra_sources[0].name.clone(),
                    "Rows".into(),
                    "Sub".into(),
                ],
                index: 2,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            },
        ];
        let raw = positions
            .iter()
            .map(TraceOwnedPositionRef::from)
            .collect::<Vec<_>>();
        let roots = [
            TraceOwnedRootRef {
                owner: TraceOwnedOwnerRef::Primary,
                input: Some(registry.primary),
            },
            TraceOwnedRootRef {
                owner: owner(&project, 0),
                input: Some(first),
            },
            TraceOwnedRootRef {
                owner: owner(&project, 1),
                input: Some(second),
            },
        ];
        let frames = context
            .frames
            .iter()
            .map(|frame| TraceOwnedFrameRef {
                owner: owner_key(&project, frame.owner),
                collection: &frame.collection,
                item: frame.item,
                position_slot: frame.position_slot,
            })
            .collect::<Vec<_>>();
        let origins = context
            .origins
            .iter()
            .map(|origin| TraceOwnedOriginRef {
                owner: owner_key(&project, origin.owner),
                path: &origin.path,
                item: origin.item,
            })
            .collect::<Vec<_>>();
        assert!(std::ptr::eq(roots[1].input.unwrap(), first));
        assert!(std::ptr::eq(frames[0].item, context.frames[0].item));
        assert!(std::ptr::eq(frames[1].item, context.frames[1].item));
        assert!(std::ptr::eq(origins[0].item, context.origins[0].item));
        let snapshot = witness(&project, &roots, &frames, &origins, &raw)
            .snapshot()
            .unwrap();
        assert_eq!(snapshot.frames[0].collection.segments, ["Rows"]);
        assert_eq!(snapshot.frames[1].collection.segments, ["Rows", "Sub"]);
        assert_eq!(
            snapshot.positions[0].collection.segments,
            [project.extra_sources[0].name.as_str(), "Rows"]
        );
        assert_eq!((snapshot.frames[0].index, snapshot.frames[1].index), (9, 2));
        assert_eq!(
            snapshot.roots[1].owner,
            TraceOwnedOwner::Named {
                declaration_index: 0,
                name: project.extra_sources[0].name.clone(),
                name_truncated: false
            }
        );
        assert_ne!(snapshot.roots[1].owner, snapshot.roots[2].owner);
        assert!(snapshot.origins[0].path.segments.is_empty());
        assert_eq!(context.field(&outer_field).unwrap(), before);
        assert_eq!(before, Value::String("outer".into()));
        assert_eq!(
            context
                .position_slot(&outer_field.source.owner, Some(&["Rows".into()]))
                .unwrap(),
            0
        );
        assert_eq!(context.frames.len(), 2);
        assert_eq!(context.origins.len(), 1);
        assert!(context.owned_ancestry);
    }

    #[test]
    fn explicit_phases_preserve_supplied_position_spaces_and_original_slots() {
        let project = witness_project(&[]);
        let input = Instance::Scalar(Value::Int(8));
        let roots = [TraceOwnedRootRef {
            owner: TraceOwnedOwnerRef::Primary,
            input: Some(&input),
        }];
        let path = vec!["Rows".into()];
        let frames = [TraceOwnedFrameRef {
            owner: TraceOwnedOwnerRef::Primary,
            collection: &path,
            item: &input,
            position_slot: 33,
        }];
        for (phase, index) in [
            (TraceOwnedPhase::Structural, 91),
            (TraceOwnedPhase::Selection, 7),
            (TraceOwnedPhase::Output, 1),
        ] {
            let positions = (0..34)
                .map(|_| TraceOwnedPositionRef {
                    collection: &path,
                    index,
                    grouped: false,
                    join: None,
                    join_position: None,
                    document_path: None,
                })
                .collect::<Vec<_>>();
            let mut view = witness(&project, &roots, &frames, &[], &positions);
            view.phase = phase;
            let snapshot = view.snapshot().unwrap();
            assert_eq!(snapshot.phase, phase);
            assert_eq!(snapshot.frames[0].position_slot, 33);
            assert_eq!(snapshot.frames[0].index, index);
            assert_eq!(snapshot.positions.len(), 32);
            assert_eq!(snapshot.omitted_positions, 2);
            assert_eq!(frames[0].position_slot, 33);
        }
        let positions = [TraceOwnedPositionRef {
            collection: &path,
            index: 1,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }];
        let frames = [TraceOwnedFrameRef {
            position_slot: 0,
            ..frames[0]
        }];
        let structural = witness(&project, &roots, &frames, &[], &positions)
            .snapshot()
            .unwrap();
        let mut view = witness(&project, &roots, &frames, &[], &positions);
        view.phase = TraceOwnedPhase::Selection;
        assert_ne!(view.snapshot().unwrap(), structural);
    }

    #[test]
    fn unavailable_null_and_inactive_root_paths_are_distinct() {
        let project = witness_project(&["Null", "Missing"]);
        let null = Instance::Scalar(Value::Null);
        let roots = [
            TraceOwnedRootRef {
                owner: owner(&project, 0),
                input: Some(&null),
            },
            TraceOwnedRootRef {
                owner: owner(&project, 1),
                input: None,
            },
        ];
        let empty = Vec::new();
        let inactive = witness(&project, &roots, &[], &[], &[]).snapshot().unwrap();
        assert_eq!(
            inactive.roots[0]
                .input
                .as_ref()
                .unwrap()
                .scalar
                .as_ref()
                .unwrap()
                .value_type,
            "null"
        );
        assert_eq!(inactive.roots[1].input, None);
        assert!(inactive.frames.is_empty());
        let positions = [TraceOwnedPositionRef {
            collection: &empty,
            index: 1,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }];
        let frames = [TraceOwnedFrameRef {
            owner: owner(&project, 0),
            collection: &empty,
            item: &null,
            position_slot: 0,
        }];
        let active = witness(&project, &roots, &frames, &[], &positions)
            .snapshot()
            .unwrap();
        assert_eq!(active.frames.len(), 1);
        assert!(active.frames[0].collection.segments.is_empty());
        assert_ne!(active, inactive);
        let frames = [TraceOwnedFrameRef {
            owner: owner(&project, 1),
            ..frames[0]
        }];
        assert_eq!(
            witness(&project, &roots, &frames, &[], &positions).snapshot(),
            Err(TraceOwnedWitnessError::UnavailableRoot {
                at: TraceOwnedLocation::Frame(0)
            })
        );
    }

    #[test]
    fn declaration_indices_names_and_duplicate_bindings_fail_without_fallback() {
        let project = witness_project(&["A", "a"]);
        let input = Instance::Scalar(Value::Int(1));
        let roots = [TraceOwnedRootRef {
            owner: TraceOwnedOwnerRef::Named {
                declaration_index: 1,
                name: "A",
            },
            input: Some(&input),
        }];
        assert_eq!(
            witness(&project, &roots, &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::DeclarationNameMismatch {
                at: TraceOwnedLocation::Root(0)
            })
        );
        let roots = [TraceOwnedRootRef {
            owner: TraceOwnedOwnerRef::Named {
                declaration_index: 2,
                name: "A",
            },
            input: Some(&input),
        }];
        assert_eq!(
            witness(&project, &roots, &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::UnknownDeclaration {
                at: TraceOwnedLocation::Root(0)
            })
        );
        let root = TraceOwnedRootRef {
            owner: owner(&project, 0),
            input: Some(&input),
        };
        assert_eq!(
            witness(&project, &[root, root], &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::DuplicateRoot {
                at: TraceOwnedLocation::Root(1)
            })
        );
        let ambiguous = witness_project(&["A", "A"]);
        assert_eq!(
            witness(&ambiguous, &[root], &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::AmbiguousDeclaration {
                at: TraceOwnedLocation::Root(0)
            })
        );
    }

    #[test]
    fn missing_bindings_and_invalid_slots_fail_before_snapshotting() {
        let project = witness_project(&["A"]);
        let input = Instance::Scalar(Value::Int(1));
        let empty = Vec::new();
        let frames = [TraceOwnedFrameRef {
            owner: owner(&project, 0),
            collection: &empty,
            item: &input,
            position_slot: 0,
        }];
        assert_eq!(
            witness(&project, &[], &frames, &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::MissingRoot {
                at: TraceOwnedLocation::Frame(0)
            })
        );
        let roots = [TraceOwnedRootRef {
            owner: owner(&project, 0),
            input: Some(&input),
        }];
        assert_eq!(
            witness(&project, &roots, &frames, &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 0
            })
        );
        let origins = [TraceOwnedOriginRef {
            owner: TraceOwnedOwnerRef::Primary,
            path: &empty,
            item: &input,
        }];
        assert_eq!(
            witness(&project, &roots, &[], &origins, &[]).snapshot(),
            Err(TraceOwnedWitnessError::MissingRoot {
                at: TraceOwnedLocation::Origin(0)
            })
        );
    }

    #[test]
    fn unsupported_contexts_and_raw_flags_are_typed_refusals() {
        let project = witness_project(&[]);
        for (domain, name) in [
            (TraceOwnedDomain::Grouped, "grouped source"),
            (TraceOwnedDomain::Joined, "joined source"),
            (TraceOwnedDomain::Documents, "document source"),
            (TraceOwnedDomain::Generated, "generated source"),
            (TraceOwnedDomain::Dynamic, "dynamic source"),
            (TraceOwnedDomain::Function, "function source"),
        ] {
            let mut view = witness(&project, &[], &[], &[], &[]);
            view.domain = domain;
            assert_eq!(
                view.snapshot(),
                Err(TraceOwnedWitnessError::UnsupportedDomain {
                    at: TraceOwnedLocation::Context,
                    domain: name
                })
            );
        }
        let path = Vec::new();
        let position = TraceOwnedPositionRef {
            collection: &path,
            index: 1,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        };
        for (position, domain) in [
            (
                TraceOwnedPositionRef {
                    grouped: true,
                    ..position
                },
                "grouped source",
            ),
            (
                TraceOwnedPositionRef {
                    join: Some(mapping::JoinId::new(3)),
                    ..position
                },
                "joined source",
            ),
            (
                TraceOwnedPositionRef {
                    join_position: Some((mapping::JoinId::new(3), 4)),
                    ..position
                },
                "joined source",
            ),
            (
                TraceOwnedPositionRef {
                    document_path: Some("file.xml"),
                    ..position
                },
                "document source",
            ),
        ] {
            assert_eq!(
                witness(&project, &[], &[], &[], &[position]).snapshot(),
                Err(TraceOwnedWitnessError::UnsupportedDomain {
                    at: TraceOwnedLocation::Position(0),
                    domain
                })
            );
        }
        for (input, domain) in [
            (Instance::MappedSequence(Vec::new()), "generated source"),
            (Instance::DocumentSet(Vec::new()), "document source"),
        ] {
            let roots = [TraceOwnedRootRef {
                owner: TraceOwnedOwnerRef::Primary,
                input: Some(&input),
            }];
            assert_eq!(
                witness(&project, &roots, &[], &[], &[]).snapshot(),
                Err(TraceOwnedWitnessError::UnsupportedDomain {
                    at: TraceOwnedLocation::Root(0),
                    domain
                })
            );
        }
        let mut dynamic = witness_project(&["A"]);
        dynamic.extra_sources[0].dynamic_path = Some(mapping::DynamicSourcePath {
            node: 0,
            iteration: Vec::new(),
        });
        let roots = [TraceOwnedRootRef {
            owner: owner(&dynamic, 0),
            input: None,
        }];
        assert_eq!(
            witness(&dynamic, &roots, &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::UnsupportedDomain {
                at: TraceOwnedLocation::Root(0),
                domain: "dynamic source"
            })
        );
    }

    #[test]
    fn snapshot_bounds_are_unicode_safe_and_do_not_hide_invalid_tail_entries() {
        let long = "é".repeat(81);
        let mut project = witness_project(&[]);
        project.extra_sources.push(named_source(long.clone()));
        let input = Instance::Scalar(Value::String("λ".repeat(161)));
        let roots = [TraceOwnedRootRef {
            owner: owner(&project, 0),
            input: Some(&input),
        }];
        let path = vec![long; 17];
        let positions = (0..33)
            .map(|index| TraceOwnedPositionRef {
                collection: &path,
                index,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            })
            .collect::<Vec<_>>();
        let frames = (0..33)
            .map(|position_slot| TraceOwnedFrameRef {
                owner: owner(&project, 0),
                collection: &path,
                item: &input,
                position_slot,
            })
            .collect::<Vec<_>>();
        let origins = (0..33)
            .map(|_| TraceOwnedOriginRef {
                owner: owner(&project, 0),
                path: &path,
                item: &input,
            })
            .collect::<Vec<_>>();
        let snapshot = witness(&project, &roots, &frames, &origins, &positions)
            .snapshot()
            .unwrap();
        assert_eq!(
            (
                snapshot.frames.len(),
                snapshot.origins.len(),
                snapshot.positions.len()
            ),
            (32, 32, 32)
        );
        assert_eq!(
            (
                snapshot.omitted_frames,
                snapshot.omitted_origins,
                snapshot.omitted_positions
            ),
            (1, 1, 1)
        );
        assert_eq!(snapshot.frames[0].collection.segments.len(), 16);
        assert_eq!(snapshot.frames[0].collection.omitted_segments, 1);
        assert!(
            snapshot.frames[0]
                .collection
                .truncated_segments
                .iter()
                .all(|value| *value)
        );
        assert_eq!(
            snapshot.frames[0].collection.segments[0].chars().count(),
            80
        );
        match &snapshot.roots[0].owner {
            TraceOwnedOwner::Named {
                declaration_index,
                name,
                name_truncated,
            } => {
                assert_eq!(*declaration_index, 0);
                assert_eq!(name.chars().count(), 80);
                assert!(*name_truncated);
            }
            _ => panic!("named endpoint lost"),
        }
        let scalar = snapshot.roots[0]
            .input
            .as_ref()
            .unwrap()
            .scalar
            .as_ref()
            .unwrap();
        assert_eq!(scalar.preview.chars().count(), 160);
        assert!(scalar.truncated);
        let mut invalid_positions = positions.clone();
        invalid_positions[32].document_path = Some("hidden.xml");
        assert_eq!(
            witness(&project, &roots, &frames, &origins, &invalid_positions).snapshot(),
            Err(TraceOwnedWitnessError::UnsupportedDomain {
                at: TraceOwnedLocation::Position(32),
                domain: "document source"
            })
        );
        let mut invalid_frames = frames.clone();
        invalid_frames[32].position_slot = 33;
        assert_eq!(
            witness(&project, &roots, &invalid_frames, &origins, &positions).snapshot(),
            Err(TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(32),
                slot: 33
            })
        );
    }

    #[test]
    fn root_retention_reports_omissions_and_validates_the_original_tail() {
        let mut project = witness_project(&[]);
        project.extra_sources = (0..33)
            .map(|index| named_source(format!("Source {index}")))
            .collect();
        let input = Instance::Scalar(Value::Int(3));
        let mut roots = (0..33)
            .map(|index| TraceOwnedRootRef {
                owner: owner(&project, index),
                input: Some(&input),
            })
            .collect::<Vec<_>>();
        let snapshot = witness(&project, &roots, &[], &[], &[]).snapshot().unwrap();
        assert_eq!(snapshot.roots.len(), 32);
        assert_eq!(snapshot.omitted_roots, 1);
        assert_eq!(
            snapshot.roots[31].owner,
            TraceOwnedOwner::Named {
                declaration_index: 31,
                name: "Source 31".into(),
                name_truncated: false
            }
        );
        roots[32].owner = TraceOwnedOwnerRef::Named {
            declaration_index: 99,
            name: "Source 32",
        };
        assert_eq!(
            witness(&project, &roots, &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::UnknownDeclaration {
                at: TraceOwnedLocation::Root(32)
            })
        );
    }

    #[test]
    fn declaration_path_and_registry_byte_limits_match_basic_context_preflight() {
        use crate::owned_context::{OwnedLimits, OwnedSourceError, SourceRoots};
        let input = Instance::Scalar(Value::Int(0));
        let extra = Instance::Group(Vec::new().into());
        let mut project = witness_project(&[]);
        project.extra_sources = (0..4095)
            .map(|index| named_source(index.to_string()))
            .collect();
        assert!(witness(&project, &[], &[], &[], &[]).snapshot().is_ok());
        assert!(SourceRoots::new(&project, &input, &extra, OwnedLimits::default()).is_ok());
        project.extra_sources.push(named_source("last".into()));
        assert_eq!(
            witness(&project, &[], &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::ResourceLimit {
                resource: "declaration entries",
                limit: 4096
            })
        );
        assert!(matches!(
            SourceRoots::new(&project, &input, &extra, OwnedLimits::default()),
            Err(OwnedSourceError::ResourceLimit {
                resource: "root entries",
                limit: 4096
            })
        ));
        project.extra_sources = vec![named_source("x".repeat(16 * 1024 * 1024))];
        assert!(witness(&project, &[], &[], &[], &[]).snapshot().is_ok());
        project.extra_sources[0].name.push('x');
        assert_eq!(
            witness(&project, &[], &[], &[], &[]).snapshot(),
            Err(TraceOwnedWitnessError::ResourceLimit {
                resource: "metadata bytes",
                limit: 16 * 1024 * 1024
            })
        );
        assert!(matches!(
            SourceRoots::new(&project, &input, &extra, OwnedLimits::default()),
            Err(OwnedSourceError::ResourceLimit {
                resource: "metadata bytes",
                limit: 16777216
            })
        ));
        project.extra_sources.clear();
        let mut path = vec!["a".into(); 256];
        let position = TraceOwnedPositionRef {
            collection: &path,
            index: 1,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        };
        assert!(
            witness(&project, &[], &[], &[], &[position])
                .snapshot()
                .is_ok()
        );
        path.push("b".into());
        let position = TraceOwnedPositionRef {
            collection: &path,
            index: 1,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        };
        assert_eq!(
            witness(&project, &[], &[], &[], &[position]).snapshot(),
            Err(TraceOwnedWitnessError::ResourceLimit {
                resource: "path segments",
                limit: 256
            })
        );
    }

    #[test]
    fn legacy_position_fields_and_public_observer_refusals_remain_exact() {
        use crate::source_iteration::PositionFrame;
        let position = PositionFrame {
            collection: vec!["Root".into(), "Rows".into()],
            index: 4,
            grouped: true,
            join: Some(mapping::JoinId::new(2)),
            join_position: Some((mapping::JoinId::new(2), 9)),
            document_path: Some("original.xml".into()),
        };
        let legacy = TracePosition::from(&position);
        let borrowed = TraceOwnedPositionRef::from(&position);
        let from_legacy = TraceOwnedPositionRef::from(&legacy);
        assert_eq!(borrowed.collection, legacy.collection);
        assert_eq!(borrowed.index, legacy.index);
        assert_eq!(borrowed.grouped, legacy.grouped);
        assert_eq!(borrowed.join, legacy.join);
        assert_eq!(borrowed.join_position, legacy.join_position);
        assert_eq!(borrowed.document_path, legacy.document_path.as_deref());
        assert_eq!(from_legacy.collection, borrowed.collection);
        assert_eq!(from_legacy.document_path, borrowed.document_path);
        let mut project = witness_project(&[]);
        project.graph.nodes.insert(
            0,
            Node::OwnedSourceField(OwnedSourceField {
                source: OwnedSourceRef {
                    owner: SourceOwner::Primary,
                    path: vec!["Value".into()],
                },
                frame: None,
            }),
        );
        project.root.bindings.push(mapping::Binding {
            target_field: "Result".into(),
            node: 0,
        });
        for (observers, context) in [
            (
                SourceOwnershipObservers {
                    tracing: true,
                    debugging: false,
                },
                OwnedCapabilityContext::Tracing,
            ),
            (
                SourceOwnershipObservers {
                    tracing: false,
                    debugging: true,
                },
                OwnedCapabilityContext::Debugging,
            ),
            (
                SourceOwnershipObservers {
                    tracing: true,
                    debugging: true,
                },
                OwnedCapabilityContext::Tracing,
            ),
        ] {
            assert_eq!(
                check_source_ownership_capability_with_observers(&project, None, observers),
                Err(EngineError::UnsupportedOwnedContext {
                    node: None,
                    function: None,
                    scope: None,
                    context
                })
            );
        }
        assert!(
            check_source_ownership_capability_with_observers(
                &project,
                None,
                SourceOwnershipObservers::default()
            )
            .is_ok()
        );
    }
}

mod owned_trace_producer {
    use super::{Collector, field_trace_project};
    use crate::eval_expr::{EvalProgram, eval_expr};
    use crate::owned_context::{
        OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
    };
    use crate::source_iteration::PositionFrame;
    use crate::trace::TraceOwnedRegistry;
    use crate::{
        EngineError, TraceEvent, TraceOwnedContextError, TraceOwnedLocation, TraceOwnedPhase,
        TraceOwnedSnapshot, TraceOwnedWitness, TraceOwnedWitnessError, TraceSink,
    };
    use ir::{Instance, ScalarType, SchemaNode, Value};
    use mapping::{
        Binding, Graph, NamedSource, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef,
        Project, Scope, ScopeIteration, SequenceWindow, SortFilterOrder, SourceOwner,
    };
    use std::cell::{Cell, RefCell};
    use std::collections::HashSet;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Outcome {
        Snapshot(Result<TraceOwnedSnapshot, TraceOwnedWitnessError>),
        Projection(TraceOwnedContextError),
    }

    #[derive(Debug)]
    struct Observation {
        node: u32,
        input: Option<(u32, usize)>,
        outcome: Outcome,
    }

    type WitnessCheck<'a> = dyn Fn(&TraceOwnedWitness<'_>) + 'a;

    struct Paired<'a> {
        events: RefCell<Vec<TraceEvent>>,
        observations: RefCell<Vec<Observation>>,
        root_addresses: RefCell<Vec<usize>>,
        enabled: Cell<bool>,
        check: Option<&'a WitnessCheck<'a>>,
    }

    impl Paired<'_> {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                observations: RefCell::new(Vec::new()),
                root_addresses: RefCell::new(Vec::new()),
                enabled: Cell::new(true),
                check: None,
            }
        }
    }

    impl TraceSink for Paired<'_> {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }
        fn wants_owned_context(&self) -> bool {
            self.enabled.get()
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            let outcome = match context {
                Ok(view) => {
                    if let Some(check) = self.check {
                        check(&view);
                    }
                    self.root_addresses
                        .borrow_mut()
                        .push(view.roots.as_ptr() as usize);
                    Outcome::Snapshot(view.snapshot())
                }
                Err(error) => Outcome::Projection(error.clone()),
            };
            let (node, input) = match &event {
                TraceEvent::NodeValue { node, .. } => (*node, None),
                TraceEvent::NodeInputValue {
                    consumer,
                    input,
                    input_index,
                    ..
                } => (*consumer, Some((*input, *input_index))),
                _ => panic!("only expression value/input events receive owned context"),
            };
            self.observations.borrow_mut().push(Observation {
                node,
                input,
                outcome,
            });
            self.record(event);
        }
    }

    fn trace_project(names: &[&str], nodes: Vec<(u32, Node)>) -> Project {
        let mut project = field_trace_project(
            SchemaNode::group("Output", Vec::new()),
            Graph {
                nodes: nodes.into_iter().collect(),
            },
            Scope::default(),
        );
        project.extra_sources = names
            .iter()
            .map(|name| NamedSource {
                name: (*name).into(),
                path: String::new(),
                schema: SchemaNode::group("Input", Vec::new()),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect();
        project
    }

    fn owned_field(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> Node {
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path.iter().map(|name| (*name).into()).collect(),
            },
            frame: frame.map(|path| path.iter().map(|name| (*name).into()).collect()),
        })
    }

    fn evaluate(
        project: &Project,
        owned: &OwnedContext<'_>,
        registry: &dyn crate::trace::TraceOwnedRegistryView,
        sink: Option<&dyn TraceSink>,
        node: u32,
        positions: &[PositionFrame],
        phase: TraceOwnedPhase,
    ) -> Result<Value, EngineError> {
        let first_failure = Cell::new(false);
        let program = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            sink,
            &first_failure,
        )
        .with_owned_context(owned)
        .with_owned_trace_registry(registry)
        .with_owned_trace_phase(phase);
        eval_expr(
            program,
            node,
            &[owned.roots.extras_wrapper, owned.roots.primary],
            positions,
            &mut HashSet::new(),
        )
    }

    #[test]
    fn real_expression_producer_borrows_original_registry_frames_origins_and_positions() {
        let names = [" Ref/Rows α ", " ref/Rows α "];
        let project = trace_project(
            &names,
            vec![
                (
                    0,
                    owned_field(
                        SourceOwner::Named {
                            name: names[0].into(),
                        },
                        &["Rows", "Value"],
                        Some(&["Rows"]),
                    ),
                ),
                (
                    1,
                    owned_field(
                        SourceOwner::Named {
                            name: names[1].into(),
                        },
                        &["Rows", "Value"],
                        Some(&["Rows"]),
                    ),
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 1],
                    },
                ),
            ],
        );
        let primary = Instance::Scalar(Value::Int(9));
        let row = || {
            Instance::Group(
                vec![(
                    "Value".into(),
                    Instance::Scalar(Value::String("same".into())),
                )]
                .into(),
            )
        };
        let extras = Instance::Group(
            names
                .iter()
                .map(|name| {
                    (
                        (*name).into(),
                        Instance::Group(
                            vec![("Rows".into(), Instance::Repeated(vec![row()]))].into(),
                        ),
                    )
                })
                .collect::<Vec<_>>()
                .into(),
        );
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let first = roots.root(OwnerKey::Named(0)).unwrap();
        let second = roots.root(OwnerKey::Named(1)).unwrap();
        let first_row = &first.field("Rows").unwrap().as_repeated().unwrap()[0];
        let second_row = &second.field("Rows").unwrap().as_repeated().unwrap()[0];
        assert_eq!(first_row, second_row);
        assert!(!std::ptr::eq(first_row, second_row));
        let owned = OwnedContext {
            roots: &roots,
            frames: vec![
                SourceFrame {
                    owner: OwnerKey::Named(0),
                    collection: vec!["Rows".into()],
                    item: first_row,
                    position_slot: 0,
                },
                SourceFrame {
                    owner: OwnerKey::Named(1),
                    collection: vec!["Rows".into()],
                    item: second_row,
                    position_slot: 1,
                },
            ],
            origins: vec![SourceOrigin {
                owner: OwnerKey::Named(0),
                path: Vec::new(),
                item: first,
            }],
            owned_ancestry: true,
        };
        let positions = names
            .iter()
            .enumerate()
            .map(|(index, name)| PositionFrame {
                collection: vec![(*name).into(), "Rows".into()],
                index: [9, 2][index],
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            })
            .collect::<Vec<_>>();
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let check = |view: &TraceOwnedWitness<'_>| {
            assert!(std::ptr::eq(view.project, &project));
            assert!(std::ptr::eq(view.roots[0].input.unwrap(), roots.primary));
            assert!(std::ptr::eq(view.roots[1].input.unwrap(), first));
            assert!(std::ptr::eq(view.roots[2].input.unwrap(), second));
            for index in 0..2 {
                let crate::TraceOwnedOwnerRef::Named {
                    declaration_index,
                    name,
                } = view.roots[index + 1].owner
                else {
                    panic!("named registry identity lost");
                };
                assert_eq!(declaration_index, index);
                assert_eq!(name.as_ptr(), project.extra_sources[index].name.as_ptr());
                assert_eq!(view.frames[index].owner, view.roots[index + 1].owner);
                assert!(std::ptr::eq(
                    view.frames[index].item,
                    owned.frames[index].item
                ));
                assert_eq!(
                    view.frames[index].collection.as_ptr(),
                    owned.frames[index].collection.as_ptr()
                );
                assert_eq!(
                    view.positions[index].collection.as_ptr(),
                    positions[index].collection.as_ptr()
                );
                assert_eq!(view.positions[index].index, positions[index].index);
            }
            assert!(std::ptr::eq(view.origins[0].item, owned.origins[0].item));
            assert_eq!(
                view.origins[0].path.as_ptr(),
                owned.origins[0].path.as_ptr()
            );
        };
        let mut paired = Paired::new();
        paired.check = Some(&check);
        let baseline = evaluate(
            &project,
            &owned,
            &registry,
            None,
            2,
            &positions,
            TraceOwnedPhase::Output,
        )
        .unwrap();
        assert_eq!(baseline, Value::String("samesame".into()));
        assert!(registry.cache.get().is_none());
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &registry,
                Some(&paired),
                2,
                &positions,
                TraceOwnedPhase::Output
            )
            .unwrap(),
            baseline
        );
        let observations = paired.observations.borrow();
        assert_eq!(observations.len(), 5);
        assert_eq!(
            observations
                .iter()
                .map(|o| (o.node, o.input))
                .collect::<Vec<_>>(),
            vec![
                (0, None),
                (2, Some((0, 0))),
                (1, None),
                (2, Some((1, 1))),
                (2, None)
            ]
        );
        assert!(observations.iter().all(|o| matches!(&o.outcome, Outcome::Snapshot(Ok(view)) if view.phase == TraceOwnedPhase::Output)));
        let addresses = paired.root_addresses.borrow();
        assert!(addresses.iter().all(|address| *address == addresses[0]));
    }

    #[test]
    fn default_opt_in_dispatch_preserves_exact_events_and_once_delivery() {
        struct DefaultPair(RefCell<Vec<TraceEvent>>);
        impl TraceSink for DefaultPair {
            fn record(&self, event: TraceEvent) {
                self.0.borrow_mut().push(event);
            }
            fn wants_owned_context(&self) -> bool {
                true
            }
        }
        let project = trace_project(
            &[],
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::String("a".into()),
                    },
                ),
                (
                    1,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![0, 0],
                    },
                ),
            ],
        );
        let primary = Instance::Scalar(Value::Int(8));
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let legacy = Collector::default();
        let contextual = DefaultPair(RefCell::new(Vec::new()));
        let before = evaluate(
            &project,
            &owned,
            &registry,
            Some(&legacy),
            1,
            &[],
            TraceOwnedPhase::Structural,
        )
        .unwrap();
        assert!(registry.cache.get().is_none());
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &registry,
                Some(&contextual),
                1,
                &[],
                TraceOwnedPhase::Structural
            )
            .unwrap(),
            before
        );
        assert_eq!(*legacy.0.borrow(), *contextual.0.borrow());
        assert_eq!(contextual.0.borrow().len(), 5);
    }

    #[test]
    fn changing_observer_requests_builds_once_and_reuses_the_root_projection() {
        let project = trace_project(
            &[],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(1),
                },
            )],
        );
        let primary = Instance::Scalar(Value::Int(8));
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        for enabled in [false, true, false, true] {
            paired.enabled.set(enabled);
            assert_eq!(
                evaluate(
                    &project,
                    &owned,
                    &registry,
                    Some(&paired),
                    0,
                    &[],
                    TraceOwnedPhase::Structural
                )
                .unwrap(),
                Value::Int(1)
            );
            if paired.events.borrow().len() == 1 {
                assert!(registry.cache.get().is_none());
            }
        }
        assert_eq!(paired.events.borrow().len(), 4);
        assert_eq!(paired.observations.borrow().len(), 2);
        let addresses = paired.root_addresses.borrow();
        assert_eq!(addresses[0], addresses[1]);
    }

    #[test]
    fn unavailable_null_and_ambiguous_unused_inputs_stay_observer_metadata() {
        let project = trace_project(
            &["Null", "Missing"],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(8),
                },
            )],
        );
        let primary = Instance::Scalar(Value::Int(0));
        let extras = Instance::Group(vec![("Null".into(), Instance::Scalar(Value::Null))].into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &registry,
                Some(&paired),
                0,
                &[],
                TraceOwnedPhase::Structural
            )
            .unwrap(),
            Value::Int(8)
        );
        let observations = paired.observations.borrow();
        let Outcome::Snapshot(Ok(snapshot)) = &observations[0].outcome else {
            panic!("expected snapshot");
        };
        assert_eq!(
            snapshot.roots[1]
                .input
                .as_ref()
                .unwrap()
                .scalar
                .as_ref()
                .unwrap()
                .value_type,
            "null"
        );
        assert!(snapshot.roots[2].input.is_none());
        assert!(snapshot.frames.is_empty());
        drop(observations);
        let ambiguous = trace_project(
            &["Unused"],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(8),
                },
            )],
        );
        let extras = Instance::Group(
            vec![
                ("Unused".into(), Instance::Scalar(Value::Int(1))),
                ("Unused".into(), Instance::Scalar(Value::Int(2))),
            ]
            .into(),
        );
        let roots =
            SourceRoots::new(&ambiguous, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&ambiguous, &roots);
        let paired = Paired::new();
        let baseline = evaluate(
            &ambiguous,
            &owned,
            &registry,
            None,
            0,
            &[],
            TraceOwnedPhase::Structural,
        );
        assert_eq!(
            evaluate(
                &ambiguous,
                &owned,
                &registry,
                Some(&paired),
                0,
                &[],
                TraceOwnedPhase::Structural
            ),
            baseline
        );
        assert_eq!(
            paired.observations.borrow()[0].outcome,
            Outcome::Projection(TraceOwnedContextError::Registry {
                declaration_index: 0,
                source: crate::OwnedSourceError::AmbiguousOwner {
                    name: "Unused".into()
                }
            })
        );
    }

    #[test]
    fn projection_and_snapshot_failures_never_replace_expression_results() {
        let project = trace_project(
            &["A"],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(8),
                },
            )],
        );
        let other_project = trace_project(&["A"], vec![]);
        let primary = Instance::Scalar(Value::Int(1));
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let wrong = TraceOwnedRegistry::new(&other_project, &roots);
        let paired = Paired::new();
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &wrong,
                Some(&paired),
                0,
                &[],
                TraceOwnedPhase::Structural
            )
            .unwrap(),
            Value::Int(8)
        );
        assert_eq!(
            paired.observations.borrow()[0].outcome,
            Outcome::Projection(TraceOwnedContextError::MismatchedRegistry)
        );
        let malformed = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        assert_eq!(
            evaluate(
                &project,
                &malformed,
                &registry,
                Some(&paired),
                0,
                &[],
                TraceOwnedPhase::Structural
            )
            .unwrap(),
            Value::Int(8)
        );
        assert_eq!(
            paired.observations.borrow()[0].outcome,
            Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 7
            }))
        );
    }

    #[test]
    fn actual_scope_phases_preserve_sorted_selection_compaction_and_child_reentry() {
        let primary = Instance::Group(
            vec![(
                "Rows".into(),
                Instance::Repeated(
                    [(3, true), (1, false), (2, true)]
                        .into_iter()
                        .map(|(rank, keep)| {
                            Instance::Group(
                                vec![
                                    ("Rank".into(), Instance::Scalar(Value::Int(rank))),
                                    ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )]
            .into(),
        );
        for order in [
            SortFilterOrder::FilterThenSort,
            SortFilterOrder::SortThenFilter,
        ] {
            let mut project = trace_project(
                &[],
                vec![
                    (
                        0,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        1,
                        owned_field(SourceOwner::Primary, &["Rows", "Rank"], Some(&["Rows"])),
                    ),
                    (
                        2,
                        owned_field(SourceOwner::Primary, &["Rows", "Keep"], Some(&["Rows"])),
                    ),
                    (
                        3,
                        Node::OwnedPosition(OwnedPosition {
                            owner: SourceOwner::Primary,
                            collection: Some(vec!["Rows".into()]),
                        }),
                    ),
                ],
            );
            project.root = Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: vec!["Rows".into()],
                    },
                    output_path: None,
                },
                filter: Some(2),
                sort_by: Some(1),
                sort_filter_order: order,
                windows: vec![SequenceWindow::SkipFirst { count: 0 }],
                bindings: vec![
                    Binding {
                        target_field: "Rank".into(),
                        node: 1,
                    },
                    Binding {
                        target_field: "Position".into(),
                        node: 3,
                    },
                ],
                children: vec![Scope {
                    target_field: "Echo".into(),
                    windows: vec![SequenceWindow::First { count: 0 }],
                    bindings: vec![Binding {
                        target_field: "Position".into(),
                        node: 3,
                    }],
                    ..Scope::default()
                }],
                ..Scope::default()
            };
            let extras = Instance::Group(Vec::new().into());
            let roots =
                SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
            let owned = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&project, &roots);
            let paired = Paired::new();
            let legacy = Collector::default();
            let first = Cell::new(false);
            let base = EvalProgram::new(
                &project.graph,
                &project.user_functions,
                Some(&legacy),
                &first,
            )
            .with_owned_context(&owned)
            .with_owned_trace_registry(&registry);
            let context = [&extras, &primary];
            let before = crate::eval_scope::eval_scope(
                base,
                &project.root,
                None,
                &context,
                &[],
                &project.extra_sources,
                None,
                &crate::TraceScope::primary(),
            )
            .unwrap();
            let program = EvalProgram::new(
                &project.graph,
                &project.user_functions,
                Some(&paired),
                &first,
            )
            .with_owned_context(&owned)
            .with_owned_trace_registry(&registry);
            let after = crate::eval_scope::eval_scope(
                program,
                &project.root,
                None,
                &context,
                &[],
                &project.extra_sources,
                None,
                &crate::TraceScope::primary(),
            )
            .unwrap();
            assert_eq!(after, before);
            assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
            let items = after.as_repeated().unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(
                items[0].field("Rank").unwrap().as_scalar(),
                Some(&Value::Int(3))
            );
            assert_eq!(
                items[0].field("Position").unwrap().as_scalar(),
                Some(&Value::Int(1))
            );
            assert_eq!(
                items[0]
                    .field("Echo")
                    .unwrap()
                    .field("Position")
                    .unwrap()
                    .as_scalar(),
                Some(&Value::Int(1))
            );
            let observations = paired.observations.borrow();
            let snapshots = observations
                .iter()
                .filter_map(|o| match &o.outcome {
                    Outcome::Snapshot(Ok(snapshot)) => Some((o.node, snapshot)),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(snapshots.len(), observations.len());
            assert!(snapshots.iter().any(
                |(node, snapshot)| *node == 1 && snapshot.phase == TraceOwnedPhase::Structural
            ));
            assert!(snapshots.iter().any(|(node, snapshot)| *node == 3
                && snapshot.phase == TraceOwnedPhase::Output
                && snapshot.frames[0].index == 1));
            assert!(snapshots.iter().any(|(node, snapshot)| *node == 0
                && snapshot.phase == TraceOwnedPhase::Structural
                && snapshot.frames.len() == 1
                && snapshot.frames[0].index == 1));
            for (node, snapshot) in &snapshots {
                if *node == 2 {
                    assert_eq!(
                        snapshot.phase,
                        if order == SortFilterOrder::FilterThenSort {
                            TraceOwnedPhase::Structural
                        } else {
                            TraceOwnedPhase::Selection
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn lazy_and_failed_inputs_preserve_full_errors_and_do_not_fabricate_values() {
        let project = trace_project(
            &[],
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::Bool(false),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("ok".into()),
                    },
                ),
                (2, Node::Raise { message: None }),
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
                    Node::Call {
                        function: "concat".into(),
                        args: vec![1, 2],
                    },
                ),
            ],
        );
        let primary = Instance::Scalar(Value::Int(1));
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &registry,
                Some(&paired),
                3,
                &[],
                TraceOwnedPhase::Structural
            )
            .unwrap(),
            Value::String("ok".into())
        );
        assert!(
            !paired
                .observations
                .borrow()
                .iter()
                .any(|o| o.node == 2 || o.input.is_some_and(|(input, _)| input == 2))
        );
        paired.events.borrow_mut().clear();
        paired.observations.borrow_mut().clear();
        let expected = evaluate(
            &project,
            &owned,
            &registry,
            None,
            4,
            &[],
            TraceOwnedPhase::Output,
        );
        assert!(expected.is_err());
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &registry,
                Some(&paired),
                4,
                &[],
                TraceOwnedPhase::Output
            ),
            expected
        );
        assert_eq!(
            paired
                .observations
                .borrow()
                .iter()
                .map(|o| (o.node, o.input))
                .collect::<Vec<_>>(),
            vec![(1, None), (4, Some((1, 0)))]
        );
    }

    #[test]
    fn function_body_events_keep_qualified_identity_without_caller_witnesses() {
        let function = mapping::FunctionId::new(9);
        let parameter = mapping::FunctionParameterId::new(8);
        let mut project = trace_project(
            &[],
            vec![
                (0, owned_field(SourceOwner::Primary, &["Value"], None)),
                (
                    1,
                    Node::UserFunctionCall {
                        function,
                        args: vec![0],
                    },
                ),
            ],
        );
        project.user_functions.insert(
            function,
            mapping::UserFunction {
                library: "Local".into(),
                name: "Echo".into(),
                description: None,
                parameters: vec![mapping::FunctionParameter {
                    id: parameter,
                    name: "Input".into(),
                    ty: ScalarType::String,
                }],
                output_name: "Output".into(),
                output_type: ScalarType::String,
                body: Graph {
                    nodes: [(0, Node::FunctionParameter { parameter })]
                        .into_iter()
                        .collect(),
                },
                output: 0,
            },
        );
        let primary = Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String("hello".into())),
            )]
            .into(),
        );
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        assert_eq!(
            evaluate(
                &project,
                &owned,
                &registry,
                Some(&paired),
                1,
                &[],
                TraceOwnedPhase::Output
            )
            .unwrap(),
            Value::String("hello".into())
        );
        assert_eq!(paired.observations.borrow().len(), 3);
        assert!(paired.events.borrow().iter().any(|event| matches!(event, TraceEvent::FunctionNodeValue {
            function: observed, node: 0, positions, .. } if *observed == function && positions.is_empty())));
    }

    #[test]
    fn public_owned_trace_and_debug_admission_stays_closed_before_callbacks() {
        let mut project = trace_project(
            &[],
            vec![(0, owned_field(SourceOwner::Primary, &["Value"], None))],
        );
        project.root.bindings.push(Binding {
            target_field: "Result".into(),
            node: 0,
        });
        let primary = Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String("hello".into())),
            )]
            .into(),
        );
        let paired = Paired::new();
        let execution = crate::ExecutionContext::new(std::path::Path::new("project.json"))
            .with_trace_sink(&paired);
        assert_eq!(
            crate::run_with_context(&project, &primary, &execution),
            Err(EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: None,
                context: crate::OwnedCapabilityContext::Tracing
            })
        );
        assert!(paired.events.borrow().is_empty());
        assert!(paired.observations.borrow().is_empty());
        for (observers, context) in [
            (
                crate::SourceOwnershipObservers {
                    tracing: false,
                    debugging: true,
                },
                crate::OwnedCapabilityContext::Debugging,
            ),
            (
                crate::SourceOwnershipObservers {
                    tracing: true,
                    debugging: true,
                },
                crate::OwnedCapabilityContext::Tracing,
            ),
        ] {
            assert_eq!(
                crate::check_source_ownership_capability_with_observers(&project, None, observers),
                Err(EngineError::UnsupportedOwnedContext {
                    node: None,
                    function: None,
                    scope: None,
                    context
                })
            );
        }
    }
}

mod owned_output_trace_producer {
    use super::{Collector, field_trace_project};
    use crate::eval_expr::EvalProgram;
    use crate::owned_context::{
        OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
    };
    use crate::source_iteration::PositionFrame;
    use crate::trace::{TraceOwnedRegistry, record_owned_event};
    use crate::{
        EngineError, TraceEvent, TraceOutputKind, TraceOwnedContextError, TraceOwnedLocation,
        TraceOwnedOwnerRef, TraceOwnedPhase, TraceOwnedSnapshot, TraceOwnedWitness,
        TraceOwnedWitnessError, TraceScope, TraceSink, TraceTargetFieldBinding, TraceValue,
    };
    use ir::{Instance, SchemaNode, Value};
    use mapping::{
        Binding, Graph, NamedSource, Node, OwnedPosition, OwnedSourceField, OwnedSourceRef,
        Project, Scope, ScopeIteration, SequenceWindow, SortFilterOrder, SourceOwner,
    };
    use std::cell::{Cell, RefCell};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Outcome {
        Snapshot(Result<TraceOwnedSnapshot, TraceOwnedWitnessError>),
        Projection(TraceOwnedContextError),
    }

    #[derive(Debug)]
    struct Observation {
        event: TraceEvent,
        outcome: Outcome,
    }

    type WitnessCheck<'a> = dyn Fn(&TraceEvent, &TraceOwnedWitness<'_>) + 'a;

    struct Paired<'a> {
        events: RefCell<Vec<TraceEvent>>,
        observations: RefCell<Vec<Observation>>,
        root_addresses: RefCell<Vec<usize>>,
        selected: Cell<u8>,
        stop_after_first: bool,
        check: Option<&'a WitnessCheck<'a>>,
    }

    impl Paired<'_> {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                observations: RefCell::new(Vec::new()),
                root_addresses: RefCell::new(Vec::new()),
                selected: Cell::new(3),
                stop_after_first: false,
                check: None,
            }
        }
    }

    impl TraceSink for Paired<'_> {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }

        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            let selected = self.selected.get();
            match event {
                TraceEvent::TargetFieldWritten { .. } => selected & 1 != 0,
                TraceEvent::TargetProduced { .. } => selected & 2 != 0,
                _ => false,
            }
        }

        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(
                &event,
                TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
            ));
            let outcome = match context {
                Ok(view) => {
                    if let Some(check) = self.check {
                        check(&event, &view);
                    }
                    self.root_addresses
                        .borrow_mut()
                        .push(view.roots.as_ptr() as usize);
                    Outcome::Snapshot(view.snapshot())
                }
                Err(error) => Outcome::Projection(error.clone()),
            };
            self.observations.borrow_mut().push(Observation {
                event: event.clone(),
                outcome,
            });
            if self.stop_after_first {
                self.selected.set(0);
            }
            self.record(event);
        }
    }

    fn project(names: &[&str], nodes: Vec<(u32, Node)>, root: Scope) -> Project {
        let mut project = field_trace_project(
            SchemaNode::group("Output", Vec::new()),
            Graph {
                nodes: nodes.into_iter().collect(),
            },
            root,
        );
        project.extra_sources = names
            .iter()
            .map(|name| NamedSource {
                name: (*name).into(),
                path: String::new(),
                schema: SchemaNode::group("Input", Vec::new()),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect();
        project
    }

    fn constant_project(names: &[&str]) -> Project {
        project(
            names,
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::Int(8),
                    },
                ),
                (1, Node::Const { value: Value::Null }),
            ],
            Scope {
                bindings: vec![
                    Binding {
                        target_field: "Result".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Optional".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            },
        )
    }

    fn owned_field(owner: SourceOwner, path: &[&str], frame: Option<&[&str]>) -> Node {
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path.iter().map(|part| (*part).into()).collect(),
            },
            frame: frame.map(|path| path.iter().map(|part| (*part).into()).collect()),
        })
    }

    fn position(collection: &[&str], index: usize) -> PositionFrame {
        PositionFrame {
            collection: collection.iter().map(|part| (*part).into()).collect(),
            index,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }
    }

    fn evaluate(
        project: &Project,
        owned: &OwnedContext<'_>,
        registry: &dyn crate::trace::TraceOwnedRegistryView,
        sink: Option<&dyn TraceSink>,
        positions: &[PositionFrame],
    ) -> Result<Instance, EngineError> {
        let first_failure = Cell::new(false);
        let program = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            sink,
            &first_failure,
        )
        .with_owned_context(owned)
        .with_owned_trace_registry(registry);
        crate::eval_scope::eval_scope(
            program,
            &project.root,
            None,
            &[owned.roots.extras_wrapper, owned.roots.primary],
            positions,
            &project.extra_sources,
            None,
            &TraceScope::primary(),
        )
    }

    fn output_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
        events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
                )
            })
            .cloned()
            .collect()
    }

    fn snapshot(observation: &Observation) -> &TraceOwnedSnapshot {
        let Outcome::Snapshot(Ok(snapshot)) = &observation.outcome else {
            panic!(
                "expected successful output witness: {:?}",
                observation.outcome
            );
        };
        snapshot
    }

    #[test]
    fn event_selective_opt_in_preserves_legacy_callbacks_and_exact_output_events() {
        struct ExpressionOnly {
            events: RefCell<Vec<TraceEvent>>,
            contexts: Cell<usize>,
        }
        impl TraceSink for ExpressionOnly {
            fn record(&self, event: TraceEvent) {
                self.events.borrow_mut().push(event);
            }
            fn wants_owned_context(&self) -> bool {
                true
            }
            fn record_with_owned_context(
                &self,
                event: TraceEvent,
                context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
            ) {
                assert!(matches!(
                    &event,
                    TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
                ));
                assert!(context.is_ok());
                self.contexts.set(self.contexts.get() + 1);
                self.record(event);
            }
        }
        struct DefaultOutput(RefCell<Vec<TraceEvent>>);
        impl TraceSink for DefaultOutput {
            fn record(&self, event: TraceEvent) {
                self.0.borrow_mut().push(event);
            }
            fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
                matches!(
                    event,
                    TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
                )
            }
        }
        let project = constant_project(&[]);
        let primary = Instance::Scalar(Value::Int(1));
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut owned = OwnedContext::new(&roots);
        owned.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let legacy = Collector::default();
        let expected = evaluate(&project, &owned, &registry, Some(&legacy), &[]).unwrap();
        assert!(registry.cache.get().is_none());
        let first_failure = Cell::new(false);
        let silent = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            None,
            &first_failure,
        )
        .with_owned_context(&owned)
        .with_owned_trace_registry(&registry);
        record_owned_event(silent, &[], || {
            panic!("an absent sink must not construct the event")
        });
        let declining = Paired::new();
        declining.selected.set(0);
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&declining), &[]).unwrap(),
            expected
        );
        assert_eq!(*declining.events.borrow(), *legacy.0.borrow());
        assert!(declining.observations.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        let old = ExpressionOnly {
            events: RefCell::new(Vec::new()),
            contexts: Cell::new(0),
        };
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&old), &[]).unwrap(),
            expected
        );
        assert_eq!(*old.events.borrow(), *legacy.0.borrow());
        assert_eq!(old.contexts.get(), 2);
        let default = DefaultOutput(RefCell::new(Vec::new()));
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&default), &[]).unwrap(),
            expected
        );
        assert_eq!(*default.0.borrow(), *legacy.0.borrow());
        for (selected, count) in [(1, 2), (2, 1)] {
            let selective = Paired::new();
            selective.selected.set(selected);
            assert_eq!(
                evaluate(&project, &owned, &registry, Some(&selective), &[]).unwrap(),
                expected
            );
            assert_eq!(*selective.events.borrow(), *legacy.0.borrow());
            assert_eq!(selective.observations.borrow().len(), count);
            assert!(selective.observations.borrow().iter().all(|observation| {
                match &observation.event {
                    TraceEvent::TargetFieldWritten { .. } => selected == 1,
                    TraceEvent::TargetProduced { .. } => selected == 2,
                    _ => false,
                }
            }));
        }
        let mut changing = Paired::new();
        changing.stop_after_first = true;
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&changing), &[]).unwrap(),
            expected
        );
        assert_eq!(*changing.events.borrow(), *legacy.0.borrow());
        assert_eq!(changing.observations.borrow().len(), 1);
        assert!(matches!(&changing.observations.borrow()[0].event,
            TraceEvent::TargetFieldWritten { field, .. } if field == "Result"));
        let paired = Paired::new();
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap(),
            expected
        );
        assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
        assert_eq!(paired.observations.borrow().len(), 3);
        assert_eq!(
            output_events(&paired.events.borrow()),
            vec![
                TraceEvent::TargetFieldWritten {
                    scope: TraceScope::primary(),
                    field: "Result".into(),
                    binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
                    positions: Vec::new(),
                    kind: TraceOutputKind::Scalar,
                    value: Some(TraceValue::new(&Value::Int(8))),
                },
                TraceEvent::TargetFieldWritten {
                    scope: TraceScope::primary(),
                    field: "Optional".into(),
                    binding: TraceTargetFieldBinding::StaticBinding { value: 1 },
                    positions: Vec::new(),
                    kind: TraceOutputKind::Scalar,
                    value: Some(TraceValue::new(&Value::Null)),
                },
                TraceEvent::TargetProduced {
                    scope: TraceScope::primary(),
                    positions: Vec::new(),
                    output_path: None,
                    kind: TraceOutputKind::Group,
                },
            ]
        );
        let first_address = changing.root_addresses.borrow()[0];
        assert!(
            paired
                .root_addresses
                .borrow()
                .iter()
                .all(|address| *address == first_address)
        );
    }

    #[test]
    fn actual_output_producers_borrow_original_roots_frames_origins_and_positions() {
        let names = [" Ref/Rows α ", " ref/Rows α "];
        let project = constant_project(&names);
        let primary = Instance::Scalar(Value::Int(1));
        let extras = Instance::Group(
            names
                .iter()
                .map(|name| {
                    (
                        (*name).into(),
                        Instance::Group(
                            vec![(
                                "Rows".into(),
                                Instance::Repeated(vec![Instance::Group(
                                    vec![(
                                        "Value".into(),
                                        Instance::Scalar(Value::String("same".into())),
                                    )]
                                    .into(),
                                )]),
                            )]
                            .into(),
                        ),
                    )
                })
                .collect::<Vec<_>>()
                .into(),
        );
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let first = roots.root(OwnerKey::Named(0)).unwrap();
        let second = roots.root(OwnerKey::Named(1)).unwrap();
        let first_row = &first.field("Rows").unwrap().as_repeated().unwrap()[0];
        let second_row = &second.field("Rows").unwrap().as_repeated().unwrap()[0];
        assert_eq!(first_row, second_row);
        assert!(!std::ptr::eq(first_row, second_row));
        let owned = OwnedContext {
            roots: &roots,
            frames: vec![
                SourceFrame {
                    owner: OwnerKey::Named(0),
                    collection: vec!["Rows".into()],
                    item: first_row,
                    position_slot: 0,
                },
                SourceFrame {
                    owner: OwnerKey::Named(1),
                    collection: vec!["Rows".into()],
                    item: second_row,
                    position_slot: 1,
                },
            ],
            origins: vec![SourceOrigin {
                owner: OwnerKey::Named(0),
                path: Vec::new(),
                item: first,
            }],
            owned_ancestry: true,
        };
        let positions = vec![
            position(&[names[0], "Rows"], 1),
            position(&[names[1], "Rows"], 1),
        ];
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let path_addresses = RefCell::new(Vec::new());
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            assert!(std::ptr::eq(view.project, &project));
            assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
            assert!(std::ptr::eq(view.roots[1].input.unwrap(), first));
            assert!(std::ptr::eq(view.roots[2].input.unwrap(), second));
            assert_eq!(view.phase, TraceOwnedPhase::Output);
            assert!(view.owned_ancestry);
            for index in 0..2 {
                let TraceOwnedOwnerRef::Named {
                    declaration_index,
                    name,
                } = view.frames[index].owner
                else {
                    panic!("named frame identity lost");
                };
                assert_eq!(declaration_index, index);
                assert_eq!(name.as_ptr(), project.extra_sources[index].name.as_ptr());
                assert!(std::ptr::eq(
                    view.frames[index].item,
                    owned.frames[index].item
                ));
                assert_eq!(
                    view.frames[index].collection,
                    owned.frames[index].collection.as_slice()
                );
                assert_eq!(view.frames[index].position_slot, index);
                assert_eq!(
                    view.positions[index].collection,
                    positions[index].collection.as_slice()
                );
                assert_eq!(view.positions[index].index, positions[index].index);
            }
            assert!(std::ptr::eq(view.origins[0].item, first));
            assert_eq!(view.origins[0].path, owned.origins[0].path.as_slice());
            let raw = match event {
                TraceEvent::TargetFieldWritten { positions, .. }
                | TraceEvent::TargetProduced { positions, .. } => positions,
                _ => panic!("unexpected contextual event"),
            };
            assert_eq!(*raw, crate::trace::trace_positions(&positions));
            path_addresses.borrow_mut().push((
                view.frames[0].collection.as_ptr() as usize,
                view.origins[0].path.as_ptr() as usize,
                view.positions[0].collection.as_ptr() as usize,
            ));
        };
        let mut paired = Paired::new();
        paired.check = Some(&check);
        let legacy = Collector::default();
        let expected = evaluate(&project, &owned, &registry, Some(&legacy), &positions).unwrap();
        assert!(registry.cache.get().is_none());
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&paired), &positions).unwrap(),
            expected
        );
        assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
        assert_eq!(paired.observations.borrow().len(), 3);
        assert!(
            paired
                .observations
                .borrow()
                .iter()
                .all(|observation| snapshot(observation).frames.len() == 2)
        );
        let addresses = path_addresses.borrow();
        assert!(addresses.iter().all(|address| *address == addresses[0]));
        assert!(
            paired
                .root_addresses
                .borrow()
                .iter()
                .all(|address| *address == paired.root_addresses.borrow()[0])
        );

        // The shared factory borrows exactly the supplied metadata and phase;
        // the scope checks above exercise the real output producer call sites.
        let direct_check = |_: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            assert_eq!(view.phase, TraceOwnedPhase::Selection);
            assert_eq!(
                view.frames[0].collection.as_ptr(),
                owned.frames[0].collection.as_ptr()
            );
            assert_eq!(
                view.origins[0].path.as_ptr(),
                owned.origins[0].path.as_ptr()
            );
            assert_eq!(
                view.positions[0].collection.as_ptr(),
                positions[0].collection.as_ptr()
            );
        };
        let mut direct = Paired::new();
        direct.check = Some(&direct_check);
        let first_failure = Cell::new(false);
        let program = EvalProgram::new(
            &project.graph,
            &project.user_functions,
            Some(&direct),
            &first_failure,
        )
        .with_owned_context(&owned)
        .with_owned_trace_registry(&registry)
        .with_owned_trace_phase(TraceOwnedPhase::Selection);
        record_owned_event(program, &positions, || TraceEvent::TargetProduced {
            scope: TraceScope::primary(),
            positions: crate::trace::trace_positions(&positions),
            output_path: None,
            kind: TraceOutputKind::Group,
        });
        assert_eq!(direct.observations.borrow().len(), 1);
        assert_eq!(
            snapshot(&direct.observations.borrow()[0]).phase,
            TraceOwnedPhase::Selection
        );
    }

    #[test]
    fn nested_child_and_parent_writes_keep_distinct_receiving_contexts() {
        let project = project(
            &["Ref"],
            vec![
                (
                    0,
                    owned_field(SourceOwner::Primary, &["Rows", "Value"], Some(&["Rows"])),
                ),
                (
                    1,
                    owned_field(
                        SourceOwner::Named { name: "Ref".into() },
                        &["Rows", "Value"],
                        Some(&["Rows"]),
                    ),
                ),
            ],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: vec!["Rows".into()],
                    },
                    output_path: None,
                },
                bindings: vec![Binding {
                    target_field: "Parent".into(),
                    node: 0,
                }],
                children: vec![Scope {
                    target_field: "Child".into(),
                    iteration: ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Named { name: "Ref".into() },
                            path: vec!["Rows".into()],
                        },
                        output_path: None,
                    },
                    bindings: vec![
                        Binding {
                            target_field: "Value".into(),
                            node: 1,
                        },
                        Binding {
                            target_field: "Outer".into(),
                            node: 0,
                        },
                    ],
                    ..Scope::default()
                }],
                ..Scope::default()
            },
        );
        let row = |value: &str| {
            Instance::Group(
                vec![(
                    "Value".into(),
                    Instance::Scalar(Value::String(value.into())),
                )]
                .into(),
            )
        };
        let primary =
            Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![row("parent")]))].into());
        let extras = Instance::Group(
            vec![(
                "Ref".into(),
                Instance::Group(
                    vec![(
                        "Rows".into(),
                        Instance::Repeated(vec![row("one"), row("two")]),
                    )]
                    .into(),
                ),
            )]
            .into(),
        );
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let owned = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let parent = &primary.field("Rows").unwrap().as_repeated().unwrap()[0];
        let named = roots.root(OwnerKey::Named(0)).unwrap();
        let child_rows = named.field("Rows").unwrap().as_repeated().unwrap();
        let parent_scope = TraceScope::primary();
        let child_scope = parent_scope.child("Child", 0);
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            let scope = match event {
                TraceEvent::TargetFieldWritten { scope, .. }
                | TraceEvent::TargetProduced { scope, .. } => scope,
                _ => panic!("unexpected event"),
            };
            assert_eq!(view.phase, TraceOwnedPhase::Output);
            assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
            assert!(std::ptr::eq(view.roots[1].input.unwrap(), named));
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            assert!(std::ptr::eq(view.frames[0].item, parent));
            assert_eq!(view.origins[0].owner, TraceOwnedOwnerRef::Primary);
            assert!(std::ptr::eq(view.origins[0].item, parent));
            assert_eq!(view.positions[view.frames[0].position_slot].index, 1);
            if scope == &child_scope {
                assert_eq!(view.frames.len(), 2);
                let TraceOwnedOwnerRef::Named {
                    declaration_index: 0,
                    name,
                } = view.frames[1].owner
                else {
                    panic!("child owner was not retained");
                };
                assert_eq!(name.as_ptr(), project.extra_sources[0].name.as_ptr());
                let index = view.positions[view.frames[1].position_slot].index;
                assert!(std::ptr::eq(view.frames[1].item, &child_rows[index - 1]));
                assert_eq!(view.frames[0].collection, view.frames[1].collection);
                assert_eq!(view.origins.len(), 2);
                assert_eq!(view.origins[1].owner, view.frames[1].owner);
                assert!(std::ptr::eq(view.origins[1].item, &child_rows[index - 1]));
            } else {
                assert_eq!(scope, &parent_scope);
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.origins.len(), 1);
                assert_eq!(view.positions.len(), 1);
            }
        };
        let mut paired = Paired::new();
        paired.check = Some(&check);
        let legacy = Collector::default();
        let expected = evaluate(&project, &owned, &registry, Some(&legacy), &[]).unwrap();
        let actual = evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap();
        assert_eq!(actual, expected);
        assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
        let parent_output = &actual.as_repeated().unwrap()[0];
        assert_eq!(
            parent_output.field("Parent").unwrap().as_scalar(),
            Some(&Value::String("parent".into()))
        );
        let children = parent_output.field("Child").unwrap().as_repeated().unwrap();
        assert_eq!(children.len(), 2);
        assert!(
            children
                .iter()
                .all(|child| child.field("Outer").unwrap().as_scalar()
                    == Some(&Value::String("parent".into())))
        );
        let observations = paired.observations.borrow();
        assert_eq!(observations.len(), 9);
        assert!(
            observations
                .iter()
                .all(|observation| snapshot(observation).owned_ancestry)
        );
        let parent_write = observations.iter().find(|observation| matches!(&observation.event,
            TraceEvent::TargetFieldWritten { scope, binding: TraceTargetFieldBinding::StaticChild, field, .. }
                if scope == &parent_scope && field == "Child")).unwrap();
        assert_eq!(snapshot(parent_write).frames.len(), 1);
        let child_positions = observations
            .iter()
            .filter_map(|observation| match &observation.event {
                TraceEvent::TargetProduced { scope, .. } if scope == &child_scope => {
                    Some(snapshot(observation).frames[1].index)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(child_positions, vec![1, 2]);
    }

    #[test]
    fn actual_output_phase_and_positions_survive_sorting_and_compaction() {
        let primary = Instance::Group(
            vec![(
                "Rows".into(),
                Instance::Repeated(
                    [(3, true), (1, false), (2, true)]
                        .into_iter()
                        .map(|(rank, keep)| {
                            Instance::Group(
                                vec![
                                    ("Rank".into(), Instance::Scalar(Value::Int(rank))),
                                    ("Keep".into(), Instance::Scalar(Value::Bool(keep))),
                                ]
                                .into(),
                            )
                        })
                        .collect(),
                ),
            )]
            .into(),
        );
        for order in [
            SortFilterOrder::FilterThenSort,
            SortFilterOrder::SortThenFilter,
        ] {
            let project = project(
                &[],
                vec![
                    (
                        0,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (
                        1,
                        owned_field(SourceOwner::Primary, &["Rows", "Rank"], Some(&["Rows"])),
                    ),
                    (
                        2,
                        owned_field(SourceOwner::Primary, &["Rows", "Keep"], Some(&["Rows"])),
                    ),
                    (
                        3,
                        Node::OwnedPosition(OwnedPosition {
                            owner: SourceOwner::Primary,
                            collection: Some(vec!["Rows".into()]),
                        }),
                    ),
                ],
                Scope {
                    iteration: ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Primary,
                            path: vec!["Rows".into()],
                        },
                        output_path: None,
                    },
                    filter: Some(2),
                    sort_by: Some(1),
                    sort_filter_order: order,
                    windows: vec![SequenceWindow::SkipFirst { count: 0 }],
                    bindings: vec![
                        Binding {
                            target_field: "Rank".into(),
                            node: 1,
                        },
                        Binding {
                            target_field: "Position".into(),
                            node: 3,
                        },
                    ],
                    children: vec![Scope {
                        target_field: "Echo".into(),
                        windows: vec![SequenceWindow::First { count: 0 }],
                        bindings: vec![Binding {
                            target_field: "Position".into(),
                            node: 3,
                        }],
                        ..Scope::default()
                    }],
                    ..Scope::default()
                },
            );
            let extras = Instance::Group(Vec::new().into());
            let roots =
                SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
            let owned = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&project, &roots);
            let legacy = Collector::default();
            let paired = Paired::new();
            let expected = evaluate(&project, &owned, &registry, Some(&legacy), &[]).unwrap();
            let actual = evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
            let items = actual.as_repeated().unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(
                items[0].field("Rank").unwrap().as_scalar(),
                Some(&Value::Int(3))
            );
            assert_eq!(
                items[0].field("Position").unwrap().as_scalar(),
                Some(&Value::Int(1))
            );
            assert_eq!(
                items[0]
                    .field("Echo")
                    .unwrap()
                    .field("Position")
                    .unwrap()
                    .as_scalar(),
                Some(&Value::Int(1))
            );
            let observations = paired.observations.borrow();
            assert_eq!(observations.len(), 6);
            for observation in observations.iter() {
                let snapshot = snapshot(observation);
                assert_eq!(snapshot.phase, TraceOwnedPhase::Output);
                assert_eq!(snapshot.frames.len(), 1);
                assert_eq!(snapshot.frames[0].index, 1);
                assert_eq!(
                    snapshot.positions[snapshot.frames[0].position_slot].index,
                    1
                );
                let positions = match &observation.event {
                    TraceEvent::TargetFieldWritten { positions, .. }
                    | TraceEvent::TargetProduced { positions, .. } => positions,
                    _ => panic!("unexpected contextual event"),
                };
                assert_eq!(positions.len(), snapshot.positions.len());
                for (raw, copied) in positions.iter().zip(&snapshot.positions) {
                    assert_eq!(raw.collection, copied.collection.segments);
                    assert_eq!(raw.index, copied.index);
                }
            }
        }
    }

    #[test]
    fn failed_output_preserves_prefix_events_and_exact_engine_error() {
        let project = project(
            &[],
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::Int(8),
                    },
                ),
                (1, Node::Raise { message: None }),
            ],
            Scope {
                bindings: vec![
                    Binding {
                        target_field: "Good".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Failed".into(),
                        node: 1,
                    },
                ],
                ..Scope::default()
            },
        );
        let primary = Instance::Scalar(Value::Int(1));
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut owned = OwnedContext::new(&roots);
        owned.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let legacy = Collector::default();
        let paired = Paired::new();
        let expected = Err(EngineError::MappingException {
            node: 1,
            message: None,
        });
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&legacy), &[]),
            expected
        );
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&paired), &[]),
            expected
        );
        assert_eq!(*paired.events.borrow(), *legacy.0.borrow());
        assert_eq!(
            output_events(&paired.events.borrow()),
            vec![TraceEvent::TargetFieldWritten {
                scope: TraceScope::primary(),
                field: "Good".into(),
                binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
                positions: Vec::new(),
                kind: TraceOutputKind::Scalar,
                value: Some(TraceValue::new(&Value::Int(8))),
            }]
        );
        assert_eq!(paired.observations.borrow().len(), 1);
        assert_eq!(
            snapshot(&paired.observations.borrow()[0]).phase,
            TraceOwnedPhase::Output
        );
    }

    #[test]
    fn output_projection_and_snapshot_errors_are_observer_metadata() {
        let project = constant_project(&["A"]);
        let other = constant_project(&["A"]);
        let primary = Instance::Scalar(Value::Int(1));
        let extras = Instance::Group(Vec::new().into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut owned = OwnedContext::new(&roots);
        owned.owned_ancestry = true;
        let wrong = TraceOwnedRegistry::new(&other, &roots);
        let paired = Paired::new();
        let expected = evaluate(&project, &owned, &wrong, None, &[]).unwrap();
        assert!(wrong.cache.get().is_none());
        assert_eq!(
            evaluate(&project, &owned, &wrong, Some(&paired), &[]).unwrap(),
            expected
        );
        assert_eq!(paired.observations.borrow().len(), 3);
        assert!(
            paired
                .observations
                .borrow()
                .iter()
                .all(|observation| observation.outcome
                    == Outcome::Projection(TraceOwnedContextError::MismatchedRegistry))
        );
        let malformed = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        let expected = evaluate(&project, &malformed, &registry, None, &[]).unwrap();
        assert_eq!(
            evaluate(&project, &malformed, &registry, Some(&paired), &[]).unwrap(),
            expected
        );
        assert_eq!(paired.observations.borrow().len(), 3);
        assert!(
            paired
                .observations
                .borrow()
                .iter()
                .all(|observation| observation.outcome
                    == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
                        at: TraceOwnedLocation::Frame(0),
                        slot: 7,
                    })))
        );
        let project = constant_project(&["Null", "Missing"]);
        let extras = Instance::Group(vec![("Null".into(), Instance::Scalar(Value::Null))].into());
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut owned = OwnedContext::new(&roots);
        owned.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        let expected = evaluate(&project, &owned, &registry, None, &[]).unwrap();
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap(),
            expected
        );
        assert_eq!(paired.observations.borrow().len(), 3);
        for observation in paired.observations.borrow().iter() {
            let snapshot = snapshot(observation);
            assert_eq!(
                snapshot.roots[1]
                    .input
                    .as_ref()
                    .unwrap()
                    .scalar
                    .as_ref()
                    .unwrap()
                    .value_type,
                "null"
            );
            assert!(snapshot.roots[2].input.is_none());
        }
        let project = constant_project(&["Unused"]);
        let extras = Instance::Group(
            vec![
                ("Unused".into(), Instance::Scalar(Value::Int(1))),
                ("Unused".into(), Instance::Scalar(Value::Int(2))),
            ]
            .into(),
        );
        let roots = SourceRoots::new(&project, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut owned = OwnedContext::new(&roots);
        owned.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&project, &roots);
        let paired = Paired::new();
        let expected = evaluate(&project, &owned, &registry, None, &[]).unwrap();
        assert_eq!(
            evaluate(&project, &owned, &registry, Some(&paired), &[]).unwrap(),
            expected
        );
        assert_eq!(paired.observations.borrow().len(), 3);
        assert!(
            paired
                .observations
                .borrow()
                .iter()
                .all(|observation| observation.outcome
                    == Outcome::Projection(TraceOwnedContextError::Registry {
                        declaration_index: 0,
                        source: crate::OwnedSourceError::AmbiguousOwner {
                            name: "Unused".into()
                        },
                    }))
        );
    }

    #[test]
    fn public_observer_admission_and_expression_only_consumers_stay_unchanged() {
        let project = project(
            &[],
            vec![(0, owned_field(SourceOwner::Primary, &["Value"], None))],
            Scope {
                bindings: vec![Binding {
                    target_field: "Result".into(),
                    node: 0,
                }],
                ..Scope::default()
            },
        );
        let primary = Instance::Group(
            vec![(
                "Value".into(),
                Instance::Scalar(Value::String("hello".into())),
            )]
            .into(),
        );
        let paired = Paired::new();
        let execution = crate::ExecutionContext::new(std::path::Path::new("project.json"))
            .with_trace_sink(&paired);
        assert_eq!(
            crate::run_with_context(&project, &primary, &execution),
            Err(EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: None,
                context: crate::OwnedCapabilityContext::Tracing,
            })
        );
        assert!(paired.events.borrow().is_empty());
        assert!(paired.observations.borrow().is_empty());
        for (observers, context) in [
            (
                crate::SourceOwnershipObservers {
                    tracing: false,
                    debugging: true,
                },
                crate::OwnedCapabilityContext::Debugging,
            ),
            (
                crate::SourceOwnershipObservers {
                    tracing: true,
                    debugging: true,
                },
                crate::OwnedCapabilityContext::Tracing,
            ),
        ] {
            assert_eq!(
                crate::check_source_ownership_capability_with_observers(&project, None, observers),
                Err(EngineError::UnsupportedOwnedContext {
                    node: None,
                    function: None,
                    scope: None,
                    context
                })
            );
        }
        let plain = Collector::default();
        let event = TraceEvent::TargetProduced {
            scope: TraceScope::primary(),
            positions: Vec::new(),
            output_path: None,
            kind: TraceOutputKind::Group,
        };
        assert!(!plain.wants_owned_context_for(&event));
        struct OldOptIn;
        impl TraceSink for OldOptIn {
            fn record(&self, _: TraceEvent) {}
            fn wants_owned_context(&self) -> bool {
                true
            }
        }
        assert!(!OldOptIn.wants_owned_context_for(&event));
        assert!(OldOptIn.wants_owned_context_for(&TraceEvent::NodeValue {
            node: 0,
            positions: Vec::new(),
            value: TraceValue::new(&Value::Int(8)),
        }));
        assert!(
            crate::check_source_ownership_capability_with_observers(
                &project,
                None,
                crate::SourceOwnershipObservers::default(),
            )
            .is_ok()
        );
    }
}
