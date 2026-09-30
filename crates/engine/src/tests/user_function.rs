use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;

use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FunctionId, FunctionParameter, FunctionParameterId, Graph, Node, Project,
    RuntimeValue, Scope, UserFunction,
};

use crate::{
    DebugDecision, DebugHook, EngineError, ExecutionContext, PendingFunctionNodeFailure,
    PendingFunctionNodeInput, PendingFunctionNodeValue, PendingNodeFailure, PendingNodeValue,
    PendingTargetWrite, TraceEvent, TraceSink, run, run_with_context, validate,
};

#[derive(Default)]
struct TraceCollector(RefCell<Vec<TraceEvent>>);

impl TraceSink for TraceCollector {
    fn record(&self, event: TraceEvent) {
        self.0.borrow_mut().push(event);
    }
}

fn parameter(id: u64, name: &str, ty: ScalarType) -> FunctionParameter {
    FunctionParameter {
        id: FunctionParameterId::new(id),
        name: name.into(),
        ty,
    }
}

fn function(
    name: &str,
    parameters: Vec<FunctionParameter>,
    output_type: ScalarType,
    nodes: impl IntoIterator<Item = (u32, Node)>,
    output: u32,
) -> UserFunction {
    UserFunction {
        library: "tests".into(),
        name: name.into(),
        description: None,
        parameters,
        output_name: "result".into(),
        output_type,
        body: Graph {
            nodes: nodes.into_iter().collect(),
        },
        output,
    }
}

fn project(
    graph: Graph,
    user_functions: BTreeMap<FunctionId, UserFunction>,
    output: u32,
) -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![SchemaNode::scalar("value", ScalarType::String)],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("result", ScalarType::String)],
        ),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: Vec::new(),
        failure_rules: Vec::new(),
        user_functions,
        graph,
        root: Scope {
            bindings: vec![Binding {
                target_field: "result".into(),
                node: output,
            }],
            ..Scope::default()
        },
    }
}

fn source(value: &str) -> Instance {
    Instance::Group(vec![(
        "value".into(),
        Instance::Scalar(Value::String(value.into())),
    )])
}

fn output_value(output: &Instance) -> Option<&Value> {
    output.field("result").and_then(Instance::as_scalar)
}

#[test]
fn unconnected_inputs_evaluate_to_null_in_main_and_function_graphs() {
    let direct = project(
        Graph {
            nodes: BTreeMap::from([(0, Node::Unconnected)]),
        },
        BTreeMap::new(),
        0,
    );
    let direct_output = run(&direct, &source("unused")).unwrap();
    assert_eq!(output_value(&direct_output), Some(&Value::Null));

    let function_id = FunctionId::new(1);
    let called = project(
        Graph {
            nodes: BTreeMap::from([(
                0,
                Node::UserFunctionCall {
                    function: function_id,
                    args: Vec::new(),
                },
            )]),
        },
        BTreeMap::from([(
            function_id,
            function(
                "empty",
                Vec::new(),
                ScalarType::String,
                [(0, Node::Unconnected)],
                0,
            ),
        )]),
        0,
    );
    let called_output = run(&called, &source("unused")).unwrap();
    assert_eq!(output_value(&called_output), Some(&Value::Null));
}

#[test]
fn evaluates_nested_functions_with_isolated_parameters_and_coercion() {
    let increment = FunctionId::new(2);
    let parse_and_increment = FunctionId::new(1);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        increment,
        function(
            "increment",
            vec![parameter(1, "number", ScalarType::Int)],
            ScalarType::Int,
            [
                (
                    0,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "add".into(),
                        args: vec![0, 1],
                    },
                ),
            ],
            2,
        ),
    );
    user_functions.insert(
        parse_and_increment,
        function(
            "parse_and_increment",
            vec![parameter(8, "text", ScalarType::Int)],
            ScalarType::String,
            [
                (
                    10,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(8),
                    },
                ),
                (
                    11,
                    Node::UserFunctionCall {
                        function: increment,
                        args: vec![10],
                    },
                ),
            ],
            11,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::SourceField {
                    path: vec!["value".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function: parse_and_increment,
                    args: vec![0],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let project = project(graph, user_functions, 1);

    assert!(validate(&project).is_empty());
    let output = run(&project, &source("41")).unwrap();
    assert_eq!(output_value(&output), Some(&Value::String("42".into())));
}

#[test]
fn function_body_trace_qualifies_overlapping_nodes_and_preserves_lazy_order() {
    let outer = FunctionId::new(1);
    let inner = FunctionId::new(2);
    let functions = BTreeMap::from([
        (
            inner,
            function(
                "inner",
                vec![parameter(2, "text", ScalarType::String)],
                ScalarType::String,
                [
                    (
                        0,
                        Node::FunctionParameter {
                            parameter: FunctionParameterId::new(2),
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
                ],
                2,
            ),
        ),
        (
            outer,
            function(
                "outer",
                vec![parameter(1, "text", ScalarType::String)],
                ScalarType::String,
                [
                    (
                        0,
                        Node::FunctionParameter {
                            parameter: FunctionParameterId::new(1),
                        },
                    ),
                    (
                        1,
                        Node::Const {
                            value: Value::String("untaken".into()),
                        },
                    ),
                    (
                        2,
                        Node::UserFunctionCall {
                            function: inner,
                            args: vec![0],
                        },
                    ),
                    (
                        3,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    (
                        4,
                        Node::If {
                            condition: 3,
                            then: 1,
                            else_: 2,
                        },
                    ),
                ],
                4,
            ),
        ),
    ]);
    let graph = Graph {
        nodes: BTreeMap::from([
            (
                0,
                Node::SourceField {
                    path: vec!["value".into()],
                    frame: None,
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function: outer,
                    args: vec![0],
                },
            ),
        ]),
    };
    let project = project(graph, functions, 1);
    let collector = TraceCollector::default();
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_trace_sink(&collector);

    let output = run_with_context(&project, &source("A"), &execution).unwrap();
    assert_eq!(output_value(&output), Some(&Value::String("A!".into())));
    let events = collector.0.into_inner();
    let main_input = events
        .iter()
        .position(|event| matches!(event, TraceEvent::NodeValue { node: 0, .. }))
        .unwrap();
    let first_function = events
        .iter()
        .position(|event| matches!(event, TraceEvent::FunctionNodeValue { .. }))
        .unwrap();
    let main_output = events
        .iter()
        .position(|event| matches!(event, TraceEvent::NodeValue { node: 1, .. }))
        .unwrap();
    assert!(main_input < first_function && first_function < main_output);
    let function_events = events
        .iter()
        .filter_map(|event| match event {
            TraceEvent::FunctionNodeValue { function, node, .. } => Some((*function, *node, None)),
            TraceEvent::FunctionNodeInputValue {
                function,
                consumer,
                input_index,
                ..
            } => Some((*function, *consumer, Some(*input_index))),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(
        function_events,
        [
            (outer, 3, None),
            (outer, 4, Some(0)),
            (outer, 0, None),
            (outer, 2, Some(0)),
            (inner, 0, None),
            (inner, 2, Some(0)),
            (inner, 1, None),
            (inner, 2, Some(1)),
            (inner, 2, None),
            (outer, 2, None),
            (outer, 4, Some(2)),
            (outer, 4, None),
        ]
    );
    assert!(!events.iter().any(|event| matches!(
        event,
        TraceEvent::FunctionNodeValue { function, node: 1, .. } if *function == outer
    )));
    assert!(
        events
            .iter()
            .any(|event| matches!(event, TraceEvent::NodeValue { node: 0, .. }))
    );
    assert!(events.iter().any(|event| matches!(
        event,
        TraceEvent::FunctionNodeValue { function, node: 0, .. } if *function == outer
    )));
}

#[derive(Default)]
struct FunctionBreakpointHook {
    opt_in: bool,
    cancel_inner: bool,
    function_values: RefCell<Vec<(FunctionId, u32, String)>>,
    main_nodes: RefCell<Vec<u32>>,
    writes: RefCell<usize>,
}

impl DebugHook for FunctionBreakpointHook {
    fn before_target_write(&self, _write: &PendingTargetWrite) -> DebugDecision {
        *self.writes.borrow_mut() += 1;
        DebugDecision::Resume
    }

    fn wants_function_node_values(&self) -> bool {
        self.opt_in
    }

    fn after_node_value(&self, node: &PendingNodeValue) -> DebugDecision {
        self.main_nodes.borrow_mut().push(node.node);
        DebugDecision::Resume
    }

    fn after_function_node_value(&self, node: &PendingFunctionNodeValue) -> DebugDecision {
        assert!(
            node.source.frames.is_empty(),
            "function bodies have no source frames"
        );
        self.function_values.borrow_mut().push((
            node.function,
            node.node,
            format!("{}:{}", node.value.value_type, node.value.preview),
        ));
        if self.cancel_inner && node.function == FunctionId::new(2) && node.node == 0 {
            DebugDecision::Cancel
        } else {
            DebugDecision::Resume
        }
    }
}

#[test]
fn function_node_breakpoint_is_opt_in_qualified_and_cancels_before_target_write() {
    let outer = FunctionId::new(1);
    let inner = FunctionId::new(2);
    let functions = BTreeMap::from([
        (
            inner,
            function(
                "inner",
                Vec::new(),
                ScalarType::String,
                [(
                    0,
                    Node::Const {
                        value: Value::String("chosen".into()),
                    },
                )],
                0,
            ),
        ),
        (
            outer,
            function(
                "outer",
                Vec::new(),
                ScalarType::String,
                [
                    (
                        0,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    (
                        1,
                        Node::Const {
                            value: Value::String("untaken".into()),
                        },
                    ),
                    (
                        2,
                        Node::UserFunctionCall {
                            function: inner,
                            args: Vec::new(),
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
                ],
                3,
            ),
        ),
    ]);
    let project = project(
        Graph {
            nodes: BTreeMap::from([(
                0,
                Node::UserFunctionCall {
                    function: outer,
                    args: Vec::new(),
                },
            )]),
        },
        functions,
        0,
    );
    let ordinary_hook = FunctionBreakpointHook::default();
    let execution =
        ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&ordinary_hook);
    let output = run_with_context(&project, &source("A"), &execution).unwrap();
    assert_eq!(output_value(&output), Some(&Value::String("chosen".into())));
    assert!(ordinary_hook.function_values.borrow().is_empty());
    assert_eq!(*ordinary_hook.main_nodes.borrow(), [0]);
    assert_eq!(*ordinary_hook.writes.borrow(), 1);

    let breakpoint_hook = FunctionBreakpointHook {
        opt_in: true,
        cancel_inner: true,
        ..Default::default()
    };
    let execution =
        ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&breakpoint_hook);
    assert!(matches!(
        run_with_context(&project, &source("A"), &execution),
        Err(EngineError::DebugCancelled)
    ));
    assert_eq!(
        *breakpoint_hook.function_values.borrow(),
        [
            (outer, 0, "bool:false".into()),
            (inner, 0, "string:chosen".into()),
        ]
    );
    assert!(breakpoint_hook.main_nodes.borrow().is_empty());
    assert_eq!(*breakpoint_hook.writes.borrow(), 0);
}

struct FunctionInputHook<'a> {
    trace: &'a TraceCollector,
    opt_in: bool,
    cancel_inner: bool,
    delivered: RefCell<Vec<(FunctionId, u32, usize, String)>>,
    writes: RefCell<usize>,
}

impl DebugHook for FunctionInputHook<'_> {
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

    fn wants_function_node_inputs(&self) -> bool {
        self.opt_in
    }

    fn after_function_node_input(&self, input: &PendingFunctionNodeInput) -> DebugDecision {
        assert!(input.source.frames.is_empty());
        assert!(matches!(
            self.trace.0.borrow().last(),
            Some(TraceEvent::FunctionNodeInputValue { function, consumer, input_index, .. })
                if (*function, *consumer, *input_index)
                    == (input.function, input.consumer, input.input_index)
        ));
        self.delivered.borrow_mut().push((
            input.function,
            input.consumer,
            input.input_index,
            format!("{}:{}", input.value.value_type, input.value.preview),
        ));
        if self.cancel_inner
            && input.function == FunctionId::new(2)
            && input.consumer == 2
            && input.input_index == 1
        {
            DebugDecision::Cancel
        } else {
            DebugDecision::Resume
        }
    }
}

#[test]
fn function_input_breakpoint_is_qualified_lazy_and_cancels_before_consumer_and_write() {
    let outer = FunctionId::new(1);
    let inner = FunctionId::new(2);
    let functions = BTreeMap::from([
        (
            inner,
            function(
                "inner",
                vec![parameter(1, "value", ScalarType::String)],
                ScalarType::String,
                [
                    (
                        0,
                        Node::FunctionParameter {
                            parameter: FunctionParameterId::new(1),
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
                ],
                2,
            ),
        ),
        (
            outer,
            function(
                "outer",
                Vec::new(),
                ScalarType::String,
                [
                    (
                        0,
                        Node::Const {
                            value: Value::Bool(false),
                        },
                    ),
                    (
                        1,
                        Node::Const {
                            value: Value::String("untaken".into()),
                        },
                    ),
                    (
                        2,
                        Node::UserFunctionCall {
                            function: inner,
                            args: vec![4],
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
                    (
                        4,
                        Node::Const {
                            value: Value::String("chosen".into()),
                        },
                    ),
                ],
                3,
            ),
        ),
    ]);
    let project = project(
        Graph {
            nodes: BTreeMap::from([(
                0,
                Node::UserFunctionCall {
                    function: outer,
                    args: Vec::new(),
                },
            )]),
        },
        functions,
        0,
    );

    let trace = TraceCollector::default();
    let ordinary = FunctionInputHook {
        trace: &trace,
        opt_in: false,
        cancel_inner: false,
        delivered: RefCell::new(Vec::new()),
        writes: RefCell::new(0),
    };
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&ordinary);
    let output = run_with_context(&project, &source("unused"), &execution).unwrap();
    assert_eq!(
        output_value(&output),
        Some(&Value::String("chosen!".into()))
    );
    assert!(ordinary.delivered.borrow().is_empty());
    assert_eq!(*ordinary.writes.borrow(), 1);
    assert!(!trace.0.borrow().iter().any(|event| matches!(
        event,
        TraceEvent::FunctionNodeInputValue { function, consumer: 3, input_index: 1, .. }
            if *function == outer
    )));

    let trace = TraceCollector::default();
    let breakpoint = FunctionInputHook {
        trace: &trace,
        opt_in: true,
        cancel_inner: true,
        delivered: RefCell::new(Vec::new()),
        writes: RefCell::new(0),
    };
    let execution = ExecutionContext::new(Path::new("mapping.json"))
        .with_trace_sink(&trace)
        .with_debug_hook(&breakpoint);
    assert!(matches!(
        run_with_context(&project, &source("unused"), &execution),
        Err(EngineError::DebugCancelled)
    ));
    assert_eq!(
        *breakpoint.delivered.borrow(),
        [
            (outer, 3, 0, "bool:false".into()),
            (outer, 2, 0, "string:chosen".into()),
            (inner, 2, 0, "string:chosen".into()),
            (inner, 2, 1, "string:!".into()),
        ]
    );
    assert_eq!(*breakpoint.writes.borrow(), 0);
    assert!(!trace.0.borrow().iter().any(|event| matches!(
        event,
        TraceEvent::FunctionNodeValue { function, node: 2, .. } if *function == inner
    )));
}

struct FunctionFailureHook {
    decision: DebugDecision,
    function_failures: RefCell<Vec<PendingFunctionNodeFailure>>,
    graph_failures: RefCell<Vec<PendingNodeFailure>>,
    writes: RefCell<usize>,
}

impl DebugHook for FunctionFailureHook {
    fn before_target_write(&self, _write: &PendingTargetWrite) -> DebugDecision {
        *self.writes.borrow_mut() += 1;
        DebugDecision::Resume
    }

    fn wants_node_failures(&self) -> bool {
        true
    }

    fn wants_function_node_failures(&self) -> bool {
        true
    }

    fn after_node_failure(&self, failure: &PendingNodeFailure) -> DebugDecision {
        self.graph_failures.borrow_mut().push(failure.clone());
        self.decision
    }

    fn after_function_node_failure(&self, failure: &PendingFunctionNodeFailure) -> DebugDecision {
        self.function_failures.borrow_mut().push(failure.clone());
        self.decision
    }
}

#[test]
fn first_function_failure_is_qualified_and_does_not_repause_as_graph_error() {
    let function_id = FunctionId::new(7);
    let mut failing = function(
        "failing",
        Vec::new(),
        ScalarType::String,
        [
            (
                0,
                Node::Const {
                    value: Value::Int(1),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::Int(0),
                },
            ),
            (
                2,
                Node::Call {
                    function: "divide".into(),
                    args: vec![0, 1],
                },
            ),
        ],
        2,
    );
    let failing_project = project(
        Graph {
            nodes: BTreeMap::from([(
                2,
                Node::UserFunctionCall {
                    function: function_id,
                    args: Vec::new(),
                },
            )]),
        },
        BTreeMap::from([(function_id, failing.clone())]),
        2,
    );
    for decision in [DebugDecision::Resume, DebugDecision::Cancel] {
        let hook = FunctionFailureHook {
            decision,
            function_failures: RefCell::new(Vec::new()),
            graph_failures: RefCell::new(Vec::new()),
            writes: RefCell::new(0),
        };
        let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
        let error = run_with_context(&failing_project, &source("unused"), &execution).unwrap_err();
        match decision {
            DebugDecision::Resume => assert!(matches!(
                error,
                EngineError::UserFunctionBuiltin {
                    function,
                    node: 2,
                    source: functions::FunctionError::DivideByZero,
                } if function == function_id
            )),
            DebugDecision::Cancel => assert!(matches!(error, EngineError::DebugCancelled)),
        }
        let failures = hook.function_failures.borrow();
        assert_eq!(failures.len(), 1);
        assert_eq!((failures[0].function, failures[0].node), (function_id, 2));
        assert!(failures[0].source.frames.is_empty());
        assert!(hook.graph_failures.borrow().is_empty());
        assert_eq!(*hook.writes.borrow(), 0);
    }

    failing.body.nodes.insert(
        3,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    failing.body.nodes.insert(
        4,
        Node::Const {
            value: Value::String("safe".into()),
        },
    );
    failing.body.nodes.insert(
        5,
        Node::If {
            condition: 3,
            then: 2,
            else_: 4,
        },
    );
    failing.output = 5;
    let lazy = project(
        Graph {
            nodes: BTreeMap::from([(
                2,
                Node::UserFunctionCall {
                    function: function_id,
                    args: Vec::new(),
                },
            )]),
        },
        BTreeMap::from([(function_id, failing)]),
        2,
    );
    let hook = FunctionFailureHook {
        decision: DebugDecision::Resume,
        function_failures: RefCell::new(Vec::new()),
        graph_failures: RefCell::new(Vec::new()),
        writes: RefCell::new(0),
    };
    let execution = ExecutionContext::new(Path::new("mapping.json")).with_debug_hook(&hook);
    let output = run_with_context(&lazy, &source("unused"), &execution).unwrap();
    assert_eq!(output_value(&output), Some(&Value::String("safe".into())));
    assert!(hook.function_failures.borrow().is_empty());
    assert!(hook.graph_failures.borrow().is_empty());
}

#[test]
fn user_functions_can_read_the_stable_runtime_clock() -> Result<(), EngineError> {
    let function_id = FunctionId::new(1);
    let user_functions = BTreeMap::from([(
        function_id,
        function(
            "timestamp",
            Vec::new(),
            ScalarType::String,
            [(
                0,
                Node::RuntimeValue {
                    value: RuntimeValue::CurrentDateTime,
                },
            )],
            0,
        ),
    )]);
    let graph = Graph {
        nodes: [(
            0,
            Node::UserFunctionCall {
                function: function_id,
                args: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    };
    let project = project(graph, user_functions, 0);
    let execution = ExecutionContext::new(Path::new("/maps/test.json"))
        .with_current_datetime("2026-07-24T12:34:56-07:00");

    assert!(validate(&project).is_empty());
    let output = run_with_context(&project, &source("unused"), &execution)?;
    assert_eq!(
        output_value(&output),
        Some(&Value::String("2026-07-24T12:34:56-07:00".into()))
    );
    Ok(())
}

#[test]
fn user_functions_can_read_typed_host_runtime_parameters() -> Result<(), Box<dyn std::error::Error>>
{
    let function_id = FunctionId::new(1);
    let user_functions = BTreeMap::from([(
        function_id,
        function(
            "control_number",
            Vec::new(),
            ScalarType::Int,
            [(
                7,
                Node::RuntimeParameter {
                    name: "control_number".into(),
                    ty: ScalarType::Int,
                    preview: Some("17".into()),
                },
            )],
            7,
        ),
    )]);
    let graph = Graph {
        nodes: [(
            0,
            Node::UserFunctionCall {
                function: function_id,
                args: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    };
    let project = project(graph, user_functions, 0);
    assert!(validate(&project).is_empty());

    let mut parameters = crate::RuntimeParameters::new();
    parameters.insert("control_number", Value::String("42".into()))?;
    let execution =
        ExecutionContext::new(Path::new("/maps/test.json")).with_parameters(&parameters);
    let output = run_with_context(&project, &source("unused"), &execution)?;
    assert_eq!(output_value(&output), Some(&Value::Int(42)));
    let preview = execution.with_purpose(crate::ExecutionPurpose::Preview);
    assert_eq!(
        output_value(&run_with_context(&project, &source("unused"), &preview)?),
        Some(&Value::Int(42))
    );
    let preview = ExecutionContext::new(Path::new("/maps/test.json"))
        .with_purpose(crate::ExecutionPurpose::Preview);
    assert_eq!(
        output_value(&run_with_context(&project, &source("unused"), &preview)?),
        Some(&Value::Int(17))
    );

    let empty = crate::RuntimeParameters::new();
    let execution = ExecutionContext::new(Path::new("/maps/test.json")).with_parameters(&empty);
    assert_eq!(
        run_with_context(&project, &source("unused"), &execution),
        Err(EngineError::MissingRuntimeParameter {
            node: 7,
            name: "control_number".into(),
        })
    );

    let mut wrong = crate::RuntimeParameters::new();
    wrong.insert("control_number", Value::Bool(false))?;
    let execution = ExecutionContext::new(Path::new("/maps/test.json")).with_parameters(&wrong);
    assert_eq!(
        run_with_context(&project, &source("unused"), &execution),
        Err(EngineError::RuntimeParameterType {
            node: 7,
            name: "control_number".into(),
            expected: ScalarType::Int,
            found: "bool",
        })
    );
    Ok(())
}

#[test]
fn validates_runtime_parameter_names_inside_user_functions() {
    let function_id = FunctionId::new(1);
    let project = project(
        Graph {
            nodes: [(
                0,
                Node::UserFunctionCall {
                    function: function_id,
                    args: Vec::new(),
                },
            )]
            .into_iter()
            .collect(),
        },
        BTreeMap::from([(
            function_id,
            function(
                "invalid_parameter",
                Vec::new(),
                ScalarType::String,
                [
                    (
                        0,
                        Node::RuntimeParameter {
                            name: String::new(),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                    (
                        1,
                        Node::RuntimeParameter {
                            name: "bad\0name".into(),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                    (
                        2,
                        Node::RuntimeParameter {
                            name: "x".repeat(mapping::MAX_RUNTIME_PARAMETER_NAME_BYTES + 1),
                            ty: ScalarType::String,
                            preview: None,
                        },
                    ),
                    (
                        3,
                        Node::Call {
                            function: "concat".into(),
                            args: vec![0, 1, 2],
                        },
                    ),
                ],
                3,
            ),
        )]),
        0,
    );

    let messages = validate(&project)
        .into_iter()
        .map(|issue| issue.to_string())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(messages.contains("body node 0: runtime parameter name cannot be empty"));
    assert!(messages.contains("body node 1: runtime parameter name cannot contain NUL"));
    assert!(messages.contains("body node 2: runtime parameter name exceeds 256 UTF-8 bytes"));
}

#[test]
fn evaluates_only_the_selected_function_branch() {
    let choose = FunctionId::new(1);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        choose,
        function(
            "choose",
            vec![parameter(1, "condition", ScalarType::Bool)],
            ScalarType::String,
            [
                (
                    0,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(1),
                    },
                ),
                (
                    1,
                    Node::Const {
                        value: Value::String("selected".into()),
                    },
                ),
                (
                    2,
                    Node::UserFunctionCall {
                        function: FunctionId::new(999),
                        args: Vec::new(),
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
            ],
            3,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::Const {
                    value: Value::Bool(true),
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function: choose,
                    args: vec![0],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    let output = run(&project(graph, user_functions, 1), &source("unused")).unwrap();
    assert_eq!(
        output_value(&output),
        Some(&Value::String("selected".into()))
    );
}

#[test]
fn reports_the_first_parameter_that_cannot_be_adapted() {
    let function_id = FunctionId::new(1);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        function_id,
        function(
            "typed",
            vec![
                parameter(11, "first", ScalarType::Int),
                parameter(12, "second", ScalarType::Int),
            ],
            ScalarType::Int,
            [(
                0,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(11),
                },
            )],
            0,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::Const {
                    value: Value::String("not-an-int".into()),
                },
            ),
            (
                1,
                Node::Const {
                    value: Value::String("also-not-an-int".into()),
                },
            ),
            (
                2,
                Node::UserFunctionCall {
                    function: function_id,
                    args: vec![0, 1],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    assert_eq!(
        run(&project(graph, user_functions, 2), &source("unused")),
        Err(EngineError::UserFunctionParameterType {
            function: function_id,
            parameter: FunctionParameterId::new(11),
            expected: ScalarType::Int,
            found: "string",
        })
    );
}

#[test]
fn guards_recursive_calls_even_without_prevalidation() {
    let recursive = FunctionId::new(7);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        recursive,
        function(
            "recursive",
            Vec::new(),
            ScalarType::String,
            [(
                0,
                Node::UserFunctionCall {
                    function: recursive,
                    args: Vec::new(),
                },
            )],
            0,
        ),
    );
    let graph = Graph {
        nodes: [(
            0,
            Node::UserFunctionCall {
                function: recursive,
                args: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    };

    assert_eq!(
        run(&project(graph, user_functions, 0), &source("unused")),
        Err(EngineError::UserFunctionCycle {
            function: recursive
        })
    );
}

#[test]
fn validates_function_boundaries_bodies_and_call_cycles() {
    let first = FunctionId::new(1);
    let second = FunctionId::new(2);
    let duplicated = parameter(3, "value", ScalarType::String);
    let mut user_functions = BTreeMap::new();
    user_functions.insert(
        first,
        function(
            "first",
            vec![duplicated.clone(), duplicated],
            ScalarType::String,
            [
                (
                    0,
                    Node::SourceField {
                        path: vec!["value".into()],
                        frame: None,
                    },
                ),
                (
                    1,
                    Node::UserFunctionCall {
                        function: second,
                        args: Vec::new(),
                    },
                ),
                (
                    2,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![2],
                    },
                ),
                (
                    3,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![77],
                    },
                ),
                (
                    4,
                    Node::FunctionParameter {
                        parameter: FunctionParameterId::new(999),
                    },
                ),
                (
                    5,
                    Node::Call {
                        function: "not-a-function".into(),
                        args: Vec::new(),
                    },
                ),
                (
                    6,
                    Node::Call {
                        function: "not".into(),
                        args: Vec::new(),
                    },
                ),
            ],
            99,
        ),
    );
    user_functions.insert(
        second,
        function(
            "second",
            Vec::new(),
            ScalarType::String,
            [(
                0,
                Node::UserFunctionCall {
                    function: first,
                    args: Vec::new(),
                },
            )],
            0,
        ),
    );
    user_functions.insert(
        FunctionId::new(3),
        function(
            "first",
            Vec::new(),
            ScalarType::String,
            [(
                0,
                Node::Const {
                    value: Value::String("duplicate name".into()),
                },
            )],
            0,
        ),
    );
    let graph = Graph {
        nodes: [
            (
                0,
                Node::FunctionParameter {
                    parameter: FunctionParameterId::new(3),
                },
            ),
            (
                1,
                Node::UserFunctionCall {
                    function: first,
                    args: Vec::new(),
                },
            ),
        ]
        .into_iter()
        .collect(),
    };

    let issues = validate(&project(graph, user_functions, 1));
    let messages = issues
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(messages.contains("valid only inside a user-defined function"));
    assert!(messages.contains("expects 2 argument(s), got 0"));
    assert!(messages.contains("parameter id 3 is duplicated"));
    assert!(messages.contains("parameter name `value` is duplicated"));
    assert!(messages.contains("function library and name duplicate function 1"));
    assert!(messages.contains("output references missing body node 99"));
    assert!(messages.contains("references missing body node 77"));
    assert!(messages.contains("references undeclared parameter 999"));
    assert!(messages.contains("unknown function `not-a-function`"));
    assert!(messages.contains("function `not` expects exactly 1 argument(s), got 0"));
    assert!(messages.contains("cycle reaches body node 2"));
    assert!(messages.contains("node kind is not supported"));
    assert!(messages.contains("recursive user-defined function calls are not supported"));
}

#[test]
fn validates_the_function_call_depth_limit() {
    let mut user_functions = BTreeMap::new();
    for index in 0..=64_u64 {
        let id = FunctionId::new(index);
        let node = if index == 64 {
            Node::Const {
                value: Value::String("end".into()),
            }
        } else {
            Node::UserFunctionCall {
                function: FunctionId::new(index + 1),
                args: Vec::new(),
            }
        };
        user_functions.insert(
            id,
            function(
                &format!("function_{index}"),
                Vec::new(),
                ScalarType::String,
                [(0, node)],
                0,
            ),
        );
    }
    let graph = Graph {
        nodes: [(
            0,
            Node::UserFunctionCall {
                function: FunctionId::new(0),
                args: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    };

    assert!(
        validate(&project(graph, user_functions, 0))
            .iter()
            .any(|issue| {
                issue
                    .message
                    .contains("call nesting exceeds the limit of 64 functions")
            })
    );
}

#[test]
fn nested_user_function_reads_optional_host_input_lazily() -> Result<(), Box<dyn std::error::Error>>
{
    let inner = FunctionId::new(1);
    let outer = FunctionId::new(2);
    let user_functions = BTreeMap::from([
        (
            inner,
            function(
                "optional_input",
                Vec::new(),
                ScalarType::String,
                [
                    (
                        1,
                        Node::Const {
                            value: Value::String("fallback".into()),
                        },
                    ),
                    (
                        2,
                        Node::RuntimeParameterDefault {
                            name: "override".into(),
                            ty: ScalarType::String,
                            default: 1,
                            preview: Some("nested-preview".into()),
                        },
                    ),
                ],
                2,
            ),
        ),
        (
            outer,
            function(
                "forward",
                Vec::new(),
                ScalarType::String,
                [(
                    3,
                    Node::UserFunctionCall {
                        function: inner,
                        args: Vec::new(),
                    },
                )],
                3,
            ),
        ),
    ]);
    let graph = Graph {
        nodes: [(
            0,
            Node::UserFunctionCall {
                function: outer,
                args: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    };
    let project = project(graph, user_functions, 0);
    assert!(validate(&project).is_empty());
    assert_eq!(
        output_value(&run(&project, &source("unused"))?),
        Some(&Value::String("fallback".into()))
    );
    let preview = ExecutionContext::new(Path::new("/maps/test.json"))
        .with_purpose(crate::ExecutionPurpose::Preview);
    assert_eq!(
        output_value(&run_with_context(&project, &source("unused"), &preview)?),
        Some(&Value::String("nested-preview".into()))
    );

    let mut parameters = crate::RuntimeParameters::new();
    parameters.insert("override", Value::String("supplied".into()))?;
    let execution =
        ExecutionContext::new(Path::new("/maps/test.json")).with_parameters(&parameters);
    assert_eq!(
        output_value(&run_with_context(&project, &source("unused"), &execution)?),
        Some(&Value::String("supplied".into()))
    );
    assert_eq!(
        output_value(&run_with_context(
            &project,
            &source("unused"),
            &execution.with_purpose(crate::ExecutionPurpose::Preview)
        )?),
        Some(&Value::String("supplied".into()))
    );
    Ok(())
}

#[test]
fn nested_functions_keep_same_named_previews_local_to_each_node() {
    let inner = FunctionId::new(1);
    let outer = FunctionId::new(2);
    let input = |lexical: &str| Node::RuntimeParameter {
        name: "shared".into(),
        ty: ScalarType::String,
        preview: Some(lexical.into()),
    };
    let definitions = BTreeMap::from([
        (
            inner,
            function(
                "inner_preview",
                Vec::new(),
                ScalarType::String,
                [(1, input("inner"))],
                1,
            ),
        ),
        (
            outer,
            function(
                "outer_preview",
                Vec::new(),
                ScalarType::String,
                [
                    (1, input("outer")),
                    (
                        2,
                        Node::UserFunctionCall {
                            function: inner,
                            args: vec![],
                        },
                    ),
                    (
                        3,
                        Node::Call {
                            function: "concat".into(),
                            args: vec![1, 2],
                        },
                    ),
                ],
                3,
            ),
        ),
    ]);
    let graph = Graph {
        nodes: [
            (1, input("main")),
            (
                2,
                Node::UserFunctionCall {
                    function: outer,
                    args: vec![],
                },
            ),
            (
                3,
                Node::Call {
                    function: "concat".into(),
                    args: vec![1, 2],
                },
            ),
        ]
        .into_iter()
        .collect(),
    };
    let project = project(graph, definitions, 3);
    assert!(validate(&project).is_empty());
    let preview = ExecutionContext::new(Path::new("mapping.ferrule"))
        .with_purpose(crate::ExecutionPurpose::Preview);
    assert_eq!(
        output_value(&run_with_context(&project, &source("unused"), &preview).unwrap()),
        Some(&Value::String("mainouterinner".into()))
    );
    assert!(matches!(
        run(&project, &source("unused")),
        Err(EngineError::MissingRuntimeParameter { node: 1, .. })
    ));
    let mut parameters = crate::RuntimeParameters::new();
    parameters
        .insert("shared", Value::String("host".into()))
        .unwrap();
    assert_eq!(
        output_value(
            &run_with_context(
                &project,
                &source("unused"),
                &preview.with_parameters(&parameters)
            )
            .unwrap()
        ),
        Some(&Value::String("hosthosthost".into()))
    );
}
