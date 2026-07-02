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
