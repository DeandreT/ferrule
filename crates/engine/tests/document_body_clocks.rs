use engine::{DocumentVisit, DocumentVisitOptions, EngineError, ExecutionContext, TargetSelection};
use ir::{DocumentMember, Instance, Value};
use std::{
    cell::{Cell, RefCell},
    ops::ControlFlow,
    path::Path,
};

#[path = "../../../conformance/document-clocks/project.rs"]
mod clocks;
#[path = "../../../conformance/document-body-clocks/project.rs"]
mod fixture;

fn source(modes: &[i64]) -> Instance {
    Instance::DocumentSet(
        modes
            .iter()
            .enumerate()
            .map(|(index, mode)| {
                DocumentMember::new(
                    format!("{index}.xml"),
                    Instance::Group(
                        vec![
                            ("Fail".into(), Instance::Scalar(Value::Bool(false))),
                            ("Mode".into(), Instance::Scalar(Value::Int(*mode))),
                            (
                                "Samples".into(),
                                Instance::Repeated(
                                    (0..2)
                                        .map(|_| {
                                            Instance::Group(
                                                vec![(
                                                    "Marker".into(),
                                                    Instance::Scalar(Value::String("x".into())),
                                                )]
                                                .into(),
                                            )
                                        })
                                        .collect(),
                                ),
                            ),
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
fn reached_body_reads_and_filename_aggregates_use_separate_clocks() {
    let events = RefCell::new(Vec::new());
    let bodies = Cell::new(0);
    let files = Cell::new(0);
    let body = || {
        bodies.set(bodies.get() + 1);
        let value = format!("B{}", bodies.get());
        events.borrow_mut().push(value.clone());
        Ok(value)
    };
    let file = || {
        files.set(files.get() + 1);
        let value = format!("F{}", files.get());
        events.borrow_mut().push(value.clone());
        Ok(value + "-")
    };
    let context = ExecutionContext::new(Path::new("clock.ferrule"))
        .with_current_datetime("stable")
        .with_invocation_counter_limits(engine::InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 64,
        });
    let mut documents = Vec::new();
    let result = engine::visit_target_documents_with_options(
        &fixture::project(clocks::project()),
        &source(&[1, 0, 2, 3]),
        vec![],
        &context,
        TargetSelection::Primary,
        DocumentVisitOptions::default()
            .with_body_clock(&body)
            .with_filename_clock(&file),
        |doc| {
            documents.push(doc);
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 4 });
    assert_eq!((bodies.get(), files.get()), (16, 5));
    assert_eq!(
        events
            .borrow()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        [
            "B1", "B2", "B3", "B4", "F1", "B5", "B6", "B7", "B8", "B9", "B10", "B11", "B12", "F2",
            "F3", "B13", "B14", "B15", "B16", "F4", "F5"
        ]
    );
    assert_eq!(
        documents.iter().map(|d| d.path()).collect::<Vec<_>>(),
        ["F1-0.xml", "fixed-1.xml", "F2-F3-2.xml", "F5-3.xml"]
    );
    for (index, doc) in documents.iter().enumerate() {
        assert_eq!(
            doc.value().field("Count").and_then(Instance::as_scalar),
            Some(&Value::Int(index as i64 + 1))
        );
        for (name, offset) in [("Body", 1), ("Repeated", 2), ("Fold", 4)] {
            assert_eq!(
                doc.value().field(name).and_then(Instance::as_scalar),
                Some(&Value::String(format!("B{}", index * 4 + offset)))
            );
        }
    }
}

#[test]
fn body_failure_retains_the_accepted_prefix_and_does_not_sample_its_filename() {
    let bodies = Cell::new(0);
    let files = Cell::new(0);
    let body = || {
        bodies.set(bodies.get() + 1);
        if bodies.get() == 5 {
            Err("body unavailable".into())
        } else {
            Ok("body".into())
        }
    };
    let file = || {
        files.set(files.get() + 1);
        Ok("file-".into())
    };
    let context = ExecutionContext::new(Path::new("clock.ferrule"))
        .with_current_datetime("stable")
        .with_invocation_counter_limits(engine::InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 64,
        });
    let error = engine::visit_target_documents_with_options(
        &fixture::project(clocks::project()),
        &source(&[1, 1, 1]),
        vec![],
        &context,
        TargetSelection::Primary,
        DocumentVisitOptions::default()
            .with_body_clock(&body)
            .with_filename_clock(&file),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert_eq!(error.accepted, 1);
    assert_eq!(
        error.source,
        EngineError::DocumentBodyClock {
            node: 0,
            message: "body unavailable".into()
        }
    );
    assert_eq!((bodies.get(), files.get()), (5, 1));
}

#[test]
fn body_only_opt_in_leaves_filename_time_stable_and_consumer_stop_is_lazy() {
    let bodies = Cell::new(0);
    let body = || {
        bodies.set(bodies.get() + 1);
        Ok("body".into())
    };
    let context = ExecutionContext::new(Path::new("clock.ferrule"))
        .with_current_datetime("stable-")
        .with_invocation_counter_limits(engine::InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 64,
        });
    let result = engine::visit_target_documents_with_options(
        &fixture::project(clocks::project()),
        &source(&[1, 1]),
        vec![],
        &context,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_body_clock(&body),
        |doc| {
            assert_eq!(doc.path(), "stable-0.xml");
            ControlFlow::Break("stop")
        },
    )
    .unwrap();
    assert_eq!(
        result,
        DocumentVisit::Stopped {
            accepted: 0,
            reason: "stop"
        }
    );
    assert_eq!(bodies.get(), 4);
}

#[test]
fn unsupported_body_graph_is_refused_before_any_clock_call() {
    let mut project = fixture::project(clocks::project());
    project
        .graph
        .nodes
        .insert(2, mapping::Node::Raise { message: None });
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok("clock".into())
    };
    let context = ExecutionContext::new(Path::new("clock.ferrule"));
    let error = engine::visit_target_documents_with_options(
        &project,
        &source(&[1]),
        vec![],
        &context,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_body_clock(&clock),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert!(matches!(
        error.source,
        EngineError::DocumentStreamUnsupported { .. }
    ));
    assert_eq!(calls.get(), 0);
}

#[test]
fn named_body_clock_visit_never_runs_the_unselected_primary() {
    let project = fixture::named(fixture::project(clocks::project()));
    let calls = Cell::new(0);
    let body = || {
        calls.set(calls.get() + 1);
        Ok("body".into())
    };
    let filename = || Ok("file-".into());
    let context = ExecutionContext::new(Path::new("clock.ferrule")).with_invocation_counter_limits(
        engine::InvocationCounterLimits {
            max_counters: 1,
            max_text_bytes: 64,
        },
    );
    let result = engine::visit_target_documents_with_options(
        &project,
        &source(&[1]),
        vec![],
        &context,
        TargetSelection::Named("selected"),
        DocumentVisitOptions::default()
            .with_body_clock(&body)
            .with_filename_clock(&filename),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
    assert_eq!(calls.get(), 4);
}
