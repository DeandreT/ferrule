use std::cell::RefCell;
use std::path::Path;
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    AggregateOp, Binding, FailureIteration, FailureRule, FailureSelection, Graph, Node, Project,
    Scope, ScopeIteration, SequenceExpr, XmlMixedContentReplacement,
};

use crate::{
    DebugDecision, DebugHook, EngineError, ExecutionContext, PendingNodeFailure, PendingNodeInput,
    PendingNodeValue, PendingTargetWrite, TraceEvent, TraceSink, TraceTargetFieldBinding,
    run_with_context,
};

struct FailureHook {
    decision: DebugDecision,
    failures: RefCell<Vec<PendingNodeFailure>>,
    writes: RefCell<usize>,
}

impl DebugHook for FailureHook {
    fn before_target_write(&self, _write: &PendingTargetWrite) -> DebugDecision {
        *self.writes.borrow_mut() += 1;
        DebugDecision::Resume
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

    fn after_node_failure(&self, failure: &PendingNodeFailure) -> DebugDecision {
        self.failures.borrow_mut().push(failure.clone());
        self.decision
    }
}

#[test]
fn first_graph_failure_hook_precedes_filter_error_and_preserves_or_cancels_it() {
    let mut project = two_field_project();
    project.graph.nodes.extend([
        (
            2,
            Node::Call {
                function: "divide".into(),
                args: vec![3, 4],
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(1),
            },
        ),
        (
            4,
            Node::Const {
                value: Value::Int(0),
            },
        ),
    ]);
    project.root.filter = Some(2);
    let source = Instance::Group(vec![(
        "input".into(),
        Instance::Scalar(Value::String("source value".into())),
    )]);
    for decision in [DebugDecision::Resume, DebugDecision::Cancel] {
        let hook = FailureHook {
            decision,
            failures: RefCell::new(Vec::new()),
            writes: RefCell::new(0),
        };
        let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
        let error = run_with_context(&project, &source, &execution).unwrap_err();
        match decision {
            DebugDecision::Resume => {
                assert!(matches!(
                    error,
                    EngineError::Function(functions::FunctionError::DivideByZero)
                ));
            }
            DebugDecision::Cancel => assert!(matches!(error, EngineError::DebugCancelled)),
        }
        let failures = hook.failures.borrow();
        assert_eq!(failures.len(), 1, "only the deepest failure pauses");
        assert_eq!(failures[0].node, 2);
        assert!(failures[0].error.preview.contains("division by zero"));
        assert!(!failures[0].error.truncated);
        assert_eq!(*hook.writes.borrow(), 0);
    }
}

struct PausingHook {
    writes: SyncSender<PendingTargetWrite>,
    commands: Receiver<DebugDecision>,
}

impl DebugHook for PausingHook {
    fn before_target_write(&self, write: &PendingTargetWrite) -> DebugDecision {
        if self.writes.send(write.clone()).is_err() {
            return DebugDecision::Cancel;
        }
        self.commands.recv().unwrap_or(DebugDecision::Cancel)
    }
}

#[derive(Default)]
struct Collector(RefCell<Vec<TraceEvent>>);

impl TraceSink for Collector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn two_field_project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("input", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![
                SchemaNode::scalar("first", ScalarType::String),
                SchemaNode::scalar("second", ScalarType::String),
            ],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (
                    0,
                    Node::Const {
                        value: Value::String("A".into()),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("B".into()),
                    },
                ),
            ]
            .into(),
        },
        root: Scope {
            bindings: vec![
                Binding {
                    target_field: "first".into(),
                    node: 0,
                },
                Binding {
                    target_field: "second".into(),
                    node: 1,
                },
            ],
            ..Scope::default()
        },
    }
}

struct PausedRun {
    writes: Receiver<PendingTargetWrite>,
    commands: SyncSender<DebugDecision>,
    result: Receiver<(Result<Instance, EngineError>, Vec<TraceEvent>)>,
}

fn start_paused_run() -> PausedRun {
    let (write_tx, write_rx) = mpsc::sync_channel(0);
    let (command_tx, command_rx) = mpsc::sync_channel(0);
    let (result_tx, result_rx) = mpsc::sync_channel(0);
    std::thread::spawn(move || {
        let hook = PausingHook {
            writes: write_tx,
            commands: command_rx,
        };
        let collector = Collector::default();
        let execution = ExecutionContext::new(Path::new("mapping.json"))
            .with_trace_sink(&collector)
            .with_debug_hook(&hook);
        let result = run_with_context(
            &two_field_project(),
            &Instance::Group(vec![(
                "input".into(),
                Instance::Scalar(Value::String("source value".into())),
            )]),
            &execution,
        );
        let _ = result_tx.send((result, collector.0.into_inner()));
    });
    PausedRun {
        writes: write_rx,
        commands: command_tx,
        result: result_rx,
    }
}

#[test]
fn debug_hook_pauses_before_insertion_and_resumes_without_changing_trace() {
    let PausedRun {
        writes,
        commands,
        result,
    } = start_paused_run();
    let first = writes.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(first.field, "first");
    assert!(first.draft.fields.is_empty());
    assert_eq!(first.pending.value.as_ref().unwrap().preview, "A");
    let input = first
        .source
        .frames
        .iter()
        .flat_map(|frame| &frame.fields)
        .find(|field| field.name == "input")
        .expect("active source frame includes the input field");
    assert_eq!(
        input.preview.value.as_ref().unwrap().preview,
        "source value"
    );
    assert_eq!(
        first.binding,
        TraceTargetFieldBinding::StaticBinding { value: 0 }
    );
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));

    commands.send(DebugDecision::Resume).unwrap();
    let second = writes.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(second.field, "second");
    assert_eq!(second.draft.fields.len(), 1);
    assert_eq!(second.draft.fields[0].name, "first");
    assert_eq!(
        second.draft.fields[0]
            .preview
            .value
            .as_ref()
            .unwrap()
            .preview,
        "A"
    );
    assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));

    commands.send(DebugDecision::Resume).unwrap();
    let (output, events) = result.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(
        output.unwrap(),
        Instance::Group(vec![
            ("first".into(), Instance::Scalar(Value::String("A".into()))),
            ("second".into(), Instance::Scalar(Value::String("B".into()))),
        ])
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
            .count(),
        2
    );
}

#[test]
fn cancelling_a_pending_write_returns_no_target_and_records_no_write() {
    let PausedRun {
        writes,
        commands,
        result,
    } = start_paused_run();
    assert_eq!(
        writes.recv_timeout(Duration::from_secs(5)).unwrap().field,
        "first"
    );
    commands.send(DebugDecision::Resume).unwrap();
    assert_eq!(
        writes.recv_timeout(Duration::from_secs(5)).unwrap().field,
        "second"
    );
    commands.send(DebugDecision::Cancel).unwrap();
    let (output, events) = result.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(output, Err(EngineError::DebugCancelled)));
    let written = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::TargetFieldWritten { field, .. } => Some(field.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(written, ["first"]);
}

struct ExpressionHook {
    values: RefCell<Vec<PendingNodeValue>>,
    cancel_on_node: Option<mapping::NodeId>,
}

impl DebugHook for ExpressionHook {
    fn before_target_write(&self, _write: &PendingTargetWrite) -> DebugDecision {
        DebugDecision::Resume
    }

    fn after_node_value(&self, value: &PendingNodeValue) -> DebugDecision {
        self.values.borrow_mut().push(value.clone());
        if self.cancel_on_node == Some(value.node) {
            DebugDecision::Cancel
        } else {
            DebugDecision::Resume
        }
    }
}

#[test]
fn graph_node_callback_is_typed_and_cancelable_before_a_target_write() {
    let project = two_field_project();
    let source = Instance::Group(vec![(
        "input".into(),
        Instance::Scalar(Value::String("source value".into())),
    )]);
    let hook = ExpressionHook {
        values: RefCell::new(Vec::new()),
        cancel_on_node: Some(0),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_debug_hook(&hook)
        .with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let values = hook.values.into_inner();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].node, 0);
    assert_eq!(values[0].value.value_type, "string");
    assert_eq!(values[0].value.preview, "A");
    assert!(values[0].positions.is_empty());
    assert!(
        trace
            .0
            .borrow()
            .iter()
            .any(|event| matches!(event, TraceEvent::NodeValue { node: 0, .. }))
    );
    assert!(
        !trace
            .0
            .borrow()
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn graph_node_callback_runs_for_filter_that_produces_no_write() {
    let mut project = two_field_project();
    project.source =
        SchemaNode::group("Source", vec![SchemaNode::group("Row", vec![]).repeating()]);
    project.target =
        SchemaNode::group("Target", vec![SchemaNode::group("Row", vec![]).repeating()]);
    project.graph.nodes = [(
        0,
        Node::Const {
            value: Value::Bool(false),
        },
    )]
    .into();
    project.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Source(vec!["Row".into()]),
            filter: Some(0),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![Instance::Group(vec![])]),
    )]);
    let hook = ExpressionHook {
        values: RefCell::new(Vec::new()),
        cancel_on_node: Some(0),
    };
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let values = hook.values.into_inner();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].value.value_type, "bool");
    assert_eq!(values[0].value.preview, "false");
    assert_eq!(values[0].positions.last().unwrap().index, 1);
}

#[test]
fn graph_node_callback_cancels_during_pre_target_failure_rule() {
    let mut project = two_field_project();
    project.source =
        SchemaNode::group("Source", vec![SchemaNode::group("Row", vec![]).repeating()]);
    project.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    project.failure_rules.push(FailureRule {
        iteration: FailureIteration::Source {
            collection: vec!["Row".into()],
        },
        selection: FailureSelection::WhenTrue { predicate: 2 },
        message: None,
    });
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![Instance::Group(vec![])]),
    )]);
    let hook = ExpressionHook {
        values: RefCell::new(Vec::new()),
        cancel_on_node: Some(2),
    };
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let values = hook.values.into_inner();
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].node, 2);
    assert_eq!(values[0].positions.last().unwrap().index, 1);
}

#[test]
fn uninterested_debug_hook_skips_node_snapshots() {
    struct WriteOnlyHook;

    impl DebugHook for WriteOnlyHook {
        fn wants_node_values(&self) -> bool {
            false
        }

        fn before_target_write(&self, _write: &PendingTargetWrite) -> DebugDecision {
            DebugDecision::Resume
        }

        fn after_node_value(&self, _value: &PendingNodeValue) -> DebugDecision {
            panic!("write-only hooks do not receive node callbacks")
        }
    }

    let source = Instance::Group(vec![(
        "input".into(),
        Instance::Scalar(Value::String("source value".into())),
    )]);
    let hook = WriteOnlyHook;
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
    let output = run_with_context(&two_field_project(), &source, &execution).unwrap();
    assert!(output.field("first").is_some());
}

struct PinHook {
    inputs: RefCell<Vec<PendingNodeInput>>,
    cancel_at: Option<(mapping::NodeId, usize)>,
}

impl DebugHook for PinHook {
    fn wants_node_values(&self) -> bool {
        false
    }

    fn before_target_write(&self, _write: &PendingTargetWrite) -> DebugDecision {
        DebugDecision::Resume
    }

    fn after_node_value(&self, _value: &PendingNodeValue) -> DebugDecision {
        panic!("pin-only debugging must not pause on producer nodes")
    }

    fn after_node_input(&self, input: &PendingNodeInput) -> DebugDecision {
        self.inputs.borrow_mut().push(input.clone());
        if self.cancel_at == Some((input.consumer, input.input_index)) {
            DebugDecision::Cancel
        } else {
            DebugDecision::Resume
        }
    }
}

#[test]
fn pin_hook_distinguishes_equal_values_across_consumer_pins_and_cancels_before_write() {
    let mut project = two_field_project();
    project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::String("A".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("A".into()),
            },
        ),
        (
            2,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 1],
            },
        ),
        (
            3,
            Node::Call {
                function: "concat".into(),
                args: vec![0, 1],
            },
        ),
    ]
    .into();
    project.root.bindings[0].node = 2;
    project.root.bindings[1].node = 3;
    let source = Instance::Group(vec![(
        "input".into(),
        Instance::Scalar(Value::String("source".into())),
    )]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((2, 1)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_debug_hook(&hook)
        .with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let inputs = hook.inputs.into_inner();
    assert_eq!(
        inputs
            .iter()
            .map(|input| (
                input.consumer,
                input.input,
                input.input_index,
                input.value.preview.as_str()
            ))
            .collect::<Vec<_>>(),
        [(2, 0, 0, "A"), (2, 1, 1, "A")]
    );
    assert!(trace.0.borrow().iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 2,
            input_index: 1,
            ..
        }
    )));
    assert!(
        !trace
            .0
            .borrow()
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn pin_hook_skips_untaken_if_branch() {
    let mut project = two_field_project();
    project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("then".into()),
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
                then: 1,
                else_: 2,
            },
        ),
    ]
    .into();
    project.root.bindings.truncate(1);
    project.root.bindings[0].node = 3;
    let source = Instance::Group(vec![(
        "input".into(),
        Instance::Scalar(Value::String("source".into())),
    )]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((3, 2)),
    };
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::String("then".into()))
    );
    assert_eq!(
        hook.inputs
            .into_inner()
            .iter()
            .map(|input| (input.consumer, input.input_index))
            .collect::<Vec<_>>(),
        [(3, 0), (3, 1)]
    );
}

#[test]
fn pin_hook_cancels_filter_before_any_target_write() {
    let mut project = two_field_project();
    project.source =
        SchemaNode::group("Source", vec![SchemaNode::group("Row", vec![]).repeating()]);
    project.target =
        SchemaNode::group("Target", vec![SchemaNode::group("Row", vec![]).repeating()]);
    project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::Bool(true),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::Bool(false),
            },
        ),
        (
            2,
            Node::If {
                condition: 0,
                then: 1,
                else_: 1,
            },
        ),
    ]
    .into();
    project.root = Scope {
        children: vec![Scope {
            target_field: "Row".into(),
            iteration: ScopeIteration::Source(vec!["Row".into()]),
            filter: Some(2),
            ..Scope::default()
        }],
        ..Scope::default()
    };
    let source = Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(vec![Instance::Group(vec![])]),
    )]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((2, 1)),
    };
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let inputs = hook.inputs.into_inner();
    assert_eq!(
        inputs
            .iter()
            .map(|input| input.input_index)
            .collect::<Vec<_>>(),
        [0, 1]
    );
    assert_eq!(inputs[1].positions.last().unwrap().index, 1);
}

fn collection_find_project() -> Project {
    let mut project = two_field_project();
    project.source = SchemaNode::group(
        "Source",
        vec![
            SchemaNode::group(
                "Row",
                vec![
                    SchemaNode::scalar("Match", ScalarType::Bool),
                    SchemaNode::scalar("Value", ScalarType::String),
                ],
            )
            .repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("first", ScalarType::String)],
    );
    project.graph.nodes = [
        (
            0,
            Node::SourceField {
                frame: Some(vec!["Row".into()]),
                path: vec!["Match".into()],
            },
        ),
        (
            1,
            Node::SourceField {
                frame: Some(vec!["Row".into()]),
                path: vec!["Value".into()],
            },
        ),
        (
            2,
            Node::CollectionFind {
                collection: vec!["Row".into()],
                predicate: 0,
                value: 1,
            },
        ),
    ]
    .into();
    project.root.bindings.truncate(1);
    project.root.bindings[0].node = 2;
    project
}

fn collection_find_source(rows: &[(bool, &str)]) -> Instance {
    Instance::Group(vec![(
        "Row".into(),
        Instance::Repeated(
            rows.iter()
                .map(|(matches, value)| {
                    Instance::Group(vec![
                        ("Match".into(), Instance::Scalar(Value::Bool(*matches))),
                        (
                            "Value".into(),
                            Instance::Scalar(Value::String((*value).into())),
                        ),
                    ])
                })
                .collect(),
        ),
    )])
}

#[test]
fn collection_find_delivers_predicates_per_visited_row_and_only_the_selected_value() {
    let project = collection_find_project();
    let source = collection_find_source(&[(false, "first"), (true, "selected"), (true, "later")]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::String("selected".into()))
    );
    let events = trace.0.into_inner();
    let deliveries = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeInputValue {
                consumer,
                input,
                input_index,
                positions,
                value,
            } => Some((
                *consumer,
                *input,
                *input_index,
                positions.last().map(|position| position.index),
                value.preview.as_str(),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        deliveries,
        [
            (2, 0, 0, Some(1), "false"),
            (2, 0, 0, Some(2), "true"),
            (2, 1, 1, Some(2), "selected"),
        ]
    );
    let selected_delivery = events
        .iter()
        .position(|event| {
            matches!(
                event,
                TraceEvent::NodeInputValue {
                    consumer: 2,
                    input_index: 1,
                    ..
                }
            )
        })
        .unwrap();
    let target_write = events
        .iter()
        .position(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
        .unwrap();
    assert!(selected_delivery < target_write);

    let no_match = collection_find_source(&[(false, "first"), (false, "second")]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    run_with_context(&project, &no_match, &execution).unwrap();
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 2,
                    input_index,
                    ..
                } => Some(*input_index),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [0, 0],
        "no value pin is delivered when every predicate is false"
    );
}

#[test]
fn collection_find_pin_cancel_occurs_after_delivery_and_before_target_write() {
    let project = collection_find_project();
    let source = collection_find_source(&[(false, "first"), (true, "selected")]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((2, 1)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    assert_eq!(
        hook.inputs
            .into_inner()
            .iter()
            .map(|input| (input.consumer, input.input, input.input_index))
            .collect::<Vec<_>>(),
        [(2, 0, 0), (2, 0, 0), (2, 1, 1)]
    );
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 2,
            input_index: 1,
            ..
        })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn collection_find_non_boolean_predicate_still_returns_typed_error() {
    let mut project = collection_find_project();
    project.graph.nodes.insert(
        0,
        Node::Const {
            value: Value::String("wrong".into()),
        },
    );
    let source = collection_find_source(&[(true, "unreached")]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::NotABool { node: 0, .. })
    ));
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 2,
                    input_index,
                    ..
                } => Some(*input_index),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [0]
    );
}

fn xml_mixed_content_project() -> Project {
    let mut project = two_field_project();
    project.source = SchemaNode::group(
        "Description",
        vec![
            SchemaNode::scalar(ir::XML_TEXT_FIELD, ScalarType::String).text(),
            SchemaNode::scalar("Bold", ScalarType::String).repeating(),
            SchemaNode::scalar("Italic", ScalarType::String).repeating(),
            SchemaNode::scalar("Plain", ScalarType::String).repeating(),
        ],
    );
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("first", ScalarType::String)],
    );
    project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::String("X".into()),
            },
        ),
        (
            1,
            Node::Const {
                value: Value::String("X".into()),
            },
        ),
        (
            2,
            Node::XmlMixedContent {
                path: Vec::new(),
                frame: None,
                replacements: vec![
                    XmlMixedContentReplacement {
                        element: "Bold".into(),
                        collection: vec!["Bold".into()],
                        expression: 0,
                    },
                    XmlMixedContentReplacement {
                        element: "Italic".into(),
                        collection: vec!["Italic".into()],
                        expression: 1,
                    },
                ],
            },
        ),
    ]
    .into();
    project.root.bindings.truncate(1);
    project.root.bindings[0].node = 2;
    project
}

#[test]
fn xml_mixed_content_delivers_only_selected_replacement_pins_in_document_order() {
    let project = xml_mixed_content_project();
    let source = format_xml::from_str(
        "<Description>pre<Bold>first</Bold>-<Italic>second</Italic><Plain>raw</Plain>-<Bold>third</Bold>post</Description>",
        &project.source,
    )
    .unwrap();
    assert!(source.field(ir::XML_MIXED_CONTENT_FIELD).is_some());
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::String("preX-Xraw-Xpost".into()))
    );
    let events = trace.0.into_inner();
    let deliveries = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::NodeInputValue {
                consumer,
                input,
                input_index,
                positions,
                value,
            } => Some((
                *consumer,
                *input,
                *input_index,
                positions
                    .last()
                    .map(|position| position.collection.as_slice()),
                positions.last().map(|position| position.index),
                value.preview.as_str(),
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    let bold = ["Bold".to_string()];
    let italic = ["Italic".to_string()];
    assert_eq!(
        deliveries,
        [
            (2, 0, 0, Some(bold.as_slice()), Some(1), "X"),
            (2, 1, 1, Some(italic.as_slice()), Some(1), "X"),
            (2, 0, 0, Some(bold.as_slice()), Some(2), "X"),
        ]
    );

    let fallback = Instance::Group(vec![(
        ir::XML_TEXT_FIELD.into(),
        Instance::Scalar(Value::String("plain".into())),
    )]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &fallback, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::String("plain".into()))
    );
    assert!(
        !trace
            .0
            .into_inner()
            .iter()
            .any(|event| matches!(event, TraceEvent::NodeInputValue { consumer: 2, .. }))
    );
}

#[test]
fn xml_mixed_content_pin_cancel_stops_before_later_occurrences_and_target_write() {
    let project = xml_mixed_content_project();
    let source = format_xml::from_str(
        "<Description><Bold>first</Bold><Italic>second</Italic><Bold>third</Bold></Description>",
        &project.source,
    )
    .unwrap();
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((2, 1)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    assert_eq!(
        hook.inputs
            .into_inner()
            .iter()
            .map(|input| (input.consumer, input.input, input.input_index))
            .collect::<Vec<_>>(),
        [(2, 0, 0), (2, 1, 1)]
    );
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 2,
            input_index: 1,
            ..
        })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn xml_mixed_content_skips_unseen_replacements_and_does_not_deliver_failed_values() {
    let mut project = xml_mixed_content_project();
    project.graph.nodes.insert(
        1,
        Node::RuntimeParameter {
            name: "missing".into(),
            ty: ScalarType::String,
            preview: None,
        },
    );
    let only_bold = format_xml::from_str(
        "<Description><Bold>first</Bold></Description>",
        &project.source,
    )
    .unwrap();
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    run_with_context(&project, &only_bold, &execution).unwrap();
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 2,
                    input_index,
                    ..
                } => Some(*input_index),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [0]
    );

    let with_italic = format_xml::from_str(
        "<Description><Bold>first</Bold><Italic>second</Italic></Description>",
        &project.source,
    )
    .unwrap();
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &with_italic, &execution),
        Err(EngineError::MissingRuntimeParameter { node: 1, .. })
    ));
    let events = trace.0.into_inner();
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 2,
            input_index: 1,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

fn sequence_exists_pin_project(to: Value) -> Project {
    let mut project = two_field_project();
    project.source = SchemaNode::group("Source", vec![]);
    project.target = SchemaNode::group(
        "Target",
        vec![SchemaNode::scalar("first", ScalarType::Bool)],
    );
    project.graph.nodes = [
        (0, Node::Const { value: to }),
        (
            1,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            2,
            Node::Position {
                collection: Vec::new(),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            4,
            Node::Call {
                function: "equal".into(),
                args: vec![2, 3],
            },
        ),
        (
            5,
            Node::SequenceExists {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 0,
                    item: 1,
                },
                predicate: 4,
            },
        ),
    ]
    .into();
    project.root.bindings.truncate(1);
    project.root.bindings[0].node = 5;
    project
}

#[test]
fn sequence_exists_predicate_pin_uses_visible_index_and_stops_after_first_match() {
    let project = sequence_exists_pin_project(Value::Int(3));
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    let deliveries = trace
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::NodeInputValue {
                consumer: 5,
                input,
                input_index,
                positions,
                value,
            } => Some((
                input,
                input_index,
                positions.last().map(|position| position.index),
                value.preview,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        deliveries,
        [
            (0, 0, None, "3".into()),
            (4, 1, Some(1), "false".into()),
            (4, 1, Some(2), "true".into())
        ]
    );
}

#[test]
fn sequence_exists_predicate_pin_follows_two_tokenizer_inputs() {
    let mut project = sequence_exists_pin_project(Value::String("one,two".into()));
    project.graph.nodes.insert(
        4,
        Node::Call {
            function: "equal".into(),
            args: vec![1, 3],
        },
    );
    project.graph.nodes.insert(
        3,
        Node::Const {
            value: Value::String("two".into()),
        },
    );
    project.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::String(",".into()),
        },
    );
    project.graph.nodes.insert(
        5,
        Node::SequenceExists {
            sequence: SequenceExpr::Tokenize {
                input: 0,
                delimiter: 6,
                item: 1,
            },
            predicate: 4,
        },
    );
    let trace = Collector::default();
    let source = Instance::Group(vec![]);
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 5,
                    input_index,
                    ..
                } => Some(*input_index),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [0, 1, 2, 2]
    );
}

#[test]
fn sequence_exists_predicate_pin_is_absent_for_empty_or_null_sequence() {
    let source = Instance::Group(vec![]);
    for to in [Value::Int(0), Value::Null] {
        let project = sequence_exists_pin_project(to);
        let trace = Collector::default();
        let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
        let output = run_with_context(&project, &source, &execution).unwrap();
        assert_eq!(
            output.field("first").and_then(Instance::as_scalar),
            Some(&Value::Bool(false))
        );
        assert_eq!(
            trace
                .0
                .into_inner()
                .iter()
                .filter_map(|event| match event {
                    TraceEvent::NodeInputValue {
                        consumer: 5,
                        input_index,
                        ..
                    } => Some(*input_index),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [0],
            "the generator bound is delivered even when it makes no items"
        );
    }
}

#[test]
fn sequence_exists_predicate_pin_cancel_precedes_target_write() {
    let project = sequence_exists_pin_project(Value::Int(3));
    let source = Instance::Group(vec![]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((5, 1)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let predicate_deliveries = hook
        .inputs
        .into_inner()
        .into_iter()
        .filter(|input| input.consumer == 5 && input.input_index == 1)
        .collect::<Vec<_>>();
    assert_eq!(predicate_deliveries.len(), 1);
    assert_eq!(predicate_deliveries[0].input_index, 1);
    assert_eq!(predicate_deliveries[0].positions.last().unwrap().index, 1);
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 5,
            input_index: 1,
            ..
        })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn sequence_exists_non_boolean_predicate_keeps_typed_error_after_delivery() {
    let mut project = sequence_exists_pin_project(Value::Int(1));
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("wrong".into()),
        },
    );
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::NotABool { node: 4, .. })
    ));
    assert!(trace.0.into_inner().iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 5,
            input: 4,
            input_index: 1,
            value,
            ..
        } if value.preview == "wrong"
    )));
}

fn sequence_aggregate_expression_project(with_predicate: bool) -> Project {
    let mut project = two_field_project();
    project.source = SchemaNode::group("Source", vec![]);
    project.target =
        SchemaNode::group("Target", vec![SchemaNode::scalar("first", ScalarType::Int)]);
    project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::Int(4),
            },
        ),
        (
            1,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            2,
            Node::Position {
                collection: Vec::new(),
            },
        ),
        (
            3,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            4,
            Node::Call {
                function: "greater_than".into(),
                args: vec![2, 3],
            },
        ),
        (
            5,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            6,
            Node::Call {
                function: "multiply".into(),
                args: vec![1, 5],
            },
        ),
        (
            7,
            Node::SequenceAggregate {
                function: AggregateOp::Sum,
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 0,
                    item: 1,
                },
                predicate: with_predicate.then_some(4),
                expression: Some(6),
                arg: None,
            },
        ),
    ]
    .into();
    project.root.bindings.truncate(1);
    project.root.bindings[0].node = 7;
    project
}

#[test]
fn sequence_aggregate_expression_pin_skips_rejected_items_and_keeps_raw_positions() {
    let project = sequence_aggregate_expression_project(true);
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Int(21))
    );
    let deliveries = trace
        .0
        .into_inner()
        .into_iter()
        .filter_map(|event| match event {
            TraceEvent::NodeInputValue {
                consumer: 7,
                input,
                input_index,
                positions,
                value,
            } => Some((
                input,
                input_index,
                positions.last().map(|position| position.index),
                value.preview,
            )),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        deliveries,
        [
            (0, 0, None, "4".into()),
            (4, 1, Some(1), "false".into()),
            (4, 1, Some(2), "false".into()),
            (4, 1, Some(3), "true".into()),
            (6, 2, Some(3), "9".into()),
            (4, 1, Some(4), "true".into()),
            (6, 2, Some(4), "12".into()),
        ]
    );
}

#[test]
fn sequence_aggregate_expression_pin_shifts_when_predicate_is_absent() {
    let project = sequence_aggregate_expression_project(false);
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Int(30))
    );
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 7,
                    input_index,
                    positions,
                    ..
                } => Some((
                    *input_index,
                    positions.last().map(|position| position.index)
                )),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [
            (0, None),
            (1, Some(1)),
            (1, Some(2)),
            (1, Some(3)),
            (1, Some(4)),
        ]
    );
}

fn add_missing_sequence_aggregate_arg(project: &mut Project) {
    project.graph.nodes.insert(
        8,
        Node::RuntimeParameter {
            name: "missing-argument".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    );
    let Some(Node::SequenceAggregate { arg, .. }) = project.graph.nodes.get_mut(&7) else {
        panic!("sequence aggregate exists");
    };
    *arg = Some(8);
}

fn add_constant_sequence_aggregate_arg(project: &mut Project) {
    project.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Int(99),
        },
    );
    let Some(Node::SequenceAggregate { arg, .. }) = project.graph.nodes.get_mut(&7) else {
        panic!("sequence aggregate exists");
    };
    *arg = Some(8);
}

#[test]
fn sequence_aggregate_parent_arg_pin_follows_items_in_parent_context() {
    for (with_predicate, expected_arg_index) in [(true, 3), (false, 2)] {
        let mut project = sequence_aggregate_expression_project(with_predicate);
        add_constant_sequence_aggregate_arg(&mut project);
        let source = Instance::Group(vec![]);
        let trace = Collector::default();
        let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
        let output = run_with_context(&project, &source, &execution).unwrap();
        assert_eq!(
            output.field("first").and_then(Instance::as_scalar),
            Some(&Value::Int(if with_predicate { 21 } else { 30 }))
        );
        let deliveries = trace
            .0
            .into_inner()
            .into_iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 7,
                    input,
                    input_index,
                    positions,
                    value,
                } => Some((input, input_index, positions.len(), value.preview)),
                _ => None,
            })
            .collect::<Vec<_>>();
        let arg = deliveries.last().expect("parent argument is delivered");
        assert_eq!(arg, &(8, expected_arg_index, 0, "99".into()));
        assert_eq!(
            deliveries
                .iter()
                .filter(|(input, _, _, _)| *input == 8)
                .count(),
            1
        );
    }
}

#[test]
fn sequence_aggregate_parent_arg_pin_can_cancel_before_target_write() {
    let mut project = sequence_aggregate_expression_project(true);
    add_constant_sequence_aggregate_arg(&mut project);
    let source = Instance::Group(vec![]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((7, 3)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let deliveries = hook
        .inputs
        .into_inner()
        .into_iter()
        .filter(|input| input.consumer == 7)
        .collect::<Vec<_>>();
    let arg = deliveries.last().unwrap();
    assert_eq!((arg.input, arg.input_index), (8, 3));
    assert!(arg.positions.is_empty());
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 7,
            input: 8,
            input_index: 3,
            ..
        })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn sequence_aggregate_failed_parent_arg_has_no_delivery_or_target_write() {
    let mut project = sequence_aggregate_expression_project(true);
    add_missing_sequence_aggregate_arg(&mut project);
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::MissingRuntimeParameter { node: 8, .. })
    ));
    let events = trace.0.into_inner();
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 7,
            input_index: 2,
            ..
        }
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 7,
            input_index: 3,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn sequence_aggregate_predicate_pin_cancel_skips_expression_and_parent_arg() {
    let mut project = sequence_aggregate_expression_project(true);
    add_missing_sequence_aggregate_arg(&mut project);
    let source = Instance::Group(vec![]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((7, 1)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 7,
            input: 4,
            input_index: 1,
            ..
        })
    ));
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 7,
            input_index: 2 | 3,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn sequence_aggregate_non_boolean_predicate_retains_typed_error() {
    let mut project = sequence_aggregate_expression_project(true);
    add_missing_sequence_aggregate_arg(&mut project);
    project.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::String("wrong".into()),
        },
    );
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::NotABool { node: 4, .. })
    ));
    let events = trace.0.into_inner();
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 7,
            input: 4,
            input_index: 1,
            value,
            ..
        } if value.preview == "wrong"
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 7,
            input_index: 2 | 3,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn sequence_aggregate_expression_pin_cancel_skips_parent_arg_and_target_write() {
    let mut project = sequence_aggregate_expression_project(true);
    add_missing_sequence_aggregate_arg(&mut project);
    let source = Instance::Group(vec![]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((7, 2)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    let delivered = hook
        .inputs
        .into_inner()
        .into_iter()
        .filter(|input| input.consumer == 7 && input.input_index == 2)
        .collect::<Vec<_>>();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0].input_index, 2);
    assert_eq!(delivered[0].positions.last().unwrap().index, 3);
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 7,
            input_index: 2,
            ..
        })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::NodeValue { node: 8, .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn sequence_aggregate_failed_expression_delivers_no_pin_or_parent_arg() {
    let mut project = sequence_aggregate_expression_project(true);
    add_missing_sequence_aggregate_arg(&mut project);
    project.graph.nodes.insert(
        6,
        Node::RuntimeParameter {
            name: "missing-expression".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    );
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::MissingRuntimeParameter { node: 6, .. })
    ));
    let events = trace.0.into_inner();
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::NodeInputValue {
            consumer: 7,
            input_index: 2 | 3,
            ..
        }
    )));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::NodeValue { node: 8, .. }))
    );
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn generate_arguments_follow_optional_from_pin_order_and_null_short_circuit() {
    let mut project = sequence_exists_pin_project(Value::Int(3));
    project.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::Int(2),
        },
    );
    let Some(Node::SequenceExists { sequence, .. }) = project.graph.nodes.get_mut(&5) else {
        panic!("sequence exists node");
    };
    let SequenceExpr::Generate { from, .. } = sequence else {
        panic!("generated range");
    };
    *from = Some(6);
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Bool(true))
    );
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 5,
                    input,
                    input_index,
                    value,
                    ..
                } => Some((*input, *input_index, value.preview.as_str())),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(6, 0, "2"), (0, 1, "3"), (4, 2, "false"), (4, 2, "true")]
    );

    project
        .graph
        .nodes
        .insert(6, Node::Const { value: Value::Null });
    project.graph.nodes.insert(
        0,
        Node::RuntimeParameter {
            name: "unreached-to".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    );
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 5,
                    input_index,
                    value,
                    ..
                } => Some((*input_index, value.value_type)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(0, "null")]
    );
}

#[test]
fn generator_input_cancel_skips_later_argument_and_predicate() {
    let mut project = sequence_exists_pin_project(Value::Int(3));
    project.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::Int(1),
        },
    );
    project.graph.nodes.insert(
        0,
        Node::RuntimeParameter {
            name: "unreached-to".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    );
    let Some(Node::SequenceExists { sequence, .. }) = project.graph.nodes.get_mut(&5) else {
        panic!("sequence exists node");
    };
    let SequenceExpr::Generate { from, .. } = sequence else {
        panic!("generated range");
    };
    *from = Some(6);
    let source = Instance::Group(vec![]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((5, 0)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    assert_eq!(
        hook.inputs
            .into_inner()
            .iter()
            .filter_map(|input| (input.consumer == 5).then_some((input.input, input.input_index)))
            .collect::<Vec<_>>(),
        [(6, 0)]
    );
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 5,
            input: 6,
            input_index: 0,
            ..
        })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn regex_generator_optional_flags_delivery_can_short_circuit_predicate() {
    let mut project = sequence_exists_pin_project(Value::String("one,two".into()));
    project.graph.nodes.insert(
        6,
        Node::Const {
            value: Value::String(",".into()),
        },
    );
    project
        .graph
        .nodes
        .insert(7, Node::Const { value: Value::Null });
    project.graph.nodes.insert(
        5,
        Node::SequenceExists {
            sequence: SequenceExpr::TokenizeRegex {
                input: 0,
                pattern: 6,
                flags: Some(7),
                item: 1,
            },
            predicate: 4,
        },
    );
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Bool(false))
    );
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 5,
                    input,
                    input_index,
                    ..
                } => Some((*input, *input_index)),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(0, 0), (6, 1), (7, 2)]
    );
}

fn sequence_item_at_pin_project() -> Project {
    let mut project = two_field_project();
    project.source = SchemaNode::group("Source", vec![]);
    project.target =
        SchemaNode::group("Target", vec![SchemaNode::scalar("first", ScalarType::Int)]);
    project.graph.nodes = [
        (
            0,
            Node::Const {
                value: Value::Int(3),
            },
        ),
        (
            1,
            Node::SourceField {
                path: Vec::new(),
                frame: None,
            },
        ),
        (
            6,
            Node::Const {
                value: Value::Int(2),
            },
        ),
        (
            7,
            Node::SequenceItemAt {
                sequence: SequenceExpr::Generate {
                    from: None,
                    to: 0,
                    item: 1,
                },
                index: 6,
            },
        ),
    ]
    .into();
    project.root.bindings.truncate(1);
    project.root.bindings[0].node = 7;
    project
}

#[test]
fn sequence_item_at_generator_then_index_pins_use_parent_context() {
    let mut project = sequence_item_at_pin_project();
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 7,
                    input,
                    input_index,
                    positions,
                    ..
                } => Some((*input, *input_index, positions.len())),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(0, 0, 0), (6, 1, 0)]
    );

    project.graph.nodes.insert(
        8,
        Node::Const {
            value: Value::Int(2),
        },
    );
    let Some(Node::SequenceItemAt { sequence, .. }) = project.graph.nodes.get_mut(&7) else {
        panic!("sequence item-at node");
    };
    let SequenceExpr::Generate { from, .. } = sequence else {
        panic!("generated range");
    };
    *from = Some(8);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    let output = run_with_context(&project, &source, &execution).unwrap();
    assert_eq!(
        output.field("first").and_then(Instance::as_scalar),
        Some(&Value::Int(3))
    );
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 7,
                    input,
                    input_index,
                    positions,
                    ..
                } => Some((*input, *input_index, positions.len())),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [(8, 0, 0), (0, 1, 0), (6, 2, 0)]
    );
}

#[test]
fn sequence_item_at_index_can_cancel_after_generator_before_target_write() {
    let project = sequence_item_at_pin_project();
    let source = Instance::Group(vec![]);
    let hook = PinHook {
        inputs: RefCell::new(Vec::new()),
        cancel_at: Some((7, 1)),
    };
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&hook);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::DebugCancelled)
    ));
    assert_eq!(
        hook.inputs
            .into_inner()
            .iter()
            .filter_map(|input| (input.consumer == 7).then_some(input.input_index))
            .collect::<Vec<_>>(),
        [0, 1]
    );
    let events = trace.0.into_inner();
    assert!(matches!(
        events.last(),
        Some(TraceEvent::NodeInputValue {
            consumer: 7,
            input_index: 1,
            ..
        })
    ));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event, TraceEvent::TargetFieldWritten { .. }))
    );
}

#[test]
fn sequence_item_at_failed_index_has_no_delivery() {
    let mut project = sequence_item_at_pin_project();
    project.graph.nodes.insert(
        6,
        Node::RuntimeParameter {
            name: "missing-index".into(),
            ty: ScalarType::Int,
            preview: None,
        },
    );
    let source = Instance::Group(vec![]);
    let trace = Collector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&trace);
    assert!(matches!(
        run_with_context(&project, &source, &execution),
        Err(EngineError::MissingRuntimeParameter { node: 6, .. })
    ));
    assert_eq!(
        trace
            .0
            .into_inner()
            .iter()
            .filter_map(|event| match event {
                TraceEvent::NodeInputValue {
                    consumer: 7,
                    input_index,
                    ..
                } => Some(*input_index),
                _ => None,
            })
            .collect::<Vec<_>>(),
        [0]
    );
}
