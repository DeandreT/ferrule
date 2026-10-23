use codegen_runtime::{GroupedItems, Instance, RuntimeError, ScopeContext, field, group, repeated};
use ir::{SourceDocumentOrigin, SourceRowPathError, Value};

fn marked(value: Instance, requested: &str, resolved: Option<&str>) -> Instance {
    let Instance::Group(fields) = value else {
        panic!("row")
    };
    Instance::Group(
        fields.with_source_origin(
            SourceDocumentOrigin::new(requested, resolved)
                .unwrap()
                .row(1)
                .unwrap(),
        ),
    )
}

fn assert_error(context: &ScopeContext<'_>, path: &[&str], expected: SourceRowPathError) {
    assert!(matches!(context.source_row_document_path(27, path),
        Err(RuntimeError::SourceRowDocumentPath { node: 27, collection, source })
        if collection == path && source == expected));
}

#[test]
fn exact_rows_retain_paths_after_cloning_and_grouping() {
    let source = group([field(
        "Rows",
        repeated([
            marked(group([]), "same.csv", Some(" physical/A.csv ")),
            marked(group([]), "same.csv", Some("physical/B.csv")),
        ]),
    )]);
    let root = ScopeContext::new(&source);
    assert_error(&root, &["Rows"], SourceRowPathError::InactiveFrame);
    let rows = root.walk_source(&["Rows"]);
    assert_eq!(
        rows[0].source_row_document_path(27, &["Rows"]),
        Ok(Value::String(" physical/A.csv ".into()))
    );
    assert_eq!(
        rows[1].source_row_document_path(27, &["Rows"]),
        Ok(Value::String("physical/B.csv".into()))
    );
    assert_eq!(
        rows[0].clone().source_row_document_path(27, &["Rows"]),
        rows[0].source_row_document_path(27, &["Rows"])
    );
    assert_error(&rows[0], &["rows"], SourceRowPathError::InactiveFrame);
    let grouped = GroupedItems::by(
        rows.into_iter().map(|row| (row, Value::Int(1))).collect(),
        Some("Rows"),
    );
    let contexts = grouped.contexts();
    assert_error(&contexts[0], &["Rows"], SourceRowPathError::GroupedFrame);
    let members = contexts[0].walk_source(&[]);
    assert_eq!(
        members[0].source_row_document_path(27, &["Rows"]),
        Ok(Value::String(" physical/A.csv ".into()))
    );
    assert_eq!(
        members[1].source_row_document_path(27, &["Rows"]),
        Ok(Value::String("physical/B.csv".into()))
    );
}

#[test]
fn missing_inner_origin_never_borrows_the_outer_path_or_a_suffix_match() {
    let source = repeated([marked(
        group([field(
            "Children",
            repeated([group([]), marked(group([]), "child.csv", None)]),
        )]),
        "outer.csv",
        None,
    )]);
    let outer = ScopeContext::new(&source).walk_source(&[]);
    let children = outer[0].walk_source(&["Children"]);
    assert_error(
        &children[0],
        &["Children"],
        SourceRowPathError::MissingOrigin,
    );
    assert_eq!(
        children[0].source_row_document_path(27, &[]),
        Ok(Value::String("outer.csv".into()))
    );
    assert_eq!(
        children[1].source_row_document_path(27, &["Children"]),
        Ok(Value::String("child.csv".into()))
    );
    assert_error(
        &children[1],
        &["x", "Children"],
        SourceRowPathError::InactiveFrame,
    );
    let root = marked(group([]), "root.csv", None);
    assert_error(
        &ScopeContext::new(&root),
        &[],
        SourceRowPathError::InactiveFrame,
    );
}

#[test]
fn mutating_a_row_does_not_invent_or_preserve_unproved_origin() {
    let mut row = marked(group([]), "before.csv", None);
    let Instance::Group(fields) = &mut row else {
        unreachable!()
    };
    fields.push(("data".into(), Instance::Scalar(Value::Int(1))));
    let source = repeated([row]);
    let contexts = ScopeContext::new(&source).walk_source(&[]);
    assert_error(&contexts[0], &[], SourceRowPathError::MissingOrigin);
    let malformed = repeated([repeated([group([])])]);
    let contexts = ScopeContext::new(&malformed).walk_source(&[]);
    assert_error(&contexts[0], &[], SourceRowPathError::GroupedFrame);
}
