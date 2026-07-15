use std::path::{Path, PathBuf};

use ir::{Instance, Value};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_runtime_string_parser_and_executes_each_repeated_input() {
    let imported = mfd::import(&fixture("flextext-string-parser.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert!(engine::validate(&imported.project).is_empty());

    let source = Instance::Group(vec![(
        "Line".into(),
        Instance::Repeated(vec![
            Instance::Scalar(Value::String("Ada*#*3".into())),
            Instance::Scalar(Value::String("Grace*#*5".into())),
        ]),
    )]);
    let output = engine::run(&imported.project, &source).unwrap();
    let rows = output.field("Row").and_then(Instance::as_repeated).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        rows[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("Ada".into()))
    );
    assert_eq!(
        rows[1].field("Count").and_then(Instance::as_scalar),
        Some(&Value::Int(5))
    );
}
