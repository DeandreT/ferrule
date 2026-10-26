use engine::{
    EngineError, ExecutionContext, InvocationCounterLimits, SelectedTargetOutput, TargetSelection,
};
use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Node, Project, Scope, ScopeIteration};
use std::path::Path;

#[path = "../../../conformance/invocation-counter-expressions/project.rs"]
mod fixture;

fn empty() -> Instance {
    Instance::Group(Vec::new().into())
}
fn configured() -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("counter.ferrule")).with_invocation_counter_limits(
        InvocationCounterLimits {
            max_counters: 8,
            max_text_bytes: 256,
        },
    )
}
fn run(p: &Project) -> Result<engine::ExecutionOutputs, EngineError> {
    engine::run_outputs_with_sources_and_context(p, &empty(), vec![], &configured())
}

#[test]
fn nested_counter_operands_are_ordered_without_memoization() {
    let p = fixture::nested_project();
    for _ in 0..2 {
        let output = run(&p).unwrap();
        for (field, value) in [("A", 1), ("B", 4)] {
            assert_eq!(
                output.primary.field(field).and_then(Instance::as_scalar),
                Some(&Value::Int(value))
            );
        }
        assert_eq!(
            output.extras[0]
                .instance
                .field("Value")
                .and_then(Instance::as_scalar),
            Some(&Value::Int(7))
        );
    }
}

#[test]
fn strict_counter_start_types_precede_later_operand_failure() {
    for bad in [
        Value::Null,
        Value::String("1".into()),
        Value::Float(1.0),
        Value::decimal(ir::Decimal96::parse_exact("1").unwrap()),
        Value::Bool(true),
        Value::json_null(),
        Value::xml_nil(),
    ] {
        let mut p = fixture::project();
        let found = bad.type_name();
        p.graph.nodes.insert(2, Node::Const { value: bad });
        p.graph.nodes.insert(3, Node::Raise { message: None });
        assert_eq!(
            run(&p).unwrap_err(),
            EngineError::InvocationCounterOperand {
                node: 1,
                operand: "start",
                expected: "integer",
                found
            }
        );
    }
}

#[test]
fn strict_counter_step_types_precede_restart_failure() {
    for bad in [
        Value::Null,
        Value::String("1".into()),
        Value::Float(1.0),
        Value::decimal(ir::Decimal96::parse_exact("1").unwrap()),
        Value::Bool(true),
        Value::json_null(),
        Value::xml_nil(),
    ] {
        let mut p = fixture::project();
        let found = bad.type_name();
        p.graph.nodes.insert(3, Node::Const { value: bad });
        p.graph.nodes.insert(4, Node::Raise { message: None });
        if let Node::InvocationCounter { restart, .. } = p.graph.nodes.get_mut(&1).unwrap() {
            *restart = Some(4)
        }
        assert_eq!(
            run(&p).unwrap_err(),
            EngineError::InvocationCounterOperand {
                node: 1,
                operand: "step",
                expected: "integer",
                found
            }
        );
    }
}

#[test]
fn strict_counter_restart_rejects_nontext_present_values() {
    for bad in [
        Value::Int(1),
        Value::Float(1.0),
        Value::decimal(ir::Decimal96::parse_exact("1").unwrap()),
        Value::Bool(true),
        Value::json_null(),
        Value::xml_nil(),
    ] {
        let mut p = fixture::project();
        let found = bad.type_name();
        p.graph.nodes.insert(4, Node::Const { value: bad });
        if let Node::InvocationCounter { restart, .. } = p.graph.nodes.get_mut(&1).unwrap() {
            *restart = Some(4)
        }
        assert_eq!(
            run(&p).unwrap_err(),
            EngineError::InvocationCounterOperand {
                node: 1,
                operand: "restart",
                expected: "string or missing",
                found
            }
        );
    }
}

#[test]
fn distinct_counter_nodes_share_exact_key_and_preserve_restart_presence() {
    let mut p = fixture::project();
    p.extra_targets.clear();
    p.target = SchemaNode::group(
        "Output",
        (1..=6)
            .map(|n| SchemaNode::scalar(format!("n{n}"), ScalarType::Int))
            .collect(),
    );
    p.root.bindings = (1..=6)
        .map(|n| Binding {
            target_field: format!("n{n}"),
            node: n,
        })
        .collect();
    p.graph.nodes.clear();
    for (n, start, restart) in [
        (1, 100, None),
        (2, 100, None),
        (3, 100, Some(102)),
        (4, 103, Some(102)),
        (5, 103, None),
        (6, 103, None),
    ] {
        p.graph.nodes.insert(
            n,
            Node::InvocationCounter {
                key: "same".into(),
                start,
                step: 101,
                restart,
            },
        );
    }
    for (id, value) in [
        (100, Value::Int(10)),
        (101, Value::Int(1)),
        (102, Value::String("".into())),
        (103, Value::Int(20)),
    ] {
        p.graph.nodes.insert(id, Node::Const { value });
    }
    let output = run(&p).unwrap();
    for (field, expected) in [
        ("n1", 10),
        ("n2", 11),
        ("n3", 10),
        ("n4", 11),
        ("n5", 20),
        ("n6", 21),
    ] {
        assert_eq!(
            output.primary.field(field).and_then(Instance::as_scalar),
            Some(&Value::Int(expected))
        );
    }
}

#[test]
fn counter_in_unchosen_branch_needs_no_configuration() {
    let mut p = fixture::project();
    p.extra_targets.clear();
    p.graph.nodes.insert(
        4,
        Node::Const {
            value: Value::Bool(false),
        },
    );
    p.graph.nodes.insert(
        5,
        Node::Const {
            value: Value::Int(77),
        },
    );
    p.graph.nodes.insert(
        6,
        Node::If {
            condition: 4,
            then: 1,
            else_: 5,
        },
    );
    p.root.bindings[0].node = 6;
    let output = engine::run_outputs(&p, &empty()).unwrap();
    assert_eq!(
        output.primary.field("Value").and_then(Instance::as_scalar),
        Some(&Value::Int(77))
    );
}

#[test]
fn selected_named_counter_does_not_evaluate_failing_primary() {
    let mut p = fixture::project();
    p.graph.nodes.insert(4, Node::Raise { message: None });
    p.root.bindings[0].node = 4;
    let SelectedTargetOutput::Named(output) = engine::run_selected_target_with_sources_and_context(
        &p,
        &empty(),
        vec![],
        &configured(),
        TargetSelection::Named("audit"),
    )
    .unwrap() else {
        panic!("Expected named target")
    };
    assert_eq!(
        output.instance.field("Value").and_then(Instance::as_scalar),
        Some(&Value::Int(1))
    );
    assert!(run(&p).is_err());
}

#[test]
fn failed_mapping_does_not_retain_counter_state_for_corrected_retry() {
    let mut p = fixture::project();
    p.extra_targets.clear();
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
            node: 4,
        },
    ];
    p.graph.nodes.insert(4, Node::Raise { message: None });
    let context = configured();
    assert!(engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &context).is_err());
    p.root.bindings[1].node = 1;
    for _ in 0..2 {
        let out =
            engine::run_outputs_with_sources_and_context(&p, &empty(), vec![], &context).unwrap();
        assert_eq!(
            out.primary.field("A").and_then(Instance::as_scalar),
            Some(&Value::Int(1))
        );
        assert_eq!(
            out.primary.field("B").and_then(Instance::as_scalar),
            Some(&Value::Int(2))
        );
    }
}

#[test]
fn counter_state_spans_ordinary_child_rows_and_named_output() {
    let mut p = fixture::project();
    p.source = SchemaNode::group("Input", vec![SchemaNode::group("Rows", vec![]).repeating()]);
    p.target = SchemaNode::group(
        "Output",
        vec![
            SchemaNode::group("Items", vec![SchemaNode::scalar("Value", ScalarType::Int)])
                .repeating(),
        ],
    );
    p.root = Scope {
        children: vec![Scope {
            target_field: "Items".into(),
            iteration: ScopeIteration::Source(vec!["Rows".into()]),
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 1,
            }],
            ..Default::default()
        }],
        ..Default::default()
    };
    let source =
        Instance::Group(vec![("Rows".into(), Instance::Repeated(vec![empty(), empty()]))].into());
    let output =
        engine::run_outputs_with_sources_and_context(&p, &source, vec![], &configured()).unwrap();
    let Some(Instance::Repeated(rows)) = output.primary.field("Items") else {
        panic!("Expected repeated items")
    };
    assert_eq!(rows.len(), 2);
    for (row, expected) in rows.iter().zip([1, 2]) {
        assert_eq!(
            row.field("Value").and_then(Instance::as_scalar),
            Some(&Value::Int(expected))
        );
    }
    assert_eq!(
        output.extras[0]
            .instance
            .field("Value")
            .and_then(Instance::as_scalar),
        Some(&Value::Int(3))
    );
}
