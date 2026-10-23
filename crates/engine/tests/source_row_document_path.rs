use engine::{DocumentVisitOptions, EngineError, ExecutionContext, TargetSelection};
use ir::{Instance, SourceDocumentOrigin, SourceRowPathError, Value};
use std::{cell::Cell, ops::ControlFlow, path::Path};

#[path = "../../../conformance/mapforce-json5-documents/project.rs"]
mod fixture;

fn row(order: &str, path: Option<&str>) -> Instance {
    let fields = vec![
        (
            "Order".into(),
            Instance::Scalar(Value::String(order.into())),
        ),
        ("Qty".into(), Instance::Scalar(Value::String("1".into()))),
    ];
    let group = ir::InstanceGroup::from(fields);
    Instance::Group(match path {
        Some(path) => group.with_source_origin(
            SourceDocumentOrigin::new(path, None)
                .unwrap()
                .row(1)
                .unwrap(),
        ),
        None => group,
    })
}

#[test]
fn grouped_document_members_read_their_own_original_row() {
    for named in [false, true] {
        let project = fixture::with_row_paths(fixture::project(named));
        let input = Instance::Repeated(vec![
            row("A", Some("a.csv")),
            row("A", Some("b.csv")),
            row("B", Some("b.csv")),
        ]);
        let execution =
            ExecutionContext::new(Path::new("fixture.mfd")).with_current_datetime("fixed-");
        let mut result = Vec::new();
        let status = engine::visit_target_documents_with_options(
            &project,
            &input,
            vec![],
            &execution,
            if named {
                TargetSelection::Named("selected")
            } else {
                TargetSelection::Primary
            },
            DocumentVisitOptions::default(),
            |document| {
                result.push(document);
                ControlFlow::<()>::Continue(())
            },
        )
        .unwrap();
        assert_eq!(status, engine::DocumentVisit::Complete { accepted: 1 });
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].path(), "fixed-B");
        let lines = result[0]
            .value()
            .field("Lines")
            .unwrap()
            .as_repeated()
            .unwrap();
        assert_eq!(
            lines
                .iter()
                .map(|line| line.field("Path").unwrap().as_scalar().unwrap())
                .collect::<Vec<_>>(),
            [
                &Value::String("a.csv".into()),
                &Value::String("b.csv".into()),
                &Value::String("b.csv".into())
            ]
        );
    }
}

#[test]
fn missing_origin_fails_before_filename_evaluation_and_group_header_never_picks_first_row() {
    let input = Instance::Repeated(vec![row("A", None), row("B", Some("present.csv"))]);
    let execution = ExecutionContext::new(Path::new("fixture.mfd")).with_current_datetime("fixed-");
    let files = Cell::new(0);
    let clock = || {
        files.set(files.get() + 1);
        Ok("clock".into())
    };
    let mut project = fixture::with_row_paths(fixture::project(false));
    let error = engine::visit_target_documents_with_options(
        &project,
        &input,
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default().with_filename_clock(&clock),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert!(matches!(
        error.source,
        EngineError::SourceRowDocumentPath {
            node: 4,
            source: SourceRowPathError::MissingOrigin,
            ..
        }
    ));
    assert_eq!(files.get(), 0);
    project.root.bindings[0].node = 4;
    let known_input = Instance::Repeated(vec![
        row("A", Some("first.csv")),
        row("B", Some("second.csv")),
    ]);
    let error = engine::visit_target_documents_with_options(
        &project,
        &known_input,
        vec![],
        &execution,
        TargetSelection::Primary,
        DocumentVisitOptions::default(),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err()
    .source;
    assert!(
        matches!(
            error,
            EngineError::SourceRowDocumentPath {
                node: 4,
                source: SourceRowPathError::GroupedFrame,
                ..
            }
        ),
        "{error:?}"
    );
}
