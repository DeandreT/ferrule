use std::path::Path;

use calamine::{Data, Range};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::XlsxGridLayout;

use super::{XlsxFormatError, parse_cell, worksheet_range};

struct GridSchema<'a> {
    header_value_type: ScalarType,
    rows: &'a SchemaNode,
    cells: &'a SchemaNode,
    cell_value_type: ScalarType,
}

/// Reads a two-dimensional worksheet grid into one root record per non-empty
/// header cell.
pub fn read_grid(
    path: &Path,
    schema: &SchemaNode,
    layout: &XlsxGridLayout,
) -> Result<Vec<Instance>, XlsxFormatError> {
    let bytes = std::fs::read(path)?;
    from_bytes_grid(&bytes, schema, layout)
}

/// Reads a two-dimensional worksheet grid from an in-memory workbook.
pub fn from_bytes_grid(
    bytes: &[u8],
    schema: &SchemaNode,
    layout: &XlsxGridLayout,
) -> Result<Vec<Instance>, XlsxFormatError> {
    let grid = validate_grid_layout(schema, layout)?;
    let range = worksheet_range(bytes, layout.sheet.as_deref())?;
    let Some((last_row, last_column)) = range.end() else {
        return Ok(Vec::new());
    };
    let header_row = layout.header_row.get() - 1;
    if header_row > last_row {
        return Ok(Vec::new());
    }

    let mut root_template = empty_group_instance(schema)?;
    materialize_fixed_cells(&mut root_template, schema, layout, &range)?;
    let rows = materialize_rows(&range, layout, &grid, last_row, last_column)?;

    let mut records = Vec::new();
    for column in 0..=last_column {
        let cell = range
            .get_value((header_row, column))
            .unwrap_or(&Data::Empty);
        if is_empty_cell(cell) {
            continue;
        }
        let header = parse_cell(
            cell,
            grid.header_value_type,
            layout.header_row.get(),
            &layout.header_value_field,
        )?;
        let mut record = root_template.clone();
        replace_direct(
            &mut record,
            &layout.header_value_field,
            Instance::Scalar(header),
        )?;
        replace_direct(
            &mut record,
            &layout.header_position_field,
            Instance::Scalar(Value::Int(i64::from(column + 1))),
        )?;
        replace_direct(
            &mut record,
            &layout.rows_field,
            Instance::Repeated(rows.clone()),
        )?;
        records.push(record);
    }
    Ok(records)
}

fn validate_grid_layout<'a>(
    schema: &'a SchemaNode,
    layout: &XlsxGridLayout,
) -> Result<GridSchema<'a>, XlsxFormatError> {
    if schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err(XlsxFormatError::GridRootSchema);
    }
    if layout.data_start_row.get() <= layout.header_row.get() {
        return Err(XlsxFormatError::GridLayout(
            "data_start_row must be after header_row",
        ));
    }
    for name in [
        &layout.header_value_field,
        &layout.header_position_field,
        &layout.rows_field,
        &layout.cells_field,
        &layout.cell_value_field,
        &layout.cell_position_field,
    ] {
        if name.trim().is_empty() {
            return Err(grid_field_error(name, "field name cannot be empty"));
        }
    }

    let mut root_names = std::collections::BTreeSet::new();
    for name in [
        &layout.header_value_field,
        &layout.header_position_field,
        &layout.rows_field,
    ] {
        if !root_names.insert(name.as_str()) {
            return Err(grid_field_error(name, "root field name is duplicated"));
        }
    }
    let mut root_scalar_names = std::collections::BTreeSet::from([
        layout.header_value_field.as_str(),
        layout.header_position_field.as_str(),
    ]);
    let mut fixed_coordinates = std::collections::BTreeSet::new();
    for fixed in &layout.fixed_cells {
        let [name] = fixed.path.as_slice() else {
            return Err(XlsxFormatError::GridLayout(
                "fixed cell paths must name one direct root scalar field",
            ));
        };
        if name.trim().is_empty() {
            return Err(grid_field_error(name, "fixed field name cannot be empty"));
        }
        if !root_names.insert(name) {
            return Err(grid_field_error(name, "root field name is duplicated"));
        }
        root_scalar_names.insert(name);
        if !fixed_coordinates.insert((fixed.row, fixed.column)) {
            return Err(grid_field_error(
                name,
                "fixed worksheet coordinate is mapped more than once",
            ));
        }
    }
    if layout.cell_value_field == layout.cell_position_field {
        return Err(grid_field_error(
            &layout.cell_value_field,
            "cell value and position fields must be distinct",
        ));
    }
    for name in [&layout.cell_value_field, &layout.cell_position_field] {
        if root_scalar_names.contains(name.as_str()) {
            return Err(grid_field_error(
                name,
                "root and cell scalar field names must be distinct",
            ));
        }
    }

    let header_value = direct_child(schema, &layout.header_value_field)?;
    let header_value_type = ordinary_scalar(header_value, &layout.header_value_field)?;
    let header_position = direct_child(schema, &layout.header_position_field)?;
    require_int_scalar(header_position, &layout.header_position_field)?;
    for fixed in &layout.fixed_cells {
        let name = &fixed.path[0];
        ordinary_scalar(direct_child(schema, name)?, name)?;
    }

    let rows = direct_child(schema, &layout.rows_field)?;
    require_repeating_group(rows, &layout.rows_field)?;
    let cells = direct_child(rows, &layout.cells_field)?;
    require_repeating_group(cells, &layout.cells_field)?;
    let cell_value = direct_child(cells, &layout.cell_value_field)?;
    let cell_value_type = ordinary_scalar(cell_value, &layout.cell_value_field)?;
    let cell_position = direct_child(cells, &layout.cell_position_field)?;
    require_int_scalar(cell_position, &layout.cell_position_field)?;

    Ok(GridSchema {
        header_value_type,
        rows,
        cells,
        cell_value_type,
    })
}

fn materialize_fixed_cells(
    root: &mut Instance,
    schema: &SchemaNode,
    layout: &XlsxGridLayout,
    range: &Range<Data>,
) -> Result<(), XlsxFormatError> {
    for fixed in &layout.fixed_cells {
        let name = &fixed.path[0];
        let ty = ordinary_scalar(direct_child(schema, name)?, name)?;
        let value = parse_cell(
            range
                .get_value((fixed.row.get() - 1, fixed.column.get() - 1))
                .unwrap_or(&Data::Empty),
            ty,
            fixed.row.get(),
            name,
        )?;
        replace_direct(root, name, Instance::Scalar(value))?;
    }
    Ok(())
}

fn materialize_rows(
    range: &Range<Data>,
    layout: &XlsxGridLayout,
    grid: &GridSchema<'_>,
    last_row: u32,
    last_column: u32,
) -> Result<Vec<Instance>, XlsxFormatError> {
    let first_row = layout.data_start_row.get() - 1;
    if first_row > last_row {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    for row in first_row..=last_row {
        let physical_cells = (0..=last_column)
            .map(|column| range.get_value((row, column)).unwrap_or(&Data::Empty))
            .collect::<Vec<_>>();
        if physical_cells.iter().all(|cell| is_empty_cell(cell)) {
            continue;
        }
        let cells = physical_cells
            .into_iter()
            .enumerate()
            .map(|(column, physical)| {
                let column =
                    u32::try_from(column).map_err(|_| XlsxFormatError::InvalidCoordinate)?;
                let mut cell = empty_group_instance(grid.cells)?;
                let value = parse_cell(
                    physical,
                    grid.cell_value_type,
                    row + 1,
                    &layout.cell_value_field,
                )?;
                replace_direct(&mut cell, &layout.cell_value_field, Instance::Scalar(value))?;
                replace_direct(
                    &mut cell,
                    &layout.cell_position_field,
                    Instance::Scalar(Value::Int(i64::from(column + 1))),
                )?;
                Ok(cell)
            })
            .collect::<Result<Vec<_>, XlsxFormatError>>()?;
        let mut record = empty_group_instance(grid.rows)?;
        replace_direct(&mut record, &layout.cells_field, Instance::Repeated(cells))?;
        rows.push(record);
    }
    Ok(rows)
}

fn is_empty_cell(cell: &Data) -> bool {
    matches!(cell, Data::Empty) || matches!(cell, Data::String(value) if value.is_empty())
}

fn direct_child<'a>(schema: &'a SchemaNode, name: &str) -> Result<&'a SchemaNode, XlsxFormatError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(grid_field_error(name, "parent field is not a group"));
    };
    let mut matches = children.iter().filter(|child| child.name == name);
    let Some(field) = matches.next() else {
        return Err(grid_field_error(name, "field does not exist in schema"));
    };
    if matches.next().is_some() {
        return Err(grid_field_error(name, "field is ambiguous in schema"));
    }
    Ok(field)
}

fn ordinary_scalar(schema: &SchemaNode, name: &str) -> Result<ScalarType, XlsxFormatError> {
    match schema.kind {
        SchemaKind::Scalar { ty } if !schema.repeating && !schema.attribute => Ok(ty),
        _ => Err(grid_field_error(
            name,
            "field must be a non-repeating non-attribute scalar",
        )),
    }
}

fn require_int_scalar(schema: &SchemaNode, name: &str) -> Result<(), XlsxFormatError> {
    match ordinary_scalar(schema, name)? {
        ScalarType::Int => Ok(()),
        _ => Err(grid_field_error(name, "position field must be an integer")),
    }
}

fn require_repeating_group(schema: &SchemaNode, name: &str) -> Result<(), XlsxFormatError> {
    if schema.repeating && !schema.attribute && matches!(schema.kind, SchemaKind::Group { .. }) {
        Ok(())
    } else {
        Err(grid_field_error(
            name,
            "field must be a repeating non-attribute group",
        ))
    }
}

fn empty_instance(schema: &SchemaNode) -> Instance {
    if schema.repeating {
        return Instance::Repeated(Vec::new());
    }
    match &schema.kind {
        SchemaKind::Scalar { .. } => Instance::Scalar(Value::Null),
        SchemaKind::Group { children, .. } => Instance::Group(
            children
                .iter()
                .map(|child| (child.name.clone(), empty_instance(child)))
                .collect(),
        ),
    }
}

fn empty_group_instance(schema: &SchemaNode) -> Result<Instance, XlsxFormatError> {
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(XlsxFormatError::GridRootSchema);
    };
    Ok(Instance::Group(
        children
            .iter()
            .map(|child| (child.name.clone(), empty_instance(child)))
            .collect(),
    ))
}

fn replace_direct(
    instance: &mut Instance,
    name: &str,
    replacement: Instance,
) -> Result<(), XlsxFormatError> {
    let Instance::Group(fields) = instance else {
        return Err(grid_field_error(name, "instance parent is not a group"));
    };
    let Some((_, value)) = fields.iter_mut().find(|(field, _)| field == name) else {
        return Err(grid_field_error(name, "instance field does not exist"));
    };
    *value = replacement;
    Ok(())
}

fn grid_field_error(field: &str, reason: &'static str) -> XlsxFormatError {
    XlsxFormatError::GridField {
        field: field.to_string(),
        reason,
    }
}

#[cfg(test)]
mod tests {
    use ir::{Instance, ScalarType, SchemaNode, Value};
    use mapping::{XlsxColumn, XlsxFixedCell, XlsxGridLayout, XlsxRow};
    use rust_xlsxwriter::Workbook;

    use super::{from_bytes_grid, read_grid, validate_grid_layout};
    use crate::XlsxFormatError;

    fn row(value: u32) -> XlsxRow {
        XlsxRow::new(value).unwrap()
    }

    fn column(value: u32) -> XlsxColumn {
        XlsxColumn::new(value).unwrap()
    }

    fn schema() -> SchemaNode {
        SchemaNode::group(
            "Grid",
            vec![
                SchemaNode::scalar("Year", ScalarType::String),
                SchemaNode::scalar("Header", ScalarType::String),
                SchemaNode::scalar("HeaderColumn", ScalarType::Int),
                SchemaNode::group(
                    "Rows",
                    vec![
                        SchemaNode::group(
                            "Cells",
                            vec![
                                SchemaNode::scalar("Value", ScalarType::String),
                                SchemaNode::scalar("Column", ScalarType::Int),
                            ],
                        )
                        .repeating(),
                    ],
                )
                .repeating(),
            ],
        )
    }

    fn layout() -> XlsxGridLayout {
        XlsxGridLayout {
            sheet: Some("Sales".into()),
            header_row: row(1),
            data_start_row: row(2),
            header_value_field: "Header".into(),
            header_position_field: "HeaderColumn".into(),
            rows_field: "Rows".into(),
            cells_field: "Cells".into(),
            cell_value_field: "Value".into(),
            cell_position_field: "Column".into(),
            fixed_cells: vec![XlsxFixedCell {
                path: vec!["Year".into()],
                row: row(1),
                column: column(1),
            }],
        }
    }

    fn workbook_bytes() -> Vec<u8> {
        let mut workbook = Workbook::new();
        let worksheet = workbook.add_worksheet();
        worksheet.set_name("Sales").unwrap();
        worksheet.write_string(0, 0, "2026").unwrap();
        worksheet.write_string(0, 1, "Jan").unwrap();
        worksheet.write_string(0, 3, "Mar").unwrap();
        worksheet.write_string(1, 0, "West").unwrap();
        worksheet.write_number(1, 1, 10.0).unwrap();
        worksheet.write_number(1, 3, 30.0).unwrap();
        worksheet.write_string(3, 0, "East").unwrap();
        worksheet.write_number(3, 1, 11.0).unwrap();
        worksheet.write_number(3, 3, 31.0).unwrap();
        workbook.save_to_buffer().unwrap()
    }

    fn scalar(value: Value) -> Instance {
        Instance::Scalar(value)
    }

    fn expected_cells(region: &str, jan: &str, mar: &str) -> Vec<Instance> {
        [region, jan, "", mar]
            .into_iter()
            .enumerate()
            .map(|(index, value)| {
                Instance::Group(vec![
                    (
                        "Value".into(),
                        if value.is_empty() {
                            scalar(Value::Null)
                        } else {
                            scalar(Value::String(value.to_string()))
                        },
                    ),
                    ("Column".into(), scalar(Value::Int((index + 1) as i64))),
                ])
            })
            .collect()
    }

    fn expected_record(header: &str, column: i64) -> Instance {
        Instance::Group(vec![
            ("Year".into(), scalar(Value::String("2026".into()))),
            ("Header".into(), scalar(Value::String(header.to_string()))),
            ("HeaderColumn".into(), scalar(Value::Int(column))),
            (
                "Rows".into(),
                Instance::Repeated(vec![
                    Instance::Group(vec![(
                        "Cells".into(),
                        Instance::Repeated(expected_cells("West", "10", "30")),
                    )]),
                    Instance::Group(vec![(
                        "Cells".into(),
                        Instance::Repeated(expected_cells("East", "11", "31")),
                    )]),
                ]),
            ),
        ])
    }

    #[test]
    fn byte_and_native_readers_materialize_header_records_and_complete_matrix() {
        let bytes = workbook_bytes();
        let expected = vec![
            expected_record("2026", 1),
            expected_record("Jan", 2),
            expected_record("Mar", 4),
        ];

        assert_eq!(
            from_bytes_grid(&bytes, &schema(), &layout()).unwrap(),
            expected
        );

        let path = std::env::temp_dir().join(format!(
            "ferrule-xlsx-grid-{}-{}.xlsx",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::write(&path, bytes).unwrap();
        let actual = read_grid(&path, &schema(), &layout()).unwrap();
        std::fs::remove_file(path).ok();
        assert_eq!(actual, expected);
    }

    #[test]
    fn validation_rejects_bad_shape_names_and_coordinates() {
        let mut bad = layout();
        bad.data_start_row = bad.header_row;
        assert!(matches!(
            validate_grid_layout(&schema(), &bad),
            Err(XlsxFormatError::GridLayout(_))
        ));

        let mut bad = layout();
        bad.cell_position_field = bad.header_position_field.clone();
        assert!(matches!(
            validate_grid_layout(&schema(), &bad),
            Err(XlsxFormatError::GridField { .. })
        ));

        let mut bad = layout();
        bad.fixed_cells[0].path = vec!["Rows".into(), "Year".into()];
        assert!(matches!(
            validate_grid_layout(&schema(), &bad),
            Err(XlsxFormatError::GridLayout(_))
        ));

        let bad_schema = SchemaNode::group(
            "Grid",
            vec![
                SchemaNode::scalar("Year", ScalarType::String),
                SchemaNode::scalar("Header", ScalarType::String),
                SchemaNode::scalar("HeaderColumn", ScalarType::String),
                SchemaNode::group("Rows", Vec::new()).repeating(),
            ],
        );
        assert!(matches!(
            validate_grid_layout(&bad_schema, &layout()),
            Err(XlsxFormatError::GridField { .. })
        ));
    }
}
