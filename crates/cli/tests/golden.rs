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

#[test]
fn xlsx_input_maps_to_xlsx_output_with_project_layout_options() {
    use ir::{Instance, Value};

    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut project: mapping::Project =
        serde_json::from_str(&std::fs::read_to_string(fixture_dir.join("project.json")).unwrap())
            .unwrap();
    project.source_options.xlsx_sheet = Some("People".into());
    project.source_options.xlsx_start_row = Some(3);
    project.source_options.xlsx_columns = vec![2, 4, 6];
    project.target_options.xlsx_sheet = Some("Results".into());
    project.target_options.xlsx_start_row = Some(2);
    project.target_options.xlsx_columns = vec![1, 3];

    let source_rows = vec![
        Instance::Group(vec![
            (
                "first_name".into(),
                Instance::Scalar(Value::String("Jane".into())),
            ),
            (
                "last_name".into(),
                Instance::Scalar(Value::String("Doe".into())),
            ),
            ("age".into(), Instance::Scalar(Value::Int(29))),
        ]),
        Instance::Group(vec![
            (
                "first_name".into(),
                Instance::Scalar(Value::String("John".into())),
            ),
            (
                "last_name".into(),
                Instance::Scalar(Value::String("Smith".into())),
            ),
            ("age".into(), Instance::Scalar(Value::Int(41))),
        ]),
    ];
    let expected = vec![
        Instance::Group(vec![
            (
                "full_name".into(),
                Instance::Scalar(Value::String("Jane Doe".into())),
            ),
            ("age_next_year".into(), Instance::Scalar(Value::Int(30))),
        ]),
        Instance::Group(vec![
            (
                "full_name".into(),
                Instance::Scalar(Value::String("John Smith".into())),
            ),
            ("age_next_year".into(), Instance::Scalar(Value::Int(42))),
        ]),
    ];

    let tag = format!("xlsx_{}", std::process::id());
    let project_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.json"));
    let input_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}_input.xlsx"));
    let output_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}_output.xlsx"));
    for path in [&project_path, &input_path, &output_path] {
        std::fs::remove_file(path).ok();
    }
    std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();
    format_xlsx::write(
        &input_path,
        &project.source,
        &source_rows,
        project.source_options.xlsx_sheet.as_deref(),
        project.source_options.xlsx_start_row.unwrap(),
        &project.source_options.xlsx_columns,
        true,
    )
    .unwrap();

    let written = cli::run_project(&project_path, &input_path, &output_path).unwrap();
    let actual = format_xlsx::read(
        &output_path,
        &project.target,
        project.target_options.xlsx_sheet.as_deref(),
        project.target_options.xlsx_start_row.unwrap(),
        &project.target_options.xlsx_columns,
        true,
    )
    .unwrap();
    for path in [project_path, input_path, output_path] {
        std::fs::remove_file(path).unwrap();
    }

    assert_eq!(written, 2);
    assert_eq!(actual, expected);
}

#[test]
fn transposed_xlsx_target_is_rejected_explicitly() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut project: mapping::Project =
        serde_json::from_str(&std::fs::read_to_string(fixture_dir.join("project.json")).unwrap())
            .unwrap();
    project.target_options.xlsx_rows = vec![1, 2];

    let tag = format!("xlsx_transposed_target_{}", std::process::id());
    let project_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.json"));
    let output_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.xlsx"));
    std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();

    let error =
        cli::run_project(&project_path, &fixture_dir.join("input.csv"), &output_path).unwrap_err();
    std::fs::remove_file(project_path).unwrap();
    std::fs::remove_file(output_path).ok();

    assert!(
        error
            .to_string()
            .contains("transposed XLSX output is not supported")
    );
}

#[test]
fn imported_transposed_xlsx_source_executes_to_csv() {
    use ir::{Instance, ScalarType, SchemaNode, Value};

    let design =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../mfd/tests/fixtures/xlsx-transposed.mfd");
    let imported = mfd::import(&design).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);

    let workbook_schema = SchemaNode::group(
        "physical-rows",
        vec![
            SchemaNode::scalar("A", ScalarType::String),
            SchemaNode::scalar("B", ScalarType::String),
            SchemaNode::scalar("C", ScalarType::String),
        ],
    );
    let populated = |left: &str, middle: &str, right: &str| {
        Instance::Group(vec![
            ("A".into(), Instance::Scalar(Value::String(left.into()))),
            ("B".into(), Instance::Scalar(Value::String(middle.into()))),
            ("C".into(), Instance::Scalar(Value::String(right.into()))),
        ])
    };
    let empty = || {
        Instance::Group(
            ["A", "B", "C"]
                .into_iter()
                .map(|field| (field.into(), Instance::Scalar(Value::Null)))
                .collect(),
        )
    };
    let rows = vec![
        populated("Header", "Food", "Travel"),
        empty(),
        empty(),
        empty(),
        populated("0", "12", "34"),
    ];

    let tag = format!("xlsx_transposed_source_{}", std::process::id());
    let project_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.json"));
    let input_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.xlsx"));
    let output_path = std::env::temp_dir().join(format!("ferrule_cli_{tag}.csv"));
    std::fs::write(
        &project_path,
        serde_json::to_vec(&imported.project).unwrap(),
    )
    .unwrap();
    format_xlsx::write(
        &input_path,
        &workbook_schema,
        &rows,
        Some("Quarterly"),
        3,
        &[],
        false,
    )
    .unwrap();

    let written = cli::run_project(&project_path, &input_path, &output_path).unwrap();
    let actual = std::fs::read_to_string(&output_path).unwrap();
    for path in [project_path, input_path, output_path] {
        std::fs::remove_file(path).unwrap();
    }

    assert_eq!(written, 3);
    assert_eq!(actual, "Category,Amount\nHeader,0\nFood,12\nTravel,34\n");
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

#[test]
fn composite_sqlite_input_iterates_a_selected_table() {
    use std::collections::BTreeMap;

    use ir::{Instance, ScalarType, SchemaNode, Value};
    use mapping::{Binding, FormatOptions, Graph, Node, Project, Scope, ScopeIteration};

    let table_schema = SchemaNode::group(
        "departments",
        vec![
            SchemaNode::scalar("id", ScalarType::Int),
            SchemaNode::scalar("name", ScalarType::String),
        ],
    )
    .repeating();
    let source_schema = SchemaNode::group("database", vec![table_schema.clone()]);
    let target_schema = SchemaNode::group(
        "row",
        vec![SchemaNode::scalar("department", ScalarType::String)],
    );
    let department = |id, name: &str| {
        Instance::Group(vec![
            ("id".into(), Instance::Scalar(Value::Int(id))),
            ("name".into(), Instance::Scalar(Value::String(name.into()))),
        ])
    };
    let db_path = std::env::temp_dir().join(format!(
        "ferrule_cli_composite_source_{}.db",
        std::process::id()
    ));
    let project_path = std::env::temp_dir().join(format!(
        "ferrule_cli_composite_source_{}.json",
        std::process::id()
    ));
    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_composite_source_{}.csv",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&project_path);
    let _ = std::fs::remove_file(&output_path);
    format_db::write(
        &db_path,
        &table_schema,
        &[department(1, "Engineering"), department(2, "Sales")],
    )
    .unwrap();

    let project = Project {
        source: source_schema,
        target: target_schema,
        source_path: None,
        target_path: None,
        source_options: FormatOptions::default(),
        target_options: FormatOptions::default(),
        extra_sources: Vec::new(),
        graph: Graph {
            nodes: BTreeMap::from([(
                0,
                Node::SourceField {
                    path: vec!["name".into()],
                    frame: None,
                },
            )]),
        },
        root: Scope {
            iteration: ScopeIteration::Source(vec!["departments".into()]),
            bindings: vec![Binding {
                target_field: "department".into(),
                node: 0,
            }],
            ..Scope::default()
        },
    };
    std::fs::write(&project_path, serde_json::to_vec(&project).unwrap()).unwrap();

    let rows = cli::run_project(&project_path, &db_path, &output_path).unwrap();
    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    std::fs::remove_file(&project_path).unwrap();
    std::fs::remove_file(&db_path).unwrap();

    assert_eq!(rows, 2);
    assert_eq!(actual, "department\nEngineering\nSales\n");
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

/// Stretch (EDI): an X12 850 purchase order flattened into CSV line items.
/// Exercises separator discovery from ISA, schema-guided loop matching
/// (repeating N1 and PO1/PID loops), typed elements, a two-level scope
/// source path (`Item/PID`), and three levels of broadcast: PO number from
/// the transaction header, line fields from PO1, description from PID.
#[test]
fn x12_purchase_order_flattens_into_csv() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edi");
    let project = dir.join("project.json");
    let input = dir.join("po850.edi");
    let expected = std::fs::read_to_string(dir.join("expected_po_lines.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_edi_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 3);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// EDI (EDIFACT): an ORDERS-style message flattened into CSV line items,
/// exercising the dialect dispatch (UNB trigger -> EDIFACT), composite
/// elements (LIN03 item, IMD03 description, QTY01/PRI01 components), and
/// the same broadcast-plus-multiply pattern as the X12 golden test.
#[test]
fn edifact_orders_flattens_into_csv() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edifact");
    let project = dir.join("project.json");
    let input = dir.join("orders.edifact");
    let expected = std::fs::read_to_string(dir.join("expected_lines.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_edifact_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 2);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// EDI lenient mode via project options: a HIPAA-style 837 claim where the
/// schema declares only the segments it binds (qualifier-anchored NM1s,
/// CLM, LX/SV3 service lines) and `source_options.lenient_segments` skips
/// everything else -- envelope, BHT, PER, addresses, TOO, trailers.
#[test]
fn lenient_x12_claim_flattens_into_csv() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/edi");
    let project = dir.join("project_claim.json");
    let input = dir.join("claim837.edi");
    let expected = std::fs::read_to_string(dir.join("expected_claim_lines.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_claim_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 2);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

/// Multi-source: CSV orders enriched against a JSON extra source declared
/// in the project (`extra_sources`, path relative to the project file),
/// joined per row by a `lookup` node. An order with no matching customer
/// gets an empty cell (the lookup resolves to Null).
#[test]
fn csv_orders_enriched_from_json_customers() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/enrich");
    let project = dir.join("project.json");
    let input = dir.join("orders.csv");
    let expected = std::fs::read_to_string(dir.join("expected_enriched.csv")).unwrap();

    let output_path = std::env::temp_dir().join(format!(
        "ferrule_cli_golden_test_enrich_{}.csv",
        std::process::id()
    ));

    let rows = cli::run_project(&project, &input, &output_path).unwrap();
    assert_eq!(rows, 3);

    let actual = std::fs::read_to_string(&output_path).unwrap();
    std::fs::remove_file(&output_path).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn validates_projects_before_reading_input_data() {
    let fixture_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let valid_path = fixture_dir.join("project.json");
    assert!(cli::validate_project(&valid_path).unwrap().is_empty());

    let text = std::fs::read_to_string(&valid_path).unwrap();
    let mut project: mapping::Project = serde_json::from_str(&text).unwrap();
    project.graph.nodes.insert(
        999,
        mapping::Node::Call {
            function: "not_a_builtin".into(),
            args: vec![12345],
        },
    );
    let invalid_path = std::env::temp_dir().join(format!(
        "ferrule_cli_invalid_project_{}.json",
        std::process::id()
    ));
    std::fs::write(&invalid_path, serde_json::to_string(&project).unwrap()).unwrap();

    let issues = cli::validate_project(&invalid_path).unwrap();
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("unknown function"))
    );
    assert!(
        issues
            .iter()
            .any(|issue| issue.message.contains("missing node 12345"))
    );
    let output = std::env::temp_dir().join(format!(
        "ferrule_cli_invalid_output_{}.csv",
        std::process::id()
    ));
    let error = cli::run_project(&invalid_path, Path::new("does-not-exist.csv"), &output)
        .unwrap_err()
        .to_string();
    std::fs::remove_file(invalid_path).unwrap();
    assert!(error.contains("project validation failed"));
    assert!(!output.exists());
}
