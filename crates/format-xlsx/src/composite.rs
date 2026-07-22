use std::io::Cursor;
use std::path::Path;

use calamine::{Data, Range, Reader, Xlsx};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::{XlsxColumn, XlsxCompositeLayout, XlsxFixedRecord, XlsxRow, XlsxTableRegion};

use super::{XlsxFormatError, column_indexes, parse_cell, rows_from_range};

/// Reads repeated tables and fixed worksheet records into a schema-shaped
/// composite instance.
pub fn read_composite(
    path: &Path,
    schema: &SchemaNode,
    layout: &XlsxCompositeLayout,
) -> Result<Instance, XlsxFormatError> {
    let bytes = std::fs::read(path)?;
    from_bytes_composite(&bytes, schema, layout)
}

/// Reads a composite XLSX source from an in-memory workbook.
pub fn from_bytes_composite(
    bytes: &[u8],
    schema: &SchemaNode,
    layout: &XlsxCompositeLayout,
) -> Result<Instance, XlsxFormatError> {
    validate_composite_layout(schema, layout)?;

    let mut workbook = Xlsx::new(Cursor::new(bytes))?;
    let sheet_names = workbook.sheet_names().to_vec();
    let first_sheet = sheet_names
        .first()
        .cloned()
        .ok_or(XlsxFormatError::NoWorksheets)?;
    let table_sheets = tables(layout)
        .map(|table| select_sheet(&sheet_names, &first_sheet, table.sheet.as_deref()))
        .collect::<Result<Vec<_>, _>>()?;
    let record_sheets = layout
        .records
        .iter()
        .map(|record| select_sheet(&sheet_names, &first_sheet, record.sheet.as_deref()))
        .collect::<Result<Vec<_>, _>>()?;
    let selected_sheets = table_sheets
        .iter()
        .cloned()
        .chain(record_sheets.iter().cloned())
        .collect::<std::collections::BTreeSet<_>>();
    let mut ranges = std::collections::BTreeMap::new();
    for sheet in selected_sheets {
        ranges.insert(sheet.clone(), workbook.worksheet_range(&sheet)?);
    }

    let mut instance = empty_instance(schema);
    for (record, sheet) in layout.records.iter().zip(&record_sheets) {
        let range = ranges
            .get(sheet)
            .ok_or_else(|| XlsxFormatError::MissingWorksheet(sheet.clone()))?;
        materialize_fixed_record(&mut instance, schema, record, range)?;
    }

    for (table, sheet) in tables(layout).zip(&table_sheets) {
        let table_schema = schema_node_at(schema, &table.path)?;
        let table_fields =
            flat_table_fields(table_schema, &table.path, table.row_number_field.as_deref())?;
        let columns = composite_columns(table, table_fields.len())?;
        let table_range = ranges
            .get(sheet)
            .ok_or_else(|| XlsxFormatError::MissingWorksheet(sheet.clone()))?;
        let mut rows = rows_from_range(
            table_range,
            &table_fields,
            table.start_row.get(),
            &columns,
            table.has_header,
        )?;
        materialize_row_numbers(&mut rows, table_schema, table)?;
        replace_at_path(&mut instance, &table.path, Instance::Repeated(rows))?;
    }
    Ok(instance)
}

fn tables(layout: &XlsxCompositeLayout) -> impl Iterator<Item = &XlsxTableRegion> {
    std::iter::once(&layout.table).chain(&layout.additional_tables)
}

fn select_sheet(
    sheet_names: &[String],
    first_sheet: &str,
    requested: Option<&str>,
) -> Result<String, XlsxFormatError> {
    match requested {
        Some(sheet) if sheet_names.iter().any(|name| name == sheet) => Ok(sheet.to_string()),
        Some(sheet) => Err(XlsxFormatError::MissingWorksheet(sheet.to_string())),
        None => Ok(first_sheet.to_string()),
    }
}

pub(super) fn validate_composite_layout(
    schema: &SchemaNode,
    layout: &XlsxCompositeLayout,
) -> Result<(), XlsxFormatError> {
    if schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err(XlsxFormatError::CompositeRootSchema);
    }
    let mut table_paths = std::collections::BTreeSet::new();
    for table in tables(layout) {
        if !table_paths.insert(table.path.clone()) {
            return Err(XlsxFormatError::DuplicateCompositePath(display_path(
                &table.path,
            )));
        }
        let table_schema = schema_node_at(schema, &table.path)?;
        let table_fields =
            flat_table_fields(table_schema, &table.path, table.row_number_field.as_deref())?;
        composite_columns(table, table_fields.len())?;
    }

    let mut record_paths = std::collections::BTreeSet::new();
    let mut cell_paths = std::collections::BTreeSet::new();
    for record in &layout.records {
        if !record_paths.insert(record.path.clone()) {
            return Err(XlsxFormatError::DuplicateCompositePath(display_path(
                &record.path,
            )));
        }
        let record_schema = schema_node_at(schema, &record.path)?;
        if !matches!(record_schema.kind, SchemaKind::Group { .. }) {
            return Err(composite_path_error(
                &record.path,
                "fixed record must resolve to a group",
            ));
        }
        if table_paths.contains(&record.path) {
            return Err(composite_path_error(
                &record.path,
                "fixed record and table cannot own the same group",
            ));
        }
        for cell in &record.cells {
            if cell.path.is_empty() {
                return Err(composite_path_error(
                    &record.path,
                    "fixed cell path cannot be empty",
                ));
            }
            let field = schema_node_at(record_schema, &cell.path)?;
            if field.repeating
                || field.attribute
                || !matches!(field.kind, SchemaKind::Scalar { .. })
            {
                let absolute = joined_path(&record.path, &cell.path);
                return Err(composite_path_error(
                    &absolute,
                    "fixed cell must resolve to a non-repeating scalar field",
                ));
            }
            let absolute = joined_path(&record.path, &cell.path);
            if table_paths.iter().any(|table| absolute.starts_with(table)) {
                return Err(composite_path_error(
                    &absolute,
                    "fixed cells cannot be inside the table",
                ));
            }
            if !cell_paths.insert(absolute.clone()) {
                return Err(XlsxFormatError::DuplicateCompositePath(display_path(
                    &absolute,
                )));
            }
        }
    }
    Ok(())
}

fn flat_table_fields<'a>(
    schema: &'a SchemaNode,
    path: &[String],
    row_number_field: Option<&str>,
) -> Result<Vec<(&'a str, ScalarType)>, XlsxFormatError> {
    if !schema.repeating {
        return Err(composite_path_error(
            path,
            "table must resolve to a repeating group",
        ));
    }
    let SchemaKind::Group { children, .. } = &schema.kind else {
        return Err(composite_path_error(
            path,
            "table must resolve to a repeating group",
        ));
    };
    if let Some(field_name) = row_number_field {
        let Some(field) = children.iter().find(|child| child.name == field_name) else {
            return Err(composite_path_error(
                path,
                "row-number field does not exist in the table",
            ));
        };
        if field.repeating
            || field.attribute
            || !matches!(
                field.kind,
                SchemaKind::Scalar {
                    ty: ScalarType::Int
                }
            )
        {
            return Err(composite_path_error(
                path,
                "row-number field must be a non-repeating integer scalar",
            ));
        }
    }
    children
        .iter()
        .filter(|child| row_number_field != Some(child.name.as_str()))
        .map(|child| match child.kind {
            SchemaKind::Scalar { ty } if !child.repeating && !child.attribute => {
                Ok((child.name.as_str(), ty))
            }
            _ => Err(composite_path_error(
                path,
                "table fields must be non-repeating scalar fields",
            )),
        })
        .collect()
}

fn materialize_row_numbers(
    rows: &mut [Instance],
    table_schema: &SchemaNode,
    table: &XlsxTableRegion,
) -> Result<(), XlsxFormatError> {
    let Some(field_name) = table.row_number_field.as_deref() else {
        return Ok(());
    };
    let SchemaKind::Group { children, .. } = &table_schema.kind else {
        return Err(XlsxFormatError::UnsupportedSchema);
    };
    let Some(field_index) = children.iter().position(|child| child.name == field_name) else {
        return Err(composite_path_error(
            &table.path,
            "row-number field does not exist in the table",
        ));
    };
    let first_row = table
        .start_row
        .get()
        .checked_add(u32::from(table.has_header))
        .ok_or(XlsxFormatError::InvalidCoordinate)?;
    for (offset, row) in rows.iter_mut().enumerate() {
        let physical_row = first_row
            .checked_add(u32::try_from(offset).map_err(|_| XlsxFormatError::InvalidCoordinate)?)
            .ok_or(XlsxFormatError::InvalidCoordinate)?;
        let Instance::Group(fields) = row else {
            return Err(XlsxFormatError::UnsupportedSchema);
        };
        fields.insert(
            field_index,
            (
                field_name.to_string(),
                Instance::Scalar(Value::Int(i64::from(physical_row))),
            ),
        );
    }
    Ok(())
}

fn composite_columns(
    table: &XlsxTableRegion,
    field_count: usize,
) -> Result<Vec<u32>, XlsxFormatError> {
    let columns = table
        .columns
        .iter()
        .map(|column| column.get())
        .collect::<Vec<_>>();
    column_indexes(field_count, &columns)
}

fn schema_node_at<'a>(
    schema: &'a SchemaNode,
    path: &[String],
) -> Result<&'a SchemaNode, XlsxFormatError> {
    let mut node = schema;
    for (index, segment) in path.iter().enumerate() {
        if index > 0 && node.repeating {
            return Err(composite_path_error(
                path,
                "path crosses a repeating ancestor",
            ));
        }
        let SchemaKind::Group { children, .. } = &node.kind else {
            return Err(composite_path_error(path, "path crosses a scalar field"));
        };
        let mut matches = children.iter().filter(|child| child.name == *segment);
        let Some(child) = matches.next() else {
            return Err(composite_path_error(path, "path does not exist in schema"));
        };
        if matches.next().is_some() {
            return Err(composite_path_error(path, "path is ambiguous in schema"));
        }
        node = child;
    }
    Ok(node)
}

fn materialize_fixed_record(
    root: &mut Instance,
    schema: &SchemaNode,
    record: &XlsxFixedRecord,
    range: &Range<Data>,
) -> Result<(), XlsxFormatError> {
    let record_schema = schema_node_at(schema, &record.path)?;
    if record_schema.repeating {
        let mut value = empty_group_instance(record_schema)?;
        for cell in &record.cells {
            let field = schema_node_at(record_schema, &cell.path)?;
            let absolute = joined_path(&record.path, &cell.path);
            let ty = scalar_type_of(field, &absolute)?;
            let parsed = read_fixed_cell(range, cell.row, cell.column, ty, &absolute)?;
            replace_at_path(&mut value, &cell.path, Instance::Scalar(parsed))?;
        }
        replace_at_path(root, &record.path, Instance::Repeated(vec![value]))
    } else {
        for cell in &record.cells {
            let absolute = joined_path(&record.path, &cell.path);
            let field = schema_node_at(schema, &absolute)?;
            let ty = scalar_type_of(field, &absolute)?;
            let parsed = read_fixed_cell(range, cell.row, cell.column, ty, &absolute)?;
            replace_at_path(root, &absolute, Instance::Scalar(parsed))?;
        }
        Ok(())
    }
}

fn read_fixed_cell(
    range: &Range<Data>,
    row: XlsxRow,
    column: XlsxColumn,
    ty: ScalarType,
    path: &[String],
) -> Result<Value, XlsxFormatError> {
    parse_cell(
        range
            .get_value((row.get() - 1, column.get() - 1))
            .unwrap_or(&Data::Empty),
        ty,
        row.get(),
        &display_path(path),
    )
}

fn scalar_type_of(schema: &SchemaNode, path: &[String]) -> Result<ScalarType, XlsxFormatError> {
    match schema.kind {
        SchemaKind::Scalar { ty } => Ok(ty),
        SchemaKind::Group { .. } => Err(composite_path_error(path, "field is not scalar")),
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
    match &schema.kind {
        SchemaKind::Group { children, .. } => Ok(Instance::Group(
            children
                .iter()
                .map(|child| (child.name.clone(), empty_instance(child)))
                .collect(),
        )),
        SchemaKind::Scalar { .. } => Err(XlsxFormatError::UnsupportedSchema),
    }
}

fn replace_at_path(
    instance: &mut Instance,
    path: &[String],
    replacement: Instance,
) -> Result<(), XlsxFormatError> {
    if path.is_empty() {
        *instance = replacement;
        return Ok(());
    }
    let Instance::Group(fields) = instance else {
        return Err(composite_path_error(path, "instance path is not a group"));
    };
    let Some((_, child)) = fields.iter_mut().find(|(name, _)| name == &path[0]) else {
        return Err(composite_path_error(path, "instance path does not exist"));
    };
    replace_at_path(child, &path[1..], replacement)
}

fn joined_path(parent: &[String], child: &[String]) -> Vec<String> {
    parent.iter().chain(child).cloned().collect()
}

fn display_path(path: &[String]) -> String {
    if path.is_empty() {
        "$".to_string()
    } else {
        path.join("/")
    }
}

fn composite_path_error(path: &[String], reason: &'static str) -> XlsxFormatError {
    XlsxFormatError::CompositePath {
        path: display_path(path),
        reason,
    }
}
