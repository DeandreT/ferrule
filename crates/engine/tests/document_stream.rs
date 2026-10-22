use std::ops::ControlFlow;
use std::path::Path;

use engine::{
    DocumentVisit, EngineError, ExecutionContext, InvocationCounterLimits, TargetSelection,
};
use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{Binding, Graph, NamedTarget, Node, Project, Scope, ScopeIteration};

fn project() -> Project {
    Project {
        source: SchemaNode::group("Source", vec![SchemaNode::scalar("Fail", ScalarType::Bool)]),
        target: SchemaNode::group("Target", vec![SchemaNode::scalar("Value", ScalarType::Int)]),
        source_path: Some("input-*.xml".into()),
        target_path: None,
        source_options: mapping::FormatOptions {
            xml_document: true,
            local_xml_file_set: true,
            ..Default::default()
        },
        target_options: mapping::FormatOptions {
            xml_document: true,
            ..Default::default()
        },
        extra_sources: vec![],
        extra_targets: vec![],
        failure_rules: vec![],
        user_functions: Default::default(),
        graph: Graph {
            nodes: [
                (0, Node::SourceDocumentPath),
                (
                    1,
                    Node::InvocationCounter {
                        key: "documents".into(),
                        start: 2,
                        step: 2,
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
                    Node::SourceField {
                        path: vec!["Fail".into()],
                        frame: None,
                    },
                ),
                (4, Node::Raise { message: Some(5) }),
                (
                    5,
                    Node::Const {
                        value: Value::String("bad document".into()),
                    },
                ),
                (
                    6,
                    Node::If {
                        condition: 3,
                        then: 4,
                        else_: 1,
                    },
                ),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec![],
                output_path: 0,
            },
            bindings: vec![Binding {
                target_field: "Value".into(),
                node: 6,
            }],
            ..Default::default()
        },
    }
}

fn source(flags: &[bool]) -> Instance {
    Instance::DocumentSet(
        flags
            .iter()
            .enumerate()
            .map(|(index, fail)| {
                DocumentMember::new(
                    format!("{index}.xml"),
                    Instance::Group(
                        vec![("Fail".into(), Instance::Scalar(Value::Bool(*fail)))].into(),
                    ),
                )
                .unwrap()
            })
            .collect(),
    )
}

fn context() -> ExecutionContext<'static> {
    ExecutionContext::new(Path::new("stream.ferrule")).with_invocation_counter_limits(
        InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 32,
        },
    )
}

fn value(document: &DocumentMember) -> i64 {
    match document
        .value()
        .field("Value")
        .and_then(Instance::as_scalar)
    {
        Some(Value::Int(value)) => *value,
        other => panic!("unexpected document: {other:?}"),
    }
}

#[test]
fn complete_documents_arrive_in_order_with_one_fresh_counter_table() {
    let project = project();
    assert!(engine::validate(&project).is_empty());
    let execution = context();
    for _ in 0..2 {
        let mut seen = vec![];
        let result = engine::visit_target_documents(
            &project,
            &source(&[false, false, false]),
            vec![],
            &execution,
            TargetSelection::Primary,
            |document| {
                seen.push((document.path().to_owned(), value(&document)));
                ControlFlow::<()>::Continue(())
            },
        )
        .unwrap();
        assert_eq!(result, DocumentVisit::Complete { accepted: 3 });
        assert_eq!(
            seen,
            [
                ("0.xml".into(), 1),
                ("1.xml".into(), 2),
                ("2.xml".into(), 3)
            ]
        );
    }
}

#[test]
fn failure_at_start_middle_or_end_keeps_only_earlier_accepted_documents() {
    let project = project();
    let execution = context();
    for failing in 0..3 {
        let mut flags = [false; 3];
        flags[failing] = true;
        let mut seen = vec![];
        let error = engine::visit_target_documents(
            &project,
            &source(&flags),
            vec![],
            &execution,
            TargetSelection::Primary,
            |document| {
                seen.push((document.path().to_owned(), value(&document)));
                ControlFlow::<()>::Continue(())
            },
        )
        .unwrap_err();
        assert_eq!(error.accepted, failing);
        assert_eq!(
            error.source,
            EngineError::MappingException {
                node: 4,
                message: Some("bad document".into())
            }
        );
        assert_eq!(seen.len(), failing);
        assert!(
            seen.iter()
                .enumerate()
                .all(|(index, (_, value))| *value == index as i64 + 1)
        );

        // The host retries just the corrected failed document, not its earlier
        // successes. Reusing host settings still starts a fresh invocation.
        let mut retry = vec![];
        let result = engine::visit_target_documents(
            &project,
            &source(&[false]),
            vec![],
            &execution,
            TargetSelection::Primary,
            |document| {
                retry.push(value(&document));
                ControlFlow::<()>::Continue(())
            },
        )
        .unwrap();
        assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
        assert_eq!(retry, [1]);
        assert_eq!(seen.len(), failing);
    }
}

#[test]
fn consumer_stop_is_not_complete_and_does_not_evaluate_later_bodies() {
    let mut offered = vec![];
    let result = engine::visit_target_documents(
        &project(),
        &source(&[false, false, true]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            offered.push(value(&document));
            if offered.len() == 2 {
                ControlFlow::Break("publication refused")
            } else {
                ControlFlow::Continue(())
            }
        },
    )
    .unwrap();
    assert_eq!(
        result,
        DocumentVisit::Stopped {
            accepted: 1,
            reason: "publication refused"
        }
    );
    assert_eq!(offered, [1, 2]);
}

#[test]
fn body_failure_precedes_a_failing_filename_expression() {
    let mut project = project();
    project.graph.nodes.insert(7, Node::Raise { message: None });
    assert!(project.root.set_output_path(Some(7)));
    let failure = engine::visit_target_documents(
        &project,
        &source(&[true]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert_eq!(
        failure.source,
        EngineError::MappingException {
            node: 4,
            message: Some("bad document".into())
        }
    );
    let failure = engine::visit_target_documents(
        &project,
        &source(&[false]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert_eq!(failure.accepted, 0);
    assert_eq!(
        failure.source,
        EngineError::MappingException {
            node: 7,
            message: None
        }
    );
}

#[test]
fn selected_named_target_does_not_evaluate_the_primary_body() {
    let mut project = project();
    project.graph.nodes.insert(
        7,
        Node::Const {
            value: Value::Int(99),
        },
    );
    let mut named = project.root.clone();
    named.bindings[0].node = 7;
    project.extra_targets.push(NamedTarget {
        name: "audit".into(),
        path: None,
        schema: project.target.clone(),
        options: project.target_options.clone(),
        root: named,
    });
    let mut seen = vec![];
    let result = engine::visit_target_documents(
        &project,
        &source(&[true]),
        vec![],
        &context(),
        TargetSelection::Named("audit"),
        |document| {
            seen.push(value(&document));
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
    assert_eq!(seen, [99]);
}

#[test]
fn static_targets_are_refused_before_body_or_consumer_effects() {
    let mut project = project();
    project.root.set_output_path(None);
    let mut called = false;
    let failure = engine::visit_target_documents(
        &project,
        &source(&[true]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |_| {
            called = true;
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.accepted, 0);
    assert!(matches!(
        failure.source,
        EngineError::DocumentStreamUnsupported { .. }
    ));
    assert!(!called);
}

#[test]
fn filtered_document_positions_do_not_depend_on_retaining_outputs() {
    let mut project = project();
    project.graph.nodes.insert(
        7,
        Node::Call {
            function: "not".into(),
            args: vec![3],
        },
    );
    project
        .graph
        .nodes
        .insert(8, Node::Position { collection: vec![] });
    project.root.filter = Some(7);
    project.root.bindings[0].node = 8;
    let mut seen = vec![];
    engine::visit_target_documents(
        &project,
        &source(&[false, true, false]),
        vec![],
        &context(),
        TargetSelection::Primary,
        |document| {
            seen.push((document.path().to_owned(), value(&document)));
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(seen, [("0.xml".into(), 1), ("2.xml".into(), 2)]);
}
