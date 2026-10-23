use ir::{Instance, Value};
use std::path::PathBuf;

fn group(fields: Vec<(&str, Instance)>) -> Instance {
    Instance::Group(
        fields
            .into_iter()
            .map(|(n, v)| (n.to_string(), v))
            .collect::<Vec<_>>()
            .into(),
    )
}
fn repeated(items: Vec<Instance>) -> Instance {
    Instance::Repeated(items)
}
fn text(value: &str) -> Instance {
    Instance::Scalar(Value::String(value.into()))
}
fn field(values: &[&str]) -> Instance {
    repeated(
        values
            .iter()
            .map(|v| group(vec![("Value", text(v))]))
            .collect(),
    )
}
fn row(name: &[&str], qty: &[&str]) -> Instance {
    group(vec![("name", field(name)), ("qty", field(qty))])
}
fn document(name: &str, rows: Vec<Instance>) -> Instance {
    group(vec![
        ("Path", text(name)),
        (
            "Root",
            repeated(vec![group(vec![(
                "rows",
                repeated(vec![group(vec![("Items", repeated(rows))])]),
            )])]),
        ),
    ])
}
fn project() -> mfd::ImportedJsonEdiDocument {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/json_edi_documents");
    mfd::import_json_edi_document_with_options(
        &root.join("mapping.mfd"),
        &mfd::ImportOptions::default().with_package_root(&root),
    )
    .unwrap()
}
#[test]
fn repeated_documents_and_rows_never_fall_back_to_a_previous_owner() {
    let imported = project();
    let source = group(vec![(
        "Documents",
        repeated(vec![
            document(
                "one",
                vec![row(&["", "A"], &["1.50"]), row(&["B"], &["2.50"])],
            ),
            document("two", vec![row(&["C"], &["3"])]),
        ]),
    )]);
    let value = engine::run(imported.document.project(), &source).unwrap();
    let interchanges = value.field("Interchange").unwrap().as_repeated().unwrap();
    assert_eq!(interchanges.len(), 2);
    let lines = |doc: usize| {
        interchanges[doc]
            .field("Group")
            .unwrap()
            .as_repeated()
            .unwrap()[0]
            .field("Message_850")
            .unwrap()
            .as_repeated()
            .unwrap()[0]
            .field("Lines")
            .unwrap()
            .as_repeated()
            .unwrap()
    };
    assert_eq!(lines(0).len(), 2);
    assert_eq!(lines(1).len(), 1);
    for (line, expected) in [
        (&lines(0)[0], "A"),
        (&lines(0)[1], "B"),
        (&lines(1)[0], "C"),
    ] {
        assert_eq!(
            line.field("LIN").unwrap().field("F1"),
            Some(&text(expected))
        );
    }
    let filename = engine::run(&imported.output_filename, &group(vec![])).unwrap();
    assert_eq!(filename.field("Path"), Some(&text("out.edi")));
}
#[test]
fn a_later_numeric_candidate_is_evaluated_before_first_value_selection() {
    let imported = project();
    let source = group(vec![(
        "Documents",
        repeated(vec![document("one", vec![row(&["A"], &["1", "1e999"])])]),
    )]);
    assert!(engine::run(imported.document.project(), &source).is_err());
}
