use std::path::{Path, PathBuf};

use ir::{Instance, ScalarType, SchemaKind, Value};
use mapping::{AggregateOp, Node};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

#[test]
fn imports_static_flat_xlsx_table_with_sparse_columns() {
    let imported = mfd::import(&fixture("xlsx-flat.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source_path.as_deref(), Some("scores.xlsx"));
    assert_eq!(project.source_options.xlsx_sheet.as_deref(), Some("Data"));
    assert_eq!(project.source_options.xlsx_start_row, Some(2));
    assert_eq!(project.source_options.xlsx_columns, vec![2, 4]);
    assert_eq!(project.source_options.has_header_row, Some(true));
    assert!(matches!(
        project.source.child("Name").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
    assert!(matches!(
        project.source.child("Age").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));

    let person = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Person")
        .unwrap();
    assert_eq!(person.source().map(|path| path.to_vec()), Some(Vec::new()));
    assert!(person.bindings.iter().any(|binding| {
        binding.target_field == "Name"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Name"]
            )
    }));
    assert!(person.bindings.iter().any(|binding| {
        binding.target_field == "Age"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Age"]
            )
    }));
    assert!(engine::validate(project).is_empty());
}

#[test]
fn imports_fixed_rows_with_open_cells_as_a_transposed_table() {
    let imported = mfd::import(&fixture("xlsx-transposed.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source_path.as_deref(), Some("ledger.xlsx"));
    assert_eq!(
        project.source_options.xlsx_sheet.as_deref(),
        Some("Quarterly")
    );
    assert_eq!(project.source_options.xlsx_rows, vec![3, 7]);
    assert!(project.source_options.xlsx_columns.is_empty());
    assert!(project.source_options.xlsx_start_row.is_none());
    assert_eq!(project.source_options.has_header_row, Some(false));
    assert!(matches!(
        project.source.child("Category").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
    assert!(matches!(
        project.source.child("Range9").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    assert!(matches!(
        project.source.child("n").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));

    assert_eq!(project.root.source(), Some(&[][..]));
    assert!(project.root.bindings.iter().any(|binding| {
        binding.target_field == "Category"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Category"]
            )
    }));
    let amount = project
        .root
        .bindings
        .iter()
        .find(|binding| binding.target_field == "Amount")
        .unwrap();
    let argument = match project.graph.nodes.get(&amount.node) {
        Some(Node::Aggregate {
            function: AggregateOp::ItemAt,
            collection,
            value,
            arg: Some(argument),
            ..
        }) if collection.is_empty() && value == &["Range9"] => *argument,
        other => panic!("unexpected Amount expression: {other:?}"),
    };
    assert!(matches!(
        project.graph.nodes.get(&argument),
        Some(Node::SourceField { path, .. }) if path == &["n"]
    ));
    assert!(engine::validate(project).is_empty());
}

#[test]
fn imports_fixed_record_and_table_as_a_composite_xml_source() {
    let imported = mfd::import(&fixture("xlsx-composite-xml.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source_path.as_deref(), Some("branches.xlsx"));
    assert!(project.source_options.xlsx_sheet.is_none());
    assert!(project.source_options.xlsx_start_row.is_none());
    assert!(project.source_options.xlsx_columns.is_empty());
    assert!(project.source_options.xlsx_rows.is_empty());
    assert!(project.source_options.has_header_row.is_none());
    let layout = project.source_options.xlsx_composite.as_ref().unwrap();
    assert_eq!(layout.table.path, ["Roster"]);
    assert_eq!(layout.table.sheet.as_deref(), Some("Roster"));
    assert_eq!(layout.table.start_row.get(), 1);
    assert_eq!(
        layout
            .table
            .columns
            .iter()
            .map(|column| column.get())
            .collect::<Vec<_>>(),
        vec![1, 3]
    );
    assert!(layout.table.has_header);
    assert_eq!(layout.records.len(), 1);
    assert_eq!(layout.records[0].path, ["Branch"]);
    assert_eq!(layout.records[0].sheet.as_deref(), Some("Branch"));
    assert_eq!(
        layout.records[0]
            .cells
            .iter()
            .map(|cell| {
                (
                    cell.path.first().map(String::as_str),
                    cell.row.get(),
                    cell.column.get(),
                )
            })
            .collect::<Vec<_>>(),
        vec![(Some("Name"), 2, 4), (Some("City"), 4, 4)]
    );

    let branch_schema = project.source.child("Branch").unwrap();
    assert!(!branch_schema.repeating);
    assert!(matches!(branch_schema.kind, SchemaKind::Group { .. }));
    let roster_schema = project.source.child("Roster").unwrap();
    assert!(roster_schema.repeating);
    assert!(matches!(roster_schema.kind, SchemaKind::Group { .. }));

    let branch = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Branch")
        .unwrap();
    assert_eq!(branch.source(), Some(&[][..]));
    assert!(branch.bindings.iter().any(|binding| {
        binding.target_field == "Name"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Branch", "Name"]
            )
    }));
    let member = branch
        .children
        .iter()
        .find(|scope| scope.target_field == "Member")
        .unwrap();
    assert!(member.source().is_some_and(|path| path == ["Roster"]));
    assert!(member.bindings.iter().any(|binding| {
        binding.target_field == "First"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, frame })
                    if path == &["First"]
                        && frame.as_deref().is_some_and(|path| path == ["Roster"])
            )
    }));
    assert!(engine::validate(project).is_empty());

    let source = Instance::Group(vec![
        (
            "Branch".into(),
            Instance::Group(vec![
                (
                    "Name".into(),
                    Instance::Scalar(Value::String("North".into())),
                ),
                (
                    "City".into(),
                    Instance::Scalar(Value::String("Seattle".into())),
                ),
            ]),
        ),
        (
            "Roster".into(),
            Instance::Repeated(vec![
                Instance::Group(vec![
                    (
                        "First".into(),
                        Instance::Scalar(Value::String("Ada".into())),
                    ),
                    (
                        "Team".into(),
                        Instance::Scalar(Value::String("Platform".into())),
                    ),
                ]),
                Instance::Group(vec![
                    (
                        "First".into(),
                        Instance::Scalar(Value::String("Lin".into())),
                    ),
                    (
                        "Team".into(),
                        Instance::Scalar(Value::String("Data".into())),
                    ),
                ]),
            ]),
        ),
    ]);
    let output = engine::run(project, &source).unwrap();
    let branches = output
        .field("Branch")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(branches.len(), 1);
    assert_eq!(
        branches[0].field("Name").and_then(Instance::as_scalar),
        Some(&Value::String("North".into()))
    );
    let members = branches[0]
        .field("Member")
        .and_then(Instance::as_repeated)
        .unwrap();
    assert_eq!(members.len(), 2);
    assert_eq!(
        members[1].field("First").and_then(Instance::as_scalar),
        Some(&Value::String("Lin".into()))
    );
}

#[test]
fn imports_fixed_scalar_and_table_as_a_composite_json_source() {
    let imported = mfd::import(&fixture("xlsx-composite-json.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    let layout = project.source_options.xlsx_composite.as_ref().unwrap();
    assert_eq!(layout.table.path, ["People"]);
    assert_eq!(layout.table.sheet.as_deref(), Some("People"));
    assert_eq!(layout.table.start_row.get(), 2);
    assert_eq!(
        layout
            .table
            .columns
            .iter()
            .map(|column| column.get())
            .collect::<Vec<_>>(),
        vec![2, 5]
    );
    assert_eq!(layout.records.len(), 1);
    assert_eq!(layout.records[0].path, ["Info"]);
    assert_eq!(layout.records[0].cells[0].path, ["Organization"]);
    assert_eq!(layout.records[0].cells[0].row.get(), 3);
    assert_eq!(layout.records[0].cells[0].column.get(), 2);

    assert!(project.root.bindings.iter().any(|binding| {
        binding.target_field == "Organization"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, frame: None })
                    if path == &["Info", "Organization"]
            )
    }));
    let people = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "People")
        .unwrap();
    assert!(people.source().is_some_and(|path| path == ["People"]));
    assert!(people.bindings.iter().any(|binding| {
        binding.target_field == "Age"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, frame })
                    if path == &["Age"]
                        && frame.as_deref().is_some_and(|path| path == ["People"])
            )
    }));
    assert!(engine::validate(project).is_empty());
}

#[test]
fn imports_header_driven_nested_worksheet_grid() {
    let imported = mfd::import(&fixture("xlsx-grid.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let project = &imported.project;

    assert_eq!(project.source_path.as_deref(), Some("quarterly-grid.xlsx"));
    assert!(project.source_options.xlsx_rows.is_empty());
    assert!(project.source_options.xlsx_composite.is_none());
    let layout = project.source_options.xlsx_grid.as_ref().unwrap();
    assert_eq!(layout.sheet.as_deref(), Some("Quarterly"));
    assert_eq!(layout.header_row.get(), 1);
    assert_eq!(layout.data_start_row.get(), 2);
    assert_eq!(layout.header_value_field, "Range1");
    assert_eq!(layout.header_position_field, "HeaderColumn");
    assert_eq!(layout.rows_field, "Rows");
    assert_eq!(layout.cells_field, "Cells");
    assert_eq!(layout.cell_value_field, "value");
    assert_eq!(layout.cell_position_field, "CellColumn");
    assert_eq!(layout.fixed_cells.len(), 1);
    assert_eq!(layout.fixed_cells[0].path, ["Year"]);
    assert_eq!(layout.fixed_cells[0].row.get(), 1);
    assert_eq!(layout.fixed_cells[0].column.get(), 1);

    assert!(matches!(
        project.source.child("Range1").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::String
        })
    ));
    assert!(matches!(
        project.source.child("HeaderColumn").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Int
        })
    ));
    let rows = project.source.child("Rows").unwrap();
    assert!(rows.repeating);
    let cells = rows.child("Cells").unwrap();
    assert!(cells.repeating);
    assert!(matches!(
        cells.child("value").map(|node| &node.kind),
        Some(SchemaKind::Scalar {
            ty: ScalarType::Float
        })
    ));

    let period = project
        .root
        .children
        .iter()
        .find(|scope| scope.target_field == "Period")
        .unwrap();
    assert_eq!(period.source(), Some(&[][..]));
    assert!(period.filter.is_some());
    assert!(period.bindings.iter().any(|binding| {
        binding.target_field == "Month"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["Range1"]
            )
    }));
    assert!(period.bindings.iter().any(|binding| {
        binding.target_field == "Column"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::SourceField { path, .. }) if path == &["HeaderColumn"]
            )
    }));
    let sale = period
        .children
        .iter()
        .find(|scope| scope.target_field == "Sale")
        .unwrap();
    assert_eq!(sale.source(), Some(&["Rows".to_string()][..]));
    assert!(sale.bindings.iter().any(|binding| {
        binding.target_field == "Region"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::Aggregate {
                    function: AggregateOp::ItemAt,
                    collection,
                    value,
                    ..
                }) if collection == &["Cells"] && value == &["value"]
            )
    }));
    assert!(sale.bindings.iter().any(|binding| {
        binding.target_field == "Amount"
            && matches!(
                project.graph.nodes.get(&binding.node),
                Some(Node::Lookup {
                    collection,
                    key,
                    value,
                    ..
                }) if collection == &["Cells"]
                    && key == &["CellColumn"]
                    && value == &["value"]
            )
    }));
    assert!(engine::validate(project).is_empty());
}

#[test]
fn imported_worksheet_grid_executes_header_and_nested_cell_frames() {
    use ir::{Instance, Value};

    let imported = mfd::import(&fixture("xlsx-grid.mfd")).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let layout = imported.project.source_options.xlsx_grid.as_ref().unwrap();

    let mut workbook = rust_xlsxwriter::Workbook::new();
    let worksheet = workbook.add_worksheet();
    worksheet.set_name("Quarterly").unwrap();
    for (row, column, value) in [
        (0, 0, "2026"),
        (0, 1, "Q1"),
        (0, 2, "Q2"),
        (1, 0, "101"),
        (1, 1, "10.5"),
        (1, 2, "20.5"),
        (2, 0, "202"),
        (2, 1, "30.5"),
        (2, 2, "40.5"),
    ] {
        worksheet.write_string(row, column, value).unwrap();
    }
    let records = format_xlsx::from_bytes_grid(
        &workbook.save_to_buffer().unwrap(),
        &imported.project.source,
        layout,
    )
    .unwrap();
    let actual = engine::run(&imported.project, &Instance::Repeated(records)).unwrap();

    let scalar = |value| Instance::Scalar(value);
    let sale = |region: f64, amount: f64| {
        Instance::Group(vec![
            ("Region".into(), scalar(Value::Float(region))),
            ("Amount".into(), scalar(Value::Float(amount))),
        ])
    };
    let period = |month: &str, column: i64, first: f64, second: f64| {
        Instance::Group(vec![
            ("Month".into(), scalar(Value::String(month.into()))),
            ("Column".into(), scalar(Value::Int(column))),
            ("Year".into(), scalar(Value::String("2026".into()))),
            (
                "Sale".into(),
                Instance::Repeated(vec![sale(101.0, first), sale(202.0, second)]),
            ),
        ])
    };
    assert_eq!(
        actual,
        Instance::Group(vec![(
            "Period".into(),
            Instance::Repeated(vec![
                period("Q1", 2, 10.5, 30.5),
                period("Q2", 3, 20.5, 40.5),
            ]),
        )])
    );
}
