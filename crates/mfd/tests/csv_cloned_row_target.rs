use std::path::PathBuf;

use ir::{Instance, Value};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn item(name: &str) -> Instance {
    Instance::Group(vec![(
        "Name".into(),
        Instance::Scalar(Value::String(name.into())),
    )])
}

#[test]
fn cloned_csv_row_block_preserves_iteration_and_reducer_bindings() {
    let imported = mfd::import(&fixture("csv-cloned-row-block.mfd")).unwrap();
    assert_eq!(imported.warnings.len(), 1, "{:?}", imported.warnings);
    assert!(
        imported.warnings[0].contains(
            "singleton rows were skipped because ferrule CSV targets represent one repeated row shape"
        ),
        "{:?}",
        imported.warnings
    );
    assert!(engine::validate(&imported.project).is_empty());
    assert_eq!(imported.project.root.source, Some(vec!["Item".into()]));

    let source = Instance::Group(vec![(
        "Item".into(),
        Instance::Repeated(vec![item("Alpha"), item("Beta")]),
    )]);
    let target = engine::run(&imported.project, &source).unwrap();
    let rows = target.as_repeated().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Alpha".into()))
    );
    assert_eq!(
        rows[1].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Beta".into()))
    );
    assert!(
        rows.iter().all(|row| {
            row.field("Total").and_then(Instance::as_scalar) == Some(&Value::Int(2))
        })
    );
}
