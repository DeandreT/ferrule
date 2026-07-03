use std::path::Path;

#[test]
fn simple_name_and_age_mapping() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let project = dir.join("project.json");
    let input = dir.join("input.csv");
    let expected = std::fs::read_to_string(dir.join("expected_output.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 2);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// Flattens a real-world nested XML document (Orders -> repeating Order ->
/// repeating Item) into a flat CSV of order lines, broadcasting the
/// enclosing Order's fields (Order_ID, Cust_Name) into every Item row and
/// applying a function (upper) along the way. This is the "hard part" of
/// Milestone 3: nested repeating-element mapping plus cross-level joins.
#[test]
fn nested_xml_flattens_into_csv_with_broadcast_fields() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/orders");
    let project = dir.join("project.json");
    let input = dir.join("Orders.xml");
    let expected = std::fs::read_to_string(dir.join("expected_order_lines.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_orders_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 6);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// Milestone 4: a `Scope.filter` drops rows (minors) while an `If` node
/// categorizes the rest by age, exercising the function library's
/// comparison functions along the way.
#[test]
fn filter_and_conditional_categorize_adults() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/people");
    let project = dir.join("project.json");
    let input = dir.join("people.csv");
    let expected = std::fs::read_to_string(dir.join("expected_adults.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_people_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 3);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// Milestone 6 (JSON input): the same filter/conditional mapping fed from a
/// JSON array (source schema marked `repeating`) must produce byte-identical
/// CSV to the CSV-input variant above.
#[test]
fn json_input_produces_the_same_adults_csv() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/people");
    let project = dir.join("project_json.json");
    let input = dir.join("people.json");
    let expected = std::fs::read_to_string(dir.join("expected_adults.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_people_json_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 3);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// Milestone 6 (JSON output): the nested-XML-to-flat-rows orders mapping,
/// unchanged, written as a JSON array of objects instead of CSV.
#[test]
fn nested_xml_flattens_into_json() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/orders");
    let project = dir.join("project.json");
    let input = dir.join("Orders.xml");
    let expected = std::fs::read_to_string(dir.join("expected_order_lines.json")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_orders_json_{}.json",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 6);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// Milestone 6 (SQLite input): the people mapping unchanged, fed from a
/// SQLite table instead of CSV. The table is named `row` because that's the
/// project's source schema root name -- the convention the CLI uses to pick
/// the table. Must produce byte-identical CSV to the CSV-input variant.
#[test]
fn sqlite_input_produces_the_same_adults_csv() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/people");
    let project = dir.join("project.json");
    let expected = std::fs::read_to_string(dir.join("expected_adults.csv")).unwrap();

    let project_json = std::fs::read_to_string(&project).unwrap();
    let parsed: mapping::Project = serde_json::from_str(&project_json).unwrap();

    let db_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_people_{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db_path);
    let person = |name: &str, age: i64| {
        ir::Instance::Group(vec![
            (
                "name".into(),
                ir::Instance::Scalar(ir::Value::String(name.into())),
            ),
            ("age".into(), ir::Instance::Scalar(ir::Value::Int(age))),
        ])
    };
    format_db::write(
        &db_path,
        &parsed.source,
        &[
            person("Jane", 29),
            person("John", 41),
            person("Mary", 65),
            person("Bob", 17),
        ],
    )
    .unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_people_db_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &db_path, &output_path).unwrap();
    assert_eq!(rows, 3);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    std::fs::remove_file(&db_path).unwrap();
    assert_eq!(actual, expected);
}

/// Milestone 6 (SQLite output): the orders flattening written into a SQLite
/// table, then read back through format-db and checked row by row against
/// the JSON golden fixture.
#[test]
fn nested_xml_flattens_into_sqlite() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/orders");
    let project = dir.join("project.json");
    let input = dir.join("Orders.xml");

    let project_json = std::fs::read_to_string(&project).unwrap();
    let parsed: mapping::Project = serde_json::from_str(&project_json).unwrap();

    let db_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_orders_{}.db",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db_path);

    let rows = cli::run_project(&project, &input, &db_path).unwrap();
    assert_eq!(rows, 6);

    let read_back = format_db::read(&db_path, &parsed.target).unwrap();
    std::fs::remove_file(&db_path).unwrap();

    let expected: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dir.join("expected_order_lines.json")).unwrap(),
    )
    .unwrap();
    let expected_rows = expected.as_array().unwrap();
    assert_eq!(read_back.len(), expected_rows.len());
    for (row, expected_row) in read_back.iter().zip(expected_rows) {
        for (field, value) in expected_row.as_object().unwrap() {
            let actual = row.field(field).and_then(ir::Instance::as_scalar).unwrap();
            let matches = match (actual, value) {
                (ir::Value::String(a), serde_json::Value::String(e)) => a == e,
                (ir::Value::Int(a), serde_json::Value::Number(e)) => Some(*a) == e.as_i64(),
                (ir::Value::Float(a), serde_json::Value::Number(e)) => Some(*a) == e.as_f64(),
                _ => false,
            };
            assert!(matches, "field `{field}`: {actual:?} != {value}");
        }
    }
}
