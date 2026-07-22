use std::io::Cursor;
use std::path::Path;

use calamine::{Reader, Xlsx};
use ir::{Instance, ScalarType, SchemaKind, SchemaNode, Value};
use mapping::XlsxWorksheetSetLayout;

use super::{XlsxFormatError, column_indexes, rows_from_range};

/// Reads every worksheet and its row table into one schema-shaped instance.
pub fn read_worksheet_set(
    path: &Path,
    schema: &SchemaNode,
    layout: &XlsxWorksheetSetLayout,
) -> Result<Instance, XlsxFormatError> {
    let bytes = std::fs::read(path)?;
    from_bytes_worksheet_set(&bytes, schema, layout)
}

/// Reads an all-worksheets XLSX source from an in-memory workbook.
pub fn from_bytes_worksheet_set(
    bytes: &[u8],
    schema: &SchemaNode,
    layout: &XlsxWorksheetSetLayout,
) -> Result<Instance, XlsxFormatError> {
    let shape = validate_layout(schema, layout)?;
    let mut workbook = Xlsx::new(Cursor::new(bytes))?;
    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(XlsxFormatError::NoWorksheets);
    }
    let columns = column_indexes(
        shape.row_fields.len(),
        &layout
            .columns
            .iter()
            .map(|column| column.get())
            .collect::<Vec<_>>(),
    )?;
    let first_physical_row = layout
        .start_row
        .get()
        .checked_add(u32::from(layout.has_header))
        .ok_or(XlsxFormatError::InvalidCoordinate)?;
    let mut worksheets = Vec::with_capacity(sheet_names.len());
    for sheet_name in sheet_names {
        let range = workbook.worksheet_range(&sheet_name)?;
        let mut rows = rows_from_range(
            &range,
            &shape.row_fields,
            layout.start_row.get(),
            &columns,
            layout.has_header,
        )?;
        if let Some((field_name, field_index)) = &shape.row_number {
            for (offset, row) in rows.iter_mut().enumerate() {
                let physical_row = first_physical_row
                    .checked_add(
                        u32::try_from(offset).map_err(|_| XlsxFormatError::InvalidCoordinate)?,
                    )
                    .ok_or(XlsxFormatError::InvalidCoordinate)?;
                let Instance::Group(fields) = row else {
                    return Err(XlsxFormatError::WorksheetSetRootSchema);
                };
                fields.insert(
                    *field_index,
                    (
                        field_name.clone(),
                        Instance::Scalar(Value::Int(i64::from(physical_row))),
                    ),
                );
            }
        }
        let mut worksheet = empty_group_instance(shape.worksheet_schema)?;
        replace_at_path(
            &mut worksheet,
            &layout.worksheet_name_path,
            Instance::Scalar(Value::String(sheet_name)),
        )?;
        replace_at_path(&mut worksheet, &layout.rows_path, Instance::Repeated(rows))?;
        worksheets.push(worksheet);
    }
    let mut root = empty_instance(schema);
    replace_at_path(
        &mut root,
        &layout.worksheets_path,
        Instance::Repeated(worksheets),
    )?;
    Ok(root)
}

struct ValidatedShape<'a> {
    worksheet_schema: &'a SchemaNode,
    row_fields: Vec<(&'a str, ScalarType)>,
    row_number: Option<(String, usize)>,
}

fn validate_layout<'a>(
    schema: &'a SchemaNode,
    layout: &XlsxWorksheetSetLayout,
) -> Result<ValidatedShape<'a>, XlsxFormatError> {
    if schema.repeating || !matches!(schema.kind, SchemaKind::Group { .. }) {
        return Err(XlsxFormatError::WorksheetSetRootSchema);
    }
    let worksheet_schema = schema_node_at(schema, &layout.worksheets_path)?;
    if !worksheet_schema.repeating || !matches!(worksheet_schema.kind, SchemaKind::Group { .. }) {
        return Err(path_error(
            &layout.worksheets_path,
            "worksheets path must resolve to a repeating group",
        ));
    }
    let name = schema_node_at(worksheet_schema, &layout.worksheet_name_path)?;
    if name.repeating
        || name.attribute
        || !matches!(
            name.kind,
            SchemaKind::Scalar {
                ty: ScalarType::String
            }
        )
    {
        return Err(path_error(
            &joined_path(&layout.worksheets_path, &layout.worksheet_name_path),
            "worksheet name must resolve to a non-repeating string scalar",
        ));
    }
    let rows = schema_node_at(worksheet_schema, &layout.rows_path)?;
    if !rows.repeating {
        return Err(path_error(
            &joined_path(&layout.worksheets_path, &layout.rows_path),
            "rows path must resolve to a repeating group",
        ));
    }
    let SchemaKind::Group { children, .. } = &rows.kind else {
        return Err(path_error(
            &joined_path(&layout.worksheets_path, &layout.rows_path),
            "rows path must resolve to a repeating group",
        ));
    };
    let row_number_name = match layout.row_number_path.as_deref() {
        Some([name]) => Some(name.as_str()),
        Some(path) => {
            return Err(path_error(
                &joined_path(
                    &joined_path(&layout.worksheets_path, &layout.rows_path),
                    path,
                ),
                "row-number path must select one direct row field",
            ));
        }
        None => None,
    };
    let mut row_number = None;
    let mut row_fields = Vec::new();
    for (index, child) in children.iter().enumerate() {
        if row_number_name == Some(child.name.as_str()) {
            if child.repeating
                || child.attribute
                || !matches!(
                    child.kind,
                    SchemaKind::Scalar {
                        ty: ScalarType::Int
                    }
                )
            {
                return Err(path_error(
                    &joined_path(&layout.worksheets_path, &layout.rows_path),
                    "row-number field must be a non-repeating integer scalar",
                ));
            }
            row_number = Some((child.name.clone(), index));
            continue;
        }
        match child.kind {
            SchemaKind::Scalar { ty } if !child.repeating && !child.attribute => {
                row_fields.push((child.name.as_str(), ty));
            }
            _ => {
                return Err(path_error(
                    &joined_path(&layout.worksheets_path, &layout.rows_path),
                    "row fields must be non-repeating scalar fields",
                ));
            }
        }
    }
    if row_number_name.is_some() && row_number.is_none() {
        return Err(path_error(
            &joined_path(&layout.worksheets_path, &layout.rows_path),
            "row-number field does not exist",
        ));
    }
    column_indexes(
        row_fields.len(),
        &layout
            .columns
            .iter()
            .map(|column| column.get())
            .collect::<Vec<_>>(),
    )?;
    Ok(ValidatedShape {
        worksheet_schema,
        row_fields,
        row_number,
    })
}

fn schema_node_at<'a>(
    schema: &'a SchemaNode,
    path: &[String],
) -> Result<&'a SchemaNode, XlsxFormatError> {
    let mut node = schema;
    for segment in path {
        let SchemaKind::Group { children, .. } = &node.kind else {
            return Err(path_error(path, "path crosses a scalar field"));
        };
        let mut matches = children.iter().filter(|child| child.name == *segment);
        let Some(child) = matches.next() else {
            return Err(path_error(path, "path does not exist in schema"));
        };
        if matches.next().is_some() {
            return Err(path_error(path, "path is ambiguous in schema"));
        }
        node = child;
    }
    Ok(node)
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
        SchemaKind::Scalar { .. } => Err(XlsxFormatError::WorksheetSetRootSchema),
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
        return Err(path_error(path, "instance path is not a group"));
    };
    let Some((_, child)) = fields.iter_mut().find(|(name, _)| name == &path[0]) else {
        return Err(path_error(path, "instance path does not exist"));
    };
    replace_at_path(child, &path[1..], replacement)
}

fn joined_path(parent: &[String], child: &[String]) -> Vec<String> {
    parent.iter().chain(child).cloned().collect()
}

fn path_error(path: &[String], reason: &'static str) -> XlsxFormatError {
    XlsxFormatError::WorksheetSetPath {
        path: if path.is_empty() {
            "$".to_string()
        } else {
            path.join("/")
        },
        reason,
    }
}
