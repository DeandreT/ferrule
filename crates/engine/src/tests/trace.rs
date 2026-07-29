use std::cell::RefCell;
use std::error::Error;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, Graph, JoinConditions, JoinId, JoinKey, JoinPlan, JoinSource, Node, Project, Scope,
    ScopeIteration, SequenceExpr, SequenceWindow, SortFilterOrder,
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
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(
            ["first", "second"]
                .into_iter()
                .map(|value| {
                    Instance::Group(vec![(
                        "Value".into(),
                        Instance::Scalar(Value::String(value.into())),
                    )])
                })
                .collect(),
        ),
    )]);
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

    run_with_context(&project, &Instance::Group(Vec::new()), &execution)?;

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
        Instance::Group(vec![(name.into(), Instance::Scalar(Value::Int(value)))])
    };
    let source = Instance::Group(vec![
        (
            "Left".into(),
            Instance::Repeated(vec![row("Id", 1), row("Id", 2)]),
        ),
        (
            "Right".into(),
            Instance::Repeated(vec![row("LeftId", 2), row("LeftId", 1)]),
        ),
    ]);
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
