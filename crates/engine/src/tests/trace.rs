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

#[path = "owned_scope_observers.rs"]
mod owned_scope_observers;

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
        SourceOwnershipObservers, TraceOwnedDomain, TraceOwnedFrameRef, TraceOwnedLocation,
        TraceOwnedOriginRef, TraceOwnedOwner, TraceOwnedOwnerRef, TraceOwnedPhase,
        TraceOwnedPositionRef, TraceOwnedRootRef, TraceOwnedWitness, TraceOwnedWitnessError,
        TracePosition, check_source_ownership_capability_with_observers,
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
                    kind: crate::owned_context::OccurrenceKind::Item,
                    collection: vec!["Rows".into()],
                    item: outer,
                    position_slot: 0,
                },
                SourceFrame {
                    owner: OwnerKey::Named(0),
                    kind: crate::owned_context::OccurrenceKind::Item,
                    collection: vec!["Rows".into(), "Sub".into()],
                    item: inner,
                    position_slot: 1,
                },
            ],
            origins: vec![SourceOrigin {
                kind: crate::owned_context::SourceOriginKind::Ordinary,
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
                generated_origin: None,
                collection: vec![project.extra_sources[0].name.clone(), "Rows".into()],
                index: 9,
                grouped: false,
                join: None,
                join_position: None,
                document_path: None,
            },
            PositionFrame {
                generated_origin: None,
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
        assert!(std::ptr::eq(
            context
                .position(
                    &outer_field.source.owner,
                    Some(&["Rows".into()]),
                    &positions
                )
                .unwrap()
                .expect("the outer frame names a position"),
            &positions[0]
        ));
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
        dynamic.extra_sources[0].dynamic_path =
            Some(mapping::DynamicSourcePath::legacy(0, Vec::new()));
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
    fn legacy_position_fields_and_public_basic_observer_admission_remain_exact() {
        use crate::source_iteration::PositionFrame;
        let position = PositionFrame {
            generated_origin: None,
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
        for observers in [
            SourceOwnershipObservers {
                tracing: true,
                debugging: false,
            },
            SourceOwnershipObservers {
                tracing: false,
                debugging: true,
            },
            SourceOwnershipObservers {
                tracing: true,
                debugging: true,
            },
        ] {
            assert_eq!(
                check_source_ownership_capability_with_observers(&project, None, observers),
                Ok(())
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
                    kind: crate::owned_context::OccurrenceKind::Item,
                    collection: vec!["Rows".into()],
                    item: first_row,
                    position_slot: 0,
                },
                SourceFrame {
                    owner: OwnerKey::Named(1),
                    kind: crate::owned_context::OccurrenceKind::Item,
                    collection: vec!["Rows".into()],
                    item: second_row,
                    position_slot: 1,
                },
            ],
            origins: vec![SourceOrigin {
                kind: crate::owned_context::SourceOriginKind::Ordinary,
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
                generated_origin: None,
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
            for (index, position) in positions.iter().enumerate() {
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
                    position.collection.as_ptr()
                );
                assert_eq!(view.positions[index].index, position.index);
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
                kind: crate::owned_context::OccurrenceKind::Item,
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
    fn public_basic_trace_runs_with_expression_witnesses() {
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
            Ok(Instance::Group(
                vec![(
                    "Result".into(),
                    Instance::Scalar(Value::String("hello".into()))
                )]
                .into()
            ))
        );
        assert!(!paired.events.borrow().is_empty());
        assert_eq!(paired.observations.borrow().len(), 1);
        assert!(matches!(
            &paired.observations.borrow()[0].outcome,
            Outcome::Snapshot(Ok(snapshot)) if snapshot.phase == TraceOwnedPhase::Output
        ));
        for observers in [
            crate::SourceOwnershipObservers {
                tracing: false,
                debugging: true,
            },
            crate::SourceOwnershipObservers {
                tracing: true,
                debugging: true,
            },
        ] {
            assert_eq!(
                crate::check_source_ownership_capability_with_observers(&project, None, observers),
                Ok(())
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
            generated_origin: None,
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
                    kind: crate::owned_context::OccurrenceKind::Item,
                    collection: vec!["Rows".into()],
                    item: first_row,
                    position_slot: 0,
                },
                SourceFrame {
                    owner: OwnerKey::Named(1),
                    kind: crate::owned_context::OccurrenceKind::Item,
                    collection: vec!["Rows".into()],
                    item: second_row,
                    position_slot: 1,
                },
            ],
            origins: vec![SourceOrigin {
                kind: crate::owned_context::SourceOriginKind::Ordinary,
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
            for (index, position) in positions.iter().enumerate() {
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
                    position.collection.as_slice()
                );
                assert_eq!(view.positions[index].index, position.index);
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
                kind: crate::owned_context::OccurrenceKind::Item,
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
    fn public_output_witnesses_and_expression_only_opt_in_stay_distinct() {
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
            Ok(Instance::Group(
                vec![(
                    "Result".into(),
                    Instance::Scalar(Value::String("hello".into()))
                )]
                .into()
            ))
        );
        assert_eq!(paired.observations.borrow().len(), 2);
        assert!(
            paired
                .observations
                .borrow()
                .iter()
                .all(|observation| matches!(
                    &observation.outcome,
                    Outcome::Snapshot(Ok(snapshot)) if snapshot.phase == TraceOwnedPhase::Output
                ))
        );
        for observers in [
            crate::SourceOwnershipObservers {
                tracing: false,
                debugging: true,
            },
            crate::SourceOwnershipObservers {
                tracing: true,
                debugging: true,
            },
        ] {
            assert_eq!(
                crate::check_source_ownership_capability_with_observers(&project, None, observers),
                Ok(())
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

mod owned_scope_lifecycle {
    use super::{Collector, field_trace_project};
    use crate::eval_expr::EvalProgram;
    use crate::owned_context::{
        OwnedContext, OwnedLimits, OwnerKey, SourceFrame, SourceOrigin, SourceRoots,
    };
    use crate::source_iteration::PositionFrame;
    use crate::trace::{
        TraceOwnedRegistry, TraceOwnedRegistryView, basic_scope_iteration, scope_iteration,
    };
    use crate::{
        EngineError, OwnedSourceError, TraceEvent, TraceIteration, TraceOutputKind,
        TraceOwnedContextError, TraceOwnedLocation, TraceOwnedOwnerRef, TraceOwnedPhase,
        TraceOwnedSnapshot, TraceOwnedWitness, TraceOwnedWitnessError, TraceScope, TraceSink,
        TraceTargetFieldBinding, TraceValue,
    };
    use ir::{Instance, ScalarType, SchemaNode, Value};
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
    type Check<'a> = dyn Fn(&TraceEvent, &TraceOwnedWitness<'_>) + 'a;
    struct Lifecycle<'a> {
        events: RefCell<Vec<TraceEvent>>,
        contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
        root_addresses: RefCell<Vec<usize>>,
        selected: Cell<u8>,
        stop_after_first: bool,
        check: Option<&'a Check<'a>>,
    }
    impl Lifecycle<'_> {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                contexts: RefCell::new(Vec::new()),
                root_addresses: RefCell::new(Vec::new()),
                selected: Cell::new(3),
                stop_after_first: false,
                check: None,
            }
        }
    }
    impl TraceSink for Lifecycle<'_> {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }
        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            match event {
                TraceEvent::ScopeStarted { .. } => self.selected.get() & 1 != 0,
                TraceEvent::ScopeFinished { .. } => self.selected.get() & 2 != 0,
                _ => false,
            }
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(
                event,
                TraceEvent::ScopeStarted { .. } | TraceEvent::ScopeFinished { .. }
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
            self.contexts.borrow_mut().push((event.clone(), outcome));
            if self.stop_after_first {
                self.selected.set(0);
            }
            self.record(event);
        }
    }
    fn path(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).into()).collect()
    }
    fn text(value: &str) -> Instance {
        Instance::Scalar(Value::String(value.into()))
    }
    fn group(fields: Vec<(&str, Instance)>) -> Instance {
        Instance::Group(
            fields
                .into_iter()
                .map(|(name, value)| (name.into(), value))
                .collect::<Vec<_>>()
                .into(),
        )
    }
    fn row(value: &str) -> Instance {
        group(vec![("Value", text(value))])
    }
    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Input",
            vec![
                SchemaNode::scalar("Value", ScalarType::String),
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::scalar("Value", ScalarType::String),
                        SchemaNode::scalar("Rank", ScalarType::Int),
                        SchemaNode::scalar("Keep", ScalarType::Bool),
                    ],
                )
                .repeating(),
            ],
        )
    }
    fn project(names: &[&str], nodes: Vec<(u32, Node)>, root: Scope) -> Project {
        let mut p = field_trace_project(
            SchemaNode::group("Output", Vec::new()),
            Graph {
                nodes: nodes.into_iter().collect(),
            },
            root,
        );
        p.source = schema();
        p.extra_sources = names
            .iter()
            .map(|name| NamedSource {
                name: (*name).into(),
                path: String::new(),
                schema: schema(),
                options: Default::default(),
                dynamic_path: None,
            })
            .collect();
        p
    }
    fn constant() -> Project {
        project(
            &[],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(7),
                },
            )],
            Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        )
    }
    fn owned_field(owner: SourceOwner, parts: &[&str]) -> Node {
        Node::OwnedSourceField(OwnedSourceField {
            source: OwnedSourceRef {
                owner,
                path: path(parts),
            },
            frame: Some(path(&["Rows"])),
        })
    }
    fn repeated(owner: SourceOwner) -> Project {
        project(
            &["Ref", " Ref 雪 "],
            vec![(0, owned_field(owner.clone(), &["Rows", "Value"]))],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner,
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        )
    }
    fn position(index: usize) -> PositionFrame {
        PositionFrame {
            generated_origin: None,
            collection: path(&["Rows"]),
            index,
            grouped: false,
            join: None,
            join_position: None,
            document_path: None,
        }
    }
    fn run(
        p: &Project,
        state: &OwnedContext<'_>,
        registry: &dyn TraceOwnedRegistryView,
        sink: Option<&dyn TraceSink>,
        phase: TraceOwnedPhase,
        positions: &[PositionFrame],
    ) -> Result<Instance, EngineError> {
        let failed = Cell::new(false);
        let program = EvalProgram::new(&p.graph, &p.user_functions, sink, &failed)
            .with_owned_context(state)
            .with_owned_trace_registry(registry)
            .with_owned_trace_phase(phase);
        let result = crate::eval_scope::eval_scope(
            program,
            &p.root,
            None,
            &[state.roots.extras_wrapper, state.roots.primary],
            positions,
            &p.extra_sources,
            None,
            &TraceScope::primary(),
        );
        eprintln!(
            "INPUT lifecycle complete_project={p:#?} positions={positions:#?} phase={phase:?} RAW complete_result={result:#?}",
            positions = crate::trace::trace_positions(positions)
        );
        result
    }
    fn boundaries(events: &[TraceEvent]) -> Vec<TraceEvent> {
        events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    TraceEvent::ScopeStarted { .. } | TraceEvent::ScopeFinished { .. }
                )
            })
            .cloned()
            .collect()
    }
    fn snapshot(outcome: &Outcome) -> &TraceOwnedSnapshot {
        let Outcome::Snapshot(Ok(view)) = outcome else {
            panic!("expected complete valid witness: {outcome:?}");
        };
        view
    }

    #[test]
    fn old_legacy_events_and_event_selective_lifecycle_dispatch_remain_exact() {
        let p = constant();
        let primary = row("root");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let legacy = Collector::default();
        let expected_result = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&legacy),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected_result.clone())
        );
        let expected_events = vec![
            TraceEvent::ScopeStarted {
                scope: TraceScope::primary(),
                iteration: TraceIteration::Once,
                positions: Vec::new(),
            },
            TraceEvent::NodeValue {
                node: 0,
                positions: Vec::new(),
                value: TraceValue::new(&Value::Int(7)),
            },
            TraceEvent::TargetFieldWritten {
                scope: TraceScope::primary(),
                field: "Value".into(),
                binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
                positions: Vec::new(),
                kind: TraceOutputKind::Scalar,
                value: Some(TraceValue::new(&Value::Int(7))),
            },
            TraceEvent::TargetProduced {
                scope: TraceScope::primary(),
                positions: Vec::new(),
                output_path: None,
                kind: TraceOutputKind::Group,
            },
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: 1,
                produced: 1,
                kind: TraceOutputKind::Group,
            },
        ];
        // The unchanged legacy route already records its Once candidate.
        // This records the preexisting legacy Once sequence independently.
        let mut legacy_expected_events = expected_events.clone();
        legacy_expected_events.insert(
            1,
            TraceEvent::IterationCandidate {
                scope: TraceScope::primary(),
                ordinal: 1,
                positions: Vec::new(),
                source_row: None,
            },
        );
        assert_eq!(*legacy.0.borrow(), legacy_expected_events);
        // Candidate observation is now complete in the Basic Once route too.
        // The unchanged legacy six-event oracle above remains exact.
        let expected_events = legacy_expected_events;
        assert!(registry.cache.get().is_none());
        state.owned_ancestry = true;
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                None,
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected_result.clone())
        );
        assert!(registry.cache.get().is_none());
        let declining = Lifecycle::new();
        declining.selected.set(0);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&declining),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected_result.clone())
        );
        assert_eq!(*declining.events.borrow(), expected_events);
        assert!(declining.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        for (selected, count) in [(1, 1), (2, 1), (3, 2)] {
            let paired = Lifecycle::new();
            paired.selected.set(selected);
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&paired),
                    TraceOwnedPhase::Structural,
                    &[]
                ),
                Ok(expected_result.clone())
            );
            assert_eq!(*paired.events.borrow(), expected_events);
            assert_eq!(paired.contexts.borrow().len(), count);
        }
        let mut changing = Lifecycle::new();
        changing.stop_after_first = true;
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&changing),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected_result)
        );
        assert_eq!(*changing.events.borrow(), expected_events);
        assert_eq!(changing.contexts.borrow().len(), 1);
        assert!(matches!(
            changing.contexts.borrow()[0].0,
            TraceEvent::ScopeStarted { .. }
        ));
        struct OldBoolean {
            pairs: Cell<usize>,
        }
        impl TraceSink for OldBoolean {
            fn record(&self, _: TraceEvent) {}
            fn wants_owned_context(&self) -> bool {
                true
            }
            fn record_with_owned_context(
                &self,
                event: TraceEvent,
                context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
            ) {
                assert!(matches!(
                    event,
                    TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
                ));
                assert!(context.is_ok());
                self.pairs.set(self.pairs.get() + 1);
            }
        }
        let old = OldBoolean {
            pairs: Cell::new(0),
        };
        run(
            &p,
            &state,
            &registry,
            Some(&old),
            TraceOwnedPhase::Structural,
            &[],
        )
        .unwrap();
        assert_eq!(old.pairs.get(), 1);
        let mut scalar = constant();
        scalar.root.construction = mapping::ScopeConstruction::Scalar { value: 0 };
        scalar.root.bindings.clear();
        let roots = SourceRoots::new(&scalar, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&scalar, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &scalar,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(Instance::Scalar(Value::Int(7)))
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![
                TraceEvent::ScopeStarted {
                    scope: TraceScope::primary(),
                    iteration: TraceIteration::Once,
                    positions: Vec::new()
                },
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: 1,
                    produced: 1,
                    kind: TraceOutputKind::Scalar
                }
            ]
        );
    }

    #[test]
    fn zero_singleton_and_mixed_candidates_keep_exact_owner_lifecycle_counts() {
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named { name: "Ref".into() },
            SourceOwner::Named {
                name: " Ref 雪 ".into(),
            },
        ] {
            for case in 0..3 {
                let p = repeated(owner.clone());
                let items = match case {
                    0 => Vec::new(),
                    1 => vec![row("one")],
                    _ => vec![
                        Instance::Scalar(Value::Null),
                        row("one"),
                        Instance::Scalar(Value::JsonNull(ir::JsonNull)),
                        row("two"),
                    ],
                };
                let count = if case == 0 {
                    0
                } else if case == 1 {
                    1
                } else {
                    2
                };
                let primary = group(vec![("Rows", Instance::Repeated(items.clone()))]);
                let extras = group(vec![
                    (
                        "Ref",
                        group(vec![("Rows", Instance::Repeated(items.clone()))]),
                    ),
                    (" Ref 雪 ", group(vec![("Rows", Instance::Repeated(items))])),
                ]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let paired = Lifecycle::new();
                let plain = Collector::default();
                let expected = Instance::Repeated(if count == 0 {
                    Vec::new()
                } else if count == 1 {
                    vec![row("one")]
                } else {
                    vec![row("one"), row("two")]
                });
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&paired),
                        TraceOwnedPhase::Structural,
                        &[]
                    ),
                    Ok(expected.clone())
                );
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&plain),
                        TraceOwnedPhase::Structural,
                        &[]
                    ),
                    Ok(expected)
                );
                assert_eq!(*paired.events.borrow(), *plain.0.borrow());
                assert_eq!(
                    boundaries(&paired.events.borrow()),
                    vec![
                        TraceEvent::ScopeStarted {
                            scope: TraceScope::primary(),
                            iteration: TraceIteration::OwnedSource {
                                source: OwnedSourceRef {
                                    owner: owner.clone(),
                                    path: path(&["Rows"])
                                }
                            },
                            positions: Vec::new()
                        },
                        TraceEvent::ScopeFinished {
                            scope: TraceScope::primary(),
                            candidates: count,
                            produced: count,
                            kind: TraceOutputKind::Repeated
                        },
                    ]
                );
                assert_eq!(paired.contexts.borrow().len(), 2);
                assert!(paired.contexts.borrow().iter().all(|(_, outcome)| {
                    let view = snapshot(outcome);
                    view.phase == TraceOwnedPhase::Structural
                        && view.frames.is_empty()
                        && view.positions.is_empty()
                }));
                if case == 2 {
                    let indexes = paired
                        .events
                        .borrow()
                        .iter()
                        .filter_map(|event| match event {
                            TraceEvent::TargetProduced { positions, .. } => {
                                Some(positions[0].index)
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    assert_eq!(indexes, vec![2, 4]);
                }
            }
        }
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named { name: "Ref".into() },
            SourceOwner::Named {
                name: " Ref 雪 ".into(),
            },
        ] {
            for case in 0..3 {
                let p = project(
                    &["Ref", " Ref 雪 "],
                    vec![(
                        0,
                        Node::Const {
                            value: Value::Int(7),
                        },
                    )],
                    Scope {
                        iteration: ScopeIteration::OwnedSource {
                            source: OwnedSourceRef {
                                owner: owner.clone(),
                                path: Vec::new(),
                            },
                            output_path: None,
                        },
                        bindings: vec![Binding {
                            target_field: "Value".into(),
                            node: 0,
                        }],
                        ..Default::default()
                    },
                );
                let input = match case {
                    0 => Instance::Repeated(Vec::new()),
                    1 => row("one"),
                    _ => Instance::Repeated(vec![
                        Instance::Scalar(Value::Null),
                        row("one"),
                        Instance::Scalar(Value::JsonNull(ir::JsonNull)),
                        row("two"),
                    ]),
                };
                let count = if case == 0 {
                    0
                } else if case == 1 {
                    1
                } else {
                    2
                };
                let primary = input.clone();
                let extras = group(vec![("Ref", input.clone()), (" Ref 雪 ", input)]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let paired = Lifecycle::new();
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&paired),
                        TraceOwnedPhase::Structural,
                        &[]
                    ),
                    Ok(Instance::Repeated(
                        (0..count)
                            .map(|_| group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
                            .collect()
                    ))
                );
                assert_eq!(
                    boundaries(&paired.events.borrow()),
                    vec![
                        TraceEvent::ScopeStarted {
                            scope: TraceScope::primary(),
                            iteration: TraceIteration::OwnedSource {
                                source: OwnedSourceRef {
                                    owner: owner.clone(),
                                    path: Vec::new()
                                }
                            },
                            positions: Vec::new()
                        },
                        TraceEvent::ScopeFinished {
                            scope: TraceScope::primary(),
                            candidates: count,
                            produced: count,
                            kind: TraceOutputKind::Repeated
                        }
                    ]
                );
            }
        }
        let mut p = constant();
        p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        let primary = group(vec![("Rows", Instance::Repeated(vec![row("one")]))]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(Instance::Repeated(vec![group(vec![(
                "Value",
                Instance::Scalar(Value::Int(7))
            )])]))
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![
                TraceEvent::ScopeStarted {
                    scope: TraceScope::primary(),
                    iteration: TraceIteration::Source {
                        path: path(&["Rows"])
                    },
                    positions: Vec::new()
                },
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: 1,
                    produced: 1,
                    kind: TraceOutputKind::Repeated
                }
            ]
        );
    }

    #[test]
    fn receiving_context_references_and_supplied_phase_are_not_derived_from_boundary_names() {
        let p = constant();
        let primary = group(vec![("Rows", Instance::Repeated(vec![row("one")]))]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let item = &primary.field("Rows").unwrap().as_repeated().unwrap()[0];
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: path(&["Rows"]),
                item,
                position_slot: 0,
            }],
            origins: vec![SourceOrigin {
                kind: crate::owned_context::SourceOriginKind::Ordinary,
                owner: OwnerKey::Primary,
                path: path(&["Rows"]),
                item,
            }],
            owned_ancestry: true,
        };
        let positions = [position(1)];
        let registry = TraceOwnedRegistry::new(&p, &roots);
        for phase in [
            TraceOwnedPhase::Structural,
            TraceOwnedPhase::Selection,
            TraceOwnedPhase::Output,
        ] {
            let check = |_: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                assert_eq!(view.phase, phase);
                assert!(std::ptr::eq(view.project, &p));
                assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
                assert!(std::ptr::eq(view.frames[0].item, item));
                assert!(std::ptr::eq(view.origins[0].item, item));
                assert_eq!(
                    view.frames[0].collection.as_ptr(),
                    state.frames[0].collection.as_ptr()
                );
                assert_eq!(
                    view.origins[0].path.as_ptr(),
                    state.origins[0].path.as_ptr()
                );
                assert_eq!(
                    view.positions[0].collection.as_ptr(),
                    positions[0].collection.as_ptr()
                );
                assert_eq!(view.positions[0].index, 1);
            };
            let mut paired = Lifecycle::new();
            paired.check = Some(&check);
            assert_eq!(
                run(&p, &state, &registry, Some(&paired), phase, &positions),
                Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
            );
            assert_eq!(
                boundaries(&paired.events.borrow()),
                vec![
                    TraceEvent::ScopeStarted {
                        scope: TraceScope::primary(),
                        iteration: TraceIteration::Once,
                        positions: crate::trace::trace_positions(&positions)
                    },
                    TraceEvent::ScopeFinished {
                        scope: TraceScope::primary(),
                        candidates: 1,
                        produced: 1,
                        kind: TraceOutputKind::Group
                    }
                ]
            );
            assert_eq!(paired.contexts.borrow().len(), 2);
            assert!(
                paired
                    .contexts
                    .borrow()
                    .iter()
                    .all(|(_, outcome)| snapshot(outcome).phase == phase)
            );
            assert_eq!(
                paired.root_addresses.borrow()[0],
                paired.root_addresses.borrow()[1]
            );
        }
        let mut named = constant();
        named.extra_targets.push(mapping::NamedTarget {
            name: " Target 雪 ".into(),
            path: None,
            schema: SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]),
            options: Default::default(),
            root: named.root.clone(),
        });
        let roots = SourceRoots::new(&named, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&named, &roots);
        let paired = Lifecycle::new();
        let failed = Cell::new(false);
        let program = EvalProgram::new(&named.graph, &named.user_functions, Some(&paired), &failed)
            .with_owned_context(&state)
            .with_owned_trace_registry(&registry)
            .with_owned_trace_phase(TraceOwnedPhase::Selection);
        let target = &named.extra_targets[0];
        let scope = TraceScope::named(&target.name, 0);
        let result = crate::eval_scope::eval_scope(
            program,
            &target.root,
            Some(&target.schema),
            &[&extras, &primary],
            &[],
            &named.extra_sources,
            None,
            &scope,
        );
        eprintln!("INPUT real_named_target={named:#?} RAW result={result:#?}");
        assert_eq!(
            result,
            Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![
                TraceEvent::ScopeStarted {
                    scope: scope.clone(),
                    iteration: TraceIteration::Once,
                    positions: Vec::new()
                },
                TraceEvent::ScopeFinished {
                    scope,
                    candidates: 1,
                    produced: 1,
                    kind: TraceOutputKind::Group
                }
            ]
        );
        assert!(
            paired
                .contexts
                .borrow()
                .iter()
                .all(|(_, outcome)| snapshot(outcome).phase == TraceOwnedPhase::Selection)
        );
    }

    #[test]
    fn nested_owned_scopes_start_and_finish_in_the_actual_parent_receiving_context() {
        let p = project(
            &["Ref"],
            vec![
                (0, owned_field(SourceOwner::Primary, &["Rows", "Value"])),
                (
                    1,
                    owned_field(
                        SourceOwner::Named { name: "Ref".into() },
                        &["Rows", "Value"],
                    ),
                ),
            ],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Rows"]),
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
                            path: path(&["Rows"]),
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
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let primary = group(vec![("Rows", Instance::Repeated(vec![row("parent")]))]);
        let extras = group(vec![(
            "Ref",
            group(vec![(
                "Rows",
                Instance::Repeated(vec![row("one"), row("two")]),
            )]),
        )]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let parent = &primary.field("Rows").unwrap().as_repeated().unwrap()[0];
        let parent_scope = TraceScope::primary();
        let child_scope = parent_scope.child("Child", 0);
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            let scope = match event {
                TraceEvent::ScopeStarted { scope, .. }
                | TraceEvent::ScopeFinished { scope, .. } => scope,
                _ => unreachable!(),
            };
            assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
            assert!(std::ptr::eq(
                view.roots[1].input.unwrap(),
                roots.root(OwnerKey::Named(0)).unwrap()
            ));
            if scope == &child_scope {
                assert_eq!(view.phase, TraceOwnedPhase::Output);
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.origins.len(), 1);
                assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
                assert!(std::ptr::eq(view.frames[0].item, parent));
                assert!(std::ptr::eq(view.origins[0].item, parent));
                assert_eq!(view.positions[view.frames[0].position_slot].index, 1);
            } else {
                assert_eq!(scope, &parent_scope);
                assert_eq!(view.phase, TraceOwnedPhase::Structural);
                assert!(view.frames.is_empty());
                assert!(view.origins.is_empty());
                assert!(view.positions.is_empty());
            }
        };
        let mut paired = Lifecycle::new();
        paired.check = Some(&check);
        let expected = Instance::Repeated(vec![group(vec![
            ("Parent", text("parent")),
            (
                "Child",
                Instance::Repeated(vec![
                    group(vec![("Value", text("one")), ("Outer", text("parent"))]),
                    group(vec![("Value", text("two")), ("Outer", text("parent"))]),
                ]),
            ),
        ])]);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![
                TraceEvent::ScopeStarted {
                    scope: parent_scope.clone(),
                    iteration: TraceIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Primary,
                            path: path(&["Rows"])
                        }
                    },
                    positions: Vec::new()
                },
                TraceEvent::ScopeStarted {
                    scope: child_scope.clone(),
                    iteration: TraceIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Named { name: "Ref".into() },
                            path: path(&["Rows"])
                        }
                    },
                    positions: crate::trace::trace_positions(&[position(1)])
                },
                TraceEvent::ScopeFinished {
                    scope: child_scope.clone(),
                    candidates: 2,
                    produced: 2,
                    kind: TraceOutputKind::Repeated
                },
                TraceEvent::ScopeFinished {
                    scope: parent_scope.clone(),
                    candidates: 1,
                    produced: 1,
                    kind: TraceOutputKind::Repeated
                },
            ]
        );
        assert_eq!(paired.contexts.borrow().len(), 4);
    }

    #[test]
    fn lifecycle_counts_preserve_control_compaction_and_complete_first_candidates() {
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(
                [(3, true), (1, false), (2, true)]
                    .into_iter()
                    .map(|(rank, keep)| {
                        group(vec![
                            ("Rank", Instance::Scalar(Value::Int(rank))),
                            ("Keep", Instance::Scalar(Value::Bool(keep))),
                        ])
                    })
                    .collect(),
            ),
        )]);
        for order in [
            SortFilterOrder::FilterThenSort,
            SortFilterOrder::SortThenFilter,
        ] {
            let p = project(
                &[],
                vec![
                    (
                        0,
                        Node::Const {
                            value: Value::Int(1),
                        },
                    ),
                    (1, owned_field(SourceOwner::Primary, &["Rows", "Rank"])),
                    (2, owned_field(SourceOwner::Primary, &["Rows", "Keep"])),
                    (
                        3,
                        Node::OwnedPosition(OwnedPosition {
                            owner: SourceOwner::Primary,
                            collection: Some(path(&["Rows"])),
                        }),
                    ),
                ],
                Scope {
                    iteration: ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Primary,
                            path: path(&["Rows"]),
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
                    ..Default::default()
                },
            );
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let paired = Lifecycle::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&paired),
                    TraceOwnedPhase::Structural,
                    &[]
                ),
                Ok(Instance::Repeated(vec![group(vec![
                    ("Rank", Instance::Scalar(Value::Int(3))),
                    ("Position", Instance::Scalar(Value::Int(1)))
                ])]))
            );
            assert_eq!(
                boundaries(&paired.events.borrow()),
                vec![
                    TraceEvent::ScopeStarted {
                        scope: TraceScope::primary(),
                        iteration: TraceIteration::OwnedSource {
                            source: OwnedSourceRef {
                                owner: SourceOwner::Primary,
                                path: path(&["Rows"])
                            }
                        },
                        positions: Vec::new()
                    },
                    TraceEvent::ScopeFinished {
                        scope: TraceScope::primary(),
                        candidates: 3,
                        produced: 1,
                        kind: TraceOutputKind::Repeated
                    }
                ]
            );
            assert!(paired.contexts.borrow().iter().all(|(_, outcome)| {
                let view = snapshot(outcome);
                view.phase == TraceOwnedPhase::Structural
                    && view.frames.is_empty()
                    && view.positions.is_empty()
            }));
        }
        for empty in [false, true] {
            let mut p = repeated(SourceOwner::Primary);
            p.root.iteration_output = mapping::IterationOutput::First;
            let primary = group(vec![(
                "Rows",
                Instance::Repeated(if empty {
                    Vec::new()
                } else {
                    vec![row("one"), row("two"), row("three")]
                }),
            )]);
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let paired = Lifecycle::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&paired),
                    TraceOwnedPhase::Structural,
                    &[]
                ),
                Ok(if empty { group(Vec::new()) } else { row("one") })
            );
            assert_eq!(
                boundaries(&paired.events.borrow())[1],
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: if empty { 0 } else { 3 },
                    produced: if empty { 0 } else { 1 },
                    kind: TraceOutputKind::Group
                }
            );
        }
    }

    #[test]
    fn failed_scopes_keep_successful_prefix_and_never_fabricate_a_finish() {
        let p = project(
            &[],
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::Int(7),
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
                ..Default::default()
            },
        );
        let primary = row("root");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Err(EngineError::MappingException {
                node: 1,
                message: None
            })
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![TraceEvent::ScopeStarted {
                scope: TraceScope::primary(),
                iteration: TraceIteration::Once,
                positions: Vec::new()
            }]
        );
        let writes = paired
            .events
            .borrow()
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    TraceEvent::TargetFieldWritten { .. } | TraceEvent::TargetProduced { .. }
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            writes,
            vec![TraceEvent::TargetFieldWritten {
                scope: TraceScope::primary(),
                field: "Good".into(),
                binding: TraceTargetFieldBinding::StaticBinding { value: 0 },
                positions: Vec::new(),
                kind: TraceOutputKind::Scalar,
                value: Some(TraceValue::new(&Value::Int(7)))
            }]
        );
        assert_eq!(
            snapshot(&paired.contexts.borrow()[0].1).phase,
            TraceOwnedPhase::Selection
        );
        let p = project(
            &["Missing"],
            Vec::new(),
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Named {
                            name: "Missing".into(),
                        },
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                ..Default::default()
            },
        );
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Err(EngineError::OwnedSourceAccess {
                node: None,
                scope: Some(Box::new(TraceScope::primary().source_scope)),
                source: OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named {
                        name: "Missing".into()
                    }
                }
            })
        );
        assert_eq!(
            boundaries(&paired.events.borrow()),
            vec![TraceEvent::ScopeStarted {
                scope: TraceScope::primary(),
                iteration: TraceIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Named {
                            name: "Missing".into()
                        },
                        path: path(&["Rows"])
                    }
                },
                positions: Vec::new()
            }]
        );
    }

    #[test]
    fn lifecycle_projection_and_snapshot_failures_remain_observer_metadata() {
        let p = constant();
        let mut other = constant();
        // Distinct empty Vecs do not establish declaration-slice identity mismatch.
        other.extra_sources = project(&["Unused"], Vec::new(), Scope::default()).extra_sources;
        let primary = row("root");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        assert!(!std::ptr::eq(
            other.extra_sources.as_slice(),
            roots.declarations()
        ));
        let wrong = TraceOwnedRegistry::new(&other, &roots);
        let paired = Lifecycle::new();
        let expected = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
        assert_eq!(
            run(
                &p,
                &state,
                &wrong,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(paired.contexts.borrow().len(), 2);
        eprintln!(
            "RAW lifecycle actual foreign-registry outcomes={:#?}",
            paired.contexts.borrow()
        );
        assert!(paired.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Projection(TraceOwnedContextError::MismatchedRegistry)));
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected)
        );
        assert!(paired.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 7
            }))));
        let p = project(
            &["Null", "Missing"],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(7),
                },
            )],
            Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        );
        let extras = group(vec![("Null", Instance::Scalar(Value::Null))]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
        );
        assert_eq!(paired.contexts.borrow().len(), 2);
        for (_, outcome) in paired.contexts.borrow().iter() {
            let view = snapshot(outcome);
            assert_eq!(
                view.roots[1]
                    .input
                    .as_ref()
                    .unwrap()
                    .scalar
                    .as_ref()
                    .unwrap()
                    .value_type,
                "null"
            );
            assert!(view.roots[2].input.is_none());
        }
        let p = project(
            &["Unused"],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(7),
                },
            )],
            Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        );
        let extras = group(vec![("Unused", text("one")), ("Unused", text("two"))]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
        );
        assert_eq!(paired.contexts.borrow().len(), 2);
        assert!(paired.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Projection(TraceOwnedContextError::Registry {
                declaration_index: 0,
                source: OwnedSourceError::AmbiguousOwner {
                    name: "Unused".into()
                }
            })));
    }

    #[test]
    fn legacy_iteration_refusal_and_public_basic_lifecycle_admission_stay_distinct() {
        let owned = ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Named {
                    name: "primary".into(),
                },
                path: Vec::new(),
            },
            output_path: None,
        };
        assert_eq!(scope_iteration(&owned), Err(mapping::OwnedSourceAccess));
        assert_eq!(
            basic_scope_iteration(&owned),
            Ok(TraceIteration::OwnedSource {
                source: OwnedSourceRef {
                    owner: SourceOwner::Named {
                        name: "primary".into()
                    },
                    path: Vec::new()
                }
            })
        );
        let documented = ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: Vec::new(),
            },
            output_path: Some(0),
        };
        assert_eq!(
            basic_scope_iteration(&documented),
            Err(mapping::OwnedSourceAccess)
        );
        let p = project(
            &[],
            vec![(
                0,
                Node::OwnedSourceField(OwnedSourceField {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Value"]),
                    },
                    frame: None,
                }),
            )],
            Scope {
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        );
        let primary = row("root");
        let paired = Lifecycle::new();
        let execution = crate::ExecutionContext::new(std::path::Path::new("project.json"))
            .with_trace_sink(&paired);
        assert_eq!(
            crate::run_with_context(&p, &primary, &execution),
            Ok(primary.clone())
        );
        assert_eq!(paired.contexts.borrow().len(), 2);
        assert!(paired.contexts.borrow().iter().all(|(_, outcome)| matches!(
            outcome,
            Outcome::Snapshot(Ok(snapshot)) if snapshot.phase == TraceOwnedPhase::Structural
        )));
        for observers in [
            crate::SourceOwnershipObservers {
                tracing: false,
                debugging: true,
            },
            crate::SourceOwnershipObservers {
                tracing: true,
                debugging: true,
            },
        ] {
            assert_eq!(
                crate::check_source_ownership_capability_with_observers(&p, None, observers),
                Ok(())
            );
        }
        let mut p = constant();
        p.root.iteration = ScopeIteration::DynamicDocuments {
            source: path(&["Rows"]),
            output_path: 0,
        };
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Err(EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: Some(TraceScope::primary().source_scope),
                context: crate::OwnedCapabilityContext::Document
            })
        );
        assert!(paired.events.borrow().is_empty());
        let p = project(
            &[],
            vec![
                (
                    0,
                    Node::Const {
                        value: Value::Int(7),
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: Vec::new(),
                        frame: None,
                    },
                ),
            ],
            Scope {
                iteration: ScopeIteration::Sequence(mapping::SequenceExpr::Generate {
                    from: None,
                    to: 0,
                    item: 1,
                }),
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        );
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let paired = Lifecycle::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&paired),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Err(EngineError::UnsupportedOwnedContext {
                node: None,
                function: None,
                scope: Some(TraceScope::primary().source_scope),
                context: crate::OwnedCapabilityContext::GeneratedItem
            })
        );
        assert!(paired.events.borrow().is_empty());
    }

    struct Candidates<'a> {
        events: RefCell<Vec<TraceEvent>>,
        contexts: RefCell<Vec<Outcome>>,
        selected: Cell<bool>,
        stop_after_first: bool,
        source_rows: bool,
        check: Option<&'a Check<'a>>,
    }
    impl Candidates<'_> {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                contexts: RefCell::new(Vec::new()),
                selected: Cell::new(true),
                stop_after_first: false,
                source_rows: true,
                check: None,
            }
        }
    }
    impl TraceSink for Candidates<'_> {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }
        fn wants_source_row(&self) -> bool {
            self.source_rows
        }
        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            self.selected.get() && matches!(event, TraceEvent::IterationCandidate { .. })
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(event, TraceEvent::IterationCandidate { .. }));
            let outcome = match context {
                Ok(view) => {
                    if let Some(check) = self.check {
                        check(&event, &view);
                    }
                    Outcome::Snapshot(view.snapshot())
                }
                Err(error) => Outcome::Projection(error.clone()),
            };
            self.contexts.borrow_mut().push(outcome);
            if self.stop_after_first {
                self.selected.set(false);
            }
            self.record(event);
        }
    }
    fn candidate_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
        events
            .iter()
            .filter(|event| matches!(event, TraceEvent::IterationCandidate { .. }))
            .cloned()
            .collect()
    }
    fn candidate_event(
        scope: TraceScope,
        ordinal: usize,
        positions: &[PositionFrame],
        item: Option<&Instance>,
    ) -> TraceEvent {
        TraceEvent::IterationCandidate {
            scope,
            ordinal,
            positions: crate::trace::trace_positions(positions),
            source_row: item.map(crate::TraceSourceRow::new),
        }
    }

    #[test]
    fn candidates_keep_exact_primary_named_owners_and_raw_null_gap_positions() {
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named { name: "Ref".into() },
            SourceOwner::Named {
                name: " Ref 雪 ".into(),
            },
        ] {
            for empty in [false, true] {
                let p = repeated(owner.clone());
                let rows = || {
                    Instance::Repeated(if empty {
                        Vec::new()
                    } else {
                        vec![
                            Instance::Scalar(Value::Null),
                            row("one"),
                            Instance::Scalar(Value::JsonNull(ir::JsonNull)),
                            row("two"),
                        ]
                    })
                };
                let primary = group(vec![("Rows", rows())]);
                let extras = group(vec![
                    ("Ref", group(vec![("Rows", rows())])),
                    (" Ref 雪 ", group(vec![("Rows", rows())])),
                ]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let key = match &owner {
                    SourceOwner::Primary => OwnerKey::Primary,
                    SourceOwner::Named { name } if name == "Ref" => OwnerKey::Named(0),
                    SourceOwner::Named { .. } => OwnerKey::Named(1),
                };
                let original = roots.root(key).unwrap();
                let items = original.field("Rows").unwrap().as_repeated().unwrap();
                let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                    let TraceEvent::IterationCandidate { ordinal, .. } = event else {
                        unreachable!()
                    };
                    let raw = if *ordinal == 1 { 2 } else { 4 };
                    assert_eq!(view.phase, TraceOwnedPhase::Structural);
                    assert!(view.owned_ancestry);
                    assert_eq!(view.frames.len(), 1);
                    assert_eq!(view.origins.len(), 1);
                    assert_eq!(view.positions.len(), 1);
                    let expected_owner = match &owner {
                        SourceOwner::Primary => TraceOwnedOwnerRef::Primary,
                        SourceOwner::Named { name } => TraceOwnedOwnerRef::Named {
                            declaration_index: if name == "Ref" { 0 } else { 1 },
                            name,
                        },
                    };
                    assert_eq!(view.frames[0].owner, expected_owner);
                    assert_eq!(view.origins[0].owner, expected_owner);
                    assert_eq!(view.frames[0].collection, path(&["Rows"]));
                    assert_eq!(view.origins[0].path, path(&["Rows"]));
                    assert!(std::ptr::eq(view.frames[0].item, &items[raw - 1]));
                    assert!(std::ptr::eq(view.origins[0].item, &items[raw - 1]));
                    assert_eq!(view.positions[view.frames[0].position_slot].index, raw);
                    assert!(std::ptr::eq(
                        view.roots[match key {
                            OwnerKey::Primary => 0,
                            OwnerKey::Named(i) => i + 1,
                        }]
                        .input
                        .unwrap(),
                        original
                    ));
                };
                let mut sink = Candidates::new();
                sink.check = Some(&check);
                let expected = Instance::Repeated(if empty {
                    Vec::new()
                } else {
                    vec![row("one"), row("two")]
                });
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&sink),
                        TraceOwnedPhase::Output,
                        &[]
                    ),
                    Ok(expected)
                );
                let expected_events = if empty {
                    Vec::new()
                } else {
                    vec![
                        candidate_event(TraceScope::primary(), 1, &[position(2)], Some(&items[1])),
                        candidate_event(TraceScope::primary(), 2, &[position(4)], Some(&items[3])),
                    ]
                };
                assert_eq!(candidate_events(&sink.events.borrow()), expected_events);
                assert_eq!(sink.contexts.borrow().len(), if empty { 0 } else { 2 });
                assert_eq!(registry.cache.get().is_some(), !empty);
                assert!(state.frames.is_empty() && state.origins.is_empty());
            }
        }
    }

    #[test]
    fn singleton_root_and_once_candidates_keep_identity_and_target_scope() {
        let primary = row("root");
        let extras = group(Vec::new());
        for once in [false, true] {
            let mut p = constant();
            if !once {
                p.root.iteration = ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: Vec::new(),
                    },
                    output_path: None,
                };
            }
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let mut state = OwnedContext::new(&roots);
            state.owned_ancestry = true;
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = Candidates::new();
            let expected = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Selection,
                    &[]
                ),
                Ok(if once {
                    expected.clone()
                } else {
                    Instance::Repeated(vec![expected.clone()])
                })
            );
            assert_eq!(
                candidate_events(&sink.events.borrow()),
                vec![candidate_event(TraceScope::primary(), 1, &[], None)]
            );
            let contexts = sink.contexts.borrow();
            let Outcome::Snapshot(Ok(view)) = &contexts[0] else {
                panic!("complete candidate witness")
            };
            assert_eq!(view.phase, TraceOwnedPhase::Structural);
            assert!(view.frames.is_empty() && view.positions.is_empty());
            assert!(state.frames.is_empty() && state.origins.is_empty());
            let named_scope = TraceScope::named(" Target 雪 ", 0);
            let sink = Candidates::new();
            let failed = Cell::new(false);
            let program = EvalProgram::new(&p.graph, &p.user_functions, Some(&sink), &failed)
                .with_owned_context(&state)
                .with_owned_trace_registry(&registry)
                .with_owned_trace_phase(TraceOwnedPhase::Output);
            let result = crate::eval_scope::eval_scope(
                program,
                &p.root,
                None,
                &[&extras, &primary],
                &[],
                &p.extra_sources,
                None,
                &named_scope,
            );
            assert_eq!(
                result,
                Ok(if once {
                    expected
                } else {
                    Instance::Repeated(vec![expected])
                })
            );
            assert_eq!(
                candidate_events(&sink.events.borrow()),
                vec![candidate_event(named_scope, 1, &[], None)]
            );
        }
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named { name: "Ref".into() },
        ] {
            for empty in [false, true] {
                let mut p = constant();
                p.source.repeating = true;
                p.extra_sources = project(&["Ref"], Vec::new(), Scope::default()).extra_sources;
                p.extra_sources[0].schema.repeating = true;
                p.root.iteration = ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: owner.clone(),
                        path: Vec::new(),
                    },
                    output_path: None,
                };
                let items = || {
                    Instance::Repeated(if empty {
                        Vec::new()
                    } else {
                        vec![row("one"), row("two")]
                    })
                };
                let primary = items();
                let extras = group(vec![("Ref", items())]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let key = if owner == SourceOwner::Primary {
                    OwnerKey::Primary
                } else {
                    OwnerKey::Named(0)
                };
                let original = roots.root(key).unwrap().as_repeated().unwrap();
                let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                    let TraceEvent::IterationCandidate { ordinal, .. } = event else {
                        unreachable!()
                    };
                    assert_eq!(view.phase, TraceOwnedPhase::Structural);
                    assert_eq!(view.frames.len(), 1);
                    assert!(view.frames[0].collection.is_empty());
                    assert!(view.positions[0].collection.is_empty());
                    assert_eq!(view.positions[0].index, *ordinal);
                    assert!(std::ptr::eq(view.frames[0].item, &original[*ordinal - 1]));
                };
                let mut sink = Candidates::new();
                sink.check = Some(&check);
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&sink),
                        TraceOwnedPhase::Output,
                        &[]
                    ),
                    Ok(Instance::Repeated(if empty {
                        Vec::new()
                    } else {
                        vec![
                            group(vec![("Value", Instance::Scalar(Value::Int(7)))]),
                            group(vec![("Value", Instance::Scalar(Value::Int(7)))]),
                        ]
                    }))
                );
                let expected = original
                    .iter()
                    .enumerate()
                    .map(|(index, item)| {
                        let mut raw = position(index + 1);
                        raw.collection.clear();
                        candidate_event(TraceScope::primary(), index + 1, &[raw], Some(item))
                    })
                    .collect::<Vec<_>>();
                assert_eq!(candidate_events(&sink.events.borrow()), expected);
            }
        }
    }

    #[test]
    fn nested_candidates_borrow_outer_and_inner_exact_owners_in_structural_phase() {
        let mut p = repeated(SourceOwner::Primary);
        let mut child = repeated(SourceOwner::Named { name: "Ref".into() }).root;
        child.target_field = "Child".into();
        child.bindings[0].node = 1;
        child.bindings.push(Binding {
            target_field: "Outer".into(),
            node: 0,
        });
        p.graph.nodes.insert(
            1,
            owned_field(
                SourceOwner::Named { name: "Ref".into() },
                &["Rows", "Value"],
            ),
        );
        p.root.children.push(child);
        let primary = group(vec![("Rows", Instance::Repeated(vec![row("parent")]))]);
        let extras = group(vec![(
            "Ref",
            group(vec![(
                "Rows",
                Instance::Repeated(vec![row("one"), row("two")]),
            )]),
        )]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let parent = &primary.field("Rows").unwrap().as_repeated().unwrap()[0];
        let named = roots
            .root(OwnerKey::Named(0))
            .unwrap()
            .field("Rows")
            .unwrap()
            .as_repeated()
            .unwrap();
        let child_scope = TraceScope::primary().child("Child", 0);
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            let TraceEvent::IterationCandidate { scope, ordinal, .. } = event else {
                unreachable!()
            };
            assert_eq!(view.phase, TraceOwnedPhase::Structural);
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            assert!(std::ptr::eq(view.frames[0].item, parent));
            assert!(std::ptr::eq(view.origins[0].item, parent));
            assert_eq!(view.positions[view.frames[0].position_slot].index, 1);
            if scope == &child_scope {
                assert_eq!(view.frames.len(), 2);
                assert_eq!(view.origins.len(), 2);
                assert_eq!(view.positions.len(), 2);
                assert_eq!(
                    view.frames[1].owner,
                    TraceOwnedOwnerRef::Named {
                        declaration_index: 0,
                        name: "Ref"
                    }
                );
                assert!(std::ptr::eq(view.frames[1].item, &named[*ordinal - 1]));
                assert!(std::ptr::eq(view.origins[1].item, &named[*ordinal - 1]));
                assert_eq!(view.positions[view.frames[1].position_slot].index, *ordinal);
            } else {
                assert_eq!(scope, &TraceScope::primary());
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.positions.len(), 1);
            }
        };
        let mut sink = Candidates::new();
        sink.check = Some(&check);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Ok(Instance::Repeated(vec![group(vec![
                ("Value", text("parent")),
                (
                    "Child",
                    Instance::Repeated(vec![
                        group(vec![("Value", text("one")), ("Outer", text("parent"))]),
                        group(vec![("Value", text("two")), ("Outer", text("parent"))])
                    ])
                )
            ])]))
        );
        assert_eq!(
            candidate_events(&sink.events.borrow()),
            vec![
                candidate_event(TraceScope::primary(), 1, &[position(1)], Some(parent)),
                candidate_event(
                    child_scope.clone(),
                    1,
                    &[position(1), position(1)],
                    Some(&named[0])
                ),
                candidate_event(
                    child_scope.clone(),
                    2,
                    &[position(1), position(2)],
                    Some(&named[1])
                )
            ]
        );
        assert_eq!(sink.contexts.borrow().len(), 3);
        assert!(state.frames.is_empty() && state.origins.is_empty());
        // An already-produced outer context retains its genuine index greater than one.
        let p = repeated(SourceOwner::Named { name: "Ref".into() });
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![row("outer-one"), row("outer-two")]),
        )]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let outer = &primary.field("Rows").unwrap().as_repeated().unwrap()[1];
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: path(&["Rows"]),
                item: outer,
                position_slot: 0,
            }],
            origins: vec![SourceOrigin {
                kind: crate::owned_context::SourceOriginKind::Ordinary,
                owner: OwnerKey::Primary,
                path: path(&["Rows"]),
                item: outer,
            }],
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let positions = [position(2)];
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            let TraceEvent::IterationCandidate { ordinal, .. } = event else {
                unreachable!()
            };
            assert_eq!(view.phase, TraceOwnedPhase::Structural);
            assert_eq!(view.frames.len(), 2);
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            assert!(std::ptr::eq(view.frames[0].item, outer));
            assert!(std::ptr::eq(view.origins[0].item, outer));
            assert_eq!(view.frames[0].position_slot, 0);
            assert_eq!(view.positions[0].index, 2);
            assert_eq!(view.frames[1].position_slot, 1);
            assert_eq!(view.positions[1].index, *ordinal);
        };
        let mut sink = Candidates::new();
        sink.check = Some(&check);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &positions
            ),
            Ok(Instance::Repeated(vec![row("one"), row("two")]))
        );
        let named = roots
            .root(OwnerKey::Named(0))
            .unwrap()
            .field("Rows")
            .unwrap()
            .as_repeated()
            .unwrap();
        assert_eq!(
            candidate_events(&sink.events.borrow()),
            vec![
                candidate_event(
                    TraceScope::primary(),
                    1,
                    &[position(2), position(1)],
                    Some(&named[0])
                ),
                candidate_event(
                    TraceScope::primary(),
                    2,
                    &[position(2), position(2)],
                    Some(&named[1])
                )
            ]
        );
        assert_eq!(state.frames[0].position_slot, 0);
        assert!(std::ptr::eq(state.frames[0].item, outer));
    }

    #[test]
    fn first_observes_complete_candidates_without_retaining_tail_or_replaying_events() {
        use crate::source_iteration::{
            ObservedWalkRequest, select_observed, select_observed_with_candidate_observer,
        };
        const COUNT: usize = 4096;
        let mut p = repeated(SourceOwner::Primary);
        p.root.iteration_output = mapping::IterationOutput::First;
        let primary = group(vec![(
            "Rows",
            Instance::Repeated((0..COUNT).map(|i| row(&i.to_string())).collect()),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let source = OwnedSourceRef {
            owner: SourceOwner::Primary,
            path: path(&["Rows"]),
        };
        let (owner, base, prefix, tail, budget) = state.select(&source).unwrap();
        let request = ObservedWalkRequest {
            base,
            path: tail,
            raw_prefix: prefix,
            canonical_prefix: prefix,
            owner: Some(owner),
            context: &state,
            position_offset: 0,
            strict_collection: true,
        };
        let observed = Cell::new(0usize);
        let mut observe = |extension: &mut crate::source_iteration::ObservedExtension<'_>,
                           ordinal| {
            assert_eq!(ordinal, observed.get() + 1);
            assert_eq!(extension.extension.positions[0].index, ordinal);
            observed.set(ordinal);
        };
        let selected =
            select_observed_with_candidate_observer(request, &budget, true, &mut observe).unwrap();
        assert_eq!(observed.get(), COUNT);
        assert_eq!(selected.candidates, COUNT);
        assert_eq!(selected.extensions.len(), 1);
        assert!(std::ptr::eq(
            selected.extensions[0].frames[0].item,
            &primary.field("Rows").unwrap().as_repeated().unwrap()[0]
        ));
        let replay = selected.replay.unwrap();
        let mut replay_count = 0;
        replay
            .request
            .visit(&replay.budget, &mut |_| {
                replay_count += 1;
                Ok(())
            })
            .unwrap();
        assert_eq!(replay_count, COUNT);
        assert_eq!(observed.get(), COUNT);
        let (_, _, _, _, silent_budget) = state.select(&source).unwrap();
        let silent = select_observed(request, &silent_budget, true).unwrap();
        assert_eq!(silent.candidates, COUNT);
        assert_eq!(silent.extensions.len(), 1);
        struct Counting {
            candidates: Cell<usize>,
            produced: Cell<usize>,
            finishes: Cell<usize>,
        }
        impl TraceSink for Counting {
            fn wants_source_row(&self) -> bool {
                false
            }
            fn record(&self, event: TraceEvent) {
                match event {
                    TraceEvent::IterationCandidate {
                        ordinal,
                        source_row,
                        ..
                    } => {
                        assert_eq!(ordinal, self.candidates.get() + 1);
                        assert!(source_row.is_none());
                        self.candidates.set(ordinal);
                    }
                    TraceEvent::TargetProduced { .. } => self.produced.set(self.produced.get() + 1),
                    TraceEvent::ScopeFinished {
                        candidates,
                        produced,
                        ..
                    } => {
                        assert_eq!(candidates, COUNT);
                        assert_eq!(produced, 1);
                        self.finishes.set(self.finishes.get() + 1);
                    }
                    _ => {}
                }
            }
        }
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Counting {
            candidates: Cell::new(0),
            produced: Cell::new(0),
            finishes: Cell::new(0),
        };
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(row("0"))
        );
        assert_eq!(
            (
                sink.candidates.get(),
                sink.produced.get(),
                sink.finishes.get()
            ),
            (COUNT, 1, 1)
        );
        assert!(registry.cache.get().is_none());
        for count in [0, 3] {
            let primary = group(vec![(
                "Rows",
                Instance::Repeated((0..count).map(|i| row(&i.to_string())).collect()),
            )]);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = Candidates::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Selection,
                    &[]
                ),
                Ok(if count == 0 {
                    group(Vec::new())
                } else {
                    row("0")
                })
            );
            assert_eq!(candidate_events(&sink.events.borrow()).len(), count);
            assert_eq!(sink.contexts.borrow().len(), count);
            assert_eq!(registry.cache.get().is_some(), count != 0);
            assert_eq!(
                sink.events
                    .borrow()
                    .iter()
                    .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
                    .count(),
                usize::from(count != 0)
            );
            assert_eq!(
                boundaries(&sink.events.borrow())[1],
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: count,
                    produced: usize::from(count != 0),
                    kind: TraceOutputKind::Group
                }
            );
            if count == 3 {
                let items = primary.field("Rows").unwrap().as_repeated().unwrap();
                assert_eq!(
                    candidate_events(&sink.events.borrow()),
                    (1..=3)
                        .map(|index| candidate_event(
                            TraceScope::primary(),
                            index,
                            &[position(index)],
                            Some(&items[index - 1])
                        ))
                        .collect::<Vec<_>>()
                );
            }
        }
    }

    #[test]
    fn malformed_first_tail_keeps_candidate_prefix_and_original_error_before_output() {
        let p = project(
            &[],
            vec![(
                0,
                Node::Const {
                    value: Value::Int(7),
                },
            )],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner: SourceOwner::Primary,
                        path: path(&["Buckets", "Rows"]),
                    },
                    output_path: None,
                },
                iteration_output: mapping::IterationOutput::First,
                bindings: vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }],
                ..Default::default()
            },
        );
        let good = group(vec![("Rows", Instance::Repeated(vec![row("one")]))]);
        let primary = group(vec![(
            "Buckets",
            Instance::Repeated(vec![good, text("malformed")]),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let silent = run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]);
        let sink = Candidates::new();
        let traced = run(
            &p,
            &state,
            &registry,
            Some(&sink),
            TraceOwnedPhase::Output,
            &[],
        );
        assert_eq!(traced, silent);
        assert_eq!(
            traced,
            Err(EngineError::OwnedSourceAccess {
                node: None,
                scope: Some(Box::new(TraceScope::primary().source_scope)),
                source: OwnedSourceError::MalformedOwnedTraversal {
                    path: path(&["Buckets"]),
                    found: "a non-null scalar"
                }
            })
        );
        let buckets = primary.field("Buckets").unwrap().as_repeated().unwrap();
        let item = &buckets[0].field("Rows").unwrap().as_repeated().unwrap()[0];
        let mut outer = position(1);
        outer.collection = path(&["Buckets"]);
        let mut inner = position(1);
        inner.collection = path(&["Buckets", "Rows"]);
        assert_eq!(
            candidate_events(&sink.events.borrow()),
            vec![candidate_event(
                TraceScope::primary(),
                1,
                &[outer, inner],
                Some(item)
            )]
        );
        assert!(!sink.events.borrow().iter().any(|event| matches!(
            event,
            TraceEvent::TargetProduced { .. }
                | TraceEvent::ScopeFinished { .. }
                | TraceEvent::NodeValue { .. }
        )));
        assert_eq!(sink.contexts.borrow().len(), 1);
    }

    #[test]
    fn candidate_event_preferences_keep_registry_lazy_and_previews_bounded() {
        let p = repeated(SourceOwner::Primary);
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![row(&"é".repeat(400)), row("two")]),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let silent = run(
            &p,
            &state,
            &registry,
            None,
            TraceOwnedPhase::Structural,
            &[],
        );
        assert!(registry.cache.get().is_none());
        let mut declining = Candidates::new();
        declining.selected.set(false);
        declining.source_rows = false;
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&declining),
                TraceOwnedPhase::Structural,
                &[]
            ),
            silent
        );
        assert!(registry.cache.get().is_none());
        assert!(declining.contexts.borrow().is_empty());
        assert!(
            candidate_events(&declining.events.borrow())
                .iter()
                .all(|event| matches!(
                    event,
                    TraceEvent::IterationCandidate {
                        source_row: None,
                        ..
                    }
                ))
        );
        let mut changing = Candidates::new();
        changing.stop_after_first = true;
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&changing),
                TraceOwnedPhase::Structural,
                &[]
            ),
            silent
        );
        assert_eq!(changing.contexts.borrow().len(), 1);
        assert_eq!(candidate_events(&changing.events.borrow()).len(), 2);
        let events = changing.events.borrow();
        let candidates = candidate_events(&events);
        let TraceEvent::IterationCandidate {
            source_row: Some(preview),
            ..
        } = &candidates[0]
        else {
            panic!("requested row preview")
        };
        assert_eq!(preview.fields.len(), 1);
        let value = preview.fields[0].value.as_ref().unwrap();
        assert!(value.truncated);
        assert!(value.preview.chars().count() <= 160);
        struct BooleanOnly(Cell<usize>);
        impl TraceSink for BooleanOnly {
            fn record(&self, _: TraceEvent) {}
            fn wants_owned_context(&self) -> bool {
                true
            }
            fn record_with_owned_context(
                &self,
                event: TraceEvent,
                _: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
            ) {
                assert!(matches!(
                    event,
                    TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
                ));
                self.0.set(self.0.get() + 1);
            }
        }
        let boolean = BooleanOnly(Cell::new(0));
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&boolean),
                TraceOwnedPhase::Structural,
                &[]
            ),
            silent
        );
        assert_eq!(boolean.0.get(), 2);
    }

    #[test]
    fn candidate_projection_and_snapshot_errors_remain_observer_only() {
        let p = constant();
        let mut foreign = constant();
        foreign.extra_sources = project(&["Unused"], Vec::new(), Scope::default()).extra_sources;
        let primary = row("root");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let mut state = OwnedContext::new(&roots);
        state.owned_ancestry = true;
        let wrong = TraceOwnedRegistry::new(&foreign, &roots);
        let sink = Candidates::new();
        let expected = group(vec![("Value", Instance::Scalar(Value::Int(7)))]);
        assert_eq!(
            run(
                &p,
                &state,
                &wrong,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(
            *sink.contexts.borrow(),
            vec![Outcome::Projection(
                TraceOwnedContextError::MismatchedRegistry
            )]
        );
        assert!(
            sink.events
                .borrow()
                .iter()
                .any(|event| matches!(event, TraceEvent::ScopeFinished { .. }))
        );
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = Candidates::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(
            *sink.contexts.borrow(),
            vec![Outcome::Snapshot(Err(
                TraceOwnedWitnessError::InvalidPositionSlot {
                    at: TraceOwnedLocation::Frame(0),
                    slot: 7
                }
            ))]
        );
        assert_eq!(state.frames.len(), 1);
        assert!(std::ptr::eq(state.frames[0].item, &primary));
    }

    #[test]
    fn candidates_precede_control_compaction_and_keep_original_structural_positions() {
        for order in [
            SortFilterOrder::FilterThenSort,
            SortFilterOrder::SortThenFilter,
        ] {
            let p = project(
                &[],
                vec![
                    (0, owned_field(SourceOwner::Primary, &["Rows", "Rank"])),
                    (1, owned_field(SourceOwner::Primary, &["Rows", "Keep"])),
                ],
                Scope {
                    iteration: ScopeIteration::OwnedSource {
                        source: OwnedSourceRef {
                            owner: SourceOwner::Primary,
                            path: path(&["Rows"]),
                        },
                        output_path: None,
                    },
                    filter: Some(1),
                    sort_by: Some(0),
                    sort_filter_order: order,
                    bindings: vec![Binding {
                        target_field: "Rank".into(),
                        node: 0,
                    }],
                    ..Default::default()
                },
            );
            let primary = group(vec![(
                "Rows",
                Instance::Repeated(
                    [(3, true), (1, false), (2, true)]
                        .into_iter()
                        .map(|(rank, keep)| {
                            group(vec![
                                ("Rank", Instance::Scalar(Value::Int(rank))),
                                ("Keep", Instance::Scalar(Value::Bool(keep))),
                            ])
                        })
                        .collect(),
                ),
            )]);
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = Candidates::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(Instance::Repeated(vec![
                    group(vec![("Rank", Instance::Scalar(Value::Int(2)))]),
                    group(vec![("Rank", Instance::Scalar(Value::Int(3)))])
                ]))
            );
            let items = primary.field("Rows").unwrap().as_repeated().unwrap();
            assert_eq!(
                candidate_events(&sink.events.borrow()),
                (1..=3)
                    .map(|index| candidate_event(
                        TraceScope::primary(),
                        index,
                        &[position(index)],
                        Some(&items[index - 1])
                    ))
                    .collect::<Vec<_>>()
            );
            assert_eq!(sink.contexts.borrow().len(), 3);
            for (index, outcome) in sink.contexts.borrow().iter().enumerate() {
                let Outcome::Snapshot(Ok(view)) = outcome else {
                    panic!("candidate snapshot")
                };
                assert_eq!(view.phase, TraceOwnedPhase::Structural);
                assert_eq!(view.positions[0].index, index + 1);
                assert_eq!(view.frames[0].index, index + 1);
            }
            let events = sink.events.borrow();
            let last_candidate = events
                .iter()
                .rposition(|event| matches!(event, TraceEvent::IterationCandidate { .. }))
                .unwrap();
            let first_value = events
                .iter()
                .position(|event| matches!(event, TraceEvent::NodeValue { .. }))
                .unwrap();
            assert!(last_candidate < first_value);
        }
    }

    struct FilterDecisions<'a> {
        events: RefCell<Vec<TraceEvent>>,
        contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
        root_addresses: RefCell<Vec<usize>>,
        selected: Cell<bool>,
        stop_after_first: bool,
        check: Option<&'a Check<'a>>,
    }
    impl FilterDecisions<'_> {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                contexts: RefCell::new(Vec::new()),
                root_addresses: RefCell::new(Vec::new()),
                selected: Cell::new(true),
                stop_after_first: false,
                check: None,
            }
        }
    }
    impl TraceSink for FilterDecisions<'_> {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }
        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            self.selected.get() && matches!(event, TraceEvent::FilterDecision { .. })
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(event, TraceEvent::FilterDecision { .. }));
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
            self.contexts.borrow_mut().push((event.clone(), outcome));
            if self.stop_after_first {
                self.selected.set(false);
            }
            self.record(event);
        }
    }
    fn decision_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
        events
            .iter()
            .filter(|event| matches!(event, TraceEvent::FilterDecision { .. }))
            .cloned()
            .collect()
    }
    fn decision_event(
        scope: TraceScope,
        node: mapping::NodeId,
        phase: crate::TraceFilterPhase,
        positions: &[PositionFrame],
        passed: bool,
    ) -> TraceEvent {
        TraceEvent::FilterDecision {
            scope,
            node,
            phase,
            positions: crate::trace::trace_positions(positions),
            passed,
        }
    }
    fn rank_row(rank: i64, keep: bool) -> Instance {
        group(vec![
            ("Rank", Instance::Scalar(Value::Int(rank))),
            ("Keep", Instance::Scalar(Value::Bool(keep))),
        ])
    }
    fn rank_output(rank: i64, index: i64) -> Instance {
        group(vec![
            ("Rank", Instance::Scalar(Value::Int(rank))),
            ("Index", Instance::Scalar(Value::Int(index))),
        ])
    }
    fn filter_project(owner: SourceOwner, sort: Option<SortFilterOrder>) -> Project {
        project(
            &["Ref", " Ref 雪 "],
            vec![
                (0, owned_field(owner.clone(), &["Rows", "Rank"])),
                (1, owned_field(owner.clone(), &["Rows", "Keep"])),
                (
                    2,
                    Node::OwnedPosition(OwnedPosition {
                        owner: owner.clone(),
                        collection: Some(path(&["Rows"])),
                    }),
                ),
            ],
            Scope {
                iteration: ScopeIteration::OwnedSource {
                    source: OwnedSourceRef {
                        owner,
                        path: path(&["Rows"]),
                    },
                    output_path: None,
                },
                filter: Some(1),
                sort_by: sort.map(|_| 0),
                sort_filter_order: sort.unwrap_or(SortFilterOrder::SortThenFilter),
                bindings: vec![
                    Binding {
                        target_field: "Rank".into(),
                        node: 0,
                    },
                    Binding {
                        target_field: "Index".into(),
                        node: 2,
                    },
                ],
                ..Default::default()
            },
        )
    }

    #[test]
    fn filter_decisions_keep_actual_phase_positions_and_exact_colliding_owner_identity() {
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named { name: "Ref".into() },
            SourceOwner::Named {
                name: " Ref 雪 ".into(),
            },
        ] {
            for sort in [
                None,
                Some(SortFilterOrder::FilterThenSort),
                Some(SortFilterOrder::SortThenFilter),
            ] {
                let p = filter_project(owner.clone(), sort);
                let primary = group(vec![(
                    "Rows",
                    Instance::Repeated(vec![
                        rank_row(3, true),
                        rank_row(1, false),
                        rank_row(2, true),
                    ]),
                )]);
                let extras = group(vec![
                    (
                        "Ref",
                        group(vec![(
                            "Rows",
                            Instance::Repeated(vec![
                                rank_row(30, true),
                                rank_row(10, false),
                                rank_row(20, true),
                            ]),
                        )]),
                    ),
                    (
                        " Ref 雪 ",
                        group(vec![(
                            "Rows",
                            Instance::Repeated(vec![
                                rank_row(300, true),
                                rank_row(100, false),
                                rank_row(200, true),
                            ]),
                        )]),
                    ),
                ]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let (key, multiplier, named) = match &owner {
                    SourceOwner::Primary => (OwnerKey::Primary, 1, None),
                    SourceOwner::Named { name } if name == "Ref" => {
                        (OwnerKey::Named(0), 10, Some((0, name.as_str())))
                    }
                    SourceOwner::Named { name } => {
                        (OwnerKey::Named(1), 100, Some((1, name.as_str())))
                    }
                };
                let original = roots.root(key).unwrap();
                let items = original.field("Rows").unwrap().as_repeated().unwrap();
                let sorted_filter = sort == Some(SortFilterOrder::SortThenFilter);
                let expected_phase = if sort == Some(SortFilterOrder::FilterThenSort) {
                    crate::TraceFilterPhase::BeforeSort
                } else if sorted_filter {
                    crate::TraceFilterPhase::AfterSort
                } else {
                    crate::TraceFilterPhase::Selection
                };
                let expected_witness_phase =
                    if expected_phase == crate::TraceFilterPhase::BeforeSort {
                        TraceOwnedPhase::Structural
                    } else {
                        TraceOwnedPhase::Selection
                    };
                let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                    let TraceEvent::FilterDecision {
                        node,
                        phase,
                        positions,
                        passed,
                        ..
                    } = event
                    else {
                        unreachable!()
                    };
                    assert_eq!(*node, 1);
                    assert_eq!(*phase, expected_phase);
                    assert_eq!(view.phase, expected_witness_phase);
                    let index = positions[0].index;
                    let original_index = if sorted_filter {
                        [1, 2, 0][index - 1]
                    } else {
                        index - 1
                    };
                    let expected_owner =
                        named.map_or(TraceOwnedOwnerRef::Primary, |(declaration_index, name)| {
                            TraceOwnedOwnerRef::Named {
                                declaration_index,
                                name,
                            }
                        });
                    assert_eq!(view.frames.len(), 1);
                    assert_eq!(view.origins.len(), 1);
                    assert_eq!(view.frames[0].owner, expected_owner);
                    assert_eq!(view.origins[0].owner, expected_owner);
                    if let TraceOwnedOwnerRef::Named {
                        declaration_index,
                        name,
                    } = view.frames[0].owner
                    {
                        assert_eq!(
                            name.as_ptr(),
                            p.extra_sources[declaration_index].name.as_ptr()
                        );
                    }
                    assert_eq!(view.frames[0].collection, path(&["Rows"]));
                    assert_eq!(view.origins[0].path, path(&["Rows"]));
                    assert!(std::ptr::eq(view.frames[0].item, &items[original_index]));
                    assert!(std::ptr::eq(view.origins[0].item, &items[original_index]));
                    assert_eq!(view.positions[view.frames[0].position_slot].index, index);
                    assert!(std::ptr::eq(
                        view.roots[match key {
                            OwnerKey::Primary => 0,
                            OwnerKey::Named(i) => i + 1,
                        }]
                        .input
                        .unwrap(),
                        original
                    ));
                    assert_eq!(*passed, original_index != 1);
                };
                let mut paired = FilterDecisions::new();
                paired.check = Some(&check);
                let plain = Collector::default();
                let expected = Instance::Repeated(if sort.is_some() {
                    vec![
                        rank_output(2 * multiplier, 1),
                        rank_output(3 * multiplier, 2),
                    ]
                } else {
                    vec![
                        rank_output(3 * multiplier, 1),
                        rank_output(2 * multiplier, 2),
                    ]
                });
                assert_eq!(
                    run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
                    Ok(expected.clone())
                );
                assert!(registry.cache.get().is_none());
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&plain),
                        TraceOwnedPhase::Output,
                        &[]
                    ),
                    Ok(expected.clone())
                );
                assert!(registry.cache.get().is_none());
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&paired),
                        TraceOwnedPhase::Output,
                        &[]
                    ),
                    Ok(expected)
                );
                assert_eq!(*paired.events.borrow(), *plain.0.borrow());
                assert_eq!(
                    decision_events(&paired.events.borrow()),
                    (1..=3)
                        .map(|index| decision_event(
                            TraceScope::primary(),
                            1,
                            expected_phase,
                            &[position(index)],
                            if sorted_filter {
                                index != 1
                            } else {
                                index != 2
                            }
                        ))
                        .collect::<Vec<_>>()
                );
                assert_eq!(paired.contexts.borrow().len(), 3);
                assert!(
                    paired
                        .root_addresses
                        .borrow()
                        .windows(2)
                        .all(|pair| pair[0] == pair[1])
                );
                let produced = paired
                    .events
                    .borrow()
                    .iter()
                    .filter_map(|event| match event {
                        TraceEvent::TargetProduced { positions, .. } => Some(positions[0].index),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                assert_eq!(produced, vec![1, 2]);
                assert!(state.frames.is_empty() && state.origins.is_empty());
            }
        }
    }

    #[test]
    fn nested_filter_decisions_preserve_parent_output_positions_and_borrowed_inner_frames() {
        let mut p = filter_project(SourceOwner::Primary, None);
        let mut child = filter_project(SourceOwner::Named { name: "Ref".into() }, None).root;
        child.target_field = "Child".into();
        child.filter = Some(4);
        child.bindings[0].node = 3;
        child.bindings[1].node = 5;
        p.graph.nodes.insert(
            3,
            owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Rank"]),
        );
        p.graph.nodes.insert(
            4,
            owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Keep"]),
        );
        p.graph.nodes.insert(
            5,
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Named { name: "Ref".into() },
                collection: Some(path(&["Rows"])),
            }),
        );
        p.root.children.push(child);
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                rank_row(5, false),
                rank_row(3, true),
                rank_row(2, true),
            ]),
        )]);
        let extras = group(vec![(
            "Ref",
            group(vec![(
                "Rows",
                Instance::Repeated(vec![
                    rank_row(7, true),
                    rank_row(8, false),
                    rank_row(9, true),
                ]),
            )]),
        )]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let parents = primary.field("Rows").unwrap().as_repeated().unwrap();
        let children = roots
            .root(OwnerKey::Named(0))
            .unwrap()
            .field("Rows")
            .unwrap()
            .as_repeated()
            .unwrap();
        let child_scope = TraceScope::primary().child("Child", 0);
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            let TraceEvent::FilterDecision {
                scope, positions, ..
            } = event
            else {
                unreachable!()
            };
            assert_eq!(view.phase, TraceOwnedPhase::Selection);
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            let parent_index = if scope == &child_scope {
                positions[0].index
            } else {
                positions[0].index - 1
            };
            assert!(std::ptr::eq(view.frames[0].item, &parents[parent_index]));
            assert!(std::ptr::eq(view.origins[0].item, &parents[parent_index]));
            assert_eq!(view.frames[0].position_slot, 0);
            assert_eq!(view.positions[0].index, positions[0].index);
            if scope == &child_scope {
                assert_eq!(view.frames.len(), 2);
                assert_eq!(view.origins.len(), 2);
                assert_eq!(
                    view.frames[1].owner,
                    TraceOwnedOwnerRef::Named {
                        declaration_index: 0,
                        name: "Ref"
                    }
                );
                assert_eq!(view.frames[1].position_slot, 1);
                assert_eq!(view.positions[1].index, positions[1].index);
                assert!(std::ptr::eq(
                    view.frames[1].item,
                    &children[positions[1].index - 1]
                ));
                assert!(std::ptr::eq(
                    view.origins[1].item,
                    &children[positions[1].index - 1]
                ));
            } else {
                assert_eq!(view.frames.len(), 1);
            }
        };
        let mut sink = FilterDecisions::new();
        sink.check = Some(&check);
        let make_parent = |rank, index| {
            group(vec![
                ("Rank", Instance::Scalar(Value::Int(rank))),
                ("Index", Instance::Scalar(Value::Int(index))),
                (
                    "Child",
                    Instance::Repeated(vec![rank_output(7, 1), rank_output(9, 2)]),
                ),
            ])
        };
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(Instance::Repeated(vec![
                make_parent(3, 1),
                make_parent(2, 2)
            ]))
        );
        let mut expected = vec![decision_event(
            TraceScope::primary(),
            1,
            crate::TraceFilterPhase::Selection,
            &[position(1)],
            false,
        )];
        for outer in 1..=2 {
            expected.push(decision_event(
                TraceScope::primary(),
                1,
                crate::TraceFilterPhase::Selection,
                &[position(outer + 1)],
                true,
            ));
            for inner in 1..=3 {
                expected.push(decision_event(
                    child_scope.clone(),
                    4,
                    crate::TraceFilterPhase::Selection,
                    &[position(outer), position(inner)],
                    inner != 2,
                ));
            }
        }
        assert_eq!(decision_events(&sink.events.borrow()), expected);
        assert_eq!(sink.contexts.borrow().len(), 9);
        assert!(state.frames.is_empty() && state.origins.is_empty());
    }

    #[test]
    fn filter_decision_preferences_keep_legacy_events_and_cache_lazy() {
        let mut p = filter_project(SourceOwner::Primary, None);
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                rank_row(3, true),
                rank_row(1, false),
                rank_row(2, true),
            ]),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let expected = Instance::Repeated(vec![rank_output(3, 1), rank_output(2, 2)]);
        let declining = FilterDecisions::new();
        declining.selected.set(false);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                None,
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&declining),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected.clone())
        );
        assert!(declining.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        let mut changing = FilterDecisions::new();
        changing.stop_after_first = true;
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&changing),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(*changing.events.borrow(), *declining.events.borrow());
        assert_eq!(changing.contexts.borrow().len(), 1);
        assert_eq!(decision_events(&changing.events.borrow()).len(), 3);
        struct BooleanOnly(Cell<usize>);
        impl TraceSink for BooleanOnly {
            fn record(&self, _: TraceEvent) {}
            fn wants_owned_context(&self) -> bool {
                true
            }
            fn record_with_owned_context(
                &self,
                event: TraceEvent,
                _: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
            ) {
                assert!(matches!(
                    event,
                    TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
                ));
                self.0.set(self.0.get() + 1);
            }
        }
        let boolean = BooleanOnly(Cell::new(0));
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&boolean),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(boolean.0.get(), 7);
        p.root.filter = None;
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = FilterDecisions::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(Instance::Repeated(vec![
                rank_output(3, 1),
                rank_output(1, 2),
                rank_output(2, 3)
            ]))
        );
        assert!(decision_events(&sink.events.borrow()).is_empty());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        p.root.filter = Some(1);
        p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        p.graph.nodes.insert(
            0,
            Node::SourceField {
                path: path(&["Rank"]),
                frame: Some(path(&["Rows"])),
            },
        );
        p.graph.nodes.insert(
            1,
            Node::SourceField {
                path: path(&["Keep"]),
                frame: Some(path(&["Rows"])),
            },
        );
        p.root.bindings.retain(|binding| binding.node == 0);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = FilterDecisions::new();
        let plain = Collector::default();
        let expected = Instance::Repeated(vec![
            group(vec![("Rank", Instance::Scalar(Value::Int(3)))]),
            group(vec![("Rank", Instance::Scalar(Value::Int(2)))]),
        ]);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
    }

    #[test]
    fn filter_decision_observation_does_not_change_first_predicate_count_or_retention_eligibility()
    {
        let mut p = filter_project(SourceOwner::Primary, None);
        p.root.iteration_output = mapping::IterationOutput::First;
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                rank_row(3, true),
                rank_row(1, false),
                rank_row(2, true),
            ]),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = FilterDecisions::new();
        let plain = Collector::default();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(rank_output(3, 1))
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(rank_output(3, 1))
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert_eq!(decision_events(&sink.events.borrow()).len(), 3);
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|event| matches!(event, TraceEvent::NodeValue { node: 1, .. }))
                .count(),
            3
        );
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
                .count(),
            1
        );
        assert_eq!(
            boundaries(&sink.events.borrow())[1],
            TraceEvent::ScopeFinished {
                scope: TraceScope::primary(),
                candidates: 3,
                produced: 1,
                kind: TraceOutputKind::Group
            }
        );
        p.root.filter = None;
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = FilterDecisions::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(rank_output(3, 1))
        );
        assert_eq!(candidate_events(&sink.events.borrow()).len(), 3);
        assert!(decision_events(&sink.events.borrow()).is_empty());
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
                .count(),
            1
        );
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
    }

    #[test]
    fn failed_filter_predicates_keep_exact_errors_and_prefix_without_fabricated_decisions() {
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                rank_row(3, true),
                rank_row(1, false),
                rank_row(2, true),
            ]),
        )]);
        let extras = group(Vec::new());
        let bad = [
            (
                Node::Const {
                    value: Value::String("not_bool".into()),
                },
                EngineError::NotABool {
                    node: 1,
                    found: "string",
                },
            ),
            (
                Node::Raise { message: None },
                EngineError::MappingException {
                    node: 1,
                    message: None,
                },
            ),
            (
                owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Keep"]),
                EngineError::OwnedSourceAccess {
                    node: Some(1),
                    scope: None,
                    source: OwnedSourceError::SourceUnavailable {
                        owner: SourceOwner::Named { name: "Ref".into() },
                    },
                },
            ),
        ];
        for (predicate, expected) in bad {
            let mut p = filter_project(SourceOwner::Primary, None);
            p.graph.nodes.insert(1, predicate);
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = FilterDecisions::new();
            let plain = Collector::default();
            assert_eq!(
                run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]).as_ref(),
                Err(&expected)
            );
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&plain),
                    TraceOwnedPhase::Output,
                    &[]
                )
                .as_ref(),
                Err(&expected)
            );
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Err(expected)
            );
            assert_eq!(*sink.events.borrow(), *plain.0.borrow());
            assert!(decision_events(&sink.events.borrow()).is_empty());
            assert!(sink.contexts.borrow().is_empty());
            assert!(registry.cache.get().is_none());
            assert!(!sink.events.borrow().iter().any(|event| matches!(
                event,
                TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
            )));
        }
        let mut p = filter_project(SourceOwner::Primary, None);
        p.graph.nodes.insert(
            3,
            Node::If {
                condition: 1,
                then: 4,
                else_: 5,
            },
        );
        p.graph.nodes.insert(
            4,
            Node::Const {
                value: Value::Bool(true),
            },
        );
        p.graph.nodes.insert(5, Node::Raise { message: None });
        p.root.filter = Some(3);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = FilterDecisions::new();
        let plain = Collector::default();
        let expected = Err(EngineError::MappingException {
            node: 5,
            message: None,
        });
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            ),
            expected
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            expected
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert_eq!(
            decision_events(&sink.events.borrow()),
            vec![decision_event(
                TraceScope::primary(),
                3,
                crate::TraceFilterPhase::Selection,
                &[position(1)],
                true
            )]
        );
        assert_eq!(sink.contexts.borrow().len(), 1);
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|event| matches!(event, TraceEvent::TargetProduced { .. }))
                .count(),
            1
        );
        assert!(
            !sink
                .events
                .borrow()
                .iter()
                .any(|event| matches!(event, TraceEvent::ScopeFinished { .. }))
        );
    }

    #[test]
    fn filter_decision_projection_and_snapshot_failures_stay_observer_metadata() {
        let p = filter_project(SourceOwner::Primary, None);
        let foreign = constant();
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                rank_row(3, true),
                rank_row(1, false),
                rank_row(2, true),
            ]),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let wrong = TraceOwnedRegistry::new(&foreign, &roots);
        let sink = FilterDecisions::new();
        assert!(!std::ptr::eq(
            foreign.extra_sources.as_slice(),
            roots.declarations()
        ));
        let expected = Instance::Repeated(vec![rank_output(3, 1), rank_output(2, 2)]);
        assert_eq!(
            run(
                &p,
                &state,
                &wrong,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(sink.contexts.borrow().len(), 3);
        assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Projection(TraceOwnedContextError::MismatchedRegistry)));
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = FilterDecisions::new();
        assert_eq!(
            run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
            Ok(expected.clone())
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(sink.contexts.borrow().len(), 3);
        assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 7
            }))));
        assert_eq!(state.frames.len(), 1);
        assert!(std::ptr::eq(state.frames[0].item, &primary));
    }

    struct SortObservations<'a> {
        events: RefCell<Vec<TraceEvent>>,
        contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
        root_addresses: RefCell<Vec<usize>>,
        selected: Cell<u8>,
        stop_after_first: bool,
        check: Option<&'a Check<'a>>,
    }
    impl SortObservations<'_> {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                contexts: RefCell::new(Vec::new()),
                root_addresses: RefCell::new(Vec::new()),
                selected: Cell::new(3),
                stop_after_first: false,
                check: None,
            }
        }
    }
    impl TraceSink for SortObservations<'_> {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }
        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            match event {
                TraceEvent::SortCandidate { .. } => self.selected.get() & 1 != 0,
                TraceEvent::SortPosition { .. } => self.selected.get() & 2 != 0,
                _ => false,
            }
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(
                event,
                TraceEvent::SortCandidate { .. } | TraceEvent::SortPosition { .. }
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
            self.contexts.borrow_mut().push((event.clone(), outcome));
            if self.stop_after_first {
                self.selected.set(0);
            }
            self.record(event);
        }
    }
    fn sort_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
        events
            .iter()
            .filter(|event| {
                matches!(
                    event,
                    TraceEvent::SortCandidate { .. } | TraceEvent::SortPosition { .. }
                )
            })
            .cloned()
            .collect()
    }
    fn sort_candidate(
        scope: TraceScope,
        positions: &[PositionFrame],
        keys: &[(mapping::NodeId, bool, Value)],
    ) -> TraceEvent {
        TraceEvent::SortCandidate {
            scope,
            positions: crate::trace::trace_positions(positions),
            keys: keys
                .iter()
                .map(|(node, descending, value)| crate::TraceSortKey {
                    node: *node,
                    descending: *descending,
                    value: TraceValue::new(value),
                })
                .collect(),
        }
    }
    fn sort_position(
        scope: TraceScope,
        positions: &[PositionFrame],
        output_index: usize,
    ) -> TraceEvent {
        TraceEvent::SortPosition {
            scope,
            positions: crate::trace::trace_positions(positions),
            output_index,
        }
    }
    fn tagged_row(rank: i64, keep: bool, tag: &str) -> Instance {
        group(vec![
            ("Rank", Instance::Scalar(Value::Int(rank))),
            ("Keep", Instance::Scalar(Value::Bool(keep))),
            ("Tag", text(tag)),
        ])
    }
    fn tagged_output(rank: i64, index: i64, tag: &str) -> Instance {
        group(vec![
            ("Rank", Instance::Scalar(Value::Int(rank))),
            ("Index", Instance::Scalar(Value::Int(index))),
            ("Tag", text(tag)),
        ])
    }
    fn sort_project(owner: SourceOwner, order: SortFilterOrder) -> Project {
        let mut p = filter_project(owner.clone(), Some(order));
        p.graph
            .nodes
            .insert(3, owned_field(owner, &["Rows", "Tag"]));
        p.root.bindings.push(Binding {
            target_field: "Tag".into(),
            node: 3,
        });
        p
    }
    fn tie_sort_events(
        scope: &TraceScope,
        original: &[usize],
        sorted: &[usize],
    ) -> Vec<TraceEvent> {
        let mut events = original
            .iter()
            .map(|index| {
                sort_candidate(
                    scope.clone(),
                    &[position(*index)],
                    &[(0, false, Value::Int(if *index == 2 { 1 } else { 2 }))],
                )
            })
            .collect::<Vec<_>>();
        events.extend(
            sorted
                .iter()
                .enumerate()
                .map(|(rank, index)| sort_position(scope.clone(), &[position(*index)], rank + 1)),
        );
        events
    }

    #[test]
    fn sort_events_keep_stable_ties_filter_order_and_exact_primary_named_structural_identity() {
        for owner in [
            SourceOwner::Primary,
            SourceOwner::Named { name: "Ref".into() },
            SourceOwner::Named {
                name: " Ref 雪 ".into(),
            },
        ] {
            for order in [
                SortFilterOrder::FilterThenSort,
                SortFilterOrder::SortThenFilter,
            ] {
                let p = sort_project(owner.clone(), order);
                let primary = group(vec![(
                    "Rows",
                    Instance::Repeated(vec![
                        tagged_row(2, true, "P.A"),
                        tagged_row(1, false, "P.B"),
                        tagged_row(2, true, "P.C"),
                    ]),
                )]);
                let extras = group(vec![
                    (
                        "Ref",
                        group(vec![(
                            "Rows",
                            Instance::Repeated(vec![
                                tagged_row(2, true, "N.A"),
                                tagged_row(1, false, "N.B"),
                                tagged_row(2, true, "N.C"),
                            ]),
                        )]),
                    ),
                    (
                        " Ref 雪 ",
                        group(vec![(
                            "Rows",
                            Instance::Repeated(vec![
                                tagged_row(2, true, "U.A"),
                                tagged_row(1, false, "U.B"),
                                tagged_row(2, true, "U.C"),
                            ]),
                        )]),
                    ),
                ]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let (key, prefix) = match &owner {
                    SourceOwner::Primary => (OwnerKey::Primary, "P"),
                    SourceOwner::Named { name } if name == "Ref" => (OwnerKey::Named(0), "N"),
                    SourceOwner::Named { .. } => (OwnerKey::Named(1), "U"),
                };
                let original = roots.root(key).unwrap();
                let items = original.field("Rows").unwrap().as_repeated().unwrap();
                let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                    let positions = match event {
                        TraceEvent::SortCandidate { positions, .. }
                        | TraceEvent::SortPosition { positions, .. } => positions,
                        _ => unreachable!(),
                    };
                    let raw = positions[0].index;
                    assert_eq!(view.phase, TraceOwnedPhase::Structural);
                    assert_eq!(view.frames.len(), 1);
                    assert_eq!(view.origins.len(), 1);
                    let expected_owner = match key {
                        OwnerKey::Primary => TraceOwnedOwnerRef::Primary,
                        OwnerKey::Named(index) => TraceOwnedOwnerRef::Named {
                            declaration_index: index,
                            name: &p.extra_sources[index].name,
                        },
                    };
                    assert_eq!(view.frames[0].owner, expected_owner);
                    assert_eq!(view.origins[0].owner, expected_owner);
                    if let TraceOwnedOwnerRef::Named {
                        declaration_index,
                        name,
                    } = view.frames[0].owner
                    {
                        assert_eq!(
                            name.as_ptr(),
                            p.extra_sources[declaration_index].name.as_ptr()
                        );
                    }
                    assert!(std::ptr::eq(view.frames[0].item, &items[raw - 1]));
                    assert!(std::ptr::eq(view.origins[0].item, &items[raw - 1]));
                    assert_eq!(view.frames[0].collection, path(&["Rows"]));
                    assert_eq!(view.origins[0].path, path(&["Rows"]));
                    assert_eq!(view.positions[view.frames[0].position_slot].index, raw);
                    assert!(std::ptr::eq(
                        view.roots[match key {
                            OwnerKey::Primary => 0,
                            OwnerKey::Named(i) => i + 1,
                        }]
                        .input
                        .unwrap(),
                        original
                    ));
                };
                let mut paired = SortObservations::new();
                paired.check = Some(&check);
                let plain = Collector::default();
                let expected = Instance::Repeated(vec![
                    tagged_output(2, 1, &format!("{prefix}.A")),
                    tagged_output(2, 2, &format!("{prefix}.C")),
                ]);
                assert_eq!(
                    run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
                    Ok(expected.clone())
                );
                assert!(registry.cache.get().is_none());
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&plain),
                        TraceOwnedPhase::Output,
                        &[]
                    ),
                    Ok(expected.clone())
                );
                assert!(registry.cache.get().is_none());
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&paired),
                        TraceOwnedPhase::Output,
                        &[]
                    ),
                    Ok(expected)
                );
                assert_eq!(*paired.events.borrow(), *plain.0.borrow());
                let (original_order, ranked_order) = if order == SortFilterOrder::FilterThenSort {
                    (vec![1, 3], vec![1, 3])
                } else {
                    (vec![1, 2, 3], vec![2, 1, 3])
                };
                assert_eq!(
                    sort_events(&paired.events.borrow()),
                    tie_sort_events(&TraceScope::primary(), &original_order, &ranked_order)
                );
                let phase = if order == SortFilterOrder::FilterThenSort {
                    crate::TraceFilterPhase::BeforeSort
                } else {
                    crate::TraceFilterPhase::AfterSort
                };
                assert_eq!(
                    decision_events(&paired.events.borrow()),
                    (1..=3)
                        .map(|i| decision_event(
                            TraceScope::primary(),
                            1,
                            phase,
                            &[position(i)],
                            if order == SortFilterOrder::FilterThenSort {
                                i != 2
                            } else {
                                i != 1
                            }
                        ))
                        .collect::<Vec<_>>()
                );
                let events = paired.events.borrow();
                let first_sort = events
                    .iter()
                    .position(|e| matches!(e, TraceEvent::SortCandidate { .. }))
                    .unwrap();
                let first_filter = events
                    .iter()
                    .position(|e| matches!(e, TraceEvent::FilterDecision { .. }))
                    .unwrap();
                assert_eq!(
                    first_filter < first_sort,
                    order == SortFilterOrder::FilterThenSort
                );
                assert_eq!(
                    paired.contexts.borrow().len(),
                    original_order.len() + ranked_order.len()
                );
                assert!(
                    paired
                        .root_addresses
                        .borrow()
                        .windows(2)
                        .all(|pair| pair[0] == pair[1])
                );
                assert!(state.frames.is_empty() && state.origins.is_empty());
            }
        }
    }

    #[test]
    fn multiple_sort_keys_keep_directions_bounded_values_and_exact_evaluation_count() {
        let long_a = format!("A{}", "é".repeat(400));
        let long_c = format!("C{}", "é".repeat(400));
        let tags = [long_a.as_str(), "B", long_c.as_str()];
        let ranks = [2, 1, 2];
        for rank_descending in [false, true] {
            for tag_descending in [false, true] {
                let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
                p.root.filter = None;
                p.root.sort_descending = rank_descending;
                p.root.sort_then_by = vec![mapping::SortKey {
                    node: 3,
                    descending: tag_descending,
                }];
                let primary = group(vec![(
                    "Rows",
                    Instance::Repeated(
                        (0..3)
                            .map(|i| tagged_row(ranks[i], true, tags[i]))
                            .collect(),
                    ),
                )]);
                let extras = group(Vec::new());
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let sink = SortObservations::new();
                let indices = match (rank_descending, tag_descending) {
                    (false, false) => [1, 0, 2],
                    (false, true) => [1, 2, 0],
                    (true, false) => [0, 2, 1],
                    (true, true) => [2, 0, 1],
                };
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&sink),
                        TraceOwnedPhase::Selection,
                        &[]
                    ),
                    Ok(Instance::Repeated(
                        indices
                            .iter()
                            .enumerate()
                            .map(|(i, original)| tagged_output(
                                ranks[*original],
                                i64::try_from(i + 1).unwrap(),
                                tags[*original]
                            ))
                            .collect()
                    ))
                );
                let mut expected = (0..3)
                    .map(|i| {
                        sort_candidate(
                            TraceScope::primary(),
                            &[position(i + 1)],
                            &[
                                (0, rank_descending, Value::Int(ranks[i])),
                                (3, tag_descending, Value::String(tags[i].into())),
                            ],
                        )
                    })
                    .collect::<Vec<_>>();
                expected.extend(indices.iter().enumerate().map(|(rank, original)| {
                    sort_position(TraceScope::primary(), &[position(original + 1)], rank + 1)
                }));
                assert_eq!(sort_events(&sink.events.borrow()), expected);
                for node in [0, 3] {
                    assert_eq!(sink.events.borrow().iter().filter(|e| matches!(e, TraceEvent::NodeValue { node: actual, .. } if *actual == node)).count(), 6);
                }
                let events = sink.events.borrow();
                for event in sort_events(&events).into_iter().take(3) {
                    let TraceEvent::SortCandidate { keys, .. } = event else {
                        unreachable!()
                    };
                    assert_eq!(keys.len(), 2);
                    assert!(keys[1].value.preview.chars().count() <= 160);
                    if keys[1].value.preview.starts_with('A')
                        || keys[1].value.preview.starts_with('C')
                    {
                        assert!(keys[1].value.truncated);
                    }
                }
                assert_eq!(sink.contexts.borrow().len(), 6);
            }
        }
    }

    #[test]
    fn nested_sort_witnesses_preserve_compacted_parent_context_and_original_inner_ranks() {
        let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
        p.root.filter = None;
        let mut child = sort_project(
            SourceOwner::Named { name: "Ref".into() },
            SortFilterOrder::SortThenFilter,
        )
        .root;
        child.target_field = "Child".into();
        child.filter = None;
        child.sort_by = Some(4);
        child.bindings[0].node = 4;
        child.bindings[1].node = 6;
        child.bindings[2].node = 7;
        p.graph.nodes.insert(
            4,
            owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Rank"]),
        );
        p.graph.nodes.insert(
            6,
            Node::OwnedPosition(OwnedPosition {
                owner: SourceOwner::Named { name: "Ref".into() },
                collection: Some(path(&["Rows"])),
            }),
        );
        p.graph.nodes.insert(
            7,
            owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Tag"]),
        );
        p.root.children.push(child);
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![tagged_row(30, true, "A"), tagged_row(10, true, "B")]),
        )]);
        let extras = group(vec![(
            "Ref",
            group(vec![(
                "Rows",
                Instance::Repeated(vec![tagged_row(4, true, "X"), tagged_row(3, true, "Y")]),
            )]),
        )]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let parents = primary.field("Rows").unwrap().as_repeated().unwrap();
        let children = roots
            .root(OwnerKey::Named(0))
            .unwrap()
            .field("Rows")
            .unwrap()
            .as_repeated()
            .unwrap();
        let child_scope = TraceScope::primary().child("Child", 0);
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            let (scope, positions) = match event {
                TraceEvent::SortCandidate {
                    scope, positions, ..
                }
                | TraceEvent::SortPosition {
                    scope, positions, ..
                } => (scope, positions),
                _ => unreachable!(),
            };
            assert_eq!(view.phase, TraceOwnedPhase::Structural);
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            let parent_index = if scope == &child_scope {
                if positions[0].index == 1 { 1 } else { 0 }
            } else {
                positions[0].index - 1
            };
            assert!(std::ptr::eq(view.frames[0].item, &parents[parent_index]));
            assert!(std::ptr::eq(view.origins[0].item, &parents[parent_index]));
            assert_eq!(view.positions[0].index, positions[0].index);
            if scope == &child_scope {
                assert_eq!(view.frames.len(), 2);
                assert_eq!(view.frames[1].position_slot, 1);
                assert_eq!(
                    view.frames[1].owner,
                    TraceOwnedOwnerRef::Named {
                        declaration_index: 0,
                        name: "Ref"
                    }
                );
                assert!(std::ptr::eq(
                    view.frames[1].item,
                    &children[positions[1].index - 1]
                ));
                assert!(std::ptr::eq(
                    view.origins[1].item,
                    &children[positions[1].index - 1]
                ));
                assert_eq!(view.positions[1].index, positions[1].index);
            } else {
                assert_eq!(view.frames.len(), 1);
            }
        };
        let mut sink = SortObservations::new();
        sink.check = Some(&check);
        let parent_output = |rank, index, tag| {
            group(vec![
                ("Rank", Instance::Scalar(Value::Int(rank))),
                ("Index", Instance::Scalar(Value::Int(index))),
                ("Tag", text(tag)),
                (
                    "Child",
                    Instance::Repeated(vec![tagged_output(3, 1, "Y"), tagged_output(4, 2, "X")]),
                ),
            ])
        };
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(Instance::Repeated(vec![
                parent_output(10, 1, "B"),
                parent_output(30, 2, "A")
            ]))
        );
        let mut expected = vec![
            sort_candidate(
                TraceScope::primary(),
                &[position(1)],
                &[(0, false, Value::Int(30))],
            ),
            sort_candidate(
                TraceScope::primary(),
                &[position(2)],
                &[(0, false, Value::Int(10))],
            ),
            sort_position(TraceScope::primary(), &[position(2)], 1),
            sort_position(TraceScope::primary(), &[position(1)], 2),
        ];
        for outer in 1..=2 {
            expected.extend([
                sort_candidate(
                    child_scope.clone(),
                    &[position(outer), position(1)],
                    &[(4, false, Value::Int(4))],
                ),
                sort_candidate(
                    child_scope.clone(),
                    &[position(outer), position(2)],
                    &[(4, false, Value::Int(3))],
                ),
                sort_position(child_scope.clone(), &[position(outer), position(2)], 1),
                sort_position(child_scope.clone(), &[position(outer), position(1)], 2),
            ]);
        }
        assert_eq!(sort_events(&sink.events.borrow()), expected);
        assert_eq!(sink.contexts.borrow().len(), 12);
        assert!(state.frames.is_empty() && state.origins.is_empty());
    }

    #[test]
    fn sort_event_preferences_keep_default_legacy_vectors_and_root_cache_lazy() {
        let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
        p.root.filter = None;
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                tagged_row(2, true, "A"),
                tagged_row(1, true, "B"),
                tagged_row(2, true, "C"),
            ]),
        )]);
        let extras = group(Vec::new());
        for (selected, callbacks) in [(0, 0), (1, 3), (2, 3), (3, 6)] {
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = SortObservations::new();
            sink.selected.set(selected);
            let plain = Collector::default();
            let expected = Instance::Repeated(vec![
                tagged_output(1, 1, "B"),
                tagged_output(2, 2, "A"),
                tagged_output(2, 3, "C"),
            ]);
            assert_eq!(
                run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
                Ok(expected.clone())
            );
            assert!(registry.cache.get().is_none());
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&plain),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(expected.clone())
            );
            assert!(registry.cache.get().is_none());
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(expected)
            );
            assert_eq!(*sink.events.borrow(), *plain.0.borrow());
            assert_eq!(sink.contexts.borrow().len(), callbacks);
            assert_eq!(registry.cache.get().is_some(), callbacks != 0);
            assert_eq!(
                sort_events(&sink.events.borrow()),
                tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
            );
            assert!(
                sink.root_addresses
                    .borrow()
                    .windows(2)
                    .all(|pair| pair[0] == pair[1])
            );
        }
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let mut changing = SortObservations::new();
        changing.stop_after_first = true;
        run(
            &p,
            &state,
            &registry,
            Some(&changing),
            TraceOwnedPhase::Output,
            &[],
        )
        .unwrap();
        assert_eq!(changing.contexts.borrow().len(), 1);
        assert_eq!(sort_events(&changing.events.borrow()).len(), 6);
        struct BooleanOnly(Cell<usize>);
        impl TraceSink for BooleanOnly {
            fn record(&self, _: TraceEvent) {}
            fn wants_owned_context(&self) -> bool {
                true
            }
            fn record_with_owned_context(
                &self,
                event: TraceEvent,
                _: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
            ) {
                assert!(matches!(
                    event,
                    TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
                ));
                self.0.set(self.0.get() + 1);
            }
        }
        let old = BooleanOnly(Cell::new(0));
        run(
            &p,
            &state,
            &registry,
            Some(&old),
            TraceOwnedPhase::Output,
            &[],
        )
        .unwrap();
        assert_eq!(old.0.get(), 12);
        p.root.sort_by = None;
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(Instance::Repeated(vec![
                tagged_output(2, 1, "A"),
                tagged_output(1, 2, "B"),
                tagged_output(2, 3, "C")
            ]))
        );
        assert!(sort_events(&sink.events.borrow()).is_empty());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        p.root.sort_by = Some(0);
        p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        p.graph.nodes.insert(
            0,
            Node::SourceField {
                path: path(&["Rank"]),
                frame: Some(path(&["Rows"])),
            },
        );
        p.graph.nodes.insert(
            3,
            Node::SourceField {
                path: path(&["Tag"]),
                frame: Some(path(&["Rows"])),
            },
        );
        p.root.bindings.retain(|binding| binding.node != 2);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        let plain = Collector::default();
        let expected = Instance::Repeated(
            [("B", 1), ("A", 2), ("C", 2)]
                .into_iter()
                .map(|(tag, rank)| {
                    group(vec![
                        ("Rank", Instance::Scalar(Value::Int(rank))),
                        ("Tag", text(tag)),
                    ])
                })
                .collect(),
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert_eq!(
            sort_events(&sink.events.borrow()),
            tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
        );
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
    }

    #[test]
    fn sorted_first_and_explicit_windows_observe_full_rank_stage_before_output_truncation() {
        for first in [false, true] {
            let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
            p.root.filter = None;
            if first {
                p.root.iteration_output = mapping::IterationOutput::First;
            } else {
                p.graph.nodes.insert(
                    4,
                    Node::Const {
                        value: Value::Int(2),
                    },
                );
                p.root.windows.push(SequenceWindow::First { count: 4 });
            }
            assert!(!crate::eval_scope::retain_first_source_candidate(&p.root));
            let primary = group(vec![(
                "Rows",
                Instance::Repeated(vec![
                    tagged_row(2, true, "A"),
                    tagged_row(1, true, "B"),
                    tagged_row(2, true, "C"),
                ]),
            )]);
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = SortObservations::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Selection,
                    &[]
                ),
                Ok(if first {
                    tagged_output(1, 1, "B")
                } else {
                    Instance::Repeated(vec![tagged_output(1, 1, "B"), tagged_output(2, 2, "A")])
                })
            );
            assert_eq!(
                sort_events(&sink.events.borrow()),
                tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
            );
            assert_eq!(sink.contexts.borrow().len(), 6);
            assert_eq!(
                sink.events
                    .borrow()
                    .iter()
                    .filter(|e| matches!(e, TraceEvent::NodeValue { node: 0, .. }))
                    .count(),
                if first { 4 } else { 5 }
            );
            assert_eq!(
                sink.events
                    .borrow()
                    .iter()
                    .filter(|e| matches!(e, TraceEvent::TargetProduced { .. }))
                    .count(),
                if first { 1 } else { 2 }
            );
            assert_eq!(
                boundaries(&sink.events.borrow())[1],
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: 3,
                    produced: if first { 1 } else { 2 },
                    kind: if first {
                        TraceOutputKind::Group
                    } else {
                        TraceOutputKind::Repeated
                    }
                }
            );
            let events = sink.events.borrow();
            let last_rank = events
                .iter()
                .rposition(|e| matches!(e, TraceEvent::SortPosition { .. }))
                .unwrap();
            let first_target = events
                .iter()
                .position(|e| matches!(e, TraceEvent::TargetProduced { .. }))
                .unwrap();
            assert!(last_rank < first_target);
        }
        for first in [false, true] {
            let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
            p.root.filter = None;
            if first {
                p.root.iteration_output = mapping::IterationOutput::First;
            } else {
                p.graph.nodes.insert(
                    4,
                    Node::Const {
                        value: Value::Int(2),
                    },
                );
                p.root.windows.push(SequenceWindow::First { count: 4 });
            }
            let primary = group(vec![("Rows", Instance::Repeated(Vec::new()))]);
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = SortObservations::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(if first {
                    group(Vec::new())
                } else {
                    Instance::Repeated(Vec::new())
                })
            );
            assert!(sort_events(&sink.events.borrow()).is_empty());
            assert!(sink.contexts.borrow().is_empty());
            assert!(registry.cache.get().is_none());
            assert!(
                !sink
                    .events
                    .borrow()
                    .iter()
                    .any(|e| matches!(e, TraceEvent::TargetProduced { .. }))
            );
            assert_eq!(
                boundaries(&sink.events.borrow())[1],
                TraceEvent::ScopeFinished {
                    scope: TraceScope::primary(),
                    candidates: 0,
                    produced: 0,
                    kind: if first {
                        TraceOutputKind::Group
                    } else {
                        TraceOutputKind::Repeated
                    }
                }
            );
        }
    }

    #[test]
    fn sort_key_target_and_observer_failures_keep_exact_prefixes_and_mapping_priority() {
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                tagged_row(2, true, "A"),
                tagged_row(1, false, "B"),
                tagged_row(2, true, "C"),
            ]),
        )]);
        let extras = group(Vec::new());
        let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
        p.root.filter = None;
        p.root.sort_then_by.push(mapping::SortKey {
            node: 4,
            descending: false,
        });
        p.graph.nodes.insert(
            4,
            Node::If {
                condition: 1,
                then: 5,
                else_: 6,
            },
        );
        p.graph.nodes.insert(
            5,
            Node::Const {
                value: Value::Int(7),
            },
        );
        p.graph.nodes.insert(6, Node::Raise { message: None });
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        let plain = Collector::default();
        let expected = Err(EngineError::MappingException {
            node: 6,
            message: None,
        });
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            ),
            expected
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            expected
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert_eq!(
            sort_events(&sink.events.borrow()),
            vec![sort_candidate(
                TraceScope::primary(),
                &[position(1)],
                &[(0, false, Value::Int(2)), (4, false, Value::Int(7))]
            )]
        );
        assert_eq!(sink.contexts.borrow().len(), 1);
        assert!(!sink.events.borrow().iter().any(|e| matches!(
            e,
            TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
        )));
        p.graph.nodes.insert(
            4,
            owned_field(SourceOwner::Named { name: "Ref".into() }, &["Rows", "Rank"]),
        );
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Err(EngineError::OwnedSourceAccess {
                node: Some(4),
                scope: None,
                source: OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named { name: "Ref".into() }
                }
            })
        );
        assert!(sort_events(&sink.events.borrow()).is_empty());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        p.root.sort_then_by.clear();
        p.graph.nodes.insert(3, Node::Raise { message: None });
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Err(EngineError::MappingException {
                node: 3,
                message: None
            })
        );
        assert_eq!(
            sort_events(&sink.events.borrow()),
            tie_sort_events(&TraceScope::primary(), &[1, 2, 3], &[2, 1, 3])
        );
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|e| matches!(e, TraceEvent::TargetFieldWritten { .. }))
                .count(),
            2
        );
        assert!(!sink.events.borrow().iter().any(|e| matches!(
            e,
            TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
        )));
        let mut p = sort_project(SourceOwner::Primary, SortFilterOrder::SortThenFilter);
        p.root.filter = None;
        let foreign = constant();
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let wrong = TraceOwnedRegistry::new(&foreign, &roots);
        let sink = SortObservations::new();
        let expected = Instance::Repeated(vec![
            tagged_output(1, 1, "B"),
            tagged_output(2, 2, "A"),
            tagged_output(2, 3, "C"),
        ]);
        assert_eq!(
            run(
                &p,
                &state,
                &wrong,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(sink.contexts.borrow().len(), 6);
        assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Projection(TraceOwnedContextError::MismatchedRegistry)));
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = SortObservations::new();
        assert_eq!(
            run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
            Ok(expected.clone())
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(sink.contexts.borrow().len(), 6);
        assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 7
            }))));
        assert!(std::ptr::eq(state.frames[0].item, &primary));
    }

    struct WindowObservations<'a> {
        events: RefCell<Vec<TraceEvent>>,
        contexts: RefCell<Vec<(TraceEvent, Outcome)>>,
        root_addresses: RefCell<Vec<usize>>,
        selected: Cell<bool>,
        stop_after_first: bool,
        check: Option<&'a Check<'a>>,
    }
    impl WindowObservations<'_> {
        fn new() -> Self {
            Self {
                events: RefCell::new(Vec::new()),
                contexts: RefCell::new(Vec::new()),
                root_addresses: RefCell::new(Vec::new()),
                selected: Cell::new(true),
                stop_after_first: false,
                check: None,
            }
        }
    }
    impl TraceSink for WindowObservations<'_> {
        fn record(&self, event: TraceEvent) {
            self.events.borrow_mut().push(event);
        }
        fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
            self.selected.get() && matches!(event, TraceEvent::WindowApplied { .. })
        }
        fn record_with_owned_context(
            &self,
            event: TraceEvent,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) {
            assert!(matches!(event, TraceEvent::WindowApplied { .. }));
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
            self.contexts.borrow_mut().push((event.clone(), outcome));
            if self.stop_after_first {
                self.selected.set(false);
            }
            self.record(event);
        }
    }
    fn window_events(events: &[TraceEvent]) -> Vec<TraceEvent> {
        events
            .iter()
            .filter(|event| matches!(event, TraceEvent::WindowApplied { .. }))
            .cloned()
            .collect()
    }
    fn window_event(
        scope: TraceScope,
        index: usize,
        window: crate::TraceWindow,
        before: usize,
        after: usize,
    ) -> TraceEvent {
        TraceEvent::WindowApplied {
            scope,
            window_index: index,
            window,
            before,
            after,
        }
    }
    fn value_rows(values: &[&str]) -> Instance {
        group(vec![(
            "Rows",
            Instance::Repeated(values.iter().map(|value| row(value)).collect()),
        )])
    }

    #[test]
    fn chained_windows_keep_all_transform_counts_and_original_count_value_policy() {
        let primary = value_rows(&["a", "b", "c", "d", "e", "f"]);
        let extras = group(Vec::new());
        for first in [false, true] {
            let mut p = repeated(SourceOwner::Primary);
            for (node, value) in [
                (10, Value::String(" 1 ".into())),
                (11, Value::Float(4.9)),
                (12, Value::String("2".into())),
                (13, Value::Int(2)),
                (14, Value::Float(3.8)),
                (15, Value::String("1".into())),
            ] {
                p.graph.nodes.insert(node, Node::Const { value });
            }
            p.root.windows = vec![
                SequenceWindow::SkipFirst { count: 10 },
                SequenceWindow::First { count: 11 },
                SequenceWindow::From { position: 12 },
                SequenceWindow::FromTo {
                    first: 13,
                    last: 14,
                },
                SequenceWindow::Last { count: 15 },
            ];
            if first {
                p.root.iteration_output = mapping::IterationOutput::First;
            }
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let plain = Collector::default();
            let sink = WindowObservations::new();
            let expected = if first {
                row("e")
            } else {
                Instance::Repeated(vec![row("e")])
            };
            assert_eq!(
                run(&p, &state, &registry, None, TraceOwnedPhase::Selection, &[]),
                Ok(expected.clone())
            );
            assert!(registry.cache.get().is_none());
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&plain),
                    TraceOwnedPhase::Selection,
                    &[]
                ),
                Ok(expected.clone())
            );
            assert!(registry.cache.get().is_none());
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Selection,
                    &[]
                ),
                Ok(expected)
            );
            assert_eq!(*sink.events.borrow(), *plain.0.borrow());
            let scope = TraceScope::primary();
            let mut expected_events = vec![
                window_event(scope.clone(), 1, crate::TraceWindow::SkipFirst(1), 6, 5),
                window_event(scope.clone(), 2, crate::TraceWindow::First(4), 5, 4),
                window_event(scope.clone(), 3, crate::TraceWindow::From(2), 4, 3),
                window_event(
                    scope.clone(),
                    4,
                    crate::TraceWindow::FromTo { first: 2, last: 3 },
                    3,
                    2,
                ),
                window_event(scope.clone(), 5, crate::TraceWindow::Last(1), 2, 1),
            ];
            if first {
                expected_events.push(window_event(scope, 6, crate::TraceWindow::First(1), 1, 1));
            }
            assert_eq!(window_events(&sink.events.borrow()), expected_events);
            assert_eq!(sink.contexts.borrow().len(), if first { 6 } else { 5 });
            for (_, outcome) in sink.contexts.borrow().iter() {
                let Outcome::Snapshot(Ok(view)) = outcome else {
                    panic!("window witness")
                };
                assert_eq!(view.phase, TraceOwnedPhase::Selection);
                assert!(
                    view.frames.is_empty() && view.origins.is_empty() && view.positions.is_empty()
                );
                assert!(!view.owned_ancestry);
            }
            assert!(
                sink.root_addresses
                    .borrow()
                    .windows(2)
                    .all(|pair| pair[0] == pair[1])
            );
            for node in 10..=15 {
                assert_eq!(sink.events.borrow().iter().filter(|event| matches!(event, TraceEvent::NodeValue { node: actual, .. } if *actual == node)).count(), 1);
            }
        }
        for (count, evaluated, kept) in
            [(Value::Int(-4), 0, 0), (Value::String("99".into()), 99, 6)]
        {
            let mut p = repeated(SourceOwner::Primary);
            p.graph.nodes.insert(10, Node::Const { value: count });
            p.root.windows = vec![SequenceWindow::First { count: 10 }];
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = WindowObservations::new();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(Instance::Repeated(
                    ["a", "b", "c", "d", "e", "f"]
                        .into_iter()
                        .take(kept)
                        .map(row)
                        .collect()
                ))
            );
            assert_eq!(
                window_events(&sink.events.borrow()),
                vec![window_event(
                    TraceScope::primary(),
                    1,
                    crate::TraceWindow::First(evaluated),
                    6,
                    kept
                )]
            );
        }
    }

    #[test]
    fn window_lengths_measure_sorted_filter_survivors_before_first_truncation() {
        for order in [
            SortFilterOrder::FilterThenSort,
            SortFilterOrder::SortThenFilter,
        ] {
            for first in [false, true] {
                let mut p = filter_project(SourceOwner::Primary, Some(order));
                p.graph.nodes.insert(
                    10,
                    Node::Const {
                        value: Value::Int(1),
                    },
                );
                p.root.windows.push(SequenceWindow::First { count: 10 });
                if first {
                    p.root.iteration_output = mapping::IterationOutput::First;
                }
                let primary = group(vec![(
                    "Rows",
                    Instance::Repeated(vec![
                        rank_row(3, true),
                        rank_row(1, false),
                        rank_row(2, true),
                    ]),
                )]);
                let extras = group(Vec::new());
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let state = OwnedContext::new(&roots);
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let sink = WindowObservations::new();
                assert_eq!(
                    run(
                        &p,
                        &state,
                        &registry,
                        Some(&sink),
                        TraceOwnedPhase::Output,
                        &[]
                    ),
                    Ok(if first {
                        rank_output(2, 1)
                    } else {
                        Instance::Repeated(vec![rank_output(2, 1)])
                    })
                );
                let mut expected = vec![window_event(
                    TraceScope::primary(),
                    1,
                    crate::TraceWindow::First(1),
                    2,
                    1,
                )];
                if first {
                    expected.push(window_event(
                        TraceScope::primary(),
                        2,
                        crate::TraceWindow::First(1),
                        1,
                        1,
                    ));
                }
                assert_eq!(window_events(&sink.events.borrow()), expected);
                assert_eq!(decision_events(&sink.events.borrow()).len(), 3);
                let events = sink.events.borrow();
                let last_filter = events
                    .iter()
                    .rposition(|e| matches!(e, TraceEvent::FilterDecision { .. }))
                    .unwrap();
                let first_window = events
                    .iter()
                    .position(|e| matches!(e, TraceEvent::WindowApplied { .. }))
                    .unwrap();
                let first_target = events
                    .iter()
                    .position(|e| matches!(e, TraceEvent::TargetProduced { .. }))
                    .unwrap();
                assert!(last_filter < first_window && first_window < first_target);
                for (_, outcome) in sink.contexts.borrow().iter() {
                    let Outcome::Snapshot(Ok(view)) = outcome else {
                        panic!("window witness")
                    };
                    assert_eq!(view.phase, TraceOwnedPhase::Output);
                    assert!(view.frames.is_empty() && view.positions.is_empty());
                }
                assert_eq!(
                    events
                        .iter()
                        .filter(|e| matches!(e, TraceEvent::TargetProduced { .. }))
                        .count(),
                    1
                );
            }
        }
    }

    #[test]
    fn bounded_first_window_reports_complete_logical_cohort_without_retaining_tail() {
        struct Counting<'a> {
            expected: usize,
            root: &'a Instance,
            candidates: Cell<usize>,
            produced: Cell<usize>,
            callbacks: Cell<usize>,
            window: RefCell<Option<TraceEvent>>,
        }
        impl TraceSink for Counting<'_> {
            fn wants_source_row(&self) -> bool {
                false
            }
            fn wants_owned_context_for(&self, event: &TraceEvent) -> bool {
                matches!(event, TraceEvent::WindowApplied { .. })
            }
            fn record(&self, event: TraceEvent) {
                match event {
                    TraceEvent::IterationCandidate {
                        ordinal,
                        source_row,
                        ..
                    } => {
                        assert_eq!(ordinal, self.candidates.get() + 1);
                        assert!(source_row.is_none());
                        self.candidates.set(ordinal);
                    }
                    TraceEvent::WindowApplied { .. } => {
                        assert!(self.window.borrow().is_none());
                        *self.window.borrow_mut() = Some(event);
                    }
                    TraceEvent::TargetProduced { .. } => self.produced.set(self.produced.get() + 1),
                    TraceEvent::ScopeFinished {
                        candidates,
                        produced,
                        ..
                    } => {
                        assert_eq!(candidates, self.expected);
                        assert_eq!(produced, usize::from(self.expected != 0));
                    }
                    _ => {}
                }
            }
            fn record_with_owned_context(
                &self,
                event: TraceEvent,
                context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
            ) {
                let view = context.unwrap();
                assert_eq!(view.phase, TraceOwnedPhase::Selection);
                assert!(
                    view.frames.is_empty() && view.origins.is_empty() && view.positions.is_empty()
                );
                assert!(std::ptr::eq(view.roots[0].input.unwrap(), self.root));
                assert_eq!(view.snapshot().unwrap().phase, TraceOwnedPhase::Selection);
                self.callbacks.set(self.callbacks.get() + 1);
                self.record(event);
            }
        }
        for count in [0, 1, 3, 4096] {
            let mut p = repeated(SourceOwner::Primary);
            p.root.iteration_output = mapping::IterationOutput::First;
            assert!(crate::eval_scope::retain_first_source_candidate(&p.root));
            let primary = group(vec![(
                "Rows",
                Instance::Repeated((0..count).map(|i| row(&i.to_string())).collect()),
            )]);
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = Counting {
                expected: count,
                root: &primary,
                candidates: Cell::new(0),
                produced: Cell::new(0),
                callbacks: Cell::new(0),
                window: RefCell::new(None),
            };
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Selection,
                    &[]
                ),
                Ok(if count == 0 {
                    group(Vec::new())
                } else {
                    row("0")
                })
            );
            assert_eq!(sink.candidates.get(), count);
            assert_eq!(sink.produced.get(), usize::from(count != 0));
            assert_eq!(sink.callbacks.get(), 1);
            assert_eq!(
                *sink.window.borrow(),
                Some(window_event(
                    TraceScope::primary(),
                    1,
                    crate::TraceWindow::First(1),
                    count,
                    usize::from(count != 0)
                ))
            );
            assert!(registry.cache.get().is_some());
            let source = OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Rows"]),
            };
            let (owner, base, prefix, tail, budget) = state.select(&source).unwrap();
            let selected = crate::source_iteration::select_observed(
                crate::source_iteration::ObservedWalkRequest {
                    base,
                    path: tail,
                    raw_prefix: prefix,
                    canonical_prefix: prefix,
                    owner: Some(owner),
                    context: &state,
                    position_offset: 0,
                    strict_collection: true,
                },
                &budget,
                true,
            )
            .unwrap();
            assert_eq!(selected.candidates, count);
            assert_eq!(selected.extensions.len(), usize::from(count != 0));
        }
        let mut p = repeated(SourceOwner::Primary);
        p.root.iteration_output = mapping::IterationOutput::First;
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                Instance::Scalar(Value::Null),
                row("one"),
                Instance::Scalar(Value::JsonNull(ir::JsonNull)),
                row("two"),
            ]),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Structural,
                &[]
            ),
            Ok(row("one"))
        );
        assert_eq!(candidate_events(&sink.events.borrow()).len(), 2);
        assert_eq!(
            window_events(&sink.events.borrow()),
            vec![window_event(
                TraceScope::primary(),
                1,
                crate::TraceWindow::First(1),
                2,
                1
            )]
        );
    }

    #[test]
    fn window_witness_borrows_actual_receiving_parent_phase_and_named_identity() {
        for key in [OwnerKey::Primary, OwnerKey::Named(1)] {
            for phase in [
                TraceOwnedPhase::Structural,
                TraceOwnedPhase::Selection,
                TraceOwnedPhase::Output,
            ] {
                let mut p = constant();
                p.extra_sources =
                    project(&["Ref", " Ref 雪 "], Vec::new(), Scope::default()).extra_sources;
                p.graph.nodes.insert(
                    10,
                    Node::Const {
                        value: Value::Int(1),
                    },
                );
                p.root.windows.push(SequenceWindow::First { count: 10 });
                let primary = value_rows(&["P.one", "P.two"]);
                let extras = group(vec![
                    ("Ref", value_rows(&["N.one", "N.two"])),
                    (" Ref 雪 ", value_rows(&["U.one", "U.two"])),
                ]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let original = roots.root(key).unwrap();
                let item = &original.field("Rows").unwrap().as_repeated().unwrap()[1];
                let state = OwnedContext {
                    roots: &roots,
                    frames: vec![SourceFrame {
                        owner: key,
                        kind: crate::owned_context::OccurrenceKind::Item,
                        collection: path(&["Rows"]),
                        item,
                        position_slot: 0,
                    }],
                    origins: vec![SourceOrigin {
                        kind: crate::owned_context::SourceOriginKind::Ordinary,
                        owner: key,
                        path: path(&["Rows"]),
                        item,
                    }],
                    owned_ancestry: true,
                };
                let positions = [position(2)];
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let check = |_: &TraceEvent, view: &TraceOwnedWitness<'_>| {
                    assert_eq!(view.phase, phase);
                    assert!(view.owned_ancestry);
                    assert_eq!(view.frames.len(), 1);
                    assert_eq!(view.origins.len(), 1);
                    assert_eq!(view.positions.len(), 1);
                    assert!(std::ptr::eq(view.frames[0].item, item));
                    assert!(std::ptr::eq(view.origins[0].item, item));
                    assert_eq!(
                        view.frames[0].collection.as_ptr(),
                        state.frames[0].collection.as_ptr()
                    );
                    assert_eq!(
                        view.origins[0].path.as_ptr(),
                        state.origins[0].path.as_ptr()
                    );
                    assert_eq!(
                        view.positions[0].collection.as_ptr(),
                        positions[0].collection.as_ptr()
                    );
                    assert_eq!(view.positions[0].index, 2);
                    match key {
                        OwnerKey::Primary => {
                            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary)
                        }
                        OwnerKey::Named(index) => {
                            let TraceOwnedOwnerRef::Named {
                                declaration_index,
                                name,
                            } = view.frames[0].owner
                            else {
                                panic!("named receiving owner")
                            };
                            assert_eq!(declaration_index, index);
                            assert_eq!(name, " Ref 雪 ");
                            assert_eq!(name.as_ptr(), p.extra_sources[index].name.as_ptr());
                            assert!(std::ptr::eq(view.roots[index + 1].input.unwrap(), original));
                        }
                    }
                };
                let mut sink = WindowObservations::new();
                sink.check = Some(&check);
                assert_eq!(
                    run(&p, &state, &registry, Some(&sink), phase, &positions),
                    Ok(group(vec![("Value", Instance::Scalar(Value::Int(7)))]))
                );
                assert_eq!(
                    window_events(&sink.events.borrow()),
                    vec![window_event(
                        TraceScope::primary(),
                        1,
                        crate::TraceWindow::First(1),
                        1,
                        1
                    )]
                );
                assert_eq!(sink.contexts.borrow().len(), 1);
            }
        }
        let mut p = repeated(SourceOwner::Primary);
        let mut child = repeated(SourceOwner::Named { name: "Ref".into() }).root;
        child.target_field = "Child".into();
        child.bindings[0].node = 1;
        child.windows.push(SequenceWindow::First { count: 10 });
        p.graph.nodes.insert(
            1,
            owned_field(
                SourceOwner::Named { name: "Ref".into() },
                &["Rows", "Value"],
            ),
        );
        p.graph.nodes.insert(
            10,
            Node::Const {
                value: Value::Int(1),
            },
        );
        p.root.children.push(child);
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                Instance::Scalar(Value::Null),
                row("parent-one"),
                row("parent-two"),
            ]),
        )]);
        let extras = group(vec![("Ref", value_rows(&["child-one", "child-two"]))]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let parents = primary.field("Rows").unwrap().as_repeated().unwrap();
        let scope = TraceScope::primary().child("Child", 0);
        let seen = Cell::new(0usize);
        let check = |event: &TraceEvent, view: &TraceOwnedWitness<'_>| {
            let TraceEvent::WindowApplied { scope: actual, .. } = event else {
                unreachable!()
            };
            assert_eq!(actual, &scope);
            assert_eq!(view.phase, TraceOwnedPhase::Output);
            assert_eq!(view.frames.len(), 1);
            assert_eq!(view.origins.len(), 1);
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            let raw = seen.get() + 2;
            assert!(std::ptr::eq(view.frames[0].item, &parents[raw - 1]));
            assert!(std::ptr::eq(view.origins[0].item, &parents[raw - 1]));
            assert_eq!(view.positions[0].index, raw);
            seen.set(seen.get() + 1);
        };
        let mut sink = WindowObservations::new();
        sink.check = Some(&check);
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Selection,
                &[]
            ),
            Ok(Instance::Repeated(
                ["parent-one", "parent-two"]
                    .into_iter()
                    .map(|value| group(vec![
                        ("Value", text(value)),
                        ("Child", Instance::Repeated(vec![row("child-one")]))
                    ]))
                    .collect()
            ))
        );
        assert_eq!(
            window_events(&sink.events.borrow()),
            vec![
                window_event(scope.clone(), 1, crate::TraceWindow::First(1), 2, 1),
                window_event(scope.clone(), 1, crate::TraceWindow::First(1), 2, 1)
            ]
        );
        assert_eq!(seen.get(), 2);
    }

    #[test]
    fn window_observer_preferences_legacy_events_and_metadata_errors_stay_isolated() {
        let mut p = repeated(SourceOwner::Primary);
        p.graph.nodes.insert(
            10,
            Node::Const {
                value: Value::Int(1),
            },
        );
        p.graph.nodes.insert(
            11,
            Node::Const {
                value: Value::Int(1),
            },
        );
        p.root.windows = vec![
            SequenceWindow::SkipFirst { count: 10 },
            SequenceWindow::First { count: 11 },
        ];
        let primary = value_rows(&["a", "b", "c"]);
        let extras = group(Vec::new());
        let expected = Instance::Repeated(vec![row("b")]);
        for selected in [false, true] {
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = WindowObservations::new();
            sink.selected.set(selected);
            let plain = Collector::default();
            assert_eq!(
                run(&p, &state, &registry, None, TraceOwnedPhase::Output, &[]),
                Ok(expected.clone())
            );
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&plain),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(expected.clone())
            );
            assert!(registry.cache.get().is_none());
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Ok(expected.clone())
            );
            assert_eq!(*sink.events.borrow(), *plain.0.borrow());
            assert_eq!(sink.contexts.borrow().len(), if selected { 2 } else { 0 });
            assert_eq!(registry.cache.get().is_some(), selected);
            assert!(
                sink.root_addresses
                    .borrow()
                    .windows(2)
                    .all(|pair| pair[0] == pair[1])
            );
        }
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let mut changing = WindowObservations::new();
        changing.stop_after_first = true;
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&changing),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(changing.contexts.borrow().len(), 1);
        assert_eq!(window_events(&changing.events.borrow()).len(), 2);
        struct BooleanOnly(Cell<usize>);
        impl TraceSink for BooleanOnly {
            fn record(&self, _: TraceEvent) {}
            fn wants_owned_context(&self) -> bool {
                true
            }
            fn record_with_owned_context(
                &self,
                event: TraceEvent,
                _: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
            ) {
                assert!(matches!(
                    event,
                    TraceEvent::NodeValue { .. } | TraceEvent::NodeInputValue { .. }
                ));
                self.0.set(self.0.get() + 1);
            }
        }
        let old = BooleanOnly(Cell::new(0));
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&old),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(old.0.get(), 3);
        let foreign = constant();
        let wrong = TraceOwnedRegistry::new(&foreign, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &wrong,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(sink.contexts.borrow().len(), 2);
        assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Projection(TraceOwnedContextError::MismatchedRegistry)));
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert!(sink.contexts.borrow().iter().all(|(_, outcome)| *outcome
            == Outcome::Snapshot(Err(TraceOwnedWitnessError::InvalidPositionSlot {
                at: TraceOwnedLocation::Frame(0),
                slot: 7
            }))));
        assert_eq!(sink.contexts.borrow().len(), 2);
        p.root.windows.clear();
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(Instance::Repeated(vec![row("a"), row("b"), row("c")]))
        );
        assert!(window_events(&sink.events.borrow()).is_empty());
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        p.root.windows = vec![
            SequenceWindow::SkipFirst { count: 10 },
            SequenceWindow::First { count: 11 },
        ];
        p.root.iteration = ScopeIteration::Source(path(&["Rows"]));
        p.graph.nodes.insert(
            0,
            Node::SourceField {
                path: path(&["Value"]),
                frame: Some(path(&["Rows"])),
            },
        );
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        let plain = Collector::default();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected.clone())
        );
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&plain),
                TraceOwnedPhase::Output,
                &[]
            ),
            Ok(expected)
        );
        assert_eq!(*sink.events.borrow(), *plain.0.borrow());
        assert_eq!(
            window_events(&sink.events.borrow()),
            vec![
                window_event(
                    TraceScope::primary(),
                    1,
                    crate::TraceWindow::SkipFirst(1),
                    3,
                    2
                ),
                window_event(TraceScope::primary(), 2, crate::TraceWindow::First(1), 2, 1)
            ]
        );
        assert!(sink.contexts.borrow().is_empty());
        assert!(registry.cache.get().is_none());
    }

    #[test]
    fn failed_window_inputs_and_preapplication_work_preserve_error_order_and_event_prefixes() {
        let primary = value_rows(&["a", "b", "c"]);
        let extras = group(Vec::new());
        for (node, expected) in [
            (
                Node::Const {
                    value: Value::Bool(true),
                },
                EngineError::NotAnItemCount {
                    node: 11,
                    found: "bool",
                },
            ),
            (
                Node::Raise { message: None },
                EngineError::MappingException {
                    node: 11,
                    message: None,
                },
            ),
            (
                owned_field(
                    SourceOwner::Named { name: "Ref".into() },
                    &["Rows", "Value"],
                ),
                EngineError::OwnedSourceAccess {
                    node: Some(11),
                    scope: None,
                    source: OwnedSourceError::SourceUnavailable {
                        owner: SourceOwner::Named { name: "Ref".into() },
                    },
                },
            ),
        ] {
            let mut p = repeated(SourceOwner::Primary);
            p.graph.nodes.insert(
                10,
                Node::Const {
                    value: Value::Int(1),
                },
            );
            p.graph.nodes.insert(11, node);
            p.root.windows = vec![
                SequenceWindow::First { count: 10 },
                SequenceWindow::Last { count: 11 },
            ];
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = WindowObservations::new();
            let plain = Collector::default();
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&plain),
                    TraceOwnedPhase::Output,
                    &[]
                )
                .as_ref(),
                Err(&expected)
            );
            assert_eq!(
                run(
                    &p,
                    &state,
                    &registry,
                    Some(&sink),
                    TraceOwnedPhase::Output,
                    &[]
                ),
                Err(expected)
            );
            assert_eq!(*sink.events.borrow(), *plain.0.borrow());
            assert!(window_events(&sink.events.borrow()).is_empty());
            assert!(sink.contexts.borrow().is_empty());
            assert!(registry.cache.get().is_none());
            assert_eq!(
                sink.events
                    .borrow()
                    .iter()
                    .filter(|event| matches!(event, TraceEvent::NodeValue { node: 10, .. }))
                    .count(),
                1
            );
            assert!(!sink.events.borrow().iter().any(|e| matches!(
                e,
                TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
            )));
        }
        let mut p = repeated(SourceOwner::Primary);
        p.root.iteration_output = mapping::IterationOutput::First;
        let roots = SourceRoots::new(
            &p,
            &primary,
            &extras,
            OwnedLimits {
                walk_contexts: 2,
                ..OwnedLimits::default()
            },
        )
        .unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Err(EngineError::OwnedSourceAccess {
                node: None,
                scope: Some(Box::new(TraceScope::primary().source_scope)),
                source: OwnedSourceError::ResourceLimit {
                    resource: "walk contexts",
                    limit: 2
                }
            })
        );
        assert!(window_events(&sink.events.borrow()).is_empty());
        assert_eq!(candidate_events(&sink.events.borrow()).len(), 2);
        p.root.iteration = ScopeIteration::OwnedSource {
            source: OwnedSourceRef {
                owner: SourceOwner::Primary,
                path: path(&["Buckets", "Rows"]),
            },
            output_path: None,
        };
        let malformed = group(vec![(
            "Buckets",
            Instance::Repeated(vec![
                group(vec![("Rows", Instance::Repeated(vec![row("good")]))]),
                text("bad"),
            ]),
        )]);
        let roots = SourceRoots::new(&p, &malformed, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Err(EngineError::OwnedSourceAccess {
                node: None,
                scope: Some(Box::new(TraceScope::primary().source_scope)),
                source: OwnedSourceError::MalformedOwnedTraversal {
                    path: path(&["Buckets"]),
                    found: "a non-null scalar"
                }
            })
        );
        assert!(window_events(&sink.events.borrow()).is_empty());
        assert_eq!(candidate_events(&sink.events.borrow()).len(), 1);
        let mut p = repeated(SourceOwner::Primary);
        p.graph.nodes.insert(
            10,
            Node::Const {
                value: Value::Int(1),
            },
        );
        p.graph.nodes.insert(20, Node::Raise { message: None });
        p.root.windows = vec![SequenceWindow::First { count: 10 }];
        p.root.filter = Some(20);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Err(EngineError::MappingException {
                node: 20,
                message: None
            })
        );
        assert!(window_events(&sink.events.borrow()).is_empty());
        assert!(decision_events(&sink.events.borrow()).is_empty());
        p.root.filter = None;
        p.root.bindings.push(Binding {
            target_field: "Failed".into(),
            node: 20,
        });
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let sink = WindowObservations::new();
        assert_eq!(
            run(
                &p,
                &state,
                &registry,
                Some(&sink),
                TraceOwnedPhase::Output,
                &[]
            ),
            Err(EngineError::MappingException {
                node: 20,
                message: None
            })
        );
        assert_eq!(
            window_events(&sink.events.borrow()),
            vec![window_event(
                TraceScope::primary(),
                1,
                crate::TraceWindow::First(1),
                3,
                1
            )]
        );
        assert_eq!(
            sink.events
                .borrow()
                .iter()
                .filter(|e| matches!(e, TraceEvent::TargetFieldWritten { .. }))
                .count(),
            1
        );
        assert!(!sink.events.borrow().iter().any(|e| matches!(
            e,
            TraceEvent::TargetProduced { .. } | TraceEvent::ScopeFinished { .. }
        )));
    }

    type FailureTypedCheck<'a> = dyn Fn(&crate::PendingNodeFailure, &EngineError) + 'a;
    type FailureWitnessCheck<'a> = dyn Fn(&crate::PendingNodeFailure, &TraceOwnedWitness<'_>) + 'a;
    struct FailureObservations<'a> {
        pending: RefCell<Vec<crate::PendingNodeFailure>>,
        outcomes: RefCell<Vec<Outcome>>,
        root_addresses: RefCell<Vec<usize>>,
        selected: Cell<bool>,
        interested: bool,
        cancel: bool,
        select_at_value: Option<(u32, bool)>,
        typed: Option<&'a FailureTypedCheck<'a>>,
        check: Option<&'a FailureWitnessCheck<'a>>,
        function_failures: Cell<usize>,
        function_interest: bool,
    }
    impl FailureObservations<'_> {
        fn new() -> Self {
            Self {
                pending: RefCell::new(Vec::new()),
                outcomes: RefCell::new(Vec::new()),
                root_addresses: RefCell::new(Vec::new()),
                selected: Cell::new(true),
                interested: true,
                cancel: false,
                select_at_value: None,
                typed: None,
                check: None,
                function_failures: Cell::new(0),
                function_interest: false,
            }
        }
    }
    impl crate::DebugHook for FailureObservations<'_> {
        fn before_target_write(&self, _: &crate::PendingTargetWrite) -> crate::DebugDecision {
            crate::DebugDecision::Resume
        }
        fn wants_node_values(&self) -> bool {
            self.select_at_value.is_some()
        }
        fn wants_node_inputs(&self) -> bool {
            false
        }
        fn wants_node_failures(&self) -> bool {
            self.interested
        }
        fn wants_owned_context_for_node_failure(&self, _: &crate::PendingNodeFailure) -> bool {
            self.selected.get()
        }
        fn after_node_value(&self, value: &crate::PendingNodeValue) -> crate::DebugDecision {
            if let Some((node, selected)) = self.select_at_value
                && value.node == node
            {
                self.selected.set(selected);
            }
            crate::DebugDecision::Resume
        }
        fn after_node_failure(&self, pending: &crate::PendingNodeFailure) -> crate::DebugDecision {
            self.pending.borrow_mut().push(pending.clone());
            if self.cancel {
                crate::DebugDecision::Cancel
            } else {
                crate::DebugDecision::Resume
            }
        }
        fn after_node_failure_with_owned_context(
            &self,
            pending: &crate::PendingNodeFailure,
            error: &EngineError,
            context: Result<TraceOwnedWitness<'_>, &TraceOwnedContextError>,
        ) -> crate::DebugDecision {
            if let Some(check) = self.typed {
                check(pending, error);
            }
            let outcome = match context {
                Ok(view) => {
                    if let Some(check) = self.check {
                        check(pending, &view);
                    }
                    self.root_addresses
                        .borrow_mut()
                        .push(view.roots.as_ptr() as usize);
                    Outcome::Snapshot(view.snapshot())
                }
                Err(error) => Outcome::Projection(error.clone()),
            };
            self.outcomes.borrow_mut().push(outcome);
            self.after_node_failure(pending)
        }
        fn wants_function_node_failures(&self) -> bool {
            self.function_interest
        }
        fn after_function_node_failure(
            &self,
            pending: &crate::PendingFunctionNodeFailure,
        ) -> crate::DebugDecision {
            assert_eq!(pending.function, mapping::FunctionId::new(9));
            assert_eq!(pending.node, 2);
            self.function_failures.set(self.function_failures.get() + 1);
            crate::DebugDecision::Resume
        }
    }
    #[allow(clippy::too_many_arguments)]
    fn failure_expr(
        p: &Project,
        state: &OwnedContext<'_>,
        registry: Option<&dyn TraceOwnedRegistryView>,
        sink: Option<&dyn TraceSink>,
        hook: Option<&dyn crate::DebugHook>,
        reported: &Cell<bool>,
        phase: TraceOwnedPhase,
        positions: &[PositionFrame],
        node: u32,
    ) -> Result<Value, EngineError> {
        let mut program = EvalProgram::new(&p.graph, &p.user_functions, sink, reported)
            .with_owned_context(state)
            .with_owned_trace_phase(phase)
            .with_primary_source(state.roots.primary)
            .with_debug_hook(hook);
        if let Some(registry) = registry {
            program = program.with_owned_trace_registry(registry);
        }
        let result = crate::eval_expr::eval_expr(
            program,
            node,
            &[state.roots.extras_wrapper, state.roots.primary],
            positions,
            &mut std::collections::HashSet::new(),
        );
        eprintln!(
            "RAW failure expression node={node} phase={phase:?} positions={positions:#?} complete_result={result:#?}",
            positions = crate::trace::trace_positions(positions)
        );
        result
    }
    fn failure_scope(
        p: &Project,
        state: &OwnedContext<'_>,
        registry: &dyn TraceOwnedRegistryView,
        sink: Option<&dyn TraceSink>,
        hook: &dyn crate::DebugHook,
    ) -> Result<Instance, EngineError> {
        let reported = Cell::new(false);
        let program = EvalProgram::new(&p.graph, &p.user_functions, sink, &reported)
            .with_owned_context(state)
            .with_owned_trace_registry(registry)
            .with_primary_source(state.roots.primary)
            .with_debug_hook(Some(hook));
        let result = crate::eval_scope::eval_scope(
            program,
            &p.root,
            None,
            &[state.roots.extras_wrapper, state.roots.primary],
            &[],
            &p.extra_sources,
            None,
            &TraceScope::primary(),
        );
        eprintln!("RAW failure scope complete_result={result:#?}");
        result
    }

    #[test]
    fn failure_owned_default_declining_and_forwarding_hooks_preserve_original_contract() {
        struct ForwardOnly {
            pending: RefCell<Vec<crate::PendingNodeFailure>>,
            owned: bool,
        }
        impl crate::DebugHook for ForwardOnly {
            fn before_target_write(&self, _: &crate::PendingTargetWrite) -> crate::DebugDecision {
                crate::DebugDecision::Resume
            }
            fn wants_node_values(&self) -> bool {
                false
            }
            fn wants_node_inputs(&self) -> bool {
                false
            }
            fn wants_node_failures(&self) -> bool {
                true
            }
            fn wants_owned_context_for_node_failure(&self, _: &crate::PendingNodeFailure) -> bool {
                self.owned
            }
            fn after_node_failure(
                &self,
                pending: &crate::PendingNodeFailure,
            ) -> crate::DebugDecision {
                self.pending.borrow_mut().push(pending.clone());
                crate::DebugDecision::Resume
            }
        }
        let mut p = constant();
        p.graph.nodes.insert(0, Node::Raise { message: None });
        let primary = row("primary");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let expected = EngineError::MappingException {
            node: 0,
            message: None,
        };
        let plain = Collector::default();
        let flag = Cell::new(false);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                Some(&plain),
                None,
                &flag,
                TraceOwnedPhase::Selection,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert!(!flag.get());
        assert!(registry.cache.get().is_none());
        let mut old_pending = None;
        for owned in [false, true] {
            let hook = ForwardOnly {
                pending: RefCell::new(Vec::new()),
                owned,
            };
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = Collector::default();
            let flag = Cell::new(false);
            assert_eq!(
                failure_expr(
                    &p,
                    &state,
                    Some(&registry),
                    Some(&sink),
                    Some(&hook),
                    &flag,
                    TraceOwnedPhase::Selection,
                    &[],
                    0
                )
                .as_ref(),
                Err(&expected)
            );
            assert!(flag.get());
            assert_eq!(*sink.0.borrow(), *plain.0.borrow());
            assert_eq!(hook.pending.borrow().len(), 1);
            assert_eq!(hook.pending.borrow()[0].node, 0);
            assert_eq!(hook.pending.borrow()[0].error.preview, expected.to_string());
            assert!(!hook.pending.borrow()[0].error.truncated);
            if let Some(old) = &old_pending {
                assert_eq!(&*hook.pending.borrow(), old);
            } else {
                old_pending = Some(hook.pending.borrow().clone());
            }
            assert_eq!(registry.cache.get().is_some(), owned);
        }
        let mut hook = FailureObservations::new();
        hook.interested = false;
        let flag = Cell::new(false);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Selection,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert!(!flag.get());
        assert!(hook.pending.borrow().is_empty());
        assert!(hook.outcomes.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        let hook = FailureObservations::new();
        let flag = Cell::new(false);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                None,
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Selection,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert_eq!(hook.pending.borrow().len(), 1);
        assert!(hook.outcomes.borrow().is_empty());
    }

    #[test]
    fn failure_owned_deepest_callback_borrows_full_typed_leaf_and_wrapped_causes() {
        for missing in [false, true] {
            let p = project(
                &[],
                vec![
                    (
                        0,
                        Node::If {
                            condition: 1,
                            then: 2,
                            else_: 4,
                        },
                    ),
                    (
                        1,
                        Node::Const {
                            value: Value::Bool(true),
                        },
                    ),
                    (
                        2,
                        if missing {
                            Node::SourceField {
                                path: path(&["Absent"]),
                                frame: None,
                            }
                        } else {
                            Node::Raise { message: Some(3) }
                        },
                    ),
                    (
                        3,
                        Node::Const {
                            value: Value::String("full typed message".into()),
                        },
                    ),
                    (4, Node::Raise { message: None }),
                ],
                Scope::default(),
            );
            let primary = row("primary");
            let extras = group(Vec::new());
            let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
            let state = OwnedContext::new(&roots);
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let sink = Collector::default();
            let reported = Cell::new(false);
            let expected = if missing {
                EngineError::MissingSourceField("Absent".into())
            } else {
                EngineError::MappingException {
                    node: 2,
                    message: Some("full typed message".into()),
                }
            };
            let typed = |pending: &crate::PendingNodeFailure, error: &EngineError| {
                assert_eq!(pending.node, 2);
                assert_eq!(error, &expected);
            };
            let mut hook = FailureObservations::new();
            hook.typed = Some(&typed);
            assert_eq!(
                failure_expr(
                    &p,
                    &state,
                    Some(&registry),
                    Some(&sink),
                    Some(&hook),
                    &reported,
                    TraceOwnedPhase::Structural,
                    &[],
                    0
                )
                .as_ref(),
                Err(&expected)
            );
            assert_eq!(hook.pending.borrow().len(), 1);
            assert_eq!(hook.pending.borrow()[0].node, 2);
            assert_eq!(hook.outcomes.borrow().len(), 1);
            assert!(reported.get());
            assert!(!sink.0.borrow().iter().any(|event| matches!(
                event,
                TraceEvent::NodeValue {
                    node: 0 | 2 | 4,
                    ..
                } | TraceEvent::NodeInputValue {
                    consumer: 0,
                    input: 2 | 4,
                    ..
                }
            )));
            assert_eq!(
                snapshot(&hook.outcomes.borrow()[0]).phase,
                TraceOwnedPhase::Structural
            );
            let run =
                crate::filter_map::FilterMapRunState::new(crate::FilterMapLimits::default(), None);
            let boundary = crate::FilterMapBoundary {
                item: 99,
                phase: crate::FilterMapPhase::Mapper,
                capture_index: Some(3),
                source_position: Some(7),
                function: None,
                node: Some(2),
                kind: crate::FilterMapBoundaryKind::NodeEvaluation,
            };
            let wrapped = EngineError::FilterMapRuntime {
                boundary,
                source: Box::new(if missing {
                    EngineError::MissingSourceField("Absent".into())
                } else {
                    EngineError::MappingException {
                        node: 2,
                        message: Some("full typed message".into()),
                    }
                }),
            };
            let typed = |pending: &crate::PendingNodeFailure, error: &EngineError| {
                assert_eq!(pending.node, 2);
                assert_eq!(error, &wrapped);
            };
            let mut hook = FailureObservations::new();
            hook.typed = Some(&typed);
            let reported = Cell::new(false);
            let program = EvalProgram::new(&p.graph, &p.user_functions, None, &reported)
                .with_owned_context(&state)
                .with_owned_trace_registry(&registry)
                .with_debug_hook(Some(&hook))
                .with_filter_map_work(crate::filter_map::FilterMapWork {
                    state: &run,
                    boundary,
                });
            let result = crate::eval_expr::eval_expr(
                program,
                0,
                &[&extras, &primary],
                &[],
                &mut std::collections::HashSet::new(),
            );
            eprintln!("RAW complete wrapped failure={result:#?}");
            assert_eq!(result.as_ref(), Err(&wrapped));
            assert_eq!(hook.pending.borrow().len(), 1);
            assert_eq!(hook.outcomes.borrow().len(), 1);
        }
    }

    #[test]
    fn failure_owned_witness_uses_actual_named_roots_frames_phases_and_compacted_parent() {
        for phase in [
            TraceOwnedPhase::Structural,
            TraceOwnedPhase::Selection,
            TraceOwnedPhase::Output,
        ] {
            for key in [OwnerKey::Primary, OwnerKey::Named(1)] {
                let mut p = project(
                    &["Ref", " Ref 雪 "],
                    vec![(0, Node::Raise { message: None })],
                    Scope::default(),
                );
                p.root.bindings = vec![Binding {
                    target_field: "Value".into(),
                    node: 0,
                }];
                let primary = group(vec![
                    ("Rows", Instance::Repeated(vec![row("P.one"), row("P.two")])),
                    ("Ref", row("primary shadow")),
                ]);
                let extras = group(vec![
                    ("Ref", value_rows(&["N.one", "N.two"])),
                    (" Ref 雪 ", value_rows(&["U.one", "U.two"])),
                ]);
                let roots =
                    SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
                let original = roots.root(key).unwrap();
                let item = &original.field("Rows").unwrap().as_repeated().unwrap()[1];
                let state = OwnedContext {
                    roots: &roots,
                    frames: vec![SourceFrame {
                        owner: key,
                        kind: crate::owned_context::OccurrenceKind::Item,
                        collection: path(&["Rows"]),
                        item,
                        position_slot: 0,
                    }],
                    origins: vec![SourceOrigin {
                        kind: crate::owned_context::SourceOriginKind::Ordinary,
                        owner: key,
                        path: path(&["Rows"]),
                        item,
                    }],
                    owned_ancestry: true,
                };
                let positions = [position(2)];
                let registry = TraceOwnedRegistry::new(&p, &roots);
                let check = |pending: &crate::PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
                    assert_eq!(pending.node, 0);
                    assert_eq!(view.phase, phase);
                    assert!(view.owned_ancestry);
                    assert_eq!(view.frames.len(), 1);
                    assert_eq!(view.origins.len(), 1);
                    assert_eq!(view.positions.len(), 1);
                    assert!(std::ptr::eq(view.frames[0].item, item));
                    assert!(std::ptr::eq(view.origins[0].item, item));
                    assert_eq!(
                        view.frames[0].collection.as_ptr(),
                        state.frames[0].collection.as_ptr()
                    );
                    assert_eq!(
                        view.origins[0].path.as_ptr(),
                        state.origins[0].path.as_ptr()
                    );
                    assert_eq!(
                        view.positions[0].collection.as_ptr(),
                        positions[0].collection.as_ptr()
                    );
                    assert_eq!(view.positions[0].index, 2);
                    assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
                    if let OwnerKey::Named(index) = key {
                        let TraceOwnedOwnerRef::Named {
                            declaration_index,
                            name,
                        } = view.frames[0].owner
                        else {
                            panic!("named owner")
                        };
                        assert_eq!(declaration_index, index);
                        assert_eq!(name, " Ref 雪 ");
                        assert_eq!(name.as_ptr(), p.extra_sources[index].name.as_ptr());
                        assert!(std::ptr::eq(view.roots[index + 1].input.unwrap(), original));
                    } else {
                        assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
                    }
                };
                let mut hook = FailureObservations::new();
                hook.check = Some(&check);
                let reported = Cell::new(false);
                assert_eq!(
                    failure_expr(
                        &p,
                        &state,
                        Some(&registry),
                        None,
                        Some(&hook),
                        &reported,
                        phase,
                        &positions,
                        0
                    ),
                    Err(EngineError::MappingException {
                        node: 0,
                        message: None
                    })
                );
                assert_eq!(hook.outcomes.borrow().len(), 1);
                assert!(snapshot(&hook.outcomes.borrow()[0]).owned_ancestry);
            }
        }
        let mut p = repeated(SourceOwner::Primary);
        p.graph
            .nodes
            .insert(10, owned_field(SourceOwner::Primary, &["Rows", "Keep"]));
        p.root.filter = Some(10);
        let mut child = repeated(SourceOwner::Named { name: "Ref".into() }).root;
        child.target_field = "Child".into();
        child.bindings[0].node = 20;
        p.root.children.push(child);
        p.graph.nodes.insert(20, Node::Raise { message: Some(21) });
        p.graph.nodes.insert(
            21,
            owned_field(
                SourceOwner::Named { name: "Ref".into() },
                &["Rows", "Value"],
            ),
        );
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(vec![
                group(vec![
                    ("Value", text("discard")),
                    ("Keep", Instance::Scalar(Value::Bool(false))),
                ]),
                group(vec![
                    ("Value", text("keep")),
                    ("Keep", Instance::Scalar(Value::Bool(true))),
                ]),
            ]),
        )]);
        let extras = group(vec![("Ref", value_rows(&["named-child", "later"]))]);
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let parent = &primary.field("Rows").unwrap().as_repeated().unwrap()[1];
        let named = roots.root(OwnerKey::Named(0)).unwrap();
        let child = &named.field("Rows").unwrap().as_repeated().unwrap()[0];
        let check = |pending: &crate::PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
            assert_eq!(pending.node, 20);
            assert_eq!(view.phase, TraceOwnedPhase::Output);
            assert_eq!(view.frames.len(), 2);
            assert_eq!(view.origins.len(), 2);
            assert_eq!(view.positions.len(), 2);
            assert_eq!(view.frames[0].owner, TraceOwnedOwnerRef::Primary);
            assert!(std::ptr::eq(view.frames[0].item, parent));
            assert!(std::ptr::eq(view.origins[0].item, parent));
            assert_eq!(view.positions[0].index, 1);
            assert_eq!(view.frames[0].position_slot, 0);
            assert!(std::ptr::eq(view.frames[1].item, child));
            assert!(std::ptr::eq(view.origins[1].item, child));
            assert_eq!(view.positions[1].index, 1);
            assert_eq!(view.frames[1].position_slot, 1);
            let TraceOwnedOwnerRef::Named {
                declaration_index,
                name,
            } = view.frames[1].owner
            else {
                panic!("child owner")
            };
            assert_eq!(declaration_index, 0);
            assert_eq!(name.as_ptr(), p.extra_sources[0].name.as_ptr());
        };
        let mut hook = FailureObservations::new();
        hook.check = Some(&check);
        assert_eq!(
            failure_scope(&p, &state, &registry, None, &hook),
            Err(EngineError::MappingException {
                node: 20,
                message: Some("named-child".into())
            })
        );
        assert_eq!(hook.outcomes.borrow().len(), 1);
    }

    #[test]
    fn failure_owned_selection_and_projection_snapshot_errors_remain_metadata() {
        let p = project(
            &[],
            vec![
                (0, Node::Raise { message: Some(1) }),
                (
                    1,
                    Node::Const {
                        value: Value::String("message".into()),
                    },
                ),
            ],
            Scope::default(),
        );
        let primary = row("primary");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let expected = EngineError::MappingException {
            node: 0,
            message: Some("message".into()),
        };
        for selected in [false, true] {
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let mut hook = FailureObservations::new();
            hook.selected.set(!selected);
            hook.select_at_value = Some((1, selected));
            let flag = Cell::new(false);
            assert_eq!(
                failure_expr(
                    &p,
                    &state,
                    Some(&registry),
                    None,
                    Some(&hook),
                    &flag,
                    TraceOwnedPhase::Output,
                    &[],
                    0
                )
                .as_ref(),
                Err(&expected)
            );
            assert_eq!(hook.pending.borrow().len(), 1);
            assert_eq!(hook.outcomes.borrow().len(), usize::from(selected));
            assert_eq!(registry.cache.get().is_some(), selected);
        }
        let mut foreign = constant();
        foreign.extra_sources = project(&["Unused"], Vec::new(), Scope::default()).extra_sources;
        let registry = TraceOwnedRegistry::new(&foreign, &roots);
        let hook = FailureObservations::new();
        let flag = Cell::new(false);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Output,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert_eq!(
            *hook.outcomes.borrow(),
            vec![Outcome::Projection(
                TraceOwnedContextError::MismatchedRegistry
            )]
        );
        assert_eq!(hook.pending.borrow().len(), 1);
        let state = OwnedContext {
            roots: &roots,
            frames: vec![SourceFrame {
                owner: OwnerKey::Primary,
                kind: crate::owned_context::OccurrenceKind::Item,
                collection: Vec::new(),
                item: &primary,
                position_slot: 7,
            }],
            origins: Vec::new(),
            owned_ancestry: true,
        };
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let hook = FailureObservations::new();
        let flag = Cell::new(false);
        assert_eq!(
            failure_expr(
                &p,
                &state,
                Some(&registry),
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Selection,
                &[],
                0
            )
            .as_ref(),
            Err(&expected)
        );
        assert_eq!(
            *hook.outcomes.borrow(),
            vec![Outcome::Snapshot(Err(
                TraceOwnedWitnessError::InvalidPositionSlot {
                    at: TraceOwnedLocation::Frame(0),
                    slot: 7
                }
            ))]
        );
        assert_eq!(hook.pending.borrow().len(), 1);
    }

    #[test]
    fn failure_owned_cancel_function_priority_lazy_branches_and_public_failures_stay_exact() {
        let p = project(
            &[],
            vec![
                (
                    0,
                    Node::If {
                        condition: 1,
                        then: 2,
                        else_: 3,
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Bool(true),
                    },
                ),
                (2, Node::Raise { message: None }),
                (
                    3,
                    Node::Const {
                        value: Value::Int(7),
                    },
                ),
            ],
            Scope::default(),
        );
        let primary = row("primary");
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let error = EngineError::MappingException {
            node: 2,
            message: None,
        };
        for cancel in [false, true] {
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let mut hook = FailureObservations::new();
            hook.cancel = cancel;
            let flag = Cell::new(false);
            let result = failure_expr(
                &p,
                &state,
                Some(&registry),
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Selection,
                &[],
                0,
            );
            assert_eq!(
                result.as_ref(),
                if cancel {
                    Err(&EngineError::DebugCancelled)
                } else {
                    Err(&error)
                }
            );
            assert_eq!(hook.pending.borrow().len(), 1);
            assert_eq!(hook.pending.borrow()[0].node, 2);
            assert_eq!(hook.outcomes.borrow().len(), 1);
        }
        for function_interest in [false, true] {
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let mut hook = FailureObservations::new();
            hook.function_interest = function_interest;
            let flag = Cell::new(false);
            assert_eq!(
                crate::debug::after_function_node_failure(
                    Some(&hook),
                    &flag,
                    mapping::FunctionId::new(9),
                    2,
                    &error,
                    &[]
                ),
                Ok(())
            );
            assert_eq!(
                failure_expr(
                    &p,
                    &state,
                    Some(&registry),
                    None,
                    Some(&hook),
                    &flag,
                    TraceOwnedPhase::Selection,
                    &[],
                    0
                )
                .as_ref(),
                Err(&error)
            );
            assert_eq!(hook.function_failures.get(), usize::from(function_interest));
            assert_eq!(hook.pending.borrow().len(), usize::from(!function_interest));
            assert_eq!(registry.cache.get().is_some(), !function_interest);
        }
        let registry = TraceOwnedRegistry::new(&p, &roots);
        let hook = FailureObservations::new();
        let flag = Cell::new(false);
        let program = EvalProgram::new(&p.graph, &p.user_functions, None, &flag)
            .with_owned_context(&state)
            .with_owned_trace_registry(&registry)
            .with_debug_hook(Some(&hook));
        assert_eq!(
            crate::debug::after_node_failure(
                program,
                2,
                &EngineError::DebugCancelled,
                &[],
                &[&primary],
                Some(&primary)
            ),
            Ok(())
        );
        assert!(!flag.get());
        assert!(hook.pending.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        let mut lazy = p.clone();
        lazy.graph.nodes.insert(
            1,
            Node::Const {
                value: Value::Bool(false),
            },
        );
        let registry = TraceOwnedRegistry::new(&lazy, &roots);
        let flag = Cell::new(false);
        assert_eq!(
            failure_expr(
                &lazy,
                &state,
                Some(&registry),
                None,
                Some(&hook),
                &flag,
                TraceOwnedPhase::Structural,
                &[],
                0
            ),
            Ok(Value::Int(7))
        );
        assert!(!flag.get());
        assert!(hook.pending.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        let mut owned = repeated(SourceOwner::Named { name: "Ref".into() });
        owned.graph.nodes.insert(
            10,
            Node::Const {
                value: Value::Bool(true),
            },
        );
        owned.failure_rules.push(mapping::FailureRule {
            iteration: mapping::FailureIteration::Source {
                collection: Vec::new(),
            },
            selection: mapping::FailureSelection::WhenTrue { predicate: 10 },
            message: None,
        });
        let roots = SourceRoots::new(&owned, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let registry = TraceOwnedRegistry::new(&owned, &roots);
        let hook = FailureObservations::new();
        assert_eq!(
            failure_scope(&owned, &state, &registry, None, &hook),
            Err(EngineError::OwnedSourceAccess {
                node: None,
                scope: Some(Box::new(TraceScope::primary().source_scope)),
                source: OwnedSourceError::SourceUnavailable {
                    owner: SourceOwner::Named { name: "Ref".into() }
                }
            })
        );
        assert!(hook.pending.borrow().is_empty());
        assert!(hook.outcomes.borrow().is_empty());
        assert!(registry.cache.get().is_none());
        let flag = Cell::new(false);
        let program = EvalProgram::new(&owned.graph, &owned.user_functions, None, &flag)
            .with_owned_context(&state)
            .with_owned_trace_registry(&registry)
            .with_debug_hook(Some(&hook));
        assert_eq!(
            crate::failure::evaluate(program, &owned.failure_rules, &[&extras, &primary]),
            Err(EngineError::MappingFailure {
                rule: 1,
                message: None
            })
        );
        assert!(hook.pending.borrow().is_empty());
        assert!(!flag.get());
        let sink = Collector::default();
        let execution = crate::ExecutionContext::new(std::path::Path::new("mapping.json"))
            .with_debug_hook(&hook);
        assert_eq!(
            crate::run_with_context(&owned, &primary, &execution),
            Err(EngineError::MappingFailure {
                rule: 1,
                message: None
            })
        );
        let execution = crate::ExecutionContext::new(std::path::Path::new("mapping.json"))
            .with_debug_hook(&hook)
            .with_trace_sink(&sink);
        assert_eq!(
            crate::run_with_context(&owned, &primary, &execution),
            Err(EngineError::MappingFailure {
                rule: 1,
                message: None
            })
        );
        assert!(
            sink.0
                .borrow()
                .iter()
                .all(|event| matches!(event, TraceEvent::NodeValue { node: 10, .. }))
        );
        assert_eq!(sink.0.borrow().len(), 1);
        assert!(hook.pending.borrow().is_empty());
    }

    #[test]
    fn failure_owned_late_large_source_retains_one_bounded_observation_without_success_history() {
        struct Counts {
            candidates: Cell<usize>,
            produced: Cell<usize>,
            values: Cell<usize>,
        }
        impl TraceSink for Counts {
            fn wants_source_row(&self) -> bool {
                false
            }
            fn record(&self, event: TraceEvent) {
                match event {
                    TraceEvent::IterationCandidate { .. } => {
                        self.candidates.set(self.candidates.get() + 1)
                    }
                    TraceEvent::TargetProduced { .. } => self.produced.set(self.produced.get() + 1),
                    TraceEvent::NodeValue { .. } => self.values.set(self.values.get() + 1),
                    _ => {}
                }
            }
        }
        let mut p = repeated(SourceOwner::Primary);
        p.graph.nodes.insert(
            0,
            Node::If {
                condition: 10,
                then: 11,
                else_: 12,
            },
        );
        p.graph
            .nodes
            .insert(10, owned_field(SourceOwner::Primary, &["Rows", "Keep"]));
        p.graph.nodes.insert(11, Node::Raise { message: Some(13) });
        p.graph
            .nodes
            .insert(12, owned_field(SourceOwner::Primary, &["Rows", "Value"]));
        p.graph
            .nodes
            .insert(13, owned_field(SourceOwner::Primary, &["Rows", "Value"]));
        let message = "late full typed message 雪 ".repeat(300);
        let primary = group(vec![(
            "Rows",
            Instance::Repeated(
                (0..4096)
                    .map(|index| {
                        group(vec![
                            ("Value", text(if index == 4095 { &message } else { "ok" })),
                            ("Keep", Instance::Scalar(Value::Bool(index == 4095))),
                        ])
                    })
                    .collect(),
            ),
        )]);
        let extras = group(Vec::new());
        let roots = SourceRoots::new(&p, &primary, &extras, OwnedLimits::default()).unwrap();
        let state = OwnedContext::new(&roots);
        let last = &primary.field("Rows").unwrap().as_repeated().unwrap()[4095];
        let expected = EngineError::MappingException {
            node: 11,
            message: Some(message.clone()),
        };
        let mut baseline = None;
        for selected in [false, true] {
            let registry = TraceOwnedRegistry::new(&p, &roots);
            let counts = Counts {
                candidates: Cell::new(0),
                produced: Cell::new(0),
                values: Cell::new(0),
            };
            let mut hook = FailureObservations::new();
            hook.selected.set(selected);
            let typed = |pending: &crate::PendingNodeFailure, error: &EngineError| {
                assert_eq!(pending.node, 11);
                assert_eq!(error, &expected);
                assert!(pending.error.truncated);
                assert_eq!(pending.error.preview.chars().count(), 512);
            };
            let check = |_: &crate::PendingNodeFailure, view: &TraceOwnedWitness<'_>| {
                assert_eq!(view.phase, TraceOwnedPhase::Output);
                assert_eq!(view.frames.len(), 1);
                assert_eq!(view.positions[0].index, 4096);
                assert!(std::ptr::eq(view.frames[0].item, last));
                assert!(std::ptr::eq(view.origins[0].item, last));
                assert!(std::ptr::eq(view.roots[0].input.unwrap(), &primary));
            };
            hook.typed = Some(&typed);
            hook.check = Some(&check);
            assert_eq!(
                failure_scope(&p, &state, &registry, Some(&counts), &hook).as_ref(),
                Err(&expected)
            );
            assert_eq!(hook.pending.borrow().len(), 1);
            assert_eq!(hook.pending.borrow()[0].node, 11);
            assert!(hook.pending.borrow()[0].error.truncated);
            assert_eq!(hook.pending.borrow()[0].error.preview.chars().count(), 512);
            assert_eq!(hook.outcomes.borrow().len(), usize::from(selected));
            assert_eq!(hook.root_addresses.borrow().len(), usize::from(selected));
            assert_eq!(counts.candidates.get(), 4096);
            assert_eq!(counts.produced.get(), 4095);
            let summary = (
                counts.candidates.get(),
                counts.produced.get(),
                counts.values.get(),
            );
            if let Some(old) = baseline {
                assert_eq!(summary, old);
            } else {
                baseline = Some(summary);
            }
            if selected {
                assert_eq!(
                    snapshot(&hook.outcomes.borrow()[0]).phase,
                    TraceOwnedPhase::Output
                );
            }
            assert_eq!(registry.cache.get().is_some(), selected);
        }
    }
}
