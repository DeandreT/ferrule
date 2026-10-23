use engine::{DocumentVisit, DocumentVisitOptions, EngineError, ExecutionContext, TargetSelection};
use ir::{DocumentMember, Instance, Value};
use std::{cell::Cell, ops::ControlFlow, path::Path};

#[path = "../../../conformance/document-clocks/project.rs"]
mod base;
#[path = "../../../conformance/distinct-text-map/project.rs"]
mod fixture;

fn row(key: Option<Value>, marker: &str, skip: bool, invalid: bool, fail: bool) -> Instance {
    let mut fields = vec![
        (
            "Marker".into(),
            Instance::Scalar(Value::String(marker.into())),
        ),
        ("Skip".into(), Instance::Scalar(Value::Bool(skip))),
        ("Invalid".into(), Instance::Scalar(Value::Bool(invalid))),
        ("Fail".into(), Instance::Scalar(Value::Bool(fail))),
    ];
    if let Some(key) = key {
        fields.push(("Key".into(), Instance::Scalar(key)));
    }
    Instance::Group(fields.into())
}
fn text(value: &str) -> Option<Value> {
    Some(Value::String(value.into()))
}
fn input(rows: Vec<Instance>) -> Instance {
    Instance::DocumentSet(vec![
        DocumentMember::new(
            "source.xml",
            Instance::Group(vec![("Rows".into(), Instance::Repeated(rows))].into()),
        )
        .unwrap(),
    ])
}

#[test]
fn distinct_keys_are_selected_before_mapping_and_preserve_first_item_positions() {
    let source = input(vec![
        row(text("A"), "A", false, false, false),
        row(text("B"), "B", false, false, false),
        row(text("A"), "ignored", false, false, true),
    ]);
    let body_calls = Cell::new(0);
    let file_calls = Cell::new(0);
    let body = || {
        body_calls.set(body_calls.get() + 1);
        Ok(format!("B{}", body_calls.get()))
    };
    let file = || {
        file_calls.set(file_calls.get() + 1);
        Ok(format!("F{}", file_calls.get()))
    };
    let context = ExecutionContext::new(Path::new("mapping"));
    let mut documents = vec![];
    let result = engine::visit_target_documents_with_options(
        &fixture::project(base::project()),
        &source,
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
    assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
    assert_eq!((body_calls.get(), file_calls.get()), (2, 2));
    assert_eq!(documents[0].path(), "out-B@2:F2.txt");
    assert_eq!(
        documents[0]
            .value()
            .field("Selected")
            .and_then(Instance::as_scalar),
        Some(&Value::String("B@2:B2".into()))
    );
}

#[test]
fn later_invalid_key_fails_before_an_earlier_mapped_error_or_clock() {
    let source = input(vec![
        row(text("A"), "A", false, false, true),
        row(Some(Value::Int(3)), "invalid key", false, false, false),
    ]);
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok("clock".into())
    };
    let context = ExecutionContext::new(Path::new("mapping"));
    let error = engine::visit_target_documents_with_options(
        &fixture::project(base::project()),
        &source,
        vec![],
        &context,
        TargetSelection::Primary,
        DocumentVisitOptions::default()
            .with_body_clock(&clock)
            .with_filename_clock(&clock),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert_eq!(
        error.source,
        EngineError::DistinctTextMapKind {
            node: 50,
            phase: "key",
            found: "int"
        }
    );
    assert_eq!(calls.get(), 0);
}

#[test]
fn missing_keys_and_null_mapped_values_are_not_empty_text_keys_or_values() {
    let source = input(vec![
        row(None, "missing", false, false, true),
        row(text(""), "empty-key", false, false, false),
        row(text(" "), "space-key", true, false, false),
    ]);
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok("time".into())
    };
    let context = ExecutionContext::new(Path::new("mapping"));
    let mut documents = vec![];
    let result = engine::visit_target_documents_with_options(
        &fixture::project(base::project()),
        &source,
        vec![],
        &context,
        TargetSelection::Primary,
        DocumentVisitOptions::default()
            .with_body_clock(&clock)
            .with_filename_clock(&clock),
        |doc| {
            documents.push(doc);
            ControlFlow::<()>::Continue(())
        },
    )
    .unwrap();
    assert_eq!(result, DocumentVisit::Complete { accepted: 1 });
    assert_eq!(calls.get(), 2);
    assert_eq!(documents[0].path(), "out-empty-key@2:time.txt");
}

#[test]
fn non_text_mapped_result_stops_before_later_values_and_filename() {
    let source = input(vec![
        row(text("A"), "A", false, false, false),
        row(text("B"), "B", false, true, false),
        row(text("C"), "C", false, false, false),
    ]);
    let calls = Cell::new(0);
    let clock = || {
        calls.set(calls.get() + 1);
        Ok("time".into())
    };
    let context = ExecutionContext::new(Path::new("mapping"));
    let error = engine::visit_target_documents_with_options(
        &fixture::project(base::project()),
        &source,
        vec![],
        &context,
        TargetSelection::Primary,
        DocumentVisitOptions::default()
            .with_body_clock(&clock)
            .with_filename_clock(&clock),
        |_| ControlFlow::<()>::Continue(()),
    )
    .unwrap_err();
    assert_eq!(
        error.source,
        EngineError::DistinctTextMapKind {
            node: 50,
            phase: "mapped value",
            found: "int"
        }
    );
    assert_eq!(calls.get(), 1);
}
