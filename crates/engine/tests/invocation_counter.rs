use std::path::Path;

use engine::{EngineError, ExecutionContext};
use functions::{InvocationCounterError, InvocationCounterLimits};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope};

fn project() -> Project {
    let target = SchemaNode::group("Output", vec![SchemaNode::scalar("Value", ScalarType::Int)]);
    let root = Scope {
        bindings: vec![Binding {
            target_field: "Value".into(),
            node: 1,
        }],
        ..Default::default()
    };
    Project {
        source: SchemaNode::group("Input", Vec::new()),
        target: target.clone(),
        source_path: None,
        target_path: None,
        source_options: Default::default(),
        target_options: Default::default(),
        extra_sources: Vec::new(),
        extra_targets: vec![NamedTarget {
            name: "audit".into(),
            path: None,
            schema: target,
            options: Default::default(),
            root: root.clone(),
        }],
        failure_rules: Vec::new(),
        user_functions: Default::default(),
        root,
        graph: Graph {
            nodes: [
                (
                    1,
                    Node::InvocationCounter {
                        key: "line".into(),
                        start: 2,
                        step: 3,
                        restart: None,
                    },
                ),
                (
                    2,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
                (
                    3,
                    Node::Const {
                        value: Value::Int(1),
                    },
                ),
            ]
            .into(),
        },
    }
}

fn empty() -> Instance {
    Instance::Group(Vec::new().into())
}

fn configured() -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("mapping.ferrule")).with_invocation_counter_limits(
        InvocationCounterLimits {
            max_counters: 8,
            max_text_bytes: 256,
        },
    )
}

#[test]
fn counter_state_spans_output_scopes_but_not_mapping_calls() {
    let p = project();
    assert!(engine::validate(&p).is_empty());
    let context = configured();
    for _ in 0..3 {
        let output =
            engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &context).unwrap();
        assert_eq!(
            output.primary.field("Value").and_then(Instance::as_scalar),
            Some(&Value::Int(1))
        );
        assert_eq!(
            output.extras[0]
                .instance
                .field("Value")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(2))
        );
    }
}

#[test]
fn unconfigured_counter_is_refused_before_operands() {
    let mut p = project();
    p.graph.nodes.insert(2, Node::Raise { message: None });
    assert_eq!(
        engine::run_outputs(&p, &empty()).unwrap_err(),
        EngineError::MissingInvocationCounters { node: 1 }
    );
}

#[test]
fn start_type_refusal_precedes_a_later_operand_failure() {
    let mut p = project();
    p.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::String("1".into()),
        },
    );
    p.graph.nodes.insert(3, Node::Raise { message: None });
    assert_eq!(
        engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &configured())
            .unwrap_err(),
        EngineError::InvocationCounterOperand {
            node: 1,
            operand: "start",
            expected: "integer",
            found: "string"
        }
    );
}

#[test]
fn counter_addition_wraps_instead_of_using_checked_arithmetic() {
    let mut p = project();
    p.graph.nodes.insert(
        2,
        Node::Const {
            value: Value::Int(i64::MAX),
        },
    );
    let output =
        engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &configured()).unwrap();
    assert_eq!(
        output.primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::Int(i64::MAX))
    );
    assert_eq!(
        output.extras[0]
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(i64::MIN))
    );
}

#[test]
fn counter_limit_failure_keeps_node_and_typed_cause() {
    let context = ExecutionContext::new(Path::new("mapping.ferrule"))
        .with_invocation_counter_limits(InvocationCounterLimits {
            max_counters: 0,
            max_text_bytes: 256,
        });
    assert_eq!(
        engine::run_outputs_with_sources_and_context(&project(), &empty(), vec![], &context)
            .unwrap_err(),
        EngineError::InvocationCounter {
            node: 1,
            source: InvocationCounterError::CountLimit { maximum: 0 }
        }
    );
}

#[test]
fn counter_evaluations_are_not_memoized_by_node_id() {
    let mut p = project();
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::scalar("A", ScalarType::Int),
            SchemaNode::scalar("B", ScalarType::Int),
        ],
    );
    p.root.bindings = vec![
        Binding {
            target_field: "A".into(),
            node: 1,
        },
        Binding {
            target_field: "B".into(),
            node: 1,
        },
    ];
    p.extra_targets.clear();
    let output =
        engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &configured()).unwrap();
    assert_eq!(
        output.primary.field("A").and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert_eq!(
        output.primary.field("B").and_then(Instance::as_scalar),
        Some(&Value::Int(2))
    );
}

fn optional_counter_project(condition: Node) -> Project {
    let mut p = project();
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Maybe", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    p.graph.nodes.insert(4, condition);
    p.root = Scope {
        children: vec![Scope {
            target_field: "Maybe".into(),
            iteration: mapping::ScopeIteration::OptionalPresence { condition: 4 },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    p
}

#[test]
fn optional_presence_skips_counter_effects_but_shares_state_when_present() {
    for present in [false, true] {
        let p = optional_counter_project(Node::Const {
            value: Value::Bool(present),
        });
        assert!(engine::validate(&p).is_empty());
        let context = configured();
        for _ in 0..2 {
            let output =
                engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &context)
                    .unwrap();
            let items = match output.primary.field("Maybe") {
                Some(Instance::Repeated(items)) => items.as_slice(),
                None => &[],
                other => panic!("unexpected optional group: {other:?}"),
            };
            assert_eq!(items.len(), usize::from(present));
            if present {
                assert_eq!(
                    items[0].field("Value").and_then(Instance::as_scalar),
                    Some(&Value::Int(1))
                );
            }
            assert_eq!(
                output.extras[0]
                    .instance
                    .field("Value")
                    .and_then(Instance::as_scalar),
                Some(&Value::Int(if present { 2 } else { 1 }))
            );
        }
    }
}

#[test]
fn optional_presence_condition_and_body_are_separate_counter_calls() {
    let mut p = optional_counter_project(Node::Call {
        function: "equal".into(),
        args: vec![1, 5],
    });
    p.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::Int(1),
        },
    );
    assert!(engine::validate(&p).is_empty());
    let context = configured();
    for _ in 0..2 {
        let output =
            engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &context).unwrap();
        let Some(Instance::Repeated(items)) = output.primary.field("Maybe") else {
            panic!("present group is missing");
        };
        assert_eq!(items.len(), 1);
        assert_eq!(
            items[0].field("Value").and_then(Instance::as_scalar),
            Some(&Value::Int(2))
        );
        assert_eq!(
            output.extras[0]
                .instance
                .field("Value")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(3))
        );
    }
}
