use ir::{Instance, ScalarType, SchemaNode, Value};
use mapping::{
    XlsxColumn, XlsxCompositeLayout, XlsxFixedCell, XlsxGridLayout, XlsxGridRegion, XlsxRow,
    XlsxTableRegion,
};
use rust_xlsxwriter::Workbook;

use super::{from_bytes_composite, validate_composite_layout};
use crate::XlsxFormatError;

fn row(value: u32) -> XlsxRow {
    XlsxRow::new(value).unwrap()
}

fn column(value: u32) -> XlsxColumn {
    XlsxColumn::new(value).unwrap()
}

fn grid_group(name: &str, fixed: &str) -> SchemaNode {
    SchemaNode::group(
        name,
        vec![
            SchemaNode::scalar("Header", ScalarType::String),
            SchemaNode::scalar("HeaderColumn", ScalarType::Int),
            SchemaNode::scalar(fixed, ScalarType::String),
            SchemaNode::group(
                "Rows",
                vec![
                    SchemaNode::group(
                        "Cells",
                        vec![
                            SchemaNode::scalar("value", ScalarType::String),
                            SchemaNode::scalar("CellColumn", ScalarType::Int),
                        ],
                    )
                    .repeating(),
                ],
            )
            .repeating(),
        ],
    )
    .repeating()
}

fn schema() -> SchemaNode {
    SchemaNode::group(
        "Workbook",
        vec![
            grid_group("Quarterly", "Year"),
            grid_group("Annual", "Unit"),
            SchemaNode::group(
                "Staff",
                vec![SchemaNode::scalar("Name", ScalarType::String)],
            )
            .repeating(),
        ],
    )
}

fn grid(
    sheet: &str,
    header_row: u32,
    data_start_row: u32,
    fixed: &str,
    fixed_at: (u32, u32),
) -> XlsxGridRegion {
    XlsxGridRegion {
        path: vec![sheet.into()],
        layout: XlsxGridLayout {
            sheet: Some(sheet.into()),
            header_row: row(header_row),
            data_start_row: row(data_start_row),
            header_value_field: "Header".into(),
            header_position_field: "HeaderColumn".into(),
            rows_field: "Rows".into(),
            cells_field: "Cells".into(),
            cell_value_field: "value".into(),
            cell_position_field: "CellColumn".into(),
            fixed_cells: vec![XlsxFixedCell {
                path: vec![fixed.into()],
                row: row(fixed_at.0),
                column: column(fixed_at.1),
            }],
        },
    }
}

fn layout() -> XlsxCompositeLayout {
    XlsxCompositeLayout {
        table: Some(XlsxTableRegion {
            path: vec!["Staff".into()],
            sheet: Some("Staff".into()),
            start_row: row(1),
            columns: vec![column(1)],
            has_header: true,
            row_number_field: None,
        }),
        additional_tables: Vec::new(),
        records: Vec::new(),
        grids: vec![
            grid("Quarterly", 1, 2, "Year", (1, 1)),
            grid("Annual", 3, 5, "Unit", (1, 2)),
        ],
    }
}

fn workbook() -> Vec<u8> {
    let mut workbook = Workbook::new();
    let quarterly = workbook.add_worksheet();
    quarterly.set_name("Quarterly").unwrap();
    for (row, column, value) in [
        (0, 0, "2026"),
        (0, 1, "Q1"),
        (0, 2, "Q2"),
        (1, 0, "West"),
        (1, 1, "10"),
        (1, 2, "20"),
    ] {
        quarterly.write_string(row, column, value).unwrap();
    }
    let annual = workbook.add_worksheet();
    annual.set_name("Annual").unwrap();
    for (row, column, value) in [
        (0, 1, "USD"),
        (2, 2, "FY"),
        (4, 0, "North"),
        (4, 2, "99"),
        (5, 0, "South"),
        (5, 2, "77"),
    ] {
        annual.write_string(row, column, value).unwrap();
    }
    let staff = workbook.add_worksheet();
    staff.set_name("Staff").unwrap();
    staff.write_string(0, 0, "Name").unwrap();
    staff.write_string(1, 0, "Ada").unwrap();
    workbook.save_to_buffer().unwrap()
}

fn text(value: &str) -> Instance {
    if value.is_empty() {
        Instance::Scalar(Value::Null)
    } else {
        Instance::Scalar(Value::String(value.into()))
    }
}

fn cells(values: &[&str]) -> Instance {
    Instance::Group(
        (vec![(
            "Cells".into(),
            Instance::Repeated(
                values
                    .iter()
                    .enumerate()
                    .map(|(index, value)| {
                        Instance::Group(
                            (vec![
                                ("value".into(), text(value)),
                                (
                                    "CellColumn".into(),
                                    Instance::Scalar(Value::Int(index as i64 + 1)),
                                ),
                            ])
                            .into(),
                        )
                    })
                    .collect(),
            ),
        )])
        .into(),
    )
}

fn record(header: &str, position: i64, fixed: (&str, &str), rows: &[Instance]) -> Instance {
    Instance::Group(
        (vec![
            ("Header".into(), text(header)),
            (
                "HeaderColumn".into(),
                Instance::Scalar(Value::Int(position)),
            ),
            (fixed.0.into(), text(fixed.1)),
            ("Rows".into(), Instance::Repeated(rows.to_vec())),
        ])
        .into(),
    )
}

#[test]
fn composite_reads_each_grid_with_its_own_rows_and_fixed_cells_beside_a_table() {
    let quarterly_rows = [cells(&["West", "10", "20"])];
    let annual_rows = [cells(&["North", "", "99"]), cells(&["South", "", "77"])];
    let expected = Instance::Group(
        (vec![
            (
                "Quarterly".into(),
                Instance::Repeated(vec![
                    record("2026", 1, ("Year", "2026"), &quarterly_rows),
                    record("Q1", 2, ("Year", "2026"), &quarterly_rows),
                    record("Q2", 3, ("Year", "2026"), &quarterly_rows),
                ]),
            ),
            (
                "Annual".into(),
                Instance::Repeated(vec![record("FY", 3, ("Unit", "USD"), &annual_rows)]),
            ),
            (
                "Staff".into(),
                Instance::Repeated(vec![Instance::Group(
                    (vec![("Name".into(), text("Ada"))]).into(),
                )]),
            ),
        ])
        .into(),
    );

    assert_eq!(
        from_bytes_composite(&workbook(), &schema(), &layout()).unwrap(),
        expected
    );

    let mut grids_only = layout();
    grids_only.table = None;
    let actual = from_bytes_composite(&workbook(), &schema(), &grids_only).unwrap();
    let Instance::Group(fields) = actual else {
        panic!("composite root is a group");
    };
    assert_eq!(fields[2], ("Staff".into(), Instance::Repeated(Vec::new())));
}

#[test]
fn composite_grid_regions_refuse_shared_worksheets_and_invalid_shapes() {
    let mut shared = layout();
    shared.grids[1].layout.sheet = Some("Quarterly".into());
    assert!(matches!(
        validate_composite_layout(&schema(), &shared),
        Err(XlsxFormatError::CompositePath { reason, .. })
            if reason == "several grids cannot share one worksheet"
    ));

    // An omitted sheet resolves to the first worksheet, which another grid names.
    let mut defaulted = layout();
    defaulted.grids[1].layout.sheet = None;
    assert!(matches!(
        from_bytes_composite(&workbook(), &schema(), &defaulted),
        Err(XlsxFormatError::CompositePath { reason, .. })
            if reason == "several grids cannot share one worksheet"
    ));

    let mut over_table = layout();
    over_table.grids[0].path = vec!["Staff".into()];
    assert!(matches!(
        validate_composite_layout(&schema(), &over_table),
        Err(XlsxFormatError::DuplicateCompositePath(path)) if path == "Staff"
    ));

    let mut not_repeating = schema();
    let ir::SchemaKind::Group { children, .. } = &mut not_repeating.kind else {
        unreachable!()
    };
    children[0].repeating = false;
    assert!(matches!(
        validate_composite_layout(&not_repeating, &layout()),
        Err(XlsxFormatError::GridLayout(_))
    ));

    let mut bad_field = layout();
    bad_field.grids[1].layout.header_position_field = "Missing".into();
    assert!(matches!(
        validate_composite_layout(&schema(), &bad_field),
        Err(XlsxFormatError::GridField { .. })
    ));

    let mut empty = layout();
    empty.table = None;
    empty.grids.clear();
    assert!(matches!(
        validate_composite_layout(&schema(), &empty),
        Err(XlsxFormatError::CompositeLayout(_))
    ));
}
