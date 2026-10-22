use std::cell::Cell;
use std::ops::ControlFlow;
use std::path::Path;

use engine::{DocumentVisit, DocumentVisitOptions, EngineError, ExecutionContext, TargetSelection};
use ir::{DocumentMember, Instance, ScalarType, SchemaNode, Value};
use mapping::{
    Binding, FunctionId, Graph, Node, Project, RuntimeValue, Scope, ScopeIteration, UserFunction,
};

fn project() -> Project {
    Project {
        source: SchemaNode::group(
            "Source",
            vec![
                SchemaNode::scalar("Fail", ScalarType::Bool),
                SchemaNode::scalar("UseClock", ScalarType::Bool),
            ],
        ),
        target: SchemaNode::group(
            "Target",
            vec![SchemaNode::scalar("Body", ScalarType::String)],
        ),
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
                (
                    0,
                    Node::RuntimeValue {
                        value: RuntimeValue::CurrentDateTime,
                    },
                ),
                (
                    1,
                    Node::SourceField {
                        path: vec!["Fail".into()],
                        frame: None,
                    },
                ),
                (2, Node::Raise { message: None }),
                (
                    3,
                    Node::If {
                        condition: 1,
                        then: 2,
                        else_: 0,
                    },
                ),
                (
                    4,
                    Node::SourceField {
                        path: vec!["UseClock".into()],
                        frame: None,
                    },
                ),
                (
                    5,
                    Node::If {
                        condition: 4,
                        then: 0,
                        else_: 6,
                    },
                ),
                (
                    6,
                    Node::Const {
                        value: Value::String("fixed-".into()),
                    },
                ),
                (
                    7,
                    Node::Call {
                        function: "concat".into(),
                        args: vec![5, 8],
                    },
                ),
                (8, Node::SourceDocumentPath),
            ]
            .into_iter()
            .collect(),
        },
        root: Scope {
            iteration: ScopeIteration::DynamicDocuments {
                source: vec![],
                output_path: 7,
            },
            bindings: vec![Binding {
                target_field: "Body".into(),
                node: 3,
            }],
            ..Default::default()
        },
    }
}

fn source(flags: &[(bool, bool)]) -> Instance {
    Instance::DocumentSet(
        flags
            .iter()
            .enumerate()
            .map(|(index, (fail, use_clock))| {
                DocumentMember::new(
                    format!("{index}.xml"),
                    Instance::Group(
                        vec![
                            ("Fail".into(), Instance::Scalar(Value::Bool(*fail))),
                            ("UseClock".into(), Instance::Scalar(Value::Bool(*use_clock))),
                        ]
                        .into(),
                    ),
                )
                .unwrap()
            })
            .collect(),
    )
}

#[test]
fn filename_clock_is_per_document_while_body_clock_stays_stable() {
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok(format!("file-{}-", calls.get()))
    };
    let mut actual = vec![];
    let execution =
        ExecutionContext::new(Path::new("clock.ferrule")).with_current_datetime("body-clock");
    let result = engine::visit_target_documents_with_options(
        &project(),
        &source(&[(false, true); 3]),
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |document| {
            actual.push(document);
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 3 });
    assert_eq!(calls.get(), 3);
    assert_eq!(
        actual.iter().map(|doc| doc.path()).collect::<Vec<_>>(),
        ["file-1-0.xml", "file-2-1.xml", "file-3-2.xml"]
    );
    assert!(actual.iter().all(
        |doc| doc.value().field("Body").and_then(Instance::as_scalar)
            == Some(&Value::String("body-clock".into()))
    ));
}

#[test]
fn unselected_filename_clock_branch_is_not_sampled() {
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok("sample-".into())
    };
    let mut names = vec![];
    let execution = ExecutionContext::new(Path::new("clock.ferrule")).with_current_datetime("body");
    let result = engine::visit_target_documents_with_options(
        &project(),
        &source(&[(false, false), (false, true)]),
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |document| {
            names.push(document.path().to_owned());
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 2 });
    assert_eq!(calls.get(), 1);
    assert_eq!(names, ["fixed-0.xml", "sample-1.xml"]);
}

#[test]
fn body_failure_and_consumer_stop_do_not_sample_later_filenames() {
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok("sample-".into())
    };
    let execution = ExecutionContext::new(Path::new("clock.ferrule")).with_current_datetime("body");
    let failure = engine::visit_target_documents_with_options(
        &project(),
        &source(&[(true, true)]),
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert_eq!(
        failure.source,
        EngineError::MappingException {
            node: 2,
            message: None
        }
    );
    assert_eq!(calls.get(), 0);
    let result = engine::visit_target_documents_with_options(
        &project(),
        &source(&[(false, true); 2]),
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |_| ControlFlow::Break("stop"),
    )
    .unwrap();
    assert_eq!(
        result,
        DocumentVisit::Stopped {
            accepted: 0,
            reason: "stop"
        }
    );
    assert_eq!(calls.get(), 1);
}

#[test]
fn filename_clock_failure_preserves_preceding_accepted_document() {
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        if calls.get() == 2 {
            Err("clock unavailable".into())
        } else {
            Ok("sample-".into())
        }
    };
    let mut names = vec![];
    let execution = ExecutionContext::new(Path::new("clock.ferrule")).with_current_datetime("body");
    let failure = engine::visit_target_documents_with_options(
        &project(),
        &source(&[(false, true); 3]),
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |document| {
            names.push(document.path().to_owned());
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap_err();
    assert_eq!(failure.accepted, 1);
    assert_eq!(
        failure.source,
        EngineError::DocumentFilenameClock {
            node: 0,
            message: "clock unavailable".into()
        }
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(names, ["sample-0.xml"]);
}

#[test]
fn reused_clock_node_is_evaluated_at_each_reached_filename_edge() {
    let mut project = project();
    project.graph.nodes.insert(
        7,
        Node::Call {
            function: "concat".into(),
            args: vec![0, 0, 8],
        },
    );
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok(format!("{}-", calls.get()))
    };
    let mut names = vec![];
    let execution = ExecutionContext::new(Path::new("clock.ferrule")).with_current_datetime("body");
    let result = engine::visit_target_documents_with_options(
        &project,
        &source(&[(false, true)]),
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |document| {
            names.push(document.path().to_owned());
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
    assert_eq!(names, ["1-2-0.xml"]);
    assert_eq!(calls.get(), 2);
}

#[test]
fn udf_filename_clock_route_is_refused_before_body_or_clock_effects() {
    let mut project = project();
    project.user_functions.insert(
        FunctionId::new(1),
        UserFunction {
            library: "fixture".into(),
            name: "name".into(),
            description: None,
            parameters: vec![],
            output_type: ScalarType::String,
            output_name: "out".into(),
            output: 1,
            body: Graph {
                nodes: [(
                    1,
                    Node::Const {
                        value: Value::String("name.xml".into()),
                    },
                )]
                .into_iter()
                .collect(),
            },
        },
    );
    project.graph.nodes.insert(
        7,
        Node::UserFunctionCall {
            function: FunctionId::new(1),
            args: vec![],
        },
    );
    let clock = || -> Result<String, String> { panic!("unqualified route sampled the clock") };
    let failure = engine::visit_target_documents_with_options(
        &project,
        &source(&[(true, true)]),
        vec![],
        &ExecutionContext::new(Path::new("clock.ferrule")),
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert!(matches!(
        failure.source,
        EngineError::DocumentStreamUnsupported { .. }
    ));
    assert_eq!(failure.accepted, 0);
}
